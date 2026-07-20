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
        let result = Command::new("loginctl").arg("lock-sessions").status();
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

/// Default wall-clock budget for completion-action user commands (PERF-07).
pub const USER_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// S-4: Validate and tokenize a completion-action command without executing it.
pub fn validate_user_command(command: &str) -> Result<Vec<String>, String> {
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
    Ok(parts)
}

/// PERF-07: Run a validated completion-action command without blocking Tokio.
///
/// Uses `tokio::process::Command` with kill-on-drop and a wall-clock timeout so
/// a hung user script cannot pin a download-worker thread or stall dispatch.
pub async fn run_user_command(command: &str) -> Result<(), String> {
    run_user_command_with_timeout(command, USER_COMMAND_TIMEOUT).await
}

/// Same as [`run_user_command`] with an explicit timeout (used by tests).
pub async fn run_user_command_with_timeout(
    command: &str,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let parts = validate_user_command(command)?;
    tracing::info!(
        executable = %parts[0],
        arg_count = parts.len() - 1,
        timeout_secs = timeout.as_secs(),
        "user command requested by confirmed completion action"
    );

    let mut cmd = tokio::process::Command::new(&parts[0]);
    cmd.args(&parts[1..]);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to execute command: {e}"))?;

    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => {
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
        Ok(Err(error)) => Err(format!("Failed to wait for command: {error}")),
        Err(_elapsed) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            tracing::warn!(
                executable = %parts[0],
                timeout_secs = timeout.as_secs(),
                "user command timed out and was killed"
            );
            Err(crate::models::AppErrorPayload::new(
                "completion_command_timeout",
                format!(
                    "The completion command did not exit within {} seconds and was terminated.",
                    timeout.as_secs()
                ),
                true,
                vec!["check_url", "retry"],
            )
            .command_error())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_user_command_rejects_empty_string() {
        let err = run_user_command("").await.unwrap_err();
        assert!(err.contains("No command configured"));
    }

    #[tokio::test]
    async fn run_user_command_rejects_whitespace_only() {
        let err = run_user_command("   ").await.unwrap_err();
        assert!(err.contains("No command configured"));
    }

    #[tokio::test]
    async fn run_user_command_rejects_shell_metacharacters() {
        let err = run_user_command("echo hello & rm -rf /").await.unwrap_err();
        assert!(err.contains("disallowed character"));
    }

    #[tokio::test]
    async fn run_user_command_rejects_pipe() {
        let err = run_user_command("echo hello | cat").await.unwrap_err();
        assert!(err.contains("disallowed character"));
    }

    #[tokio::test]
    async fn run_user_command_rejects_redirect() {
        let err = run_user_command("echo hello > /tmp/file")
            .await
            .unwrap_err();
        assert!(err.contains("disallowed character"));
    }

    #[tokio::test]
    async fn run_user_command_rejects_unclosed_quote() {
        let err = run_user_command("echo \"unclosed").await.unwrap_err();
        assert!(err.contains("invalid quoting syntax"));
    }

    #[tokio::test]
    async fn run_user_command_executes_simple_command() {
        let cmd = if cfg!(target_os = "windows") {
            "where where"
        } else {
            "true"
        };
        let result = run_user_command(cmd).await;
        assert!(
            result.is_ok(),
            "expected simple command to succeed, got: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn run_user_command_reports_non_zero_exit() {
        let cmd = if cfg!(target_os = "windows") {
            "where nonexistent-program-xyz-12345"
        } else {
            "false"
        };
        let err = run_user_command(cmd).await.unwrap_err();
        assert!(err.contains("exited with status"));
    }

    #[tokio::test]
    async fn run_user_command_times_out_hanging_process() {
        let cmd = if cfg!(target_os = "windows") {
            // ping waits ~1s per echo; 60 echoes >> 2s timeout.
            "ping -n 60 127.0.0.1"
        } else {
            "sleep 60"
        };
        let started = std::time::Instant::now();
        let err = run_user_command_with_timeout(cmd, std::time::Duration::from_secs(2))
            .await
            .expect_err("hanging command must time out");
        assert!(
            err.contains("completion_command_timeout"),
            "expected structured timeout, got: {err}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "timeout path must not wait for the full hanging command"
        );
    }
}
