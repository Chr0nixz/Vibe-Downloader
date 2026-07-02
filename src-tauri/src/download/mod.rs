mod bt;
pub(crate) mod checksum;
mod dash;
pub(crate) mod diagnostics;
pub(crate) mod error;
mod engine;
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
pub use error::DownloadError;
pub use engine::{
    DownloadContext, DownloadEngine, EngineRegistry, ProbeOutput,
    ProbeRequest,
};
pub use ftp::FtpEngine;
pub use hls::HlsEngine;
pub use http::{DirectDownloadRequest, DirectSegmentedDownloadRequest, HttpEngine, ProbeResult};
pub use metalink::MetalinkEngine;
#[doc(hidden)]
pub use metalink::testing;
pub use sftp::SftpEngine;
pub use speed::GlobalSpeedLimiter;
pub use webdav::WebDavEngine;

// ---------------------------------------------------------------------------
// Shared idle-read timeout helper (E-1)
// ---------------------------------------------------------------------------

use std::{fmt::Display, future::Future, time::Duration};

/// Maximum idle time between reads before a non-HTTP download is considered
/// stalled. Mirrors `HTTP_CHUNK_READ_TIMEOUT` so every protocol shares the
/// same 60-second silence threshold.
pub(crate) const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

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
}
