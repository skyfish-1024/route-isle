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

    pub fn process_exe_name_pub(pid: u32) -> Option<String> {
        process_exe_name(pid)
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

    pub fn list_network_interfaces() -> Vec<String> {
        let mut size: u32 = 0;
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        unsafe {
            let res = GetIpForwardTable(std::ptr::null_mut(), &mut size, 1);
            if res != ERROR_INSUFFICIENT_BUFFER || size < 4 {
                return names;
            }
            let mut buf = vec![0u8; size as usize];
            if GetIpForwardTable(buf.as_mut_ptr(), &mut size, 1) != 0 {
                return names;
            }
            let count = u32::from_ne_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;
            let data = &buf[4..];
            let row_size = std::mem::size_of::<MibIpForwardRow>();
            for i in 0..count {
                if (i + 1) * row_size > data.len() {
                    break;
                }
                let row = std::ptr::read_unaligned(
                    data.as_ptr().add(i * row_size) as *const MibIpForwardRow
                );
                let mut luid: u64 = 0;
                if ConvertInterfaceIndexToLuid(row.dw_forward_if_index, &mut luid) != 0 {
                    continue;
                }
                let mut alias = [0u16; 257];
                if ConvertInterfaceLuidToAlias(&luid, alias.as_mut_ptr(), alias.len()) != 0 {
                    continue;
                }
                let end = alias.iter().position(|&c| c == 0).unwrap_or(alias.len());
                let name = String::from_utf16_lossy(&alias[..end]).trim().to_string();
                let lower = name.to_ascii_lowercase();
                let is_noise_vm = [
                    "loopback",
                    "routeisle",
                    "vethernet",
                    "hyper-v",
                    "wsl",
                    "vmware",
                    "virtualbox",
                    "docker",
                ]
                .iter()
                .any(|kw| lower.contains(kw));
                if !name.is_empty() && !is_noise_vm && seen.insert(lower) {
                    names.push(name);
                }
            }
        }
        names.sort_by_key(|name| {
            let lower = name.to_ascii_lowercase();
            let is_wg = lower.contains("wireguard") || lower.starts_with("wg");
            (!is_wg, lower)
        });
        names
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
    pub fn list_network_interfaces() -> Vec<String> {
        Vec::new()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProxy {
    pub name: String,
    pub protocol: String,
    pub address: String,
    pub process_name: String,
    pub port: u16,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemUpstreamScan {
    pub proxies: Vec<DetectedProxy>,
    pub interfaces: Vec<String>,
}

#[cfg(target_os = "windows")]
mod win_net_scan {
    use super::*;

    const COMMON_PROXY_PORTS: &[u16] = &[
        7890, 7891, 7897, 1080, 1081, 10800, 10808, 10809, 2080, 2081, 8080, 8888, 8889, 9090,
        10810, 10811,
    ];
    const SOCKS_PORTS: &[u16] = &[1080, 1081, 10800, 10808, 2080, 10810];
    const PROXY_PROC_KEYWORDS: &[&str] = &[
        "apexcore",
        "clash",
        "mihomo",
        "v2ray",
        "xray",
        "sing-box",
        "hysteria",
        "trojan",
        "ss-local",
        "shadowsocks",
        "naive",
        "nekoray",
        "nekobox",
        "flclash",
        "verge",
        "privoxy",
        "flyingbird",
        "feiniao",
    ];

    fn is_proxy_process(exe: &str) -> bool {
        let lower = exe.to_ascii_lowercase();
        PROXY_PROC_KEYWORDS.iter().any(|kw| lower.contains(kw))
    }

    fn friendly_name_from_exe(exe: &str, port: u16) -> String {
        let stem = exe
            .strip_suffix(".exe")
            .or_else(|| exe.strip_suffix(".EXE"))
            .unwrap_or(exe)
            .trim();
        if stem.is_empty() {
            format!("本地代理 :{port}")
        } else {
            stem.to_string()
        }
    }

    fn collect_listeners_for_af(af: u32, found: &mut Vec<(u16, u32)>) {
        const AF_INET: u32 = 2;
        const AF_INET6: u32 = 23;
        const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
        const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

        #[repr(C)]
        struct MibTcpRow {
            dw_state: u32,
            dw_local_addr: u32,
            dw_local_port: u32,
            dw_remote_addr: u32,
            dw_remote_port: u32,
            dw_owning_pid: u32,
        }

        #[repr(C)]
        struct MibTcp6Row {
            uc_local_addr: [u8; 16],
            dw_local_scope_id: u32,
            dw_local_port: u32,
            uc_remote_addr: [u8; 16],
            dw_remote_scope_id: u32,
            dw_remote_port: u32,
            dw_state: u32,
            dw_owning_pid: u32,
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
        }

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
                let row_size = std::mem::size_of::<MibTcpRow>();
                for i in 0..count {
                    if (i + 1) * row_size > data.len() {
                        break;
                    }
                    let row = std::ptr::read_unaligned(data.as_ptr().add(i * row_size) as *const MibTcpRow);
                    let port = u16::from_be((row.dw_local_port & 0xFFFF) as u16);
                    found.push((port, row.dw_owning_pid));
                }
            } else if af == AF_INET6 {
                let row_size = std::mem::size_of::<MibTcp6Row>();
                for i in 0..count {
                    if (i + 1) * row_size > data.len() {
                        break;
                    }
                    let row = std::ptr::read_unaligned(data.as_ptr().add(i * row_size) as *const MibTcp6Row);
                    let port = u16::from_be((row.dw_local_port & 0xFFFF) as u16);
                    found.push((port, row.dw_owning_pid));
                }
            }
        }
    }

    pub fn scan_proxies() -> Vec<DetectedProxy> {
        let mut raw = Vec::new();
        collect_listeners_for_af(2, &mut raw);
        collect_listeners_for_af(23, &mut raw);

        let self_pid = std::process::id();
        let mut seen_ports = HashSet::new();
        let mut proxies = Vec::new();

        for (port, pid) in raw {
            if port == 0 || port == PROXY_PORT || port == CONTROLLER_PORT || pid == self_pid {
                continue;
            }
            let exe = win_net::process_exe_name_pub(pid).unwrap_or_default();
            let known_port = COMMON_PROXY_PORTS.contains(&port);
            let known_proc = !exe.is_empty() && is_proxy_process(&exe);
            if !known_port && !known_proc {
                continue;
            }
            if !seen_ports.insert(port) {
                continue;
            }
            let protocol = if SOCKS_PORTS.contains(&port) {
                "socks5"
            } else {
                "http"
            };
            proxies.push(DetectedProxy {
                name: friendly_name_from_exe(&exe, port),
                protocol: protocol.to_string(),
                address: format!("127.0.0.1:{port}"),
                process_name: exe,
                port,
            });
        }

        proxies.sort_by_key(|p| {
            let is_known_proc = !p.process_name.is_empty() && is_proxy_process(&p.process_name);
            (!is_known_proc, p.port)
        });
        proxies
    }
}

pub fn scan_system_upstreams() -> SystemUpstreamScan {
    #[cfg(target_os = "windows")]
    let proxies = win_net_scan::scan_proxies();
    #[cfg(not(target_os = "windows"))]
    let proxies = Vec::new();

    SystemUpstreamScan {
        proxies,
        interfaces: list_network_interfaces(),
    }
}

pub fn list_network_interfaces() -> Vec<String> {
    win_net::list_network_interfaces()
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

fn parse_process_rule(raw: &str) -> Option<&str> {
    let exe = raw
        .strip_prefix("proc:")
        .or_else(|| raw.strip_prefix("process:"))?
        .trim();
    if exe.is_empty() {
        None
    } else {
        Some(exe)
    }
}

fn parse_port_rule(raw: &str) -> Option<String> {
    let expr = raw.strip_prefix("port:")?.trim();
    if expr.is_empty() {
        return None;
    }
    if let Some((start_str, end_str)) = expr.split_once('-') {
        let start: u16 = start_str.trim().parse().ok()?;
        let end: u16 = end_str.trim().parse().ok()?;
        if start == 0 || start > end {
            return None;
        }
        Some(format!("{start}-{end}"))
    } else {
        let port: u16 = expr.parse().ok()?;
        if port == 0 {
            return None;
        }
        Some(port.to_string())
    }
}

fn parse_ip_cidr(raw: &str) -> Option<(&'static str, String)> {
    let (addr_part, prefix_part) = match raw.split_once('/') {
        Some((a, p)) => (a.trim(), Some(p.trim())),
        None => (raw.trim(), None),
    };
    let ip: std::net::IpAddr = addr_part.parse().ok()?;
    match (ip, prefix_part) {
        (std::net::IpAddr::V4(v4), Some(p)) => {
            let prefix: u8 = p.parse().ok()?;
            if prefix <= 32 {
                Some(("IP-CIDR", format!("{v4}/{prefix}")))
            } else {
                None
            }
        }
        (std::net::IpAddr::V4(v4), None) => Some(("IP-CIDR", format!("{v4}/32"))),
        (std::net::IpAddr::V6(v6), Some(p)) => {
            let prefix: u8 = p.parse().ok()?;
            if prefix <= 128 {
                Some(("IP-CIDR6", format!("{v6}/{prefix}")))
            } else {
                None
            }
        }
        (std::net::IpAddr::V6(v6), None) => Some(("IP-CIDR6", format!("{v6}/128"))),
    }
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

struct ParsedWireGuard {
    proxy: Value,
    dns_servers: Vec<String>,
    full_tunnel: bool,
    endpoint_domain: Option<String>,
    allowed_cidrs: Vec<(&'static str, String)>,
}

fn parse_wireguard_conf(
    outbound_name: &str,
    label: &str,
    conf_text: &str,
    physical_iface: &str,
) -> Result<ParsedWireGuard, String> {
    let mut section = "";
    let mut private_key = String::new();
    let mut addresses: Vec<String> = Vec::new();
    let mut dns_servers: Vec<String> = Vec::new();
    let mut mtu: Option<u16> = None;
    let mut public_key = String::new();
    let mut preshared_key = String::new();
    let mut endpoint = String::new();
    let mut full_tunnel = false;
    let mut allowed_cidrs: Vec<(&'static str, String)> = Vec::new();

    for raw_line in conf_text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = match line[1..line.len() - 1].trim().to_ascii_lowercase().as_str() {
                "interface" => "interface",
                "peer" => "peer",
                _ => "",
            };
            continue;
        }
        let Some((raw_key, raw_val)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim().to_ascii_lowercase();
        let val = raw_val.trim();
        match (section, key.as_str()) {
            ("interface", "privatekey") => private_key = val.to_string(),
            ("interface", "address") => {
                for item in val.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let ip_only = item.split('/').next().unwrap_or(item).trim();
                    if !ip_only.is_empty() {
                        addresses.push(ip_only.to_string());
                    }
                }
            }
            ("interface", "dns") => {
                for item in val.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    dns_servers.push(item.to_string());
                }
            }
            ("interface", "mtu") => {
                if let Ok(parsed) = val.parse::<u16>() {
                    mtu = Some(parsed);
                }
            }
            ("peer", "publickey") if public_key.is_empty() => public_key = val.to_string(),
            ("peer", "presharedkey") if preshared_key.is_empty() => preshared_key = val.to_string(),
            ("peer", "endpoint") if endpoint.is_empty() => endpoint = val.to_string(),
            ("peer", "allowedips") => {
                for item in val.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    if item == "0.0.0.0/0" || item == "::/0" {
                        full_tunnel = true;
                    } else if let Some(parsed) = parse_ip_cidr(item) {
                        if !allowed_cidrs.contains(&parsed) {
                            allowed_cidrs.push(parsed);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if private_key.is_empty() {
        return Err(format!("WireGuard「{label}」缺少 PrivateKey"));
    }
    if public_key.is_empty() {
        return Err(format!("WireGuard「{label}」缺少 Peer PublicKey"));
    }
    if endpoint.is_empty() {
        return Err(format!("WireGuard「{label}」缺少 Peer Endpoint"));
    }
    let (host, port) = parse_host_port(&endpoint)?;
    let endpoint_domain = if host.parse::<std::net::IpAddr>().is_err() {
        Some(host.clone())
    } else {
        None
    };
    let ipv4 = addresses.iter().find(|ip| !ip.contains(':')).cloned();
    let ipv6 = addresses.iter().find(|ip| ip.contains(':')).cloned();
    if ipv4.is_none() && ipv6.is_none() {
        return Err(format!("WireGuard「{label}」缺少 Interface Address"));
    }

    let mut proxy = Map::new();
    proxy.insert("name".into(), json!(outbound_name));
    proxy.insert("type".into(), json!("wireguard"));
    proxy.insert("server".into(), json!(host));
    proxy.insert("port".into(), json!(port));
    proxy.insert("private-key".into(), json!(private_key));
    proxy.insert("public-key".into(), json!(public_key));
    proxy.insert("udp".into(), json!(true));
    proxy.insert("remote-dns-resolve".into(), json!(full_tunnel || !dns_servers.is_empty()));
    if !physical_iface.trim().is_empty() {
        proxy.insert("interface-name".into(), json!(physical_iface.trim()));
    }
    if let Some(v4) = ipv4 {
        proxy.insert("ip".into(), json!(v4));
    }
    if let Some(v6) = ipv6 {
        proxy.insert("ipv6".into(), json!(v6));
    }
    if !preshared_key.is_empty() {
        proxy.insert("pre-shared-key".into(), json!(preshared_key));
    }
    if let Some(m) = mtu {
        proxy.insert("mtu".into(), json!(m));
    }
    if !dns_servers.is_empty() {
        proxy.insert("dns".into(), json!(dns_servers));
    }

    Ok(ParsedWireGuard {
        proxy: Value::Object(proxy),
        dns_servers,
        full_tunnel,
        endpoint_domain,
        allowed_cidrs,
    })
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
    let mut interface_outbounds = HashSet::new();
    let mut wireguard_meta: HashMap<String, (Vec<String>, bool)> = HashMap::new();
    let mut wg_endpoint_domains: Vec<String> = Vec::new();
    let mut wg_allowed_cidrs: Vec<(String, &'static str, String)> = Vec::new();

    for upstream in &payload.upstreams {
        let outbound_name = outbound_name_for_upstream(&upstream.id);
        match upstream.protocol.to_lowercase().as_str() {
            "wireguard" => {
                let conf_text = upstream
                    .wireguard_conf
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("WireGuard「{}」配置内容为空", upstream.name))?;
                let parsed_wg =
                    parse_wireguard_conf(&outbound_name, &upstream.name, conf_text, &physical_iface)?;
                proxies.push(parsed_wg.proxy);
                wireguard_meta.insert(
                    outbound_name.clone(),
                    (parsed_wg.dns_servers, parsed_wg.full_tunnel),
                );
                if let Some(ep_dom) = parsed_wg.endpoint_domain {
                    if !wg_endpoint_domains.contains(&ep_dom) {
                        wg_endpoint_domains.push(ep_dom);
                    }
                }
                for (kind, cidr) in parsed_wg.allowed_cidrs {
                    wg_allowed_cidrs.push((outbound_name.clone(), kind, cidr));
                }
            }
            "interface" => {
                let iface = upstream.address.trim();
                if iface.is_empty() {
                    return Err(format!("网卡出口「{}」未指定网卡名称", upstream.name));
                }
                proxies.push(json!({
                    "name": outbound_name,
                    "type": "direct",
                    "interface-name": iface
                }));
                interface_outbounds.insert(outbound_name);
            }
            proto => {
                let (host, port) = parse_host_port(&upstream.address)?;
                if host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost") {
                    local_upstream_ports.push(port);
                }
                let protocol = match proto {
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
        }
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

    let mut reachable_outbounds = HashSet::new();
    let mut reachable_doh_outbound: Option<String> = None;
    for ((outbound_name, _, _, is_fallback), ok) in probe_targets.iter().zip(reachability.iter()) {
        if *ok {
            reachable_outbounds.insert(outbound_name.clone());
            if *is_fallback {
                reachable_doh_outbound = Some(outbound_name.clone());
            } else if reachable_doh_outbound.is_none() {
                reachable_doh_outbound = Some(outbound_name.clone());
            }
        }
    }

fn parse_dns_servers(raw: &str, defaults: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for part in raw.split(|c: char| c == '\n' || c == '\r' || c == ',' || c == ';' || c.is_whitespace()) {
        let trimmed = part.trim();
        if !trimmed.is_empty() && !out.iter().any(|item| item == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        defaults.iter().map(|s| (*s).to_string()).collect()
    } else {
        out
    }
}

fn with_dns_outbound(servers: &[String], outbound: &str) -> Vec<String> {
    servers
        .iter()
        .map(|server| {
            if server.contains('#') {
                server.clone()
            } else {
                format!("{server}#{outbound}")
            }
        })
        .collect()
}

fn plain_ip_dns(servers: &[String]) -> Vec<String> {
    let ips: Vec<String> = servers
        .iter()
        .filter(|s| {
            let base = s.split('#').next().unwrap_or(s).trim();
            base.parse::<std::net::IpAddr>().is_ok()
        })
        .map(|s| s.split('#').next().unwrap_or(s).trim().to_string())
        .collect();
    if ips.is_empty() {
        vec!["223.5.5.5".into(), "119.29.29.29".into()]
    } else {
        ips
    }
}

    let cn_dns_list = parse_dns_servers(&payload.cn_dns, &["223.5.5.5", "119.29.29.29"]);
    let overseas_dns_list = parse_dns_servers(&payload.overseas_dns, &["https://dns.google/dns-query"]);
    let wlan_dns = with_dns_outbound(&cn_dns_list, "PhysicalDirect");

    let mut dns_policy = Map::new();
    let mut domain_entries: Vec<(String, String)> = Vec::new();
    let mut group_proc_rules: Vec<String> = Vec::new();
    let mut group_ip_rules: Vec<String> = Vec::new();
    let mut group_port_rules: Vec<String> = Vec::new();
    let mut seen_domains = HashMap::new();
    let bypass_list = split_domains(&payload.bypass_domains);
    let mut fake_ip_filter = bypass_list.clone();
    for ep_dom in wg_endpoint_domains {
        if !fake_ip_filter.contains(&ep_dom) {
            fake_ip_filter.push(ep_dom);
        }
    }

    for domain in &bypass_list {
        if seen_domains.contains_key(domain) {
            continue;
        }
        seen_domains.insert(domain.clone(), true);
        if let Some(proc_name) = parse_process_rule(domain) {
            group_proc_rules.push(format!("PROCESS-NAME,{proc_name},DIRECT"));
            continue;
        }
        if let Some(port_expr) = parse_port_rule(domain) {
            group_port_rules.push(format!("DST-PORT,{port_expr},DIRECT"));
            continue;
        }
        if let Some((kind, cidr)) = parse_ip_cidr(domain) {
            group_ip_rules.push(format!("{kind},{cidr},DIRECT,no-resolve"));
            continue;
        }
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
            if let Some(proc_name) = parse_process_rule(&domain) {
                group_proc_rules.push(format!("PROCESS-NAME,{proc_name},{outbound}"));
                continue;
            }
            if let Some(port_expr) = parse_port_rule(&domain) {
                group_port_rules.push(format!("DST-PORT,{port_expr},{outbound}"));
                continue;
            }
            if let Some((kind, cidr)) = parse_ip_cidr(&domain) {
                group_ip_rules.push(format!("{kind},{cidr},{outbound},no-resolve"));
                continue;
            }
            let (rule_line, dns_key) = domain_to_rule_and_dns(&domain, &outbound);
            match group.target.as_str() {
                "reject" => {}
                "system" => {
                    dns_policy.insert(dns_key.clone(), json!(["system"]));
                    if !fake_ip_filter.contains(&dns_key) {
                        fake_ip_filter.push(dns_key);
                    }
                }
                "wlan" => {
                    dns_policy.insert(dns_key, json!(wlan_dns));
                }
                _ if interface_outbounds.contains(&outbound) => {
                    dns_policy.insert(dns_key.clone(), json!(["system"]));
                    if !fake_ip_filter.contains(&dns_key) {
                        fake_ip_filter.push(dns_key);
                    }
                }
                _ if wireguard_meta.contains_key(&outbound) => {
                    let (custom_dns, full_tunnel) = wireguard_meta
                        .get(&outbound)
                        .map(|(d, f)| (d.as_slice(), *f))
                        .unwrap_or((&[], false));
                    if !custom_dns.is_empty() {
                        let servers: Vec<String> = custom_dns
                            .iter()
                            .map(|ip| format!("{ip}#{outbound}"))
                            .collect();
                        dns_policy.insert(dns_key, json!(servers));
                    } else if full_tunnel {
                        dns_policy.insert(dns_key, json!(with_dns_outbound(&overseas_dns_list, &outbound)));
                    } else {
                        dns_policy.insert(dns_key.clone(), json!(["system"]));
                        if !fake_ip_filter.contains(&dns_key) {
                            fake_ip_filter.push(dns_key);
                        }
                    }
                }
                _ => {
                    let doh_out = if reachable_outbounds.contains(&outbound) {
                        Some(outbound.as_str())
                    } else {
                        reachable_doh_outbound.as_deref()
                    };
                    if let Some(doh_out) = doh_out {
                        dns_policy.insert(dns_key, json!(with_dns_outbound(&overseas_dns_list, doh_out)));
                    }
                }
            }
            domain_entries.push((domain, rule_line));
        }
    }

    let mut rules: Vec<String> = Vec::new();
    rules.extend(group_proc_rules);
    if payload.tun_enabled {
        for proc_name in find_local_port_process_names(&local_upstream_ports) {
            rules.push(format!("PROCESS-NAME,{proc_name},PhysicalDirect"));
        }
    }
    rules.extend(group_ip_rules);
    rules.extend(group_port_rules);
    for (wg_out, kind, cidr) in wg_allowed_cidrs {
        if !seen_domains.contains_key(&cidr) {
            seen_domains.insert(cidr.clone(), true);
            rules.push(format!("{kind},{cidr},{wg_out},no-resolve"));
        }
    }
    if payload.tun_enabled {
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
            nameservers.extend(with_dns_outbound(&overseas_dns_list, doh_out));
        }
        nameservers.extend(cn_dns_list.clone());
        json!({
            "enable": true,
            "enhanced-mode": "fake-ip",
            "fake-ip-range": "198.18.0.1/16",
            "fake-ip-filter": fake_ip_filter,
            "default-nameserver": plain_ip_dns(&cn_dns_list),
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
