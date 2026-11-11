use crate::{
    proxy::{stop_proxy_internal, ProxyManager},
    window::{toggle_panel_visibility, IslandPosition, PanelVisibilityEvent},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

fn show_capsule(app: &AppHandle) {
    if let Some(capsule) = app.get_webview_window("main") {
        let _ = capsule.show();
        let _ = capsule.set_focus();
    }
}

fn hide_all_windows(app: &AppHandle) {
    {
        let state = app.state::<IslandPosition>();
        let mut inner = state.0.lock().expect("window state lock");
        inner.panel_visible = false;
        inner.capsule_state.expanded = false;
    }
    let _ = app.emit(
        "panel-visibility",
        PanelVisibilityEvent {
            visible: false,
            pop_up: false,
        },
    );
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
    if let Some(capsule) = app.get_webview_window("main") {
        let _ = capsule.hide();
    }
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle_island = MenuItem::with_id(
        app,
        "toggle_island",
        "显示 / 隐藏分流岛",
        true,
        None::<&str>,
    )?;
    let toggle_panel =
        MenuItem::with_id(app, "toggle_panel", "展开 / 收起面板", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出分流岛", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&toggle_island, &toggle_panel, &sep, &quit_item])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .unwrap_or_else(|| tauri::include_image!("icons/icon.png"));

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .menu(&menu)
        .tooltip("分流岛 · RouteIsle")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle_island" => {
                let visible = app
                    .get_webview_window("main")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(true);
                if visible {
                    hide_all_windows(app);
                } else {
                    show_capsule(app);
                }
            }
            "toggle_panel" => {
                let _ = toggle_panel_visibility(app, None);
            }
            "quit" => {
                let proxy_state = app.state::<ProxyManager>();
                stop_proxy_internal(app, &proxy_state);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                let visible = app
                    .get_webview_window("main")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                if !visible {
                    show_capsule(app);
                } else {
                    let _ = toggle_panel_visibility(app, None);
                }
            }
        })
        .build(app)?;

    Ok(())
}
