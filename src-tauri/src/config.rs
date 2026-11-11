use crate::models::{
    split_domains, RulesPayload, Upstream, CONTROLLER_PORT, CONTROLLER_SECRET, PROXY_PORT,
};
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    process::Command,
    time::Duration,
};

#[cfg(target_os = "windows")]
use crate::models::CREATE_NO_WINDOW;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub fn outbound_name_for_upstream(id: &str) -> String {
    format!("upstream_{id}")
}

pub fn target_to_outbound(target: &str, upstreams: &[Upstream]) -> String {
    match target {
        "system" => "DIRECT".into(),
        "wlan" => "PhysicalDirect".into(),
        "reject" => "REJECT".into(),
        other => {
            if upstreams.iter().any(|u| u.id == other) {
                outbound_name_for_upstream(other)
            } else {
                "DIRECT".into()
            }
        }
    }
}

pub fn parse_host_port(address: &str) -> Result<(String, u16), String> {
    let trimmed = address.trim();
    let (host, port_str) = trimmed
        .rsplit_once(':')
        .ok_or_else(|| format!("无效的上游地址（需为 主机:端口）：{trimmed}"))?;
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() {
        return Err(format!("上游主机不能为空：{trimmed}"));
    }
    let port: u16 = port_str
        .trim()
        .parse()
        .map_err(|_| format!("无效的上游端口：{port_str}"))?;
    Ok((host.to_string(), port))
}

pub fn is_tcp_reachable(host: &str, port: u16, timeout_ms: u64) -> bool {
    let addr_str = format!("{host}:{port}");
    if let Ok(addr) = addr_str.parse::<SocketAddr>() {
        return TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)).is_ok();
    }
    false
}

fn find_local_port_process_names(ports: &[u16]) -> Vec<String> {
    if ports.is_empty() {
        return Vec::new();
    }
    let port_list = ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "Get-NetTCPConnection -LocalPort @({port_list}) -State Listen -ErrorAction SilentlyContinue | ForEach-Object {{ (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName }} | Sort-Object -Unique"
    );
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut names = Vec::new();
    if let Ok(out) = cmd.output() {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let exe = if trimmed.to_lowercase().ends_with(".exe") {
                    trimmed.to_string()
                } else {
                    format!("{trimmed}.exe")
                };
                if !names.contains(&exe) {
                    names.push(exe);
                }
            }
        }
    }
    names
}

