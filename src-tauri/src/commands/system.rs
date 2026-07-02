use crate::platform;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize, specta::Type)]
pub struct DiskSpaceInfo {
    pub path: String,
    // Stored as String to avoid Specta's BigInt restriction on u64. The
    // frontend parses with Number() for arithmetic and display.
    pub total_bytes: String,
    pub available_bytes: String,
}

/// Query total and available disk space for the volume that contains `path`.
/// Returns a clear error string when the path does not exist or the underlying
/// OS call fails, so the frontend can render a helpful toast.
#[tauri::command]
#[specta::specta]
pub async fn query_disk_space(path: String) -> Result<DiskSpaceInfo, String> {
    let path = Path::new(&path);
    // Walk up to the first existing ancestor so a not-yet-created save dir
    // still yields a meaningful volume reading instead of an error.
    let mut probe = path;
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return Err("path does not exist and has no parent".to_string()),
        }
    }

    let total = fs4::free_space(probe)
        .map_err(|e| format!("failed to read disk space: {e}"))?;
    let available = fs4::available_space(probe)
        .map_err(|e| format!("failed to read available space: {e}"))?;

    Ok(DiskSpaceInfo {
        path: probe.to_string_lossy().to_string(),
        total_bytes: total.to_string(),
        available_bytes: available.to_string(),
    })
}

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
