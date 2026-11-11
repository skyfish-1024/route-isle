use crate::models::{project_root, PROXY_PORT};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use sysproxy::Sysproxy;
use tauri::AppHandle;

const DEFAULT_BYPASS: &str = "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SysproxyBackup {
    enable: bool,
    host: String,
    port: u16,
    bypass: String,
}

fn backup_file_path(app: &AppHandle) -> PathBuf {
    project_root(app).join(".sysproxy-backup.json")
}

fn read_current_sysproxy() -> Sysproxy {
    Sysproxy::get_system_proxy().unwrap_or_else(|_| Sysproxy {
        enable: false,
        host: "127.0.0.1".into(),
        port: 7890,
        bypass: DEFAULT_BYPASS.into(),
    })
}

pub fn enable_system_proxy(app: &AppHandle) -> Result<(), String> {
    if !Sysproxy::is_support() {
        return Ok(());
    }

    let current = read_current_sysproxy();
    let backup_path = backup_file_path(app);
    let already_pointing_to_self =
        current.host == "127.0.0.1" && current.port == PROXY_PORT;

    if !already_pointing_to_self || !backup_path.exists() {
        let backup = SysproxyBackup {
            enable: current.enable,
            host: if already_pointing_to_self {
                "127.0.0.1".into()
            } else {
                current.host.clone()
            },
            port: if already_pointing_to_self {
                7890
            } else {
                current.port
            },
            bypass: if current.bypass.trim().is_empty() {
                DEFAULT_BYPASS.into()
            } else {
                current.bypass.clone()
            },
        };
        if let Ok(raw) = serde_json::to_string_pretty(&backup) {
            let _ = fs::write(&backup_path, raw);
        }
    }

    let target = Sysproxy {
        enable: true,
        host: "127.0.0.1".into(),
        port: PROXY_PORT,
        bypass: if current.bypass.trim().is_empty() {
            DEFAULT_BYPASS.into()
        } else {
            current.bypass
        },
    };
    target
        .set_system_proxy()
        .map_err(|e| format!("设置系统代理失败：{e}"))
}

pub fn restore_system_proxy(app: &AppHandle) -> Result<(), String> {
    if !Sysproxy::is_support() {
        return Ok(());
    }

    let backup_path = backup_file_path(app);
    if let Ok(raw) = fs::read_to_string(&backup_path) {
        if let Ok(backup) = serde_json::from_str::<SysproxyBackup>(&raw) {
            let restored = Sysproxy {
                enable: backup.enable,
                host: if backup.host.trim().is_empty() {
                    "127.0.0.1".into()
                } else {
                    backup.host
                },
                port: if backup.port == 0 { 7890 } else { backup.port },
                bypass: if backup.bypass.trim().is_empty() {
                    DEFAULT_BYPASS.into()
                } else {
                    backup.bypass
                },
            };
            let _ = restored.set_system_proxy();
            let _ = fs::remove_file(&backup_path);
            return Ok(());
        }
    }

    let current = read_current_sysproxy();
    if current.enable && current.host == "127.0.0.1" && current.port == PROXY_PORT {
        let off = Sysproxy {
            enable: false,
            host: current.host,
            port: current.port,
            bypass: current.bypass,
        };
        let _ = off.set_system_proxy();
    }
    Ok(())
}
