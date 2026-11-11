use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

pub const PROXY_PORT: u16 = 17890;
pub const CONTROLLER_PORT: u16 = 17891;
pub const CONTROLLER_SECRET: &str = "routeisle-local-secret";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleGroup {
    pub id: String,
    pub name: String,
    pub domains: String,
    pub target: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Upstream {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControllerInfo {
    pub host: String,
    pub port: u16,
    pub secret: String,
}

impl Default for ControllerInfo {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: CONTROLLER_PORT,
            secret: CONTROLLER_SECRET.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesPayload {
    pub groups: Vec<RuleGroup>,
    pub upstreams: Vec<Upstream>,
    pub fallback: String,
    #[serde(default = "default_enabled")]
    pub system_proxy_enabled: bool,
    #[serde(default)]
    pub tun_enabled: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub controller: ControllerInfo,
}

impl Default for RulesPayload {
    fn default() -> Self {
        Self {
            groups: vec![
                RuleGroup {
                    id: "system".into(),
                    name: "内网服务".into(),
                    domains: "intranet.example.com".into(),
                    target: "system".into(),
                    enabled: true,
                },
                RuleGroup {
                    id: "direct".into(),
                    name: "WLAN 直连".into(),
                    domains: "direct.example.com\nippure.com\nipv6.icanhazip.com\napi.ipify.org".into(),
                    target: "wlan".into(),
                    enabled: true,
                },
            ],
            upstreams: vec![Upstream {
                id: "flyingbird".into(),
                name: "飞鸟".into(),
                protocol: "http".into(),
                address: "127.0.0.1:7890".into(),
            }],
            fallback: "flyingbird".into(),
            system_proxy_enabled: true,
            tun_enabled: false,
            running: false,
            auto_start: false,
            controller: ControllerInfo::default(),
        }
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn split_domains(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in text.split(|c: char| c.is_whitespace() || c == ',') {
        let trimmed = part.trim().trim_start_matches('.').to_lowercase();
        if !trimmed.is_empty() && !out.contains(&trimmed) {
            out.push(trimmed);
        }
    }
    out
}

pub fn project_root(app: &AppHandle) -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("Start-Prototype.ps1").exists() || cwd.join("package.json").exists() {
            return cwd;
        }
        if let Some(parent) = cwd.parent() {
            if parent.join("Start-Prototype.ps1").exists() || parent.join("package.json").exists() {
                return parent.to_path_buf();
            }
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        if res.exists() {
            return res;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn rules_file_path(app: &AppHandle) -> PathBuf {
    project_root(app).join("proxy-rules.json")
}

pub fn resolve_core_path(app: &AppHandle) -> Result<PathBuf, String> {
    let root = project_root(app);
    let rules_path = root.join("proxy-rules.json");
    if let Ok(raw) = fs::read_to_string(&rules_path) {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            if let Some(core_str) = val.get("Core").and_then(Value::as_str) {
                let candidate = PathBuf::from(core_str);
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }
    let mut candidates = vec![root.join("ApexCore.exe")];
    if let Ok(res_dir) = app.path().resource_dir() {
        candidates.push(res_dir.join("ApexCore.exe"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ApexCore.exe"));
        }
    }
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("未找到 ApexCore.exe 内核文件".into())
}

pub fn load_rules_from_disk(app: &AppHandle) -> RulesPayload {
    let path = rules_file_path(app);
    if let Ok(raw) = fs::read_to_string(&path) {
        if let Ok(mut payload) = serde_json::from_str::<RulesPayload>(&raw) {
            if !payload.groups.is_empty() {
                payload.controller = ControllerInfo::default();
                return payload;
            }
        }
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            let mut payload = RulesPayload::default();
            if let Some(sys) = val.get("SystemRoute").and_then(Value::as_array) {
                let domains: Vec<String> = sys
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if let Some(g) = payload.groups.iter_mut().find(|g| g.id == "system") {
                    g.domains = domains.join("\n");
                }
            }
            if let Some(dir) = val.get("PhysicalDirect").and_then(Value::as_array) {
                let domains: Vec<String> = dir
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if let Some(g) = payload.groups.iter_mut().find(|g| g.id == "direct") {
                    g.domains = domains.join("\n");
                }
            }
            return payload;
        }
    }
    RulesPayload::default()
}

pub fn save_rules_to_disk(app: &AppHandle, rules: &RulesPayload) -> Result<(), String> {
    let path = rules_file_path(app);
    let mut map: Map<String, Value> = if let Ok(raw) = fs::read_to_string(&path) {
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        Map::new()
    };

    let mut system_route = Vec::new();
    let mut physical_direct = Vec::new();
    for group in &rules.groups {
        if !group.enabled {
            continue;
        }
        for domain in split_domains(&group.domains) {
            if group.target == "system" && !system_route.contains(&domain) {
                system_route.push(domain);
            } else if group.target == "wlan" && !physical_direct.contains(&domain) {
                physical_direct.push(domain);
            }
        }
    }

    map.insert("SystemRoute".into(), json!(system_route));
    map.insert("PhysicalDirect".into(), json!(physical_direct));
    map.insert(
        "groups".into(),
        serde_json::to_value(&rules.groups).map_err(|e| e.to_string())?,
    );
    map.insert(
        "upstreams".into(),
        serde_json::to_value(&rules.upstreams).map_err(|e| e.to_string())?,
    );
    map.insert("fallback".into(), json!(rules.fallback));
    map.insert("systemProxyEnabled".into(), json!(rules.system_proxy_enabled));
    map.insert("tunEnabled".into(), json!(rules.tun_enabled));

    let serialized = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    fs::write(&path, serialized).map_err(|e| e.to_string())
}
