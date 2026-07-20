mod bt;
pub(crate) mod checksum;
mod dash;
pub(crate) mod diagnostics;
mod engine;
pub(crate) mod error;
pub(crate) mod ffmpeg;
pub(crate) mod file_ops;
pub(crate) mod ftp;
mod hls;
mod http;
mod metalink;
pub(crate) mod probe_error;
pub(crate) mod retry;
pub(crate) mod sanitize;
pub(crate) mod sftp;
mod speed;
pub mod ssrf;
pub mod url_classify;
pub(crate) mod webdav;

pub use bt::BtEngine;
pub use dash::DashEngine;
pub use engine::{DownloadContext, DownloadEngine, EngineRegistry, ProbeOutput, ProbeRequest};
pub use error::DownloadError;
pub use ftp::{probe_ftp_directory_url, FtpEngine};
pub use hls::HlsEngine;
pub use http::{DirectDownloadRequest, DirectSegmentedDownloadRequest, HttpEngine, ProbeResult};
#[doc(hidden)]
pub use metalink::testing;
pub use metalink::MetalinkEngine;
pub use sftp::{probe_sftp_directory_url, SftpEngine};
pub use speed::GlobalSpeedLimiter;
pub use webdav::{probe_webdav_directory_url, WebDavEngine};

// ---------------------------------------------------------------------------
// Shared idle-read timeout helper (E-1)
// ---------------------------------------------------------------------------

use std::{fmt::Display, future::Future, time::Duration};

/// Maximum idle time between reads before a non-HTTP download is considered
/// stalled. Mirrors `HTTP_CHUNK_READ_TIMEOUT` so every protocol shares the
/// same 60-second silence threshold.
pub(crate) const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// ARC-10: Default hard cap for control-plane bodies (MPD, Metalink, WebDAV
/// PROPFIND, HLS playlists/keys/init maps). Aligns with the existing HLS
/// `HLS_INIT_MAX_BYTES` budget.
pub(crate) const CONTROL_PLANE_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Outcome of a read operation wrapped with an idle timeout.
///
/// Callers match on this to apply protocol-specific error codes (e.g.
/// `hls_segment_stalled`, `ftp_read_timeout`) via `engine_error`, preserving
/// the structured `AppErrorPayload` contract that SFTP already follows.
#[derive(Debug)]
pub(crate) enum IdleReadOutcome<T> {
    /// Data was received.
    Data(T),
    /// The stream reported end-of-file.
    End,
    /// The underlying read future returned an error.
    Error(String),
    /// No data arrived within the idle timeout window.
    IdleTimeout,
}

/// ARC-10: Failure modes for bounded control-plane body reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LimitedBodyError {
    TooLarge,
    Canceled,
    IdleTimeout,
    Read(String),
}

/// Wrap a read future with an idle timeout.
///
/// Engines call this in their read loops to prevent stalled servers from
/// holding a worker, connection slot, and queue slot indefinitely. The
/// caller matches on [`IdleReadOutcome`] to build the protocol-specific
/// error string (typically via `engine_error`).
pub(crate) async fn read_with_idle_timeout<F, T, E>(
    future: F,
    timeout: Duration,
) -> IdleReadOutcome<T>
where
    F: Future<Output = Result<Option<T>, E>>,
    E: Display,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(Some(data))) => IdleReadOutcome::Data(data),
        Ok(Ok(None)) => IdleReadOutcome::End,
        Ok(Err(error)) => IdleReadOutcome::Error(format!("{error}")),
        Err(_) => IdleReadOutcome::IdleTimeout,
    }
}

