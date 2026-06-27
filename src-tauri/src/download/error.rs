//! Typed error for the download engine layer.
//!
//! `DownloadError` replaces the loose `String` error type on the
//! [`DownloadEngine`](super::engine::DownloadEngine) trait so that the
//! command layer can dispatch on a stable `code()` instead of string-matching
//! error messages.
//!
//! # Phase 1.2 scope (conservative migration)
//!
//! Engine internals still produce `String` errors (frequently JSON-serialized
//! [`AppErrorPayload`](crate::models::AppErrorPayload) strings). At the trait
//! method boundary those strings are wrapped with [`DownloadError::Other`].
//! Because `Other` uses `#[error("{0}")]`, its [`Display`](std::fmt::Display)
//! output is the raw inner string, so the existing JSON parsing in
//! `mark_download_failed` / `task_error_code` keeps working unchanged when the
//! command layer converts back to `String` via [`From<DownloadError> for String`].
//!
//! `DownloadError` is internal to the Rust backend only: it is **not** exported
//! through specta. The frontend continues to receive `String` errors.
//!
//! DB-layer errors (`sqlx::Error`) are intentionally out of scope and remain
//! `String`; they are folded into [`DownloadError::Other`] at the engine
//! boundary via `map_err(|e| e.to_string())`.

/// Typed error returned by [`DownloadEngine`](super::engine::DownloadEngine)
/// trait methods.
///
/// The variants cover the error codes that historically determined task
/// attention/restart behaviour (see `mark_download_failed` and
/// `task_error_code`). Protocol-specific and legacy codes ride inside
/// [`DownloadError::Other`] as serialized [`AppErrorPayload`](crate::models::AppErrorPayload)
/// JSON strings during the conservative migration phase.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    /// A network/HTTP transport failure (DNS, connection, timeout, TLS, ...).
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    /// A local I/O failure (disk read/write, file system, ...).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The final destination path already exists and could not be reconciled.
    #[error("final path conflict: {0}")]
    FinalPathConflict(String),
    /// Browser-supplied request headers expired before use.
    #[error("auth headers expired")]
    AuthHeadersExpired,
    /// Browser-supplied request headers are missing/unavailable.
    #[error("auth headers unavailable")]
    AuthHeadersUnavailable,
    /// Resume validation detected a remote change (size/etag mismatch).
    #[error("resume mismatch: expected {expected}, actual {actual}")]
    ResumeMismatch { expected: i64, actual: i64 },
    /// The task was cancelled by the user or scheduler.
    #[error("task canceled")]
    Canceled,
    /// Catch-all carrying the original engine error string. During the
    /// conservative migration this is frequently a JSON-serialized
    /// [`AppErrorPayload`](crate::models::AppErrorPayload); `Display` yields the
    /// raw string so downstream JSON parsing is preserved.
    #[error("{0}")]
    Other(String),
}

impl DownloadError {
    /// Stable, frontend-aligned error code for this variant.
    ///
    /// These codes line up with the values historically produced by
    /// `task_error_code` / `AppErrorPayload` so attention/restart dispatch
    /// stays consistent. [`DownloadError::Other`] maps to the generic
    /// `"download_failed"` fallback; the specific code (when present) lives
    /// inside the carried string and is resolved by the command layer's
    /// `AppErrorPayload` parsing.
    pub fn code(&self) -> &'static str {
        match self {
            Self::FinalPathConflict(_) => "final_path_conflict",
            Self::AuthHeadersExpired => "auth_headers_expired",
            Self::AuthHeadersUnavailable => "auth_headers_unavailable",
            Self::ResumeMismatch { .. } => "resume_mismatch",
            Self::Canceled => "canceled",
            Self::Network(_) => "network_error",
            Self::Io(_) => "io_error",
            Self::Other(_) => "download_failed",
        }
    }
}

/// Convert a [`DownloadError`] back into the `String` shape expected by the
/// Tauri command layer and the persisted `error_message` column.
///
/// For [`DownloadError::Other`] this returns the inner string verbatim
/// (typically the original `AppErrorPayload` JSON), preserving the existing
/// JSON-based error code dispatch in `mark_download_failed` / `task_error_code`.
impl From<DownloadError> for String {
    fn from(error: DownloadError) -> String {
        error.to_string()
    }
}

/// Build a structured engine error string (JSON-serialized [`AppErrorPayload`]).
///
/// This is the canonical error builder for all download engines. It replaces
/// the per-protocol `ftp_error`/`sftp_error`/`hls_error`/`dash_error`/
/// `webdav_error`/`metalink_error` helpers with a single unified function.
///
/// Protocol-specific error codes (e.g. `sftp_host_key_changed`,
/// `hls_ffmpeg_missing`) are preserved — this function does **not** merge the
/// error-code namespace. It only normalizes the construction interface and
/// the recovery-action defaults:
///
/// - `recoverable = true`  → `actions: ["retry", "check_url"]`
/// - `recoverable = false` → `actions: ["check_url"]`
///
/// Engines that need custom actions can construct `AppErrorPayload` directly.
pub(crate) fn engine_error(
    code: &str,
    message: impl Into<String>,
    recoverable: bool,
) -> String {
    crate::models::AppErrorPayload::new(
        code,
        message,
        recoverable,
        if recoverable {
            vec!["retry", "check_url"]
        } else {
            vec!["check_url"]
        },
    )
    .command_error()
}
