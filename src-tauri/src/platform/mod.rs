use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

pub fn db_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;

    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;

    Ok(dir.join("vibe.db"))
}

pub fn configure_main_window<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        window
            .set_decorations(false)
            .map_err(|e| format!("set_decorations: {e}"))?;
    }

    #[cfg(target_os = "linux")]
    {
        window
            .set_decorations(true)
            .map_err(|e| format!("set_decorations: {e}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        use tauri::TitleBarStyle;

        window
            .set_title_bar_style(TitleBarStyle::Overlay)
            .map_err(|e| format!("set_title_bar_style: {e}"))?;
    }

    Ok(())
}

#[allow(dead_code)]
pub fn traffic_lights_inset_px() -> u32 {
    #[cfg(target_os = "macos")]
    {
        78
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

pub fn open_path(path: &std::path::Path) -> Result<(), String> {
    let status = if cfg!(target_os = "windows") {
        Command::new("explorer")
            .arg(path)
            .status()
            .map_err(|e| format!("Failed to open path: {e}"))?
    } else if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(path)
            .status()
            .map_err(|e| format!("Failed to open path: {e}"))?
    } else {
        Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|e| format!("Failed to open path: {e}"))?
    };

    if status.success() {
        Ok(())
    } else {
        Err("The operating system could not open the requested path.".to_string())
    }
}
