use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    events::{emit_tray_new_download_requested, emit_tray_settings_requested},
    focus_main_window, open_downloads_dir, AppState,
};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TrayMenuAction {
    OpenApp,
    NewDownload,
    OpenDownloads,
    Settings,
    Quit,
}

#[tauri::command]
#[specta::specta]
pub async fn run_tray_menu_action(
    app: AppHandle,
    state: State<'_, AppState>,
    action: TrayMenuAction,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(crate::TRAY_MENU_WINDOW_LABEL) {
        let _ = window.hide();
    }

    match action {
        TrayMenuAction::OpenApp => focus_main_window(&app),
        TrayMenuAction::NewDownload => {
            focus_main_window(&app);
            emit_tray_new_download_requested(&app);
        }
        TrayMenuAction::OpenDownloads => open_downloads_dir(&app),
        TrayMenuAction::Settings => {
            focus_main_window(&app);
            emit_tray_settings_requested(&app);
        }
        TrayMenuAction::Quit => {
            if state
                .quit_requested
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Ok(());
            }
            state
                .quit_requested
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = app.emit("app://shutting-down", ());

            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>();
                crate::shutdown_active_downloads(state.inner(), std::time::Duration::from_secs(3))
                    .await;
                app_handle.exit(0);
            });
        }
    }

    Ok(())
}
