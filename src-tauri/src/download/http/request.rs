use std::time::Duration;

use base64::Engine as _;
use reqwest::{
    header::{
        HeaderName, HeaderValue, ACCEPT_ENCODING, AUTHORIZATION, IF_RANGE, RANGE, RETRY_AFTER,
    },
    Client, RequestBuilder, Response, StatusCode,
};

use crate::db::TaskCredentials;
use crate::download::probe_error::reqwest_error_to_structured;
use crate::download::retry::{with_retry, RetryPolicy};

/// FUN-01: Merge decrypted task credentials into request headers for HTTP runtime.
/// Does not overwrite an existing Authorization header (e.g. browser handoff).
pub(crate) fn merge_basic_auth_headers(
    base_headers: &[(String, String)],
    credentials: Option<&TaskCredentials>,
) -> Vec<(String, String)> {
    let mut headers = base_headers.to_vec();
    let Some(credentials) = credentials else {
        return headers;
    };
    let username = credentials.username.trim();
    if username.is_empty() {
        return headers;
    }
    let has_authorization = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(AUTHORIZATION.as_str()));
    if has_authorization {
        return headers;
    }
    let token = base64::engine::general_purpose::STANDARD
        .encode(format!("{username}:{}", credentials.password));
    headers.push(("Authorization".to_string(), format!("Basic {token}")));
    headers
}

pub(super) async fn send_head_with_retry(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<Response, String> {
    let url = url.to_owned();
    // SEC-10: IP literals bypass the connection-time resolver; reject
    // private/reserved literal targets before the first attempt.
    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    crate::download::ssrf::assert_public_authority(&parsed)?;
    let headers = headers.to_owned();
    with_retry(&RetryPolicy::http_request(), |_attempt| {
        let request = apply_forwarded_headers(client.head(&url), &headers)
            .header(ACCEPT_ENCODING, "identity");
        async move {
            request.send().await.map_err(|e| {
                // Convert network errors to structured error format, matching
                // send_get_with_retry so probe-stage HEAD failures are classifiable.
                reqwest_error_to_structured(&e)
            })
        }
    })
    .await
}

pub(super) async fn send_get_with_retry(
    client: &Client,
    url: &str,
    range: Option<String>,
    if_range: Option<&str>,
    headers: &[(String, String)],
) -> Result<Response, String> {
    let url = url.to_owned();
    // SEC-10: literal-authority pre-flight, mirroring send_head_with_retry.
    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    crate::download::ssrf::assert_public_authority(&parsed)?;
    let headers = headers.to_owned();
    let range = range.clone();
    let if_range = if_range.map(str::to_owned);
    with_retry(&RetryPolicy::http_request(), |_attempt| {
        let mut request =
            apply_forwarded_headers(client.get(&url), &headers).header(ACCEPT_ENCODING, "identity");
        if let Some(ref range) = range {
            request = request.header(RANGE, range.as_str());
            if let Some(ref ifr) = if_range {
                request = request.header(IF_RANGE, ifr.as_str());
            }
        }
        async move {
            request.send().await.map_err(|e| {
                // Convert network errors to structured error format
                reqwest_error_to_structured(&e)
            })
        }
    })
    .await
}

pub(super) fn apply_forwarded_headers(
    mut request: RequestBuilder,
    headers: &[(String, String)],
) -> RequestBuilder {
    for (name, value) in headers {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(value) = HeaderValue::from_str(value) else {
            continue;
        };
        request = request.header(name, value);
    }
    request
}

pub(super) fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

pub(super) fn retry_after_duration(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .map(|duration| duration.min(Duration::from_secs(60)))
}

/// SEC-11: bind credential-bearing headers to the origin that produced them.
///
/// Authorization/Cookie may only travel to the origin host — manifest-declared
/// mirrors and cross-source media hosts are third parties from the origin's
/// perspective. Non-sensitive forwarded headers (User-Agent, Referer, ...)
/// still flow. Fail-closed: when the target URL cannot be parsed, credentials
/// are stripped.
pub(crate) fn headers_for_origin(
    headers: &[(String, String)],
    origin_host: &str,
    target_url: &str,
) -> Vec<(String, String)> {
    let same_origin = reqwest::Url::parse(target_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .is_some_and(|host| host.eq_ignore_ascii_case(origin_host));
    if same_origin {
        headers.to_vec()
    } else {
        headers
            .iter()
            .filter(|(name, _)| {
                !name.eq_ignore_ascii_case("authorization") && !name.eq_ignore_ascii_case("cookie")
            })
            .cloned()
            .collect()
    }
}

/// Extracts the host of a URL string for origin comparisons; `None` when the
/// URL is unparseable or hostless.
pub(crate) fn url_host(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
}

#[cfg(test)]
mod origin_binding_tests {
    use super::headers_for_origin;

    fn sample_headers() -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "vibe-test".to_string()),
            (
                "Authorization".to_string(),
                "Basic dXNlcjpwYXNz".to_string(),
            ),
            ("Cookie".to_string(), "session=abc".to_string()),
            (
                "Referer".to_string(),
                "https://origin.example/list".to_string(),
            ),
        ]
    }

    /// SEC-11: same-origin targets keep every forwarded header.
    #[test]
    fn same_origin_keeps_credentials() {
        let headers = sample_headers();
        let bound = headers_for_origin(
            &headers,
            "origin.example",
            "https://origin.example/seg-1.ts",
        );
        assert_eq!(bound.len(), headers.len());
        assert!(bound.iter().any(|(n, _)| n == "Authorization"));
        assert!(bound.iter().any(|(n, _)| n == "Cookie"));
    }

    /// SEC-11: cross-host targets lose Authorization/Cookie but keep the rest.
    #[test]
    fn cross_origin_strips_credentials_only() {
        let headers = sample_headers();
        let bound = headers_for_origin(
            &headers,
            "origin.example",
            "https://cdn.other.example/seg-1.ts",
        );
        assert!(bound.iter().all(|(n, _)| n != "Authorization"));
        assert!(bound.iter().all(|(n, _)| n != "Cookie"));
        assert!(bound.iter().any(|(n, _)| n == "User-Agent"));
        assert!(bound.iter().any(|(n, _)| n == "Referer"));
    }

    /// SEC-11: fail closed — an unparseable target strips credentials too.
    #[test]
    fn unparseable_target_strips_credentials() {
        let headers = sample_headers();
        let bound = headers_for_origin(&headers, "origin.example", "not a url");
        assert!(bound.iter().all(|(n, _)| n != "Authorization"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_basic_auth_injects_when_missing() {
        let creds = TaskCredentials {
            username: "alice".to_string(),
            password: "s3cret".to_string(),
            private_key_data: None,
            private_key_passphrase: None,
        };
        let headers = merge_basic_auth_headers(&[], Some(&creds));
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0, "Authorization");
        assert!(headers[0].1.starts_with("Basic "));
        assert!(!headers[0].1.contains("s3cret"));
    }

    #[test]
    fn merge_basic_auth_preserves_existing_authorization() {
        let creds = TaskCredentials {
            username: "alice".to_string(),
            password: "s3cret".to_string(),
            private_key_data: None,
            private_key_passphrase: None,
        };
        let base = vec![("Authorization".to_string(), "Bearer token".to_string())];
        let headers = merge_basic_auth_headers(&base, Some(&creds));
        assert_eq!(headers, base);
    }
}
