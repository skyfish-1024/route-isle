use crate::{
    config::{build_mihomo_config, controller_request, is_tcp_reachable},
    models::{
        load_rules_from_disk, now_millis, resolve_core_path, save_rules_to_disk, RulesPayload,
        CONTROLLER_PORT, PROXY_PORT,
    },
    sysproxy::{enable_system_proxy, restore_system_proxy},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
use crate::models::CREATE_NO_WINDOW;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
unsafe extern "system" {
    fn IsUserAnAdmin() -> i32;
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
    fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
    fn CloseHandle(handle: isize) -> i32;
}

static PENDING_AUTO_START: AtomicBool = AtomicBool::new(false);
const ELEVATED_TASK_NAME: &str = "RouteIsleElevated";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevateHandoff {
    pid: u32,
    auto_start: bool,
    ts: u64,
}

fn handoff_file_path() -> PathBuf {
    std::env::temp_dir().join("routeisle-elevate-handoff.json")
}

fn current_exe_and_cwd() -> Result<(PathBuf, PathBuf), String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("无法定位程序路径：{e}"))?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| {
        exe_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });
    Ok((exe_path, cwd))
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(unescape_xml(xml[start..end].trim()))
}

fn normalize_win_path(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn elevated_task_matches(exe_path: &Path, cwd: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("schtasks.exe");
        cmd.args(["/Query", "/TN", ELEVATED_TASK_NAME, "/XML"]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let Ok(out) = cmd.output() else {
            return false;
        };
        if !out.status.success() {
            return false;
        }
        let xml = String::from_utf8_lossy(&out.stdout);
        if !xml.contains("<RunLevel>HighestAvailable</RunLevel>") {
            return false;
        }
        let Some(task_cmd) = extract_xml_tag(&xml, "Command") else {
            return false;
        };
        let Some(task_cwd) = extract_xml_tag(&xml, "WorkingDirectory") else {
            return false;
        };
        normalize_win_path(&task_cmd) == normalize_win_path(&exe_path.to_string_lossy())
            && normalize_win_path(&task_cwd) == normalize_win_path(&cwd.to_string_lossy())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (exe_path, cwd);
        false
    }
}

fn ensure_elevated_task_registered() {
    #[cfg(target_os = "windows")]
    {
        if !is_current_process_admin() {
            return;
        }
        let Ok((exe_path, cwd)) = current_exe_and_cwd() else {
            return;
        };
        if elevated_task_matches(&exe_path, &cwd) {
            return;
        }
        let exe_escaped = exe_path.to_string_lossy().replace('\'', "''");
        let cwd_escaped = cwd.to_string_lossy().replace('\'', "''");
        let ps_script = format!(
            "$action = New-ScheduledTaskAction -Execute '{exe_escaped}' -WorkingDirectory '{cwd_escaped}'; \
             $user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name; \
             $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest; \
             $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel; \
             Register-ScheduledTask -TaskName '{ELEVATED_TASK_NAME}' -Action $action -Principal $principal -Settings $settings -Force | Out-Null"
        );
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_script]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let _ = cmd.output();
    }
}

fn try_launch_via_scheduled_task(exe_path: &Path, cwd: &Path, auto_start: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        if !elevated_task_matches(exe_path, cwd) {
            return false;
        }
        let handoff_path = handoff_file_path();
        let handoff = ElevateHandoff {
            pid: std::process::id(),
            auto_start,
            ts: now_millis(),
        };
        let Ok(payload) = serde_json::to_string(&handoff) else {
            return false;
        };
        if fs::write(&handoff_path, payload).is_err() {
            return false;
        }
        let mut cmd = Command::new("schtasks.exe");
        cmd.args(["/Run", "/TN", ELEVATED_TASK_NAME]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);
        if !ok {
            let _ = fs::remove_file(&handoff_path);
        }
        ok
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (exe_path, cwd, auto_start);
        false
    }
}

