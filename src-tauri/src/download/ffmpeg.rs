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
use tokio_util::sync::CancellationToken;

use crate::db;

struct OwnedChild {
    child: Option<tokio::process::Child>,
    lease: Option<super::lifecycle::Lease>,
}

impl OwnedChild {
    fn new(child: tokio::process::Child) -> Self {
        Self {
            child: Some(child),
            lease: super::lifecycle::Resources::current().map(|owner| owner.lease()),
        }
    }
}

impl std::ops::Deref for OwnedChild {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        self.child.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.child.as_mut().unwrap()
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let mut child = self.child.take().unwrap();
        let lease = self.lease.take();
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        // ARC-62: kill-on-drop sends a signal but does not await process exit.
        let _ = child.start_kill();
        tokio::spawn(async move {
            let _ = child.wait().await;
            drop(lease);
        });
    }
}

/// Spawn an ffmpeg command with kill-on-drop and cancel ownership (ARC-03).
pub(crate) async fn run_cancellable(
    mut command: tokio::process::Command,
    cancel: &CancellationToken,
) -> Result<(), String> {
    command.kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|e| format!("Could not start ffmpeg: {e}"))?;
    let mut child = OwnedChild::new(child);
    tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err("Download canceled.".to_string())
        }
        status = child.wait() => {
            let status = status.map_err(|e| format!("Could not wait for ffmpeg: {e}"))?;
            if !status.success() {
                return Err(format!("ffmpeg failed with status {status}."));
            }
            Ok(())
        }
    }
}

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
        // PERF-06: avoid sync exists on the async hot path (HLS/DASH probe+download).
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Some(path);
        }
    }
    if let Some(pool) = pool {
        if let Some(path_str) = db::get_ffmpeg_path_setting(pool).await {
            let path = PathBuf::from(&path_str);
            if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                return Some(path);
            }
        }
    }
    executable_in_path("ffmpeg").await
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

/// Total deadline for a `ffmpeg -version` probe. A version string is a few
/// hundred bytes printed immediately by any working binary — a probe that
/// does not answer inside this window is treated as a hung binary, not a slow
/// one (ARC-54).
const FFMPEG_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Bound on each output stream the probe retains. A malformed binary could
/// otherwise spew unbounded output that `output()` buffers in memory for the
/// whole deadline.
const FFMPEG_PROBE_MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// Probe the ffmpeg version string for a given binary path.
///
/// Used by the Settings UI to verify that a user-configured path points to a
/// working ffmpeg binary. Returns the first line of `ffmpeg -version` output
/// (e.g. `ffmpeg version 6.1.1 Copyright (c) 2000-2023 the FFmpeg developers`).
///
/// The probe has a total deadline and an output cap so a binary that hangs or
/// floods the pipes cannot stall settings verification or the environment
/// health check forever (ARC-54). On timeout the child is killed *and* waited
/// so no process is left behind.
pub(crate) async fn probe_ffmpeg_version_at_path(path: &Path) -> Result<String, String> {
    probe_ffmpeg_version_with_budget(path, FFMPEG_PROBE_TIMEOUT, FFMPEG_PROBE_MAX_OUTPUT_BYTES)
        .await
}

/// Budget-parameterized probe body so tests can exercise the deadline and
/// output-cap paths without waiting the full production timeout.
async fn probe_ffmpeg_version_with_budget(
    path: &Path,
    deadline: std::time::Duration,
    max_output_bytes: usize,
) -> Result<String, String> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Err(format!(
            "ffmpeg binary not found at the configured path: {}",
            path.display()
        ));
    }
    let mut command = tokio::process::Command::new(path);
    command
        .arg("-version")
        // Null stdin so a non-ffmpeg binary cannot hang waiting for input.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {e}"))?;
    let mut child = OwnedChild::new(child);

    let stdout_pipe = child.stdout.take().expect("stdout is piped");
    let stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stdout_task = tokio::spawn(drain_capped_output(stdout_pipe, max_output_bytes));
    let stderr_task = tokio::spawn(drain_capped_output(stderr_pipe, max_output_bytes));

    let result = tokio::time::timeout(deadline, child.wait()).await;

    let (status, stdout, stderr) = match result {
        Ok(Ok(status)) => {
            let stdout = stdout_task
                .await
                .map_err(|e| format!("ffmpeg stdout reader failed: {e}"))?;
            let stderr = stderr_task
                .await
                .map_err(|e| format!("ffmpeg stderr reader failed: {e}"))?;
            (status, stdout, stderr)
        }
        Ok(Err(e)) => {
            stdout_task.abort();
            stderr_task.abort();
            return Err(format!("Failed to wait for ffmpeg: {e}"));
        }
        Err(_elapsed) => {
            stdout_task.abort();
            stderr_task.abort();
            // Kill *and* wait: a detached child is a leaked PID, and a zombie
            // process can hold the binary path on Windows.
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!(
                "ffmpeg did not return a version within {deadline:?}."
            ));
        }
    };

    if !status.success() {
        return Err(format!(
            "ffmpeg exited with status {}. stderr: {}",
            status,
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    let stdout_text = String::from_utf8_lossy(&stdout);
    let first_line = stdout_text.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return Err("ffmpeg returned an empty version string.".to_string());
    }
    Ok(first_line.to_string())
}

/// Retain a bounded prefix while continuing to drain, so a verbose child does
/// not receive a broken pipe before it exits or the probe deadline kills it.
async fn drain_capped_output<R>(mut pipe: R, max_output_bytes: usize) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut output = Vec::with_capacity(max_output_bytes.min(8 * 1024));
    let mut buffer = [0; 8 * 1024];
    loop {
        let bytes_read = match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(bytes_read) => bytes_read,
        };
        let retained = max_output_bytes
            .saturating_sub(output.len())
            .min(bytes_read);
        output.extend_from_slice(&buffer[..retained]);
    }
    output
}

