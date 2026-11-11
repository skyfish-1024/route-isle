#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use tauri::{LogicalSize, Manager, PhysicalPosition, State, WebviewWindow};

#[derive(Default)]
struct IslandPosition(Mutex<Option<(PhysicalPosition<i32>, PhysicalPosition<i32>)>>);

fn fit_position(window: &WebviewWindow, insets: [f64; 4], snap: bool) -> tauri::Result<()> {
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

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetSystemMetrics(index: i32) -> i32;
}

#[tauri::command]
async fn drag_island(window: WebviewWindow, insets: [f64; 4]) -> Result<(), String> {
    window.start_dragging().map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    tauri::async_runtime::spawn_blocking(move || {
        use std::{thread, time::Duration};
        let primary_button = if unsafe { GetSystemMetrics(23) } == 0 { 1 } else { 2 };
        while unsafe { GetAsyncKeyState(primary_button) } < 0 {
            thread::sleep(Duration::from_millis(16));
        }
        thread::sleep(Duration::from_millis(40));
        fit_position(&window, insets, true).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(())
}

#[tauri::command]
fn resize_island(window: WebviewWindow, expanded: bool, top_inset: f64, saved: State<'_, IslandPosition>) -> Result<(), String> {
    let resize = || -> tauri::Result<()> {
        let position = window.outer_position()?;
        let mut saved = saved.0.lock().expect("island position lock");
        if expanded {
            window.set_size(LogicalSize::new(420.0, 430.0))?;
            fit_position(&window, [0.0, top_inset, 0.0, 0.0], false)?;
            *saved = Some((position, window.outer_position()?));
        } else {
            window.set_size(LogicalSize::new(420.0, 58.0))?;
            if let Some((collapsed, adjusted)) = saved.take() {
                if position == adjusted {
                    window.set_position(collapsed)?;
                    return Ok(());
                }
            }
            fit_position(&window, [0.0; 4], false)?;
        }
        Ok(())
    };
    resize().map_err(|error| error.to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(IslandPosition::default())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                if let Some(monitor) = window.primary_monitor()? {
                    let width = window.outer_size()?.width;
                    let x = monitor.position().x + monitor.size().width.saturating_sub(width) as i32 / 2;
                    let y = monitor.position().y + (16.0 * monitor.scale_factor()).round() as i32;
                    window.set_position(PhysicalPosition::new(x, y))?;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![resize_island, drag_island])
        .run(tauri::generate_context!())
        .expect("failed to run Split Proxy Island");
}
