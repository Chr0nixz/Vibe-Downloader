//! Probe error classification utilities.
//!
//! Converts low-level network, I/O, and parsing errors into structured
//! [`AppErrorPayload`] JSON strings so the frontend can dispatch on
//! error codes rather than string-matching error messages.

use std::error::Error as _;

use crate::models::AppErrorPayload;

/// Convert a `reqwest::Error` into a structured error string.
///
/// Only typed or source-chain evidence of a transient transport failure is
/// eligible for automatic retry. Decode and opaque body failures stay terminal.
pub(crate) fn reqwest_error_to_structured(error: &reqwest::Error) -> String {
    if error.to_string().contains("intranet_target_blocked") {
        return error.to_string();
    }
    let code = if error.is_connect() {
        classify_connect_error(error).unwrap_or("network_error")
    } else if error.is_timeout() {
        "timeout"
    } else if error.is_decode() {
        classify_transport_error_source(error.source()).unwrap_or("decode_error")
    } else if error.is_redirect() {
        "redirect_error"
    } else if error.is_body() {
        classify_transport_error_source(error.source()).unwrap_or("body_error")
    } else {
        "network_error"
    };

    let recoverable = matches!(
        code,
        "timeout"
            | "transport_interrupted"
            | "connection_refused"
            | "network_unreachable"
            | "dns_failure"
            | "proxy_connection_failed"
    );
    let actions: Vec<&str> = if recoverable {
        vec!["retry", "check_url"]
    } else {
        vec!["check_url"]
    };

    AppErrorPayload::new(code, error.to_string(), recoverable, actions).command_error()
}

/// ARC-30: classify a connect-phase failure from typed I/O kinds first, then
/// explicit source-chain details. Unknown connect errors remain non-retryable.
fn classify_connect_error(error: &reqwest::Error) -> Option<&'static str> {
    classify_error_source_chain(error.source())
}

/// Return a stable code only when the source identifies a concrete condition.
fn classify_error_source_chain(
    source: Option<&(dyn std::error::Error + 'static)>,
) -> Option<&'static str> {
    let mut source = source;
    while let Some(err) = source {
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            match io_err.kind() {
                std::io::ErrorKind::ConnectionRefused => return Some("connection_refused"),
                std::io::ErrorKind::TimedOut => return Some("timeout"),
                std::io::ErrorKind::NetworkUnreachable => return Some("network_unreachable"),
                std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::NotConnected => return Some("transport_interrupted"),
                _ => {}
            }
        }
        let msg = err.to_string().to_lowercase();
        let fallback = if msg.contains("dns")
            || msg.contains("resolve")
            || msg.contains("name or service not known")
            || msg.contains("getaddrinfo")
            || msg.contains("no such host")
            || msg.contains("nodename nor servname")
        {
            Some("dns_failure")
        } else if msg.contains("proxy") && (msg.contains("auth") || msg.contains("407")) {
            Some("proxy_auth_failed")
        } else if msg.contains("certificate") || msg.contains("ssl") || msg.contains("tls") {
            Some("tls_error")
        } else if msg.contains("socks") || msg.contains("proxy") {
            Some("proxy_connection_failed")
        } else if msg.contains("connection refused") || msg.contains("actively refused") {
            Some("connection_refused")
        } else if msg.contains("unreachable") || msg.contains("network is down") {
            Some("network_unreachable")
        } else if msg.contains("timed out") || msg.contains("timeout") || msg.contains("deadline") {
            Some("timeout")
        } else if msg.contains("connection reset")
            || msg.contains("connection aborted")
            || msg.contains("broken pipe")
            || msg.contains("unexpected eof")
            || msg.contains("connection closed")
        {
            Some("transport_interrupted")
        } else {
            None
        };
        if let Some(code) = fallback {
            tracing::debug!(message = %msg, code, "probe error classified by source-chain evidence");
            return Some(code);
        }
        source = err.source();
    }
    None
}

fn classify_transport_error_source(
    source: Option<&(dyn std::error::Error + 'static)>,
) -> Option<&'static str> {
    classify_error_source_chain(source).filter(|code| {
        matches!(
            *code,
            "timeout" | "transport_interrupted" | "connection_refused" | "network_unreachable"
        )
    })
}

pub(crate) fn structured_timeout_error(message: impl Into<String>) -> String {
    AppErrorPayload::new("timeout", message, true, vec!["retry", "check_url"]).command_error()
}

pub(crate) fn is_transient_reqwest_error(error: &reqwest::Error) -> bool {
    serde_json::from_str::<AppErrorPayload>(&reqwest_error_to_structured(error))
        .is_ok_and(|payload| payload.recoverable)
}

/// Convert a generic error message into a structured error string
/// using common network error patterns.
///
/// Used as a fallback for engines that produce plain-string errors.
pub(crate) fn classify_error_message(message: &str) -> Option<String> {
    let lower = message.to_lowercase();

    let code = if lower.contains("dns")
        || lower.contains("resolve")
        || lower.contains("name or service not known")
        || lower.contains("getaddrinfo")
        || lower.contains("no such host")
    {
        "dns_failure"
    } else if lower.contains("timeout") || lower.contains("timed out") || lower.contains("deadline")
    {
        "timeout"
    } else if lower.contains("proxy") && (lower.contains("auth") || lower.contains("407")) {
        "proxy_auth_failed"
    } else if lower.contains("connection refused") || lower.contains("actively refused") {
        "connection_refused"
    } else if lower.contains("certificate") || lower.contains("ssl") || lower.contains("tls") {
        "tls_error"
    } else if lower.contains("socks") || lower.contains("proxy") {
        "proxy_connection_failed"
    } else if lower.contains("unreachable") || lower.contains("network is down") {
        "network_unreachable"
    } else {
        return None;
    };

    let recoverable = matches!(
        code,
        "timeout" | "connection_refused" | "network_unreachable" | "proxy_connection_failed"
    );
    let actions: Vec<&str> = if recoverable {
        vec!["retry", "check_url"]
    } else {
        vec!["check_url"]
    };

    Some(AppErrorPayload::new(code, message, recoverable, actions).command_error())
}

