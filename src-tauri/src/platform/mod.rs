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

pub fn shutdown_now() -> Result<(), String> {
    tracing::warn!("system shutdown requested by confirmed completion action");

    let status = if cfg!(target_os = "windows") {
        Command::new("shutdown")
            .args(["/s", "/t", "0"])
            .status()
            .map_err(|e| format!("Failed to request shutdown: {e}"))?
    } else if cfg!(target_os = "macos") {
        Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to shut down"])
            .status()
            .map_err(|e| format!("Failed to request shutdown: {e}"))?
    } else {
        Command::new("systemctl")
            .arg("poweroff")
            .status()
            .map_err(|e| format!("Failed to request shutdown: {e}"))?
    };

    if status.success() {
        Ok(())
    } else {
        Err("The operating system rejected the shutdown request.".to_string())
    }
}

pub fn sleep_now() -> Result<(), String> {
    tracing::warn!("system sleep requested by confirmed completion action");

    let status = if cfg!(target_os = "windows") {
        Command::new("rundll32.exe")
            .args(["powrprof.dll,SetSuspendState", "0,1,0"])
            .status()
            .map_err(|e| format!("Failed to request sleep: {e}"))?
    } else if cfg!(target_os = "macos") {
        Command::new("pmset")
            .args(["sleepnow"])
            .status()
            .map_err(|e| format!("Failed to request sleep: {e}"))?
    } else {
        Command::new("systemctl")
            .arg("suspend")
            .status()
            .map_err(|e| format!("Failed to request sleep: {e}"))?
    };

    if status.success() {
        Ok(())
    } else {
        Err("The operating system rejected the sleep request.".to_string())
    }
}

pub fn hibernate_now() -> Result<(), String> {
    tracing::warn!("system hibernate requested by confirmed completion action");

    let status = if cfg!(target_os = "windows") {
        Command::new("rundll32.exe")
            .args(["powrprof.dll,SetSuspendState", "1,1,0"])
            .status()
            .map_err(|e| format!("Failed to request hibernate: {e}"))?
    } else if cfg!(target_os = "macos") {
        // macOS does not have a separate hibernate; pmset sleepnow uses standby mode.
        Command::new("pmset")
            .args(["sleepnow"])
            .status()
            .map_err(|e| format!("Failed to request hibernate: {e}"))?
    } else {
        Command::new("systemctl")
            .arg("hibernate")
            .status()
            .map_err(|e| format!("Failed to request hibernate: {e}"))?
    };

    if status.success() {
        Ok(())
    } else {
        Err("The operating system rejected the hibernate request.".to_string())
    }
}

pub fn lock_screen_now() -> Result<(), String> {
    tracing::warn!("screen lock requested by confirmed completion action");

    let status = if cfg!(target_os = "windows") {
        Command::new("rundll32.exe")
            .args(["user32.dll,LockWorkStation"])
            .status()
            .map_err(|e| format!("Failed to lock screen: {e}"))?
    } else if cfg!(target_os = "macos") {
        Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to keystroke \"q\" using {command down, control down}"])
            .status()
            .map_err(|e| format!("Failed to lock screen: {e}"))?
    } else {
        // Try loginctl first (systemd), fall back to xdg-screensaver.
        let result = Command::new("loginctl")
            .arg("lock-sessions")
            .status();
        match result {
            Ok(s) if s.success() => return Ok(()),
            _ => Command::new("xdg-screensaver")
                .arg("lock")
                .status()
                .map_err(|e| format!("Failed to lock screen: {e}"))?,
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err("The operating system rejected the screen lock request.".to_string())
    }
}

/// Characters that are dangerous when passed through a shell.
/// Covers both Windows cmd.exe and POSIX sh metacharacters, plus
/// control characters that could truncate or inject commands.
const SHELL_METACHARACTERS: &[char] = &[
    '&', '|', ';', '<', '>', '^', '%', '$', '`', '\n', '\r', '\0', '(', ')', '{', '}', '[', ']',
    '!', '~',
];

pub fn run_user_command(command: &str) -> Result<(), String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("No command configured for the Run command completion action.".to_string());
    }
    // Reject shell metacharacters to prevent command injection via IPC.
    if let Some(ch) = trimmed.chars().find(|c| SHELL_METACHARACTERS.contains(c)) {
        tracing::warn!(
            command = %trimmed,
            rejected_char = %ch,
            "user command rejected: contains shell metacharacter"
        );
        return Err(format!(
            "The configured command contains a disallowed character ('{ch}'). \
             Remove shell metacharacters (&, |, ;, <, >, etc.) and try again."
        ));
    }
    tracing::warn!(command = %trimmed, "user command requested by confirmed completion action");

    let shell = if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "sh"
    };
    let shell_flag = if cfg!(target_os = "windows") {
        "/C"
    } else {
        "-c"
    };

    let status = Command::new(shell)
        .args([shell_flag, command])
        .status()
        .map_err(|e| format!("Failed to execute command: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Command exited with status {}.",
            status.code().unwrap_or(-1)
        ))
    }
}