/// Locate an executable by name on `PATH`. Cross-platform: on Windows this
/// also probes `name.exe`.
///
/// PERF-06: PATH scanning runs in `spawn_blocking` so a slow/network PATH
/// entry cannot stall the Tokio worker that resolves ffmpeg for HLS/DASH.
async fn executable_in_path(name: &str) -> Option<PathBuf> {
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        let path_var = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(&name);
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
    })
    .await
    .ok()
    .flatten()
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

    #[tokio::test]
    async fn executable_in_path_returns_none_for_unlikely_name() {
        let result = executable_in_path("vibe-downloader-ffmpeg-nonexistent-xyz").await;
        assert!(result.is_none(), "expected None for nonexistent binary");
    }

    #[tokio::test]
    async fn executable_in_path_finds_common_binary() {
        // `cmd` exists on Windows; `sh` exists on most Unix systems.
        #[cfg(target_os = "windows")]
        let target = "cmd";
        #[cfg(not(target_os = "windows"))]
        let target = "sh";
        let result = executable_in_path(target).await;
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
            .map(|dir| {
                std::path::Path::new(&dir)
                    .join("System32")
                    .join("where.exe")
            })
            .expect("WINDIR must be set on Windows");
        #[cfg(not(target_os = "windows"))]
        let existing_path = std::env::var("PATH")
            .ok()
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .next()
                    .and_then(|dir| std::fs::read_dir(dir).ok()?.next()?.ok().map(|e| e.path()))
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
        let Some(ffmpeg) = executable_in_path("ffmpeg").await else {
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
            .map(|dir| {
                std::path::Path::new(&dir)
                    .join("System32")
                    .join("where.exe")
            })
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

    /// Hang fixture: never exits on its own, so the probe's deadline path is
    /// exercised deterministically. A fast-exiting binary (e.g. `sh -version`,
    /// which errors out immediately on the unknown flag) can win the race
    /// against even a zero deadline and skip the timeout branch entirely.
    /// A busy loop keeps the hang inside the spawned process itself, so
    /// `kill` reaps it directly with no orphaned grandchildren.
    fn hang_script() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-ffmpeg-probe-hang-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        #[cfg(unix)]
        let (path, body) = (
            dir.join("fake-ffmpeg-hang.sh"),
            "#!/bin/sh\nwhile true; do :; done\n".to_string(),
        );
        #[cfg(target_os = "windows")]
        let (path, body) = (
            dir.join("fake-ffmpeg-hang.cmd"),
            "@echo off\r\n:loop\r\ngoto loop\r\n".to_string(),
        );
        std::fs::write(&path, body).expect("write hang script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).expect("stat").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).expect("chmod");
        }
        path
    }

    /// Fixture that exits immediately with a non-zero status, so the probe's
    /// failure path is exercised deterministically. Relying on a system
    /// shell's reaction to `-version` is platform-specific: dash exits 2,
    /// but macOS bash prints its own version banner and exits 0.
    fn nonzero_exit_script() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-ffmpeg-probe-nonzero-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        #[cfg(unix)]
        let (path, body) = (
            dir.join("fake-ffmpeg-nonzero.sh"),
            "#!/bin/sh\nexit 3\n".to_string(),
        );
        #[cfg(target_os = "windows")]
        let (path, body) = (
            dir.join("fake-ffmpeg-nonzero.cmd"),
            "@echo off\r\nexit /b 3\r\n".to_string(),
        );
        std::fs::write(&path, body).expect("write nonzero-exit script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).expect("stat").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).expect("chmod");
        }
        path
    }

    #[tokio::test]
    async fn probe_version_deadline_kills_and_reaps_a_hung_binary() {
        let binary = hang_script();
        assert!(binary.exists(), "fixture binary must exist");
        // Duration::ZERO forces the timeout branch; the fixture never exits
        // on its own, so the kill+wait path is taken deterministically.
        let start = std::time::Instant::now();
        let err = probe_ffmpeg_version_with_budget(
            &binary,
            std::time::Duration::ZERO,
            FFMPEG_PROBE_MAX_OUTPUT_BYTES,
        )
        .await
        .expect_err("a zero deadline must time out");
        assert!(
            err.contains("did not return a version"),
            "expected the stable timeout error, got: {err}"
        );
        // The kill+wait path itself must be bounded: if the child leaked, the
        // process would still exist — this call returning quickly is the
        // observable part of "no residual PID".
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "timeout path took too long: {:?}",
            start.elapsed()
        );
        let _ = std::fs::remove_dir_all(binary.parent().expect("fixture dir"));
    }

    #[tokio::test]
    async fn probe_version_surfaces_nonzero_exit_status() {
        // Deterministic fixture: a script that exits 3 immediately. A system
        // shell cannot serve this role — its reaction to the probe's
        // `-version` argument is platform-specific (dash exits 2, macOS bash
        // prints its version banner and exits 0).
        let binary = nonzero_exit_script();
        let err = probe_ffmpeg_version_at_path(&binary)
            .await
            .expect_err("a non-ffmpeg binary exits non-zero");
        assert!(
            err.contains("exited with status") || err.contains("empty version"),
            "expected a status error, got: {err}"
        );
        let _ = std::fs::remove_dir_all(binary.parent().expect("fixture dir"));
    }

    /// Flood fixture: a script that prints far more than the pipe buffer and
    /// the probe cap, then exits. `sh` on Unix, `cmd` batch on Windows — Rust's
    /// Command runs `.cmd` scripts through cmd.exe automatically, so the probe
    /// spawns either as a normal child.
    fn flood_script(lines: usize, exit_code: i32, suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-ffmpeg-probe-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        #[cfg(unix)]
        let (path, body) = {
            let path = dir.join(format!("fake-ffmpeg-{suffix}.sh"));
            let body = format!(
                "#!/bin/sh\ni=0\nwhile [ $i -lt {lines} ]; do\n  printf 'flood-line-%08d-padding-padding-padding-padding\\n' $i\n  i=$((i+1))\ndone\necho 'ffmpeg version 6.0-test'\nexit {exit_code}\n"
            );
            (path, body)
        };
        #[cfg(target_os = "windows")]
        let (path, body) = {
            let path = dir.join(format!("fake-ffmpeg-{suffix}.cmd"));
            let body = format!(
                "@echo off\r\nfor /l %%i in (1,1,{lines}) do echo flood-line-%%i-padding-padding-padding-padding\r\necho ffmpeg version 6.0-test\r\nexit /b {exit_code}\r\n"
            );
            (path, body)
        };
        std::fs::write(&path, body).expect("write flood script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).expect("stat script").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).expect("chmod script");
        }
        path
    }

    /// Endless-writer fixture: prints `x` forever; the probe's deadline must
    /// bound it even while output keeps arriving.
    fn endless_flood_script() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vibe-ffmpeg-probe-loop-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        #[cfg(unix)]
        let (path, body) = (
            dir.join("fake-ffmpeg-loop.sh"),
            "#!/bin/sh\nwhile true; do echo x; done\n".to_string(),
        );
        #[cfg(target_os = "windows")]
        let (path, body) = (
            dir.join("fake-ffmpeg-loop.cmd"),
            "@echo off\r\n:loop\r\necho x\r\ngoto loop\r\n".to_string(),
        );
        std::fs::write(&path, body).expect("write loop script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).expect("stat").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).expect("chmod");
        }
        path
    }

    #[tokio::test]
    async fn probe_version_drains_large_output_without_deadlocking() {
        // >64 KiB of stdout then a clean exit: if the probe read output only
        // after wait(), the child would block on a full pipe forever and the
        // deadline would fire instead of a clean Ok.
        let script = flood_script(6_000, 0, "flood-ok");
        let version = probe_ffmpeg_version_with_budget(
            &script,
            std::time::Duration::from_secs(30),
            FFMPEG_PROBE_MAX_OUTPUT_BYTES,
        )
        .await
        .expect("flood output must not deadlock the probe");
        assert!(!version.is_empty(), "a first line must come back");
        let _ = std::fs::remove_dir_all(script.parent().expect("fixture dir"));
    }

    #[tokio::test]
    async fn probe_version_kills_a_flooding_binary_within_deadline() {
        // An effectively infinite writer: the deadline must bound the probe
        // even while output keeps arriving.
        let script = endless_flood_script();
        let start = std::time::Instant::now();
        let err = probe_ffmpeg_version_with_budget(
            &script,
            std::time::Duration::from_millis(500),
            FFMPEG_PROBE_MAX_OUTPUT_BYTES,
        )
        .await
        .expect_err("an infinite flood must hit the deadline");
        assert!(err.contains("did not return a version"), "got: {err}");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "kill+wait took too long"
        );
        let _ = std::fs::remove_dir_all(script.parent().expect("fixture dir"));
    }
}
