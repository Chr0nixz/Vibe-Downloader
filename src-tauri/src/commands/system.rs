use crate::platform;

#[tauri::command]
#[specta::specta]
pub async fn request_system_shutdown() -> Result<(), String> {
    platform::shutdown_now()
}
