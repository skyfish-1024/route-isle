pub mod config;
pub mod logger;
pub mod models;
pub mod proxy;
pub mod sysproxy;
pub mod tray;
pub mod window;

use logger::{
    append_connection_logs, append_session_log, clear_logs, get_log_stats, load_session_logs,
};

use proxy::{
    get_geoip_status, load_rules, refresh_geoip_db, save_rules, start_proxy, stop_proxy,
    stop_proxy_internal, ProxyManager,
};
use tauri::{Emitter, Manager, PhysicalPosition, WindowEvent};
use std::{thread, time::Duration};
use window::{
    collapse_panel, drag_island, get_window_state, hide_panel, position_panel_near_capsule,
    sync_capsule_state, toggle_panel, IslandPosition, PanelVisibilityEvent,
};

pub fn run() {
    #[cfg(target_os = "windows")]
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--no-proxy-server");
    }
    proxy::init_from_args();
    if !proxy::acquire_single_instance_lock() {
        return;
    }
    tauri::Builder::default()
        .manage(IslandPosition::default())
        .manage(ProxyManager::default())
        .setup(|app| {
            tray::create_tray(app.handle())?;
            proxy::spawn_wake_listener(app.handle().clone());
            if let Some(window) = app.get_webview_window("main") {
                if let Some(monitor) = window.primary_monitor()? {
                    let width = window.outer_size()?.width;
                    let x = monitor.position().x
                        + monitor.size().width.saturating_sub(width) as i32 / 2;
                    let y =
                        monitor.position().y + (2.0 * monitor.scale_factor()).round() as i32;
                    window.set_position(PhysicalPosition::new(x, y))?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match (window.label(), event) {
                ("main", WindowEvent::Moved(_)) => {
                    let state = app.state::<IslandPosition>();
                    let (visible, insets) = {
                        let inner = state.0.lock().expect("window state lock");
                        (inner.panel_visible, inner.capsule_insets)
                    };
                    if visible {
                        if let Ok(pop_up) = position_panel_near_capsule(app, insets) {
                            let mut inner = state.0.lock().expect("window state lock");
                            if inner.pop_up != pop_up {
                                inner.pop_up = pop_up;
                                let _ = app.emit(
                                    "panel-visibility",
                                    PanelVisibilityEvent {
                                        visible: true,
                                        pop_up,
                                    },
                                );
                            }
                        }
                    }
                }
                ("panel", WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    collapse_panel(app, true);
                }
                ("panel", WindowEvent::Focused(false)) => {
                    let app_handle = app.clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(80));
                        let main_focused = app_handle
                            .get_webview_window("main")
                            .and_then(|w| w.is_focused().ok())
                            .unwrap_or(false);
                        let panel_focused = app_handle
                            .get_webview_window("panel")
                            .and_then(|w| w.is_focused().ok())
                            .unwrap_or(false);
                        if !main_focused && !panel_focused {
                            collapse_panel(&app_handle, false);
                        }
                    });
                }
                ("main", WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed) => {
                    let state = app.state::<ProxyManager>();
                    stop_proxy_internal(app, &state);
                    app.exit(0);
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            drag_island,
            toggle_panel,
            hide_panel,
            sync_capsule_state,
            get_window_state,
            load_rules,
            save_rules,
            start_proxy,
            stop_proxy,
            get_geoip_status,
            refresh_geoip_db,
            append_connection_logs,
            append_session_log,
            load_session_logs,
            get_log_stats,
            clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Split Proxy Island");
}
