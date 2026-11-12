use crate::models::{
    split_domains, RulesPayload, Upstream, CONTROLLER_PORT, CONTROLLER_SECRET, PROXY_PORT,
};
use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    thread,
    time::Duration,
};

#[cfg(target_os = "windows")]
mod win_net {
    use super::*;

    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibTcpRowOwnerPid {
        dw_state: u32,
        dw_local_addr: u32,
        dw_local_port: u32,
        dw_remote_addr: u32,
        dw_remote_port: u32,
        dw_owning_pid: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibTcp6RowOwnerPid {
        uc_local_addr: [u8; 16],
        dw_local_scope_id: u32,
        dw_local_port: u32,
        uc_remote_addr: [u8; 16],
        dw_remote_scope_id: u32,
        dw_remote_port: u32,
        dw_state: u32,
        dw_owning_pid: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibIpForwardRow {
        dw_forward_dest: u32,
        dw_forward_mask: u32,
        dw_forward_policy: u32,
        dw_forward_next_hop: u32,
        dw_forward_if_index: u32,
        dw_forward_type: u32,
        dw_forward_proto: u32,
        dw_forward_age: u32,
        dw_forward_next_hop_as: u32,
        dw_forward_metric1: u32,
        dw_forward_metric2: u32,
        dw_forward_metric3: u32,
        dw_forward_metric4: u32,
        dw_forward_metric5: u32,
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetExtendedTcpTable(
            p_tcp_table: *mut u8,
            pdw_size: *mut u32,
            b_order: i32,
            ul_af: u32,
            table_class: u32,
            reserved: u32,
        ) -> u32;
        fn GetIpForwardTable(p_ip_forward_table: *mut u8, pdw_size: *mut u32, b_order: i32)
            -> u32;
        fn ConvertInterfaceIndexToLuid(interface_index: u32, interface_luid: *mut u64) -> u32;
        fn ConvertInterfaceLuidToAlias(
            interface_luid: *const u64,
            interface_alias: *mut u16,
            length: usize,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn QueryFullProcessImageNameW(
            h_process: isize,
            dw_flags: u32,
            lp_exe_name: *mut u16,
            lpdw_size: *mut u32,
        ) -> i32;
        fn CloseHandle(h_object: isize) -> i32;
    }

    fn process_exe_name(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle == 0 {
                return None;
            }
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
            let _ = CloseHandle(handle);
            if ok == 0 || len == 0 {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            Path::new(&full)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        }
    }

    fn collect_pids_for_af(af: u32, target_ports: &HashSet<u16>, pids: &mut HashSet<u32>) {
        let mut size: u32 = 0;
        unsafe {
            let res = GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                af,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            );
            if res != ERROR_INSUFFICIENT_BUFFER || size < 4 {
                return;
            }
            let mut buf = vec![0u8; size as usize];
            if GetExtendedTcpTable(
                buf.as_mut_ptr(),
                &mut size,
                0,
                af,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            ) != 0
            {
                return;
            }
            let count = u32::from_ne_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;
            let data = &buf[4..];
            if af == AF_INET {
                let row_size = std::mem::size_of::<MibTcpRowOwnerPid>();
                for i in 0..count {
                    if (i + 1) * row_size > data.len() {
                        break;
                    }
                    let row = std::ptr::read_unaligned(
                        data.as_ptr().add(i * row_size) as *const MibTcpRowOwnerPid
                    );
                    let port = u16::from_be((row.dw_local_port & 0xFFFF) as u16);
                    if target_ports.contains(&port) {
                        pids.insert(row.dw_owning_pid);
                    }
                }
            } else if af == AF_INET6 {
                let row_size = std::mem::size_of::<MibTcp6RowOwnerPid>();
                for i in 0..count {
                    if (i + 1) * row_size > data.len() {
                        break;
                    }
                    let row = std::ptr::read_unaligned(
                        data.as_ptr().add(i * row_size) as *const MibTcp6RowOwnerPid
                    );
                    let port = u16::from_be((row.dw_local_port & 0xFFFF) as u16);
                    if target_ports.contains(&port) {
                        pids.insert(row.dw_owning_pid);
                    }
                }
            }
        }
    }

    pub fn find_local_port_process_names(ports: &[u16]) -> Vec<String> {
        if ports.is_empty() {
            return Vec::new();
        }
        let target_ports: HashSet<u16> = ports.iter().copied().collect();
        let mut pids = HashSet::new();
        collect_pids_for_af(AF_INET, &target_ports, &mut pids);
        collect_pids_for_af(AF_INET6, &target_ports, &mut pids);

        let mut names = Vec::new();
        let mut seen_lower = HashSet::new();
        for pid in pids {
            if let Some(exe) = process_exe_name(pid) {
                let lower = exe.to_ascii_lowercase();
                if seen_lower.insert(lower) {
                    names.push(exe);
                }
            }
        }
        names
    }

    fn is_virtual_adapter_name(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        [
            "routeisle",
            "wintun",
            "tailscale",
            "clash",
            "mihomo",
            "vethernet",
            "loopback",
            "vmware",
            "virtualbox",
            "tap-",
            "zerotier",
            "hyper-v",
            "wsl",
            "wireguard",
        ]
        .iter()
        .any(|kw| lower.contains(kw))
    }

    pub fn detect_physical_interface() -> String {
        let mut size: u32 = 0;
        unsafe {
            let res = GetIpForwardTable(std::ptr::null_mut(), &mut size, 1);
            if res != ERROR_INSUFFICIENT_BUFFER || size < 4 {
                return "WLAN".into();
            }
            let mut buf = vec![0u8; size as usize];
            if GetIpForwardTable(buf.as_mut_ptr(), &mut size, 1) != 0 {
                return "WLAN".into();
            }
            let count = u32::from_ne_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;
            let data = &buf[4..];
            let row_size = std::mem::size_of::<MibIpForwardRow>();
            let mut candidates: Vec<(u32, u32)> = Vec::new();
            for i in 0..count {
                if (i + 1) * row_size > data.len() {
                    break;
                }
                let row = std::ptr::read_unaligned(
                    data.as_ptr().add(i * row_size) as *const MibIpForwardRow
                );
                if row.dw_forward_dest == 0 && row.dw_forward_mask == 0 {
                    candidates.push((row.dw_forward_metric1, row.dw_forward_if_index));
                }
            }
            candidates.sort_by_key(|(metric, _)| *metric);
            for (_, if_index) in candidates {
                let mut luid: u64 = 0;
                if ConvertInterfaceIndexToLuid(if_index, &mut luid) != 0 {
                    continue;
                }
                let mut alias = [0u16; 257];
                if ConvertInterfaceLuidToAlias(&luid, alias.as_mut_ptr(), alias.len()) != 0 {
                    continue;
                }
                let end = alias.iter().position(|&c| c == 0).unwrap_or(alias.len());
                let name = String::from_utf16_lossy(&alias[..end]).trim().to_string();
                if !name.is_empty() && !is_virtual_adapter_name(&name) {
                    return name;
                }
            }
        }
        "WLAN".into()
    }
}

#[cfg(not(target_os = "windows"))]
mod win_net {
    pub fn find_local_port_process_names(_ports: &[u16]) -> Vec<String> {
        Vec::new()
    }
    pub fn detect_physical_interface() -> String {
        "WLAN".into()
    }
}

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
    win_net::find_local_port_process_names(ports)
}

fn domain_to_rule_and_dns(domain: &str, outbound: &str) -> (String, String) {
    if let Some(suffix) = domain.strip_prefix("*.") {
        if !suffix.is_empty() && !suffix.contains('*') && !suffix.contains('?') {
            return (
                format!("DOMAIN-SUFFIX,{suffix},{outbound}"),
                format!("+.{suffix}"),
            );
        }
    }
    if domain.contains('*') || domain.contains('?') {
        (
            format!("DOMAIN-WILDCARD,{domain},{outbound}"),
            domain.to_string(),
        )
    } else {
        (format!("DOMAIN,{domain},{outbound}"), domain.to_string())
    }
}

pub fn build_mihomo_config(payload: &RulesPayload) -> Result<Value, String> {
    let physical_iface = win_net::detect_physical_interface();
    let mut proxies = vec![json!({
        "name": "PhysicalDirect",
        "type": "direct",
        "interface-name": physical_iface
    })];

    let mut local_upstream_ports = Vec::new();
    let mut probe_targets: Vec<(String, String, u16, bool)> = Vec::new();

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
        let is_fallback = upstream.id == payload.fallback;
        probe_targets.push((outbound_name, host, port, is_fallback));
    }

