use crate::{
    config::{build_mihomo_config, controller_request, is_tcp_reachable},
    models::{
        load_rules_from_disk, now_millis, resolve_core_path, save_rules_to_disk, RulesPayload,
        CONTROLLER_PORT, PROXY_PORT,
    },
    sysproxy::{enable_system_proxy, restore_system_proxy},
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, State};

#[cfg(target_os = "windows")]
use crate::models::CREATE_NO_WINDOW;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
unsafe extern "system" {
    fn IsUserAnAdmin() -> i32;
}

pub fn is_current_process_admin() -> bool {
    #[cfg(target_os = "windows")]
    {
        unsafe { IsUserAnAdmin() != 0 }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub struct ProxySession {
    pub child: Option<Child>,
    pub elevated: bool,
    pub stop_flag: PathBuf,
    pub run_dir: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Default)]
pub struct ProxyManager(pub Mutex<Option<ProxySession>>);

pub fn stop_proxy_internal(app: &AppHandle, state: &ProxyManager) {
    let mut guard = state.0.lock().expect("proxy lock");
    let _ = restore_system_proxy(app);
    if let Some(mut session) = guard.take() {
        let _ = fs::write(&session.stop_flag, "stop");
        if let Some(mut child) = session.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        } else {
            for _ in 0..15 {
                if !is_tcp_reachable("127.0.0.1", PROXY_PORT, 60) {
                    break;
                }
                thread::sleep(Duration::from_millis(80));
            }
        }
        let _ = fs::remove_dir_all(&session.run_dir);
    }
}

pub fn ensure_or_adopt_session(state: &ProxyManager, rules: &RulesPayload) -> bool {
    {
        let mut guard = state.0.lock().expect("proxy lock");
        if let Some(ref mut session) = *guard {
            if let Some(ref mut child) = session.child {
                if let Ok(Some(_)) = child.try_wait() {
                    *guard = None;
                    return false;
                }
            }
            if is_tcp_reachable("127.0.0.1", CONTROLLER_PORT, 100) {
                return true;
            }
            *guard = None;
            return false;
        }
    }

    if !is_tcp_reachable("127.0.0.1", PROXY_PORT, 80)
        || !is_tcp_reachable("127.0.0.1", CONTROLLER_PORT, 80)
    {
        return false;
    }

    let mut run_dir = std::env::temp_dir().join(format!("routeisle-{}", now_millis()));
    if let Ok(entries) = fs::read_dir(std::env::temp_dir()) {
        let mut best: Option<(SystemTime, PathBuf)> = None;
        for entry in entries.flatten() {
            let p = entry.path();
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("routeisle-")
                    && p.join("config.json").exists()
                    && !p.join("stop.flag").exists()
                {
                    let mtime = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(UNIX_EPOCH);
                    if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                        best = Some((mtime, p));
                    }
                }
            }
        }
        if let Some((_, dir)) = best {
            run_dir = dir;
        }
    }
    let _ = fs::create_dir_all(&run_dir);
    let config_path = run_dir.join("config.json");
    let stop_flag = run_dir.join("stop.flag");

    let mut guard = state.0.lock().expect("proxy lock");
    *guard = Some(ProxySession {
        child: None,
        elevated: rules.tun_enabled || is_current_process_admin(),
        stop_flag,
        run_dir,
        config_path,
    });
    true
}

#[tauri::command]
pub fn load_rules(app: AppHandle, state: State<'_, ProxyManager>) -> RulesPayload {
    let mut payload = load_rules_from_disk(&app);
    payload.running = ensure_or_adopt_session(&state, &payload);
    if !payload.running {
        let _ = restore_system_proxy(&app);
    }
    payload
}

#[tauri::command]
pub fn save_rules(
    app: AppHandle,
    rules: RulesPayload,
    state: State<'_, ProxyManager>,
) -> Result<(), String> {
    save_rules_to_disk(&app, &rules)?;

    let need_elevated_restart = {
        let guard = state.0.lock().expect("proxy lock");
        if let Some(ref session) = *guard {
            if rules.tun_enabled && !session.elevated {
                true
            } else {
                if let Ok(config) = build_mihomo_config(&rules) {
                    if let Ok(cfg_text) = serde_json::to_string_pretty(&config) {
                        if fs::write(&session.config_path, cfg_text).is_ok() {
                            let reload_body =
                                json!({ "path": session.config_path.to_string_lossy() }).to_string();
                            let _ = controller_request("PUT", "/configs?force=true", Some(&reload_body));
                        }
                    }
                }
                false
            }
        } else {
            false
        }
    };

    if need_elevated_restart {
        start_proxy(app, rules, state)?;
    }

    Ok(())
}

