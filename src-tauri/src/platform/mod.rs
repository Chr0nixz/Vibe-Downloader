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
    // S-4: Reject shell metacharacters as an early, user-facing defense.
    // Even though we now exec directly (not via shell), the blacklist stays
    // because: (1) it gives a clear error message for unsupported shell
    // features (pipes, redirects), and (2) defense-in-depth.
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
    // S-4: Tokenize the command string with shlex, then exec the first token
    // directly with the remaining tokens as args. This eliminates shell
    // injection risk — no `cmd /C` or `sh -c` intermediary is involved.
    let parts = shlex::split(trimmed).ok_or_else(|| {
        tracing::warn!(command = %trimmed, "user command rejected: invalid quoting syntax");
        "The configured command has invalid quoting syntax (unclosed quote?).".to_string()
    })?;
    if parts.is_empty() {
        return Err("No command configured for the Run command completion action.".to_string());
    }
    tracing::info!(
        executable = %parts[0],
        arg_count = parts.len() - 1,
        "user command requested by confirmed completion action"
    );

    let mut cmd = Command::new(&parts[0]);
    cmd.args(&parts[1..]);
    let status = cmd
        .status()
        .map_err(|e| format!("Failed to execute command: {e}"))?;

    if status.success() {
        tracing::info!(
            executable = %parts[0],
            exit_code = status.code().unwrap_or(-1),
            "user command completed successfully"
        );
        Ok(())
    } else {
        let code = status.code().unwrap_or(-1);
        tracing::warn!(
            executable = %parts[0],
            exit_code = code,
            "user command exited with non-zero status"
        );
        Err(format!("Command exited with status {code}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_user_command_rejects_empty_string() {
        let err = run_user_command("").unwrap_err();
        assert!(err.contains("No command configured"));
    }

    #[test]
    fn run_user_command_rejects_whitespace_only() {
        let err = run_user_command("   ").unwrap_err();
        assert!(err.contains("No command configured"));
    }

    #[test]
    fn run_user_command_rejects_shell_metacharacters() {
        let err = run_user_command("echo hello & rm -rf /").unwrap_err();
        assert!(err.contains("disallowed character"));
    }

    #[test]
    fn run_user_command_rejects_pipe() {
        let err = run_user_command("echo hello | cat").unwrap_err();
        assert!(err.contains("disallowed character"));
    }

    #[test]
    fn run_user_command_rejects_redirect() {
        let err = run_user_command("echo hello > /tmp/file").unwrap_err();
        assert!(err.contains("disallowed character"));
    }

    #[test]
    fn run_user_command_rejects_unclosed_quote() {
        // shlex::split returns None for unclosed quotes.
        let err = run_user_command("echo \"unclosed").unwrap_err();
        assert!(err.contains("invalid quoting syntax"));
    }

    #[test]
    fn run_user_command_executes_simple_command() {
        // Use a cross-platform no-op command. `cmd /c verify` on Windows
        // is not suitable (we no longer use shell). Instead, use the
        // platform-specific exit-zero command directly.
        //
        // On Windows: `where where` (finds the `where` command, exits 0)
        // On Unix: `true` (always exits 0)
        //
        // We only verify that run_user_command returns Ok — the actual
        // command output is irrelevant.
        let cmd = if cfg!(target_os = "windows") {
            "where where"
        } else {
            "true"
        };
        // These commands contain no metacharacters and should succeed.
        let result = run_user_command(cmd);
        assert!(
            result.is_ok(),
            "expected simple command to succeed, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn run_user_command_reports_non_zero_exit() {
        // A command that exits with non-zero status. No metacharacters.
        let cmd = if cfg!(target_os = "windows") {
            "where nonexistent-program-xyz-12345"
        } else {
            "false"
        };
        let err = run_user_command(cmd).unwrap_err();
        assert!(err.contains("exited with status"));
    }
}
