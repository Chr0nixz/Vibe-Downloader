//! Shared ffmpeg resolution and validation helpers.
//!
//! Provides a single source of truth for resolving the ffmpeg binary path
//! used by HLS and DASH engines. Previously each engine had its own
//! `ffmpeg_path` / `ensure_ffmpeg_available` / `executable_in_path`
//! triplicate, which led to drift (DASH download entry skipped the
//! `ensure_ffmpeg_available` check, error messages diverged, and the
//! persisted `ffmpeg_path` setting was ignored).
//!
//! # Resolution order
//!
//! 1. `VIBE_FFMPEG_PATH` environment variable (if it points to an existing file)
//! 2. `ffmpeg_path` setting in SQLite (if it points to an existing file)
//! 3. `ffmpeg` (or `ffmpeg.exe` on Windows) found on `PATH`

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::db;

/// Resolve the ffmpeg binary path using the full resolution chain.
///
/// Resolution order: `VIBE_FFMPEG_PATH` env > `ffmpeg_path` setting (if a
/// pool is provided) > PATH lookup. Pass `None` for `pool` to skip the
/// SQLite setting lookup (used by integration tests that construct
/// `ProbeRequest` without a pool).
///
/// Returns `None` if ffmpeg cannot be located.
pub(crate) async fn ffmpeg_path(pool: Option<&SqlitePool>) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("VIBE_FFMPEG_PATH") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(pool) = pool {
        if let Some(path_str) = db::get_ffmpeg_path_setting(pool).await {
            let path = PathBuf::from(&path_str);
            if path.exists() {
                return Some(path);
            }
        }
    }
    executable_in_path("ffmpeg")
}

/// Ensure ffmpeg is available, returning the resolved [`PathBuf`] on success.
///
/// Pass `Some(pool)` in production to consult the persisted `ffmpeg_path`
/// setting. Pass `None` in tests or callers without DB access to fall back
/// to `VIBE_FFMPEG_PATH` + PATH only.
pub(crate) async fn ensure_ffmpeg_available(
    pool: Option<&SqlitePool>,
    code: &str,
    message: impl Into<String>,
) -> Result<PathBuf, String> {
    match ffmpeg_path(pool).await {
        Some(path) => Ok(path),
        None => Err(crate::models::AppErrorPayload::new(
            code,
            message,
            true,
            vec!["retry", "check_url", "configure_ffmpeg"],
        )
        .command_error()),
    }
}

/// Probe the ffmpeg version string for a given binary path.
///
/// Used by the Settings UI to verify that a user-configured path points to a
/// working ffmpeg binary. Returns the first line of `ffmpeg -version` output
/// (e.g. `ffmpeg version 6.1.1 Copyright (c) 2000-2023 the FFmpeg developers`).
pub(crate) async fn probe_ffmpeg_version_at_path(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!(
            "ffmpeg binary not found at the configured path: {}",
            path.display()
        ));
    }
    let output = tokio::process::Command::new(path)
        .arg("-version")
        .output()
        .await
        .map_err(|e| format!("Failed to spawn ffmpeg: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg exited with status {}. stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return Err("ffmpeg returned an empty version string.".to_string());
    }
    Ok(first_line.to_string())
}