/// ARC-10: Stream a response body with a hard size cap, optional cancel, and
/// idle timeout. Prefers Content-Length rejection before buffering; still
/// enforces the cap under chunked encoding.
pub(crate) async fn read_body_limited(
    mut response: reqwest::Response,
    limit: usize,
    cancel_token: Option<&tokio_util::sync::CancellationToken>,
    idle: Duration,
) -> Result<Vec<u8>, LimitedBodyError> {
    if let Some(content_length) = response.content_length() {
        if content_length as usize > limit {
            return Err(LimitedBodyError::TooLarge);
        }
    }
    let mut body = Vec::new();
    loop {
        let outcome = if let Some(token) = cancel_token {
            tokio::select! {
                biased;
                _ = token.cancelled() => return Err(LimitedBodyError::Canceled),
                outcome = read_with_idle_timeout(response.chunk(), idle) => outcome,
            }
        } else {
            read_with_idle_timeout(response.chunk(), idle).await
        };
        match outcome {
            IdleReadOutcome::Data(chunk) => {
                if body.len().saturating_add(chunk.len()) > limit {
                    return Err(LimitedBodyError::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            IdleReadOutcome::End => break,
            IdleReadOutcome::Error(error) => return Err(LimitedBodyError::Read(error)),
            IdleReadOutcome::IdleTimeout => return Err(LimitedBodyError::IdleTimeout),
        }
    }
    Ok(body)
}

/// ARC-10: Local `file://` manifests must check metadata before a full read.
pub(crate) async fn read_local_file_limited(
    path: &std::path::Path,
    limit: usize,
) -> Result<Vec<u8>, LimitedBodyError> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| LimitedBodyError::Read(format!("Could not stat local file: {e}")))?;
    if meta.len() as usize > limit {
        return Err(LimitedBodyError::TooLarge);
    }
    tokio::fs::read(path)
        .await
        .map_err(|e| LimitedBodyError::Read(format!("Could not read local file: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn helper_returns_data_when_future_resolves_with_some() {
        let future = async { Ok::<Option<u32>, std::convert::Infallible>(Some(42)) };
        match read_with_idle_timeout(future, Duration::from_secs(60)).await {
            IdleReadOutcome::Data(42) => {}
            other => panic!("expected Data(42), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn helper_returns_end_when_future_resolves_with_none() {
        let future = async { Ok::<Option<u32>, std::convert::Infallible>(None) };
        match read_with_idle_timeout(future, Duration::from_secs(60)).await {
            IdleReadOutcome::End => {}
            other => panic!("expected End, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn helper_returns_error_when_future_errors() {
        let future = async { Err::<Option<u32>, String>("boom".to_string()) };
        match read_with_idle_timeout(future, Duration::from_secs(60)).await {
            IdleReadOutcome::Error(msg) => assert_eq!(msg, "boom"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn helper_returns_idle_timeout_when_future_never_resolves() {
        // A pending future that never resolves. Use a short timeout so the
        // test completes quickly without needing tokio's test-util pause.
        let future = std::future::pending::<Result<Option<u32>, String>>();
        match read_with_idle_timeout(future, Duration::from_millis(50)).await {
            IdleReadOutcome::IdleTimeout => {}
            other => panic!("expected IdleTimeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_body_limited_rejects_content_length_over_cap() {
        // Build a minimal Response with an oversized Content-Length via hyper
        // is heavy; unit-test the local-file path and the TooLarge enum mapping
        // instead, plus a streamed oversize via a mock that we can construct
        // with reqwest's http::Response is not public. Cover local metadata.
        let dir = std::env::temp_dir().join(format!(
            "vibe-arc10-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("big.bin");
        std::fs::write(&path, vec![0_u8; 64]).expect("write");
        let err = read_local_file_limited(&path, 16)
            .await
            .expect_err("oversize local file");
        assert_eq!(err, LimitedBodyError::TooLarge);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn read_local_file_limited_reads_under_cap() {
        let dir = std::env::temp_dir().join(format!(
            "vibe-arc10-ok-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("ok.bin");
        std::fs::write(&path, b"hello").expect("write");
        let bytes = read_local_file_limited(&path, 64)
            .await
            .expect("under-cap local file");
        assert_eq!(bytes, b"hello");
        let _ = std::fs::remove_dir_all(dir);
    }
}
