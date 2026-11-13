use crate::{
    config::{build_mihomo_config, controller_request, is_tcp_reachable},
    models::{
        data_dir, load_rules_from_disk, now_millis, resolve_core_path, resolve_geoip_path,
        save_rules_to_disk, split_domains, RulesPayload, CONTROLLER_PORT, PROXY_PORT,
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
    fn TerminateProcess(process_handle: isize, exit_code: u32) -> i32;
    fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
    fn CloseHandle(handle: isize) -> i32;
    fn CreateMutexW(
        lp_mutex_attributes: *const std::ffi::c_void,
        b_initial_owner: i32,
        lp_name: *const u16,
    ) -> isize;
    fn CreateEventW(
        lp_event_attributes: *const std::ffi::c_void,
        b_manual_reset: i32,
        b_initial_state: i32,
        lp_name: *const u16,
    ) -> isize;
    fn OpenEventW(dw_desired_access: u32, b_inherit_handle: i32, lp_name: *const u16) -> isize;
    fn SetEvent(h_event: isize) -> i32;
    fn GetLastError() -> u32;
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
            if now_millis().saturating_sub(handoff.ts) <= 60_000 {
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

pub fn acquire_single_instance_lock() -> bool {
    #[cfg(target_os = "windows")]
    {
        const ERROR_ALREADY_EXISTS: u32 = 183;
        const EVENT_MODIFY_STATE: u32 = 0x0002;
        let name: Vec<u16> = "Local\\RouteIsleSingleInstanceMutex\0".encode_utf16().collect();
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if handle == 0 {
                return true;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                let wake_name: Vec<u16> = "Local\\RouteIsleWakeEvent\0".encode_utf16().collect();
                let ev = OpenEventW(EVENT_MODIFY_STATE, 0, wake_name.as_ptr());
                if ev != 0 {
                    let _ = SetEvent(ev);
                    let _ = CloseHandle(ev);
                }
                return false;
            }
            // Intentionally keep `handle` open for the process lifetime.
            true
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

pub fn spawn_wake_listener(app: AppHandle) {
    #[cfg(target_os = "windows")]
    thread::spawn(move || {
        const INFINITE: u32 = 0xFFFF_FFFF;
        let wake_name: Vec<u16> = "Local\\RouteIsleWakeEvent\0".encode_utf16().collect();
        let ev = unsafe { CreateEventW(std::ptr::null(), 0, 0, wake_name.as_ptr()) };
        if ev == 0 {
            return;
        }
        loop {
            let wait = unsafe { WaitForSingleObject(ev, INFINITE) };
            if wait != 0 {
                break;
            }
            if let Some(capsule) = app.get_webview_window("main") {
                let _ = capsule.show();
                let _ = capsule.set_focus();
            }
        }
    });
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
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
pub struct ProxyManager {
    pub session: Mutex<Option<ProxySession>>,
    pub op_lock: Mutex<()>,
}

fn terminate_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    unsafe {
        const PROCESS_TERMINATE: u32 = 0x0001;
        const SYNCHRONIZE: u32 = 0x0010_0000;
        let handle = OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, pid);
        if handle != 0 {
            let _ = TerminateProcess(handle, 1);
            let _ = WaitForSingleObject(handle, 1500);
            let _ = CloseHandle(handle);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
    }
}

fn stop_proxy_locked(app: &AppHandle, state: &ProxyManager) {
    let mut guard = state.session.lock().expect("proxy lock");
    let _ = restore_system_proxy(app);
    if let Some(mut session) = guard.take() {
        let _ = fs::write(&session.stop_flag, "stop");
        if let Some(mut child) = session.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        } else {
            if let Ok(raw_pid) = fs::read_to_string(session.run_dir.join("core.pid")) {
                if let Ok(pid) = raw_pid.trim().parse::<u32>() {
                    terminate_pid(pid);
                }
            }
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

pub fn stop_proxy_internal(app: &AppHandle, state: &ProxyManager) {
    let _op = state.op_lock.lock().expect("proxy op lock");
    stop_proxy_locked(app, state);
}

pub fn ensure_or_adopt_session(state: &ProxyManager, rules: &RulesPayload) -> bool {
    {
        let mut guard = state.session.lock().expect("proxy lock");
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

    let mut guard = state.session.lock().expect("proxy lock");
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
        stop_proxy_locked(app, state);
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

    stop_proxy_locked(app, state);
    app.exit(0);
    Ok(())
}

fn load_rules_sync(app: &AppHandle, state: &ProxyManager) -> RulesPayload {
    let _op = state.op_lock.lock().expect("proxy op lock");
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
    let _op = state.op_lock.lock().expect("proxy op lock");

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
        let guard = state.session.lock().expect("proxy lock");
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
            start_proxy_locked(app, rules, state)?;
        }
        RunningAction::HotReload(config_path) => {
            if rules.system_proxy_enabled {
                let bypass = split_domains(&rules.bypass_domains);
                enable_system_proxy(app, &bypass)?;
            } else {
                restore_system_proxy(app)?;
            }
            let config = build_mihomo_config(&rules)?;
            let cfg_text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
            fs::write(&config_path, cfg_text).map_err(|e| format!("写入运行配置失败：{e}"))?;
            let reload_body = json!({ "path": config_path.to_string_lossy() }).to_string();
            controller_request("PUT", "/configs?force=true", Some(&reload_body))
                .map_err(|e| format!("热重载配置失败：{e}"))?;
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
    let _op = state.op_lock.lock().expect("proxy op lock");
    start_proxy_locked(app, rules, state)
}

fn start_proxy_locked(
    app: &AppHandle,
    rules: RulesPayload,
    state: &ProxyManager,
) -> Result<(), String> {
    if rules.tun_enabled && !is_current_process_admin() {
        return relaunch_elevated(app, &rules, state, true);
    }

    stop_proxy_locked(app, state);

    let core_path = resolve_core_path(app)?;
    let geoip_path = resolve_geoip_path(app);
    if rules.cn_fallback != rules.fallback && geoip_path.is_none() {
        return Err("未找到 Country.mmdb GeoIP 数据库文件".into());
    }
    let config = build_mihomo_config(&rules)?;

    let run_dir = std::env::temp_dir().join(format!("routeisle-{}", now_millis()));
    fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;
    if let Some(geoip_src) = geoip_path {
        fs::copy(&geoip_src, run_dir.join("Country.mmdb"))
            .map_err(|e| format!("复制 GeoIP 数据库失败：{e}"))?;
    }
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
    let _ = fs::write(run_dir.join("core.pid"), child.id().to_string());

    let mut ready = false;
    for _ in 0..85 {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = fs::remove_dir_all(&run_dir);
            return Err(format!(
                "内核进程异常退出（{status}），请检查端口 {PROXY_PORT} 是否被占用"
            ));
        }
        if is_tcp_reachable("127.0.0.1", PROXY_PORT, 50)
            && is_tcp_reachable("127.0.0.1", CONTROLLER_PORT, 50)
        {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(35));
    }
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&run_dir);
        return Err(format!("等待本地代理端口 {PROXY_PORT} 就绪超时"));
    }

    if rules.system_proxy_enabled {
        let bypass = split_domains(&rules.bypass_domains);
        if let Err(err) = enable_system_proxy(app, &bypass) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(&run_dir);
            return Err(format!("接管系统代理失败：{err}"));
        }
    } else {
        let _ = restore_system_proxy(app);
    }

    let mut guard = state.session.lock().expect("proxy lock");
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

#[tauri::command]
pub async fn restart_proxy(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let rules = load_rules_from_disk(&app);
        let state = app.state::<ProxyManager>();
        start_proxy_sync(&app, rules, &state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoIpStatus {
    pub exists: bool,
    pub size: u64,
    pub updated_at: u64,
    pub path: String,
    pub restarted: bool,
}

fn read_geoip_status(app: &AppHandle, restarted: bool) -> GeoIpStatus {
    if let Some(path) = resolve_geoip_path(app) {
        if let Ok(meta) = fs::metadata(&path) {
            let updated_at = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            return GeoIpStatus {
                exists: true,
                size: meta.len(),
                updated_at,
                path: path.to_string_lossy().into_owned(),
                restarted,
            };
        }
    }
    GeoIpStatus {
        exists: false,
        size: 0,
        updated_at: 0,
        path: String::new(),
        restarted,
    }
}

#[tauri::command]
pub async fn get_geoip_status(app: AppHandle) -> Result<GeoIpStatus, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(read_geoip_status(&app, false)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_network_interfaces() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(crate::config::list_network_interfaces)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scan_system_upstreams() -> Result<crate::config::SystemUpstreamScan, String> {
    tauri::async_runtime::spawn_blocking(crate::config::scan_system_upstreams)
        .await
        .map_err(|e| e.to_string())
}

fn download_geoip_with_curl(tmp_path: &std::path::Path, proxy_url: Option<&str>) -> Result<(), String> {
    const GEOIP_URL: &str =
        "https://github.com/Loyalsoldier/geoip/releases/latest/download/Country-only-cn-private.mmdb";
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-sSL", "--fail", "--max-time", "25"]);
    if let Some(proxy) = proxy_url {
        cmd.args(["--proxy", proxy]);
    }
    cmd.arg("-o").arg(tmp_path).arg(GEOIP_URL);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().map_err(|e| format!("无法启动 curl.exe：{e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("下载失败（退出码 {:?}）", out.status.code())
        } else {
            stderr
        });
    }
    let meta = fs::metadata(tmp_path).map_err(|e| e.to_string())?;
    if meta.len() < 50_000 {
        return Err("下载的 GeoIP 数据库文件异常偏小，已取消替换".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn refresh_geoip_db(app: AppHandle) -> Result<GeoIpStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = data_dir(&app);
        fs::create_dir_all(&dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
        let target = dir.join("Country.mmdb");
        let tmp = dir.join("Country.mmdb.tmp");
        let _ = fs::remove_file(&tmp);

        let is_running = {
            let state = app.state::<ProxyManager>();
            let guard = state.session.lock().expect("proxy lock");
            guard.is_some()
        };

        let proxy_url = format!("http://127.0.0.1:{PROXY_PORT}");
        let download_res = download_geoip_with_curl(&tmp, None).or_else(|direct_err| {
            if is_running {
                download_geoip_with_curl(&tmp, Some(&proxy_url))
            } else {
                Err(direct_err)
            }
        });
        if let Err(err) = download_res {
            let _ = fs::remove_file(&tmp);
            return Err(format!("刷新 GeoIP 数据库失败：{err}"));
        }

        if target.exists() {
            let _ = fs::remove_file(&target);
        }
        fs::rename(&tmp, &target).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("保存 GeoIP 数据库失败：{e}")
        })?;

        let mut restarted = false;
        if is_running {
            let rules = load_rules_from_disk(&app);
            let state = app.state::<ProxyManager>();
            start_proxy_sync(&app, rules, &state)?;
            restarted = true;
        }

        Ok(read_geoip_status(&app, restarted))
    })
    .await
    .map_err(|e| e.to_string())?
}