/// Ensure an error string is structured (JSON `AppErrorPayload`).
/// If the input is already valid JSON, return it as-is.
/// Otherwise, try to classify the message text. If no pattern matches,
/// wrap in a generic `"unknown_error"` payload.
pub(crate) fn ensure_structured_error(error: String) -> String {
    // Already structured JSON?
    if error.starts_with('{') && serde_json::from_str::<AppErrorPayload>(&error).is_ok() {
        return error;
    }
    // Try to classify by message patterns
    if let Some(structured) = classify_error_message(&error) {
        return structured;
    }
    // Wrap as generic
    AppErrorPayload::new("unknown_error", error, false, vec!["check_url"]).command_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_dns_error() {
        let result = classify_error_message("DNS resolution failed for example.com");
        assert!(result.is_some());
        let payload: AppErrorPayload = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(payload.code, "dns_failure");
        assert!(!payload.recoverable);
    }

    #[test]
    fn classify_timeout_error() {
        let result = classify_error_message("Connection timed out after 30s");
        assert!(result.is_some());
        let payload: AppErrorPayload = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(payload.code, "timeout");
        assert!(payload.recoverable);
    }

    #[test]
    fn classify_connection_refused() {
        let result = classify_error_message("Connection refused by remote host");
        assert!(result.is_some());
        let payload: AppErrorPayload = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(payload.code, "connection_refused");
        assert!(payload.recoverable);
    }

    #[test]
    fn classify_tls_error() {
        let result = classify_error_message("SSL certificate verification failed");
        assert!(result.is_some());
        let payload: AppErrorPayload = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(payload.code, "tls_error");
        assert!(!payload.recoverable);
    }

    #[test]
    fn classify_unknown_returns_none() {
        let result = classify_error_message("Something went wrong");
        assert!(result.is_none());
    }

    #[test]
    fn plain_connection_reset_is_not_trusted_for_automatic_retry() {
        let result = classify_error_message(
            "The connection failed while downloading: connection reset by peer",
        );
        assert!(
            result.is_none(),
            "an unstructured message cannot prove transport failure"
        );
    }

    #[test]
    fn ensure_structured_preserves_json() {
        let original =
            AppErrorPayload::new("http_denied", "denied", false, vec!["check_url"]).command_error();
        let result = ensure_structured_error(original.clone());
        assert_eq!(result, original);
    }

    #[test]
    fn ensure_structured_classifies_plain() {
        let result = ensure_structured_error("Connection timed out".to_string());
        let payload: AppErrorPayload = serde_json::from_str(&result).unwrap();
        assert_eq!(payload.code, "timeout");
    }

    #[test]
    fn ensure_structured_wraps_unknown() {
        let result = ensure_structured_error("Random unknown error".to_string());
        let payload: AppErrorPayload = serde_json::from_str(&result).unwrap();
        assert_eq!(payload.code, "unknown_error");
        assert_eq!(payload.message, "Random unknown error");
    }
    /// ARC-30: a typed io::ErrorKind decides the code regardless of message
    /// wording; the message-fallback path only handles kinds without a
    /// stable mapping (e.g. DNS resolution text).
    #[test]
    fn io_error_kinds_classify_independently_of_message_text() {
        use std::io::Error as IoError;
        use std::io::ErrorKind;

        #[derive(Debug)]
        struct Chain(std::io::Error);
        impl std::fmt::Display for Chain {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "wrapper without keywords")
            }
        }
        impl std::error::Error for Chain {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let refused = Chain(IoError::new(ErrorKind::ConnectionRefused, "server said no"));
        assert_eq!(
            classify_error_source_chain(Some(&refused)),
            Some("connection_refused")
        );

        let timed_out = Chain(IoError::new(ErrorKind::TimedOut, "gave up eventually"));
        assert_eq!(
            classify_error_source_chain(Some(&timed_out)),
            Some("timeout")
        );

        let unreachable = Chain(IoError::new(ErrorKind::NetworkUnreachable, "no path"));
        assert_eq!(
            classify_error_source_chain(Some(&unreachable)),
            Some("network_unreachable")
        );

        let reset = Chain(IoError::new(ErrorKind::ConnectionReset, "peer reset"));
        assert_eq!(
            classify_error_source_chain(Some(&reset)),
            Some("transport_interrupted")
        );
        let malformed = Chain(IoError::new(
            ErrorKind::InvalidData,
            "invalid chunk encoding",
        ));
        assert_eq!(classify_transport_error_source(Some(&malformed)), None);
        let proxy_auth = IoError::other("proxy authentication required (HTTP 407)");
        assert_eq!(
            classify_error_source_chain(Some(&proxy_auth)),
            Some("proxy_auth_failed")
        );
    }

    /// ARC-30: the message fallback still routes OS/library texts that have
    /// no stable kind (DNS wording arrives via io::Error with kind
    /// Uncategorized), and unrelated texts fall through to the default.
    #[test]
    fn message_fallback_covers_dns_and_leaves_unknown_sources_unclassified() {
        use std::io::Error as IoError;

        let dns = IoError::other("No such host is known. (os error 11001)");
        assert_eq!(classify_error_source_chain(Some(&dns)), Some("dns_failure"));

        assert_eq!(
            classify_error_source_chain(Some(&IoError::other("something novel"))),
            None
        );
    }
}
