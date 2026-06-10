use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};

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
            state
                .quit_requested
                .store(true, std::sync::atomic::Ordering::SeqCst);
            app.exit(0);
        }
    }

    Ok(())
}