    let reachability: Vec<bool> = thread::scope(|s| {
        let handles: Vec<_> = probe_targets
            .iter()
            .map(|(_, host, port, _)| s.spawn(|| is_tcp_reachable(host, *port, 250)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(false))
            .collect()
    });

    let mut reachable_doh_outbound: Option<String> = None;
    for ((outbound_name, _, _, is_fallback), ok) in probe_targets.iter().zip(reachability.iter()) {
        if *ok {
            if *is_fallback {
                reachable_doh_outbound = Some(outbound_name.clone());
                break;
            }
            if reachable_doh_outbound.is_none() {
                reachable_doh_outbound = Some(outbound_name.clone());
            }
        }
    }

    let mut dns_policy = Map::new();
    let mut domain_entries: Vec<(String, String)> = Vec::new();
    let mut seen_domains = HashMap::new();
    let bypass_list = split_domains(&payload.bypass_domains);

    for domain in &bypass_list {
        if seen_domains.contains_key(domain) {
            continue;
        }
        seen_domains.insert(domain.clone(), true);
        let (rule_line, _) = domain_to_rule_and_dns(domain, "DIRECT");
        domain_entries.push((domain.clone(), rule_line));
    }

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
            let (rule_line, dns_key) = domain_to_rule_and_dns(&domain, &outbound);
            if group.target != "reject" {
                if let Some(ref doh_out) = reachable_doh_outbound {
                    dns_policy.insert(
                        dns_key,
                        json!([format!("https://dns.google/dns-query#{doh_out}")]),
                    );
                }
            }
            domain_entries.push((domain, rule_line));
        }
    }

    domain_entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut rules: Vec<String> = Vec::new();
    if payload.tun_enabled {
        for proc_name in find_local_port_process_names(&local_upstream_ports) {
            rules.push(format!("PROCESS-NAME,{proc_name},PhysicalDirect"));
        }
        for cidr in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "127.0.0.0/8",
            "169.254.0.0/16",
            "224.0.0.0/4",
            "223.5.5.5/32",
            "119.29.29.29/32",
            "fc00::/7",
            "fe80::/10",
        ] {
            rules.push(format!("IP-CIDR,{cidr},DIRECT,no-resolve"));
        }
    }
    rules.extend(domain_entries.into_iter().map(|(_, line)| line));

    let cn_outbound = target_to_outbound(&payload.cn_fallback, &payload.upstreams);
    let overseas_outbound = target_to_outbound(&payload.fallback, &payload.upstreams);
    if cn_outbound != overseas_outbound {
        rules.push(format!("GEOIP,CN,{cn_outbound}"));
    }
    rules.push(format!("MATCH,{overseas_outbound}"));

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
            "fake-ip-filter": bypass_list,
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
            "override-destination": true,
            "sniff": {
                "HTTP": { "ports": [80, "8080-8880"], "override-destination": true },
                "TLS": { "ports": [443, 8443] },
                "QUIC": { "ports": [443] }
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
    let (head, resp_body) = response.split_once("\r\n\r\n").ok_or("Invalid HTTP response")?;
    let status_code = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    if !(200..300).contains(&status_code) {
        return Err(format!("Controller HTTP {status_code}: {resp_body}"));
    }
    Ok(resp_body.to_string())
}
