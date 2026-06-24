use crate::platform;

#[tauri::command]
#[specta::specta]
pub async fn request_system_shutdown() -> Result<(), String> {
    platform::shutdown_now()
}

#[tauri::command]
#[specta::specta]
pub async fn request_system_sleep() -> Result<(), String> {
    platform::sleep_now()
}

#[tauri::command]
#[specta::specta]
pub async fn request_system_hibernate() -> Result<(), String> {
    platform::hibernate_now()
}

#[tauri::command]
#[specta::specta]
pub async fn request_lock_screen() -> Result<(), String> {
    platform::lock_screen_now()
}