#[tauri::command]
pub fn start_proxy(
    app: AppHandle,
    rules: RulesPayload,
    state: State<'_, ProxyManager>,
) -> Result<(), String> {
    stop_proxy_internal(&app, &state);

    let core_path = resolve_core_path(&app)?;
    let config = build_mihomo_config(&rules)?;

    let run_dir = std::env::temp_dir().join(format!("routeisle-{}", now_millis()));
    fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;
    let config_path = run_dir.join("config.json");
    let stop_flag = run_dir.join("stop.flag");
    let config_str = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(&config_path, config_str).map_err(|e| e.to_string())?;

    let mut test_cmd = Command::new(&core_path);
    test_cmd
        .arg("-d")
        .arg(&run_dir)
        .arg("-f")
        .arg(&config_path)
        .arg("-t");
    #[cfg(target_os = "windows")]
    test_cmd.creation_flags(CREATE_NO_WINDOW);
    let test_out = test_cmd
        .output()
        .map_err(|e| format!("无法启动内核校验：{e}"))?;
    if !test_out.status.success() {
        let _ = fs::remove_dir_all(&run_dir);
        let msg = String::from_utf8_lossy(&test_out.stdout);
        return Err(format!("内核配置校验失败：{msg}"));
    }

    let already_admin = is_current_process_admin();
    let need_uac = rules.tun_enabled && !already_admin;
    let elevated = already_admin || need_uac;
    let mut child: Option<Child> = None;

    if need_uac {
        let guardian_path = run_dir.join("run-elevated.ps1");
        let guardian_script = format!(
            r#"$ErrorActionPreference = 'SilentlyContinue'
$core = '{core}'
$runDir = '{run_dir}'
$cfg = '{cfg}'
$stopFlag = '{stop_flag}'
$p = Start-Process -FilePath $core -ArgumentList "-d `"$runDir`" -f `"$cfg`"" -WindowStyle Hidden -PassThru
if (!$p) {{ exit 1 }}
$missingCount = 0
while (!$p.HasExited) {{
    if (Test-Path -LiteralPath $stopFlag) {{
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        break
    }}
    if (!(Get-Process -Name 'split-proxy-island' -ErrorAction SilentlyContinue)) {{
        $missingCount++
        if ($missingCount -ge 60) {{
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
            break
        }}
    }} else {{
        $missingCount = 0
    }}
    Start-Sleep -Milliseconds 200
}}
"#,
            core = core_path.to_string_lossy().replace('\'', "''"),
            run_dir = run_dir.to_string_lossy().replace('\'', "''"),
            cfg = config_path.to_string_lossy().replace('\'', "''"),
            stop_flag = stop_flag.to_string_lossy().replace('\'', "''"),
        );
        fs::write(&guardian_path, guardian_script).map_err(|e| e.to_string())?;

        let launch_cmd = format!(
            "Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-WindowStyle','Hidden','-File','{}'",
            guardian_path.to_string_lossy().replace('\'', "''")
        );
        let mut uac_cmd = Command::new("powershell.exe");
        uac_cmd.args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &launch_cmd,
        ]);
        #[cfg(target_os = "windows")]
        uac_cmd.creation_flags(CREATE_NO_WINDOW);
        let uac_out = uac_cmd
            .output()
            .map_err(|e| format!("请求管理员权限失败：{e}"))?;
        if !uac_out.status.success() {
            let _ = fs::remove_dir_all(&run_dir);
            return Err("已取消管理员授权，开启 TUN 虚拟网卡需要管理员权限".into());
        }
    } else {
        let mut child_cmd = Command::new(&core_path);
        child_cmd
            .arg("-d")
            .arg(&run_dir)
            .arg("-f")
            .arg(&config_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        child_cmd.creation_flags(CREATE_NO_WINDOW);
        child = Some(
            child_cmd
                .spawn()
                .map_err(|e| format!("启动内核失败：{e}"))?,
        );
    }

    let mut ready = false;
    let max_polls = if need_uac { 60 } else { 20 };
    for _ in 0..max_polls {
        if let Some(ref mut c) = child {
            if let Ok(Some(status)) = c.try_wait() {
                let _ = fs::remove_dir_all(&run_dir);
                return Err(format!(
                    "内核进程异常退出（{status}），请检查端口 {PROXY_PORT} 是否被占用"
                ));
            }
        }
        if is_tcp_reachable("127.0.0.1", PROXY_PORT, 100)
            && is_tcp_reachable("127.0.0.1", CONTROLLER_PORT, 100)
        {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        let _ = fs::write(&stop_flag, "stop");
        if let Some(mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = fs::remove_dir_all(&run_dir);
        return Err(format!("等待本地代理端口 {PROXY_PORT} 就绪超时"));
    }

    if let Err(err) = enable_system_proxy(&app) {
        let _ = fs::write(&stop_flag, "stop");
        if let Some(mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = fs::remove_dir_all(&run_dir);
        return Err(format!("接管系统代理失败：{err}"));
    }

    let mut guard = state.0.lock().expect("proxy lock");
    *guard = Some(ProxySession {
        child,
        elevated,
        stop_flag,
        run_dir,
        config_path,
    });

    Ok(())
}

#[tauri::command]
pub fn stop_proxy(app: AppHandle, state: State<'_, ProxyManager>) -> Result<(), String> {
    stop_proxy_internal(&app, &state);
    Ok(())
}
