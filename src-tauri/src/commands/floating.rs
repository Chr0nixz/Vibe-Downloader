use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use crate::focus_main_window;

pub(crate) const FLOATING_STATUS_WINDOW_LABEL: &str = "floating-status";
const FLOATING_STATUS_WIDTH: f64 = 280.0;
const FLOATING_STATUS_HEIGHT: f64 = 104.0;
const FLOATING_STATUS_SCREEN_MARGIN: f64 = 16.0;

#[tauri::command]
#[specta::specta]
pub async fn show_floating_status_window(app: AppHandle) -> Result<(), String> {
    show_floating_status_window_for_app(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn hide_floating_status_window(app: AppHandle) -> Result<(), String> {
    hide_floating_status_window_for_app(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_floating_status_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FLOATING_STATUS_WINDOW_LABEL) {
        if window.is_visible().unwrap_or(false) {
            window.hide().map_err(|e| e.to_string())?;
        } else {
            position_floating_status_window(&app, &window)?;
            window.show().map_err(|e| e.to_string())?;
        }
        return Ok(());
    }

    show_floating_status_window_for_app(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn focus_main_window_from_floating(app: AppHandle) -> Result<(), String> {
    focus_main_window(&app);
    Ok(())
}

pub(crate) fn sync_floating_status_window(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        show_floating_status_window_for_app(app)
    } else {
        hide_floating_status_window_for_app(app)
    }
}

fn show_floating_status_window_for_app(app: &AppHandle) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window(FLOATING_STATUS_WINDOW_LABEL) {
        window
    } else {
        WebviewWindowBuilder::new(
            app,
            FLOATING_STATUS_WINDOW_LABEL,
            WebviewUrl::App("index.html?surface=floating-status".into()),
        )
        .title("Vibe Downloader")
        .inner_size(FLOATING_STATUS_WIDTH, FLOATING_STATUS_HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .build()
        .map_err(|e| e.to_string())?
    };

    position_floating_status_window(app, &window)?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

fn hide_floating_status_window_for_app(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FLOATING_STATUS_WINDOW_LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn position_floating_status_window(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    window
        .set_position(floating_status_position(app))
        .map_err(|e| e.to_string())
}

fn floating_status_position(app: &AppHandle) -> PhysicalPosition<i32> {
    let monitor = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().and_then(|mut monitors| monitors.pop()));

    let scale_factor = monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    let window_width = FLOATING_STATUS_WIDTH * scale_factor;
    let window_height = FLOATING_STATUS_HEIGHT * scale_factor;
    let margin = FLOATING_STATUS_SCREEN_MARGIN * scale_factor;

    let (right, bottom) = monitor
        .as_ref()
        .map(|monitor| {
            let area = monitor.work_area();
            (
                area.position.x as f64 + area.size.width as f64,
                area.position.y as f64 + area.size.height as f64,
            )
        })
        .unwrap_or((window_width + margin, window_height + margin));

    PhysicalPosition::new(
        (right - window_width - margin).round() as i32,
        (bottom - window_height - margin).round() as i32,
    )
}