/// Locate an executable by name on `PATH`. Cross-platform: on Windows this
/// also probes `name.exe`.
fn executable_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        #[cfg(target_os = "windows")]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tokio::sync::Mutex;

    // Process-wide env var mutations are not safe under parallel test
    // execution. The `serial_test` attribute plus this lock together ensure
    // that any test touching `VIBE_FFMPEG_PATH` runs exclusively. We use
    // `tokio::sync::Mutex` so the guard can be held across `.await` points
    // without tripping `clippy::await_holding_lock`.
    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    /// RAII guard that saves `VIBE_FFMPEG_PATH` on construction and restores
    /// it (or removes it) on drop. Always acquire `ENV_LOCK` before
    /// constructing a guard so concurrent tests cannot race.
    struct EnvVarGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(value: Option<&str>) -> Self {
            let previous = std::env::var_os("VIBE_FFMPEG_PATH");
            match value {
                Some(v) => std::env::set_var("VIBE_FFMPEG_PATH", v),
                None => std::env::remove_var("VIBE_FFMPEG_PATH"),
            }
            Self { previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(v) => std::env::set_var("VIBE_FFMPEG_PATH", v),
                None => std::env::remove_var("VIBE_FFMPEG_PATH"),
            }
        }
    }

    #[test]
    fn executable_in_path_returns_none_for_unlikely_name() {
        let result = executable_in_path("vibe-downloader-ffmpeg-nonexistent-xyz");
        assert!(result.is_none(), "expected None for nonexistent binary");
    }

    #[test]
    fn executable_in_path_finds_common_binary() {
        // `cmd` exists on Windows; `sh` exists on most Unix systems.
        #[cfg(target_os = "windows")]
        let target = "cmd";
        #[cfg(not(target_os = "windows"))]
        let target = "sh";
        let result = executable_in_path(target);
        assert!(
            result.is_some(),
            "expected to find {target} on PATH for this test environment"
        );
    }

    #[tokio::test]
    #[serial]
    async fn ffmpeg_path_prefers_env_var_when_set() {
        let _env_lock = ENV_LOCK.lock().await;
        // Pick a path that definitely exists so env-var wins over PATH.
        #[cfg(target_os = "windows")]
        let existing_path = std::env::var("WINDIR")
            .map(|dir| std::path::Path::new(&dir).join("System32").join("where.exe"))
            .expect("WINDIR must be set on Windows");
        #[cfg(not(target_os = "windows"))]
        let existing_path = std::env::var("PATH")
            .ok()
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .next()
                    .and_then(|dir| std::fs::read_dir(dir).ok()?.next().map(|e| e.path()))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("/bin/sh"));

        let existing_str = existing_path.to_string_lossy().to_string();
        assert!(
            existing_path.exists(),
            "test fixture path must exist: {existing_str}"
        );

        let _guard = EnvVarGuard::set(Some(&existing_str));
        let resolved = ffmpeg_path(None).await.expect("env var should resolve");
        assert_eq!(
            resolved, existing_path,
            "VIBE_FFMPEG_PATH must take priority over PATH lookup"
        );
    }

    #[tokio::test]
    #[serial]
    async fn ffmpeg_path_falls_through_when_env_var_missing() {
        let _env_lock = ENV_LOCK.lock().await;
        let _guard = EnvVarGuard::set(None);

        // With no env var and no pool, we fall back to PATH lookup. Whether
        // this returns Some or None depends on the test environment; both are
        // valid outcomes. The contract we care about is "does not panic and
        // does not read the env var".
        let _ = ffmpeg_path(None).await;
    }

    #[tokio::test]
    #[serial]
    async fn ffmpeg_path_ignores_nonexistent_env_var_path() {
        let _env_lock = ENV_LOCK.lock().await;
        let _guard = EnvVarGuard::set(Some("/definitely/not/a/real/ffmpeg/path-xyz"));

        // The env var points to a non-existent file, so resolution must fall
        // through to PATH lookup. We only assert that it does not return the
        // bogus env-var value; the actual result depends on whether ffmpeg is
        // installed.
        let resolved = ffmpeg_path(None).await;
        assert!(
            resolved
                .as_ref()
                .map(|p| p != std::path::Path::new("/definitely/not/a/real/ffmpeg/path-xyz"))
                .unwrap_or(true),
            "non-existent env-var path must not be returned"
        );
    }

    #[tokio::test]
    #[serial]
    async fn ensure_ffmpeg_available_returns_err_with_configure_action() {
        let _env_lock = ENV_LOCK.lock().await;
        let _guard = EnvVarGuard::set(Some("/definitely/not/a/real/ffmpeg/path-xyz"));

        // Force a missing-ffmpeg scenario by pointing the env var at a bogus
        // path AND using no pool. We cannot reliably suppress PATH lookup, so
        // we accept either Ok (ffmpeg on PATH) or the specific error payload
        // we declared. If ffmpeg is on PATH, this still passes.
        let result = ensure_ffmpeg_available(
            None,
            "ffmpeg_missing",
            "ffmpeg is required for this download.",
        )
        .await;

        match result {
            Ok(path) => {
                // ffmpeg was found on PATH; that's a valid resolution.
                assert!(path.exists(), "resolved ffmpeg path must exist");
            }
            Err(err) => {
                // Error string comes from AppErrorPayload::command_error which
                // serializes via Display. The serialized form should reference
                // the recovery action `configure_ffmpeg` and our code/message.
                assert!(
                    err.contains("configure_ffmpeg"),
                    "error must list configure_ffmpeg recovery action, got: {err}"
                );
            }
        }
    }

    #[tokio::test]
    async fn probe_ffmpeg_version_at_path_errors_for_missing_path() {
        let path = std::path::Path::new("/definitely/not/a/real/ffmpeg/path-xyz");
        let err = probe_ffmpeg_version_at_path(path)
            .await
            .expect_err("missing path should error");
        assert!(
            err.contains("ffmpeg binary not found"),
            "expected missing-path error, got: {err}"
        );
    }

    #[tokio::test]
    async fn probe_ffmpeg_version_at_path_returns_version_string_for_valid_binary() {
        // Locate ffmpeg via PATH so the test runs only when ffmpeg is actually
        // installed. This keeps the test meaningful in CI environments that
        // install ffmpeg without making it the default assumption.
        let Some(ffmpeg) = executable_in_path("ffmpeg") else {
            eprintln!("skipping probe_ffmpeg_version_at_path test: ffmpeg not on PATH");
            return;
        };
        let version = probe_ffmpeg_version_at_path(&ffmpeg)
            .await
            .expect("valid ffmpeg should return a version string");
        assert!(
            version.to_ascii_lowercase().contains("ffmpeg"),
            "version string should mention ffmpeg, got: {version}"
        );
        assert!(
            !version.contains('\n'),
            "version probe should return only the first line, got: {version}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn ffmpeg_path_resolves_from_settings_when_env_unset() {
        let _env_lock = ENV_LOCK.lock().await;
        let _guard = EnvVarGuard::set(None);

        // Stand up an in-memory SQLite pool with a settings table mirroring
        // the production schema, then write a `ffmpeg_path` value that points
        // to a real file on disk so the resolution chain returns it.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory pool");
        sqlx::query(
            r#"
            CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("settings table");

        #[cfg(target_os = "windows")]
        let fixture = std::env::var("WINDIR")
            .map(|dir| std::path::Path::new(&dir).join("System32").join("where.exe"))
            .expect("WINDIR must be set on Windows");
        #[cfg(not(target_os = "windows"))]
        let fixture = {
            // Use /bin/sh as a stand-in for "an existing binary". The test
            // only checks that the settings value wins over PATH lookup.
            let candidate = std::path::PathBuf::from("/bin/sh");
            if !candidate.exists() {
                eprintln!("skipping settings-priority test: /bin/sh not present");
                return;
            }
            candidate
        };

        let fixture_str = fixture.to_string_lossy().to_string();
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
            .bind("ffmpeg_path")
            .bind(&fixture_str)
            .execute(&pool)
            .await
            .expect("insert setting");

        // Sanity: read the value back through the public DB helper.
        let stored = db::get_ffmpeg_path_setting(&pool).await;
        assert_eq!(stored.as_deref(), Some(fixture_str.as_str()));

        // Now verify the resolution chain: env unset → settings → (no fallthrough).
        let resolved = ffmpeg_path(Some(&pool))
            .await
            .expect("settings value should resolve to the fixture path");
        assert_eq!(
            resolved, fixture,
            "ffmpeg_path must honor the persisted settings value when env var is unset"
        );
    }
}
