use serde::{Deserialize, Serialize};
use std::{sync::Mutex, thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow};

pub const PANEL_WIN_W: f64 = 488.0;
pub const PANEL_WIN_H: f64 = 382.0;
pub const PANEL_SHADOW_PAD: f64 = 14.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsuleState {
    pub running: bool,
    pub upload_rate: u64,
    pub download_rate: u64,
    pub theme: String,
    pub expanded: bool,
}

impl Default for CapsuleState {
    fn default() -> Self {
        Self {
            running: false,
            upload_rate: 0,
            download_rate: 0,
            theme: "dark".into(),
            expanded: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelVisibilityEvent {
    pub visible: bool,
    pub pop_up: bool,
}

pub struct WindowStateInner {
    pub capsule_insets: [f64; 4],
    pub panel_visible: bool,
    pub pop_up: bool,
    pub capsule_state: CapsuleState,
}

impl Default for WindowStateInner {
    fn default() -> Self {
        Self {
            capsule_insets: [20.0, 23.0, 20.0, 23.0],
            panel_visible: false,
            pop_up: false,
            capsule_state: CapsuleState::default(),
        }
    }
}

#[derive(Default)]
pub struct IslandPosition(pub Mutex<WindowStateInner>);

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetSystemMetrics(index: i32) -> i32;
}

pub fn fit_position(window: &WebviewWindow, insets: [f64; 4], snap: bool) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let scale = monitor.scale_factor();
    let threshold = if snap { (24.0 * scale).round() as i32 } else { 0 };
    let [left_inset, top_inset, right_inset, bottom_inset] =
        insets.map(|value| (value * scale).round() as i32);
    let right = area.position.x + area.size.width.saturating_sub(size.width) as i32;
    let bottom = area.position.y + area.size.height.saturating_sub(size.height) as i32;
    let align = |value: i32, first: i32, last: i32| {
        if value - first <= threshold {
            first
        } else if last - value <= threshold {
            last
        } else {
            value
        }
    };
    let left = area.position.x - left_inset;
    let top = area.position.y - top_inset;
    let right = right + right_inset;
    let bottom = bottom + bottom_inset;
    let mut x = align(position.x, left, right);
    let mut y = align(position.y, top, bottom);
    if snap {
        let on_vertical_edge = x == left || x == right;
        let on_horizontal_edge = y == top || y == bottom;
        let middle_x = left + (right - left) / 2;
        let middle_y = top + (bottom - top) / 2;
        if on_horizontal_edge && !on_vertical_edge && x.abs_diff(middle_x) <= threshold as u32 {
            x = middle_x;
        }
        if on_vertical_edge && !on_horizontal_edge && y.abs_diff(middle_y) <= threshold as u32 {
            y = middle_y;
        }
    }
    let target = PhysicalPosition::new(x, y);
    if target != position {
        window.set_position(target)?;
    }
    Ok(())
}

pub fn position_panel_near_capsule(app: &AppHandle, insets: [f64; 4]) -> tauri::Result<bool> {
    let (Some(capsule), Some(panel)) = (
        app.get_webview_window("main"),
        app.get_webview_window("panel"),
    ) else {
        return Ok(false);
    };
    let Some(monitor) = capsule.current_monitor()? else {
        return Ok(false);
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let cap_pos = capsule.outer_position()?;
    let cap_size = capsule.outer_size()?;

    let [li, ti, ri, bi] = insets.map(|v| (v * scale).round() as i32);
    let pill_left = cap_pos.x + li;
    let pill_right = cap_pos.x + cap_size.width as i32 - ri;
    let pill_top = cap_pos.y + ti;
    let pill_bottom = cap_pos.y + cap_size.height as i32 - bi;
    let pill_cx = (pill_left + pill_right) / 2;

    let pw = (PANEL_WIN_W * scale).round() as i32;
    let ph = (PANEL_WIN_H * scale).round() as i32;
    let pad = (PANEL_SHADOW_PAD * scale).round() as i32;

    let area_left = area.position.x;
    let area_top = area.position.y;
    let area_right = area.position.x + area.size.width as i32;
    let area_bottom = area.position.y + area.size.height as i32;

    let min_x = area_left - pad;
    let max_x = (area_right - pw + pad).max(min_x);
    let target_x = (pill_cx - pw / 2).clamp(min_x, max_x);

    let min_y = area_top - pad;
    let max_y = (area_bottom - ph + pad).max(min_y);
    let fits_downward = pill_bottom + ph - pad <= area_bottom;
    let (target_y, pop_up) = if fits_downward {
        (pill_bottom.clamp(min_y, max_y), false)
    } else {
        ((pill_top - ph).clamp(min_y, max_y), true)
    };

    let target = PhysicalPosition::new(target_x, target_y);
    if panel.outer_position().ok() != Some(target) {
        panel.set_position(target)?;
    }
    Ok(pop_up)
}

#[tauri::command]
pub async fn drag_island(
    app: AppHandle,
    window: WebviewWindow,
    insets: [f64; 4],
    saved: State<'_, IslandPosition>,
) -> Result<(), String> {
    {
        let mut inner = saved.0.lock().expect("window state lock");
        inner.capsule_insets = insets;
    }
    window.start_dragging().map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    tauri::async_runtime::spawn_blocking(move || {
        let primary_button = if unsafe { GetSystemMetrics(23) } == 0 {
            1
        } else {
            2
        };
        while unsafe { GetAsyncKeyState(primary_button) } < 0 {
            thread::sleep(Duration::from_millis(16));
        }
        thread::sleep(Duration::from_millis(40));
        fit_position(&window, insets, true).map_err(|error| error.to_string())?;
        let state = app.state::<IslandPosition>();
        let panel_visible = state.0.lock().expect("window state lock").panel_visible;
        if panel_visible {
            let pop_up =
                position_panel_near_capsule(&app, insets).map_err(|error| error.to_string())?;
            let mut inner = state.0.lock().expect("window state lock");
            inner.pop_up = pop_up;
            let _ = app.emit(
                "panel-visibility",
                PanelVisibilityEvent {
                    visible: true,
                    pop_up,
                },
            );
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(())
}

pub fn toggle_panel_visibility(
    app: &AppHandle,
    insets: Option<[f64; 4]>,
) -> tauri::Result<bool> {
    let saved = app.state::<IslandPosition>();
    let (next_visible, current_insets, prev_pop_up) = {
        let mut inner = saved.0.lock().expect("window state lock");
        if let Some(i) = insets {
            inner.capsule_insets = i;
        }
        inner.panel_visible = !inner.panel_visible;
        inner.capsule_state.expanded = inner.panel_visible;
        (inner.panel_visible, inner.capsule_insets, inner.pop_up)
    };

    let pop_up = if next_visible {
        if let Some(capsule) = app.get_webview_window("main") {
            let _ = capsule.show();
        }
        let up = position_panel_near_capsule(app, current_insets)?;
        if let Some(panel) = app.get_webview_window("panel") {
            let _ = panel.show();
            let _ = panel.set_focus();
        }
        up
    } else {
        prev_pop_up
    };

    {
        let mut inner = saved.0.lock().expect("window state lock");
        inner.pop_up = pop_up;
    }

    let _ = app.emit(
        "panel-visibility",
        PanelVisibilityEvent {
            visible: next_visible,
            pop_up,
        },
    );
    Ok(next_visible)
}

pub fn collapse_panel(app: &AppHandle, immediate_hide: bool) {
    let pop_up = {
        let state = app.state::<IslandPosition>();
        let mut inner = state.0.lock().expect("window state lock");
        if !inner.panel_visible {
            return;
        }
        inner.panel_visible = false;
        inner.capsule_state.expanded = false;
        inner.pop_up
    };
    let _ = app.emit(
        "panel-visibility",
        PanelVisibilityEvent {
            visible: false,
            pop_up,
        },
    );
    if immediate_hide {
        if let Some(panel) = app.get_webview_window("panel") {
            let _ = panel.hide();
        }
    }
}

#[tauri::command]
pub fn toggle_panel(app: AppHandle, insets: Option<[f64; 4]>) -> Result<bool, String> {
    toggle_panel_visibility(&app, insets).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn hide_panel(app: AppHandle, saved: State<'_, IslandPosition>) -> Result<(), String> {
    let still_hidden = !saved.0.lock().expect("window state lock").panel_visible;
    if still_hidden {
        if let Some(panel) = app.get_webview_window("panel") {
            let _ = panel.hide();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn sync_capsule_state(
    app: AppHandle,
    running: bool,
    upload_rate: u64,
    download_rate: u64,
    theme: String,
    saved: State<'_, IslandPosition>,
) {
    let payload = {
        let mut inner = saved.0.lock().expect("window state lock");
        inner.capsule_state.running = running;
        inner.capsule_state.upload_rate = upload_rate;
        inner.capsule_state.download_rate = download_rate;
        inner.capsule_state.theme = theme;
        inner.capsule_state.clone()
    };
    let _ = app.emit_to("main", "capsule-state", &payload);
}

#[tauri::command]
pub fn get_window_state(saved: State<'_, IslandPosition>) -> (CapsuleState, PanelVisibilityEvent) {
    let inner = saved.0.lock().expect("window state lock");
    (
        inner.capsule_state.clone(),
        PanelVisibilityEvent {
            visible: inner.panel_visible,
            pop_up: inner.pop_up,
        },
    )
}