pub fn init_from_args() {
    let mut args = std::env::args().skip(1);
    let mut wait_pid: Option<u32> = None;
    while let Some(arg) = args.next() {
        if arg == "--auto-start" {
            PENDING_AUTO_START.store(true, Ordering::SeqCst);
        } else if arg == "--elevated-from" {
            if let Some(pid_str) = args.next() {
                wait_pid = pid_str.parse::<u32>().ok();
            }
        }
    }

    let handoff_path = handoff_file_path();
    if let Ok(raw) = fs::read_to_string(&handoff_path) {
        let _ = fs::remove_file(&handoff_path);
        if let Ok(handoff) = serde_json::from_str::<ElevateHandoff>(&raw) {
            if now_millis().saturating_sub(handoff.ts) <= 15_000 {
                if wait_pid.is_none() {
                    wait_pid = Some(handoff.pid);
                }
                if handoff.auto_start {
                    PENDING_AUTO_START.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    if let Some(pid) = wait_pid {
        const SYNCHRONIZE: u32 = 0x0010_0000;
        unsafe {
            let handle = OpenProcess(SYNCHRONIZE, 0, pid);
            if handle != 0 {
                let _ = WaitForSingleObject(handle, 3000);
                let _ = CloseHandle(handle);
            }
        }
    }

    if is_current_process_admin() {
        thread::spawn(ensure_elevated_task_registered);
    }
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
    pub tun_enabled: bool,
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
        tun_enabled: rules.tun_enabled,
        stop_flag,
        run_dir,
        config_path,
    });
    true
}

fn relaunch_elevated(
    app: &AppHandle,
    rules: &RulesPayload,
    state: &ProxyManager,
    auto_start: bool,
) -> Result<(), String> {
    let (exe_path, cwd) = current_exe_and_cwd()?;

    // Save tunEnabled: true first so the newly elevated process picks it up immediately; roll back if UAC is canceled
    save_rules_to_disk(app, rules)?;

    if try_launch_via_scheduled_task(&exe_path, &cwd, auto_start) {
        stop_proxy_internal(app, state);
        app.exit(0);
        return Ok(());
    }

    let pid = std::process::id();
    let mut arg_list = vec![
        format!("'--elevated-from'"),
        format!("'{pid}'"),
    ];
    if auto_start {
        arg_list.push("'--auto-start'".into());
    }
    let launch_cmd = format!(
        "Start-Process -FilePath '{}' -WorkingDirectory '{}' -Verb RunAs -ArgumentList {}",
        exe_path.to_string_lossy().replace('\'', "''"),
        cwd.to_string_lossy().replace('\'', "''"),
        arg_list.join(",")
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
        let mut reverted = rules.clone();
        reverted.tun_enabled = false;
        let _ = save_rules_to_disk(app, &reverted);
        return Err("已取消管理员授权，开启 TUN 虚拟网卡需要管理员权限".into());
    }

    stop_proxy_internal(app, state);
    app.exit(0);
    Ok(())
}

fn load_rules_sync(app: &AppHandle, state: &ProxyManager) -> RulesPayload {
    let mut payload = load_rules_from_disk(app);
    payload.running = ensure_or_adopt_session(state, &payload);
    payload.auto_start = PENDING_AUTO_START.swap(false, Ordering::SeqCst);
    if !payload.running && !payload.auto_start {
        let _ = restore_system_proxy(app);
    }
    payload
}

#[tauri::command]
pub async fn load_rules(app: AppHandle) -> Result<RulesPayload, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ProxyManager>();
        load_rules_sync(&app, &state)
    })
    .await
    .map_err(|e| e.to_string())
}

fn save_rules_sync(
    app: &AppHandle,
    rules: RulesPayload,
    was_running: bool,
    state: &ProxyManager,
) -> Result<(), String> {
    if rules.tun_enabled && !is_current_process_admin() {
        return relaunch_elevated(app, &rules, state, was_running);
    }

    save_rules_to_disk(app, &rules)?;

    enum RunningAction {
        None,
        HotReload(PathBuf),
        RestartCore,
    }

    let action = {
        let guard = state.0.lock().expect("proxy lock");
        if let Some(ref session) = *guard {
            if session.tun_enabled != rules.tun_enabled {
                RunningAction::RestartCore
            } else {
                RunningAction::HotReload(session.config_path.clone())
            }
        } else {
            RunningAction::None
        }
    };

    match action {
        RunningAction::None => {}
        RunningAction::RestartCore => {
            start_proxy_sync(app, rules, state)?;
        }
        RunningAction::HotReload(config_path) => {
            if rules.system_proxy_enabled {
                enable_system_proxy(app)?;
            } else {
                restore_system_proxy(app)?;
            }
            if let Ok(config) = build_mihomo_config(&rules) {
                if let Ok(cfg_text) = serde_json::to_string_pretty(&config) {
                    if fs::write(&config_path, cfg_text).is_ok() {
                        let reload_body =
                            json!({ "path": config_path.to_string_lossy() }).to_string();
                        let _ = controller_request("PUT", "/configs?force=true", Some(&reload_body));
                    }
                }
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn save_rules(
    app: AppHandle,
    rules: RulesPayload,
    was_running: Option<bool>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ProxyManager>();
        save_rules_sync(&app, rules, was_running.unwrap_or(false), &state)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn start_proxy_sync(
    app: &AppHandle,
    rules: RulesPayload,
    state: &ProxyManager,
) -> Result<(), String> {
    if rules.tun_enabled && !is_current_process_admin() {
        return relaunch_elevated(app, &rules, state, true);
    }

    stop_proxy_internal(app, state);

    let core_path = resolve_core_path(app)?;
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
    let mut child = child_cmd
        .spawn()
        .map_err(|e| format!("启动内核失败：{e}"))?;

    let mut ready = false;
    for _ in 0..30 {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = fs::remove_dir_all(&run_dir);
            return Err(format!(
                "内核进程异常退出（{status}），请检查端口 {PROXY_PORT} 是否被占用"
            ));
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
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&run_dir);
        return Err(format!("等待本地代理端口 {PROXY_PORT} 就绪超时"));
    }

    if rules.system_proxy_enabled {
        if let Err(err) = enable_system_proxy(app) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(&run_dir);
            return Err(format!("接管系统代理失败：{err}"));
        }
    } else {
        let _ = restore_system_proxy(app);
    }

    let mut guard = state.0.lock().expect("proxy lock");
    *guard = Some(ProxySession {
        child: Some(child),
        tun_enabled: rules.tun_enabled,
        stop_flag,
        run_dir,
        config_path,
    });

    Ok(())
}

#[tauri::command]
pub async fn start_proxy(app: AppHandle, rules: RulesPayload) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ProxyManager>();
        start_proxy_sync(&app, rules, &state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn stop_proxy(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ProxyManager>();
        stop_proxy_internal(&app, &state);
    })
    .await
    .map_err(|e| e.to_string())
}