pub fn build_mihomo_config(payload: &RulesPayload) -> Result<Value, String> {
    let mut proxies = vec![json!({
        "name": "PhysicalDirect",
        "type": "direct",
        "interface-name": "WLAN"
    })];

    let mut reachable_doh_outbound: Option<String> = None;
    let mut local_upstream_ports = Vec::new();

    for upstream in &payload.upstreams {
        let (host, port) = parse_host_port(&upstream.address)?;
        if host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost") {
            local_upstream_ports.push(port);
        }
        let outbound_name = outbound_name_for_upstream(&upstream.id);
        let protocol = match upstream.protocol.to_lowercase().as_str() {
            "socks5" | "socks" => "socks5",
            _ => "http",
        };
        proxies.push(json!({
            "name": outbound_name,
            "type": protocol,
            "server": host,
            "port": port
        }));
        if reachable_doh_outbound.is_none() && is_tcp_reachable(&host, port, 250) {
            reachable_doh_outbound = Some(outbound_name);
        }
    }

    if payload.fallback != "system" && payload.fallback != "wlan" && payload.fallback != "reject" {
        if let Some(u) = payload.upstreams.iter().find(|u| u.id == payload.fallback) {
            if let Ok((host, port)) = parse_host_port(&u.address) {
                if is_tcp_reachable(&host, port, 250) {
                    reachable_doh_outbound = Some(outbound_name_for_upstream(&u.id));
                }
            }
        }
    }

    let mut dns_policy = Map::new();
    let mut domain_entries: Vec<(String, String)> = Vec::new();
    let mut seen_domains = HashMap::new();

    for group in &payload.groups {
        if !group.enabled {
            continue;
        }
        let outbound = target_to_outbound(&group.target, &payload.upstreams);

        for domain in split_domains(&group.domains) {
            if seen_domains.contains_key(&domain) {
                continue;
            }
            seen_domains.insert(domain.clone(), true);
            if group.target != "reject" {
                if let Some(ref doh_out) = reachable_doh_outbound {
                    dns_policy.insert(
                        format!("+.{domain}"),
                        json!([format!("https://dns.google/dns-query#{doh_out}")]),
                    );
                }
            }
            let rule_line = format!("DOMAIN-SUFFIX,{domain},{outbound}");
            domain_entries.push((domain, rule_line));
        }
    }

    domain_entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut rules: Vec<String> = Vec::new();
    if payload.tun_enabled {
        for proc_name in find_local_port_process_names(&local_upstream_ports) {
            rules.push(format!("PROCESS-NAME,{proc_name},PhysicalDirect"));
        }
    }
    rules.extend(domain_entries.into_iter().map(|(_, line)| line));

    let fallback_outbound = target_to_outbound(&payload.fallback, &payload.upstreams);
    rules.push(format!("MATCH,{fallback_outbound}"));

    let dns_config = if payload.tun_enabled {
        let mut nameservers = Vec::new();
        if let Some(ref doh_out) = reachable_doh_outbound {
            nameservers.push(format!("https://dns.google/dns-query#{doh_out}"));
        }
        nameservers.push("223.5.5.5".to_string());
        json!({
            "enable": true,
            "enhanced-mode": "fake-ip",
            "fake-ip-range": "198.18.0.1/16",
            "default-nameserver": ["223.5.5.5", "119.29.29.29"],
            "nameserver": nameservers,
            "nameserver-policy": dns_policy
        })
    } else {
        json!({
            "enable": true,
            "enhanced-mode": "redir-host",
            "nameserver": ["system"],
            "nameserver-policy": dns_policy
        })
    };

    Ok(json!({
        "mixed-port": PROXY_PORT,
        "bind-address": "127.0.0.1",
        "allow-lan": false,
        "mode": "rule",
        "log-level": "info",
        "external-controller": format!("127.0.0.1:{CONTROLLER_PORT}"),
        "external-controller-cors": {
            "allow-origins": ["*"],
            "allow-private-network": true
        },
        "secret": CONTROLLER_SECRET,
        "tun": {
            "enable": payload.tun_enabled,
            "device": "RouteIsle",
            "stack": "gvisor",
            "dns-hijack": ["any:53", "tcp://any:53"],
            "auto-route": true,
            "auto-detect-interface": true,
            "inet4-address": ["198.19.0.1/30"]
        },
        "sniffer": {
            "enable": payload.tun_enabled,
            "force-dns-mapping": true,
            "parse-pure-ip": true,
            "override-destination": false,
            "sniff": {
                "HTTP": { "ports": [80, "8080-8880"], "override-destination": true },
                "TLS": { "ports": [443, 8443] }
            }
        },
        "dns": dns_config,
        "proxies": proxies,
        "rules": rules
    }))
}

pub fn controller_request(method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
    let addr: SocketAddr = format!("127.0.0.1:{CONTROLLER_PORT}")
        .parse()
        .map_err(|e: std::net::AddrParseError| e.to_string())?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))
        .map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

    let payload = body.unwrap_or("");
    let request = if payload.is_empty() {
        format!(
            "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{CONTROLLER_PORT}\r\nAuthorization: Bearer {CONTROLLER_SECRET}\r\nConnection: close\r\n\r\n"
        )
    } else {
        format!(
            "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{CONTROLLER_PORT}\r\nAuthorization: Bearer {CONTROLLER_SECRET}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    };

    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw_bytes = Vec::new();
    stream.read_to_end(&mut raw_bytes).map_err(|e| e.to_string())?;
    let response = String::from_utf8_lossy(&raw_bytes);
    let (_, resp_body) = response.split_once("\r\n\r\n").ok_or("Invalid HTTP response")?;
    Ok(resp_body.to_string())
}
