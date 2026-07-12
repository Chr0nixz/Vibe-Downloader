//! Probe error classification utilities.
//!
//! Converts low-level network, I/O, and parsing errors into structured
//! [`AppErrorPayload`] JSON strings so the frontend can dispatch on
//! error codes rather than string-matching error messages.

use std::error::Error as _;

use crate::models::AppErrorPayload;

/// Convert a `reqwest::Error` into a structured error string.
///
/// Classifies the error by inspecting error kind and chain:
/// - DNS resolution failures → `"dns_failure"`
/// - Connection timeouts → `"timeout"`
/// - Connection refused / network unreachable → `"connection_refused"`
/// - TLS/SSL errors → `"tls_error"`
/// - Other I/O or decode errors → `"network_error"`
pub(crate) fn reqwest_error_to_structured(error: &reqwest::Error) -> String {
    let code = if error.is_connect() {
        classify_connect_error(error)
    } else if error.is_timeout() {
        "timeout"
    } else if error.is_decode() {
        "decode_error"
    } else if error.is_redirect() {
        "redirect_error"
    } else if error.is_body() {
        "body_error"
    } else {
        "network_error"
    };

    let recoverable = matches!(code, "timeout" | "network_error" | "connection_refused");
    let actions: Vec<&str> = if recoverable {
        vec!["retry", "check_url"]
    } else {
        vec!["check_url"]
    };

    AppErrorPayload::new(code, error.to_string(), recoverable, actions).command_error()
}

/// Classify a connect-phase `reqwest::Error` by inspecting the error chain.
fn classify_connect_error(error: &reqwest::Error) -> &'static str {
    // Walk the source chain looking for DNS and connection-specific errors.
    let mut source: Option<&dyn std::error::Error> = error.source();
    while let Some(err) = source {
        let msg = err.to_string().to_lowercase();
        if msg.contains("dns")
            || msg.contains("resolve")
            || msg.contains("name or service not known")
            || msg.contains("getaddrinfo")
            || msg.contains("no such host")
            || msg.contains("nodename nor servname")
        {
            return "dns_failure";
        }
        if msg.contains("connection refused") || msg.contains("actively refused") {
            return "connection_refused";
        }
        if msg.contains("unreachable") || msg.contains("network is down") {
            return "network_unreachable";
        }
        if msg.contains("timed out") || msg.contains("timeout") || msg.contains("deadline") {
            return "timeout";
        }
        if msg.contains("certificate") || msg.contains("ssl") || msg.contains("tls") {
            return "tls_error";
        }
        source = err.source();
    }
    "connection_refused"
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
    } else if lower.contains("connection refused") || lower.contains("actively refused") {
        "connection_refused"
    } else if lower.contains("certificate") || lower.contains("ssl") || lower.contains("tls") {
        "tls_error"
    } else if lower.contains("unreachable") || lower.contains("network is down") {
        "network_unreachable"
    } else {
        return None;
    };

    let recoverable = matches!(
        code,
        "timeout" | "connection_refused" | "network_unreachable"
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
}
