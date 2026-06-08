use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

const APP_IDENTIFIER: &str = "com.vibe.downloader";

pub fn app_log_dir() -> Result<PathBuf, String> {
    let dir = if cfg!(target_os = "windows") {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| "LOCALAPPDATA is not set.".to_string())?;
        PathBuf::from(local_app_data)
            .join(APP_IDENTIFIER)
            .join("logs")
    } else if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set.".to_string())?;
        PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join(APP_IDENTIFIER)
    } else {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .ok_or_else(|| "Could not resolve XDG data home.".to_string())?;
        data_home.join(APP_IDENTIFIER).join("logs")
    };

    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create app log directory: {e}"))?;
    Ok(dir)
}

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
    tracing::debug!(path = %path.display(), "opening path");
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
        tracing::warn!(path = %path.display(), "operating system could not open path");
        Err("The operating system could not open the requested path.".to_string())
    }
}
