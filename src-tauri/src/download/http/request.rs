use std::time::Duration;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use reqwest::{
    header::{
        HeaderName, HeaderValue, ACCEPT_ENCODING, AUTHORIZATION, IF_RANGE, RANGE, RETRY_AFTER,
    },
    Client, RequestBuilder, Response, StatusCode,
};

use crate::db::TaskCredentials;
use crate::download::network_policy::NetworkPolicy;
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

/// Follow redirects with the same origin/SSRF rules as manifest subrequests.
/// Clients disable automatic redirects so secret custom headers cannot bypass
/// this filter, even when a redirect changes only scheme or port.
pub(crate) async fn send_request(
    builder: RequestBuilder,
    network_policy: &NetworkPolicy,
) -> Result<Response, String> {
    send_request_with_error_mapper(builder, network_policy, reqwest_error_to_structured).await
}

pub(crate) async fn send_request_with_error_mapper(
    builder: RequestBuilder,
    network_policy: &NetworkPolicy,
    map_error: impl Fn(&reqwest::Error) -> String,
) -> Result<Response, String> {
    let (client, request) = builder.build_split();
    let mut request = request.map_err(|e| map_error(&e))?;
    for hop in 0..=10 {
        network_policy.resolve(request.url()).await?;
        let mut next = request
            .try_clone()
            .ok_or_else(|| redirect_error("Request body cannot be replayed."))?;
        let response = client.execute(request).await.map_err(|e| map_error(&e))?;
        if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
            return Ok(response);
        };
        if hop == 10 {
            return Err(redirect_error("Too many redirects."));
        }
        let location = location
            .to_str()
            .map_err(|_| redirect_error("Invalid redirect location."))?;
        let target = response
            .url()
            .join(location)
            .map_err(|_| redirect_error("Invalid redirect URL."))?;
        if !matches!(target.scheme(), "http" | "https")
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err(redirect_error("Redirect target is not an HTTP origin."));
        }
        if url_origin(next.url().as_str()) != url_origin(target.as_str()) {
            let names: Vec<_> = next
                .headers()
                .keys()
                .filter(|name| crate::models::is_sensitive_request_header(name.as_str()))
                .cloned()
                .collect();
            for name in names {
                next.headers_mut().remove(name);
            }
        }
        if response.status() == StatusCode::SEE_OTHER && next.method() != reqwest::Method::HEAD
            || matches!(response.status().as_u16(), 301 | 302)
                && next.method() == reqwest::Method::POST
        {
            *next.method_mut() = reqwest::Method::GET;
            *next.body_mut() = None;
            next.headers_mut().remove(reqwest::header::CONTENT_LENGTH);
            next.headers_mut().remove(reqwest::header::CONTENT_TYPE);
            next.headers_mut()
                .remove(reqwest::header::TRANSFER_ENCODING);
        }
        // A downgrade must not expose a Referer that contains HTTPS query data.
        if next.url().scheme() == "https" && target.scheme() == "http" {
            next.headers_mut().remove(reqwest::header::REFERER);
        }
        *next.url_mut() = target;
        request = next;
    }
    unreachable!("redirect loop returns within its bounded iteration")
}

fn redirect_error(detail: &str) -> String {
    crate::models::AppErrorPayload::new("redirect_error", detail, false, vec!["check_url"])
        .command_error()
}

pub(super) async fn send_head_with_retry(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
    network_policy: &NetworkPolicy,
) -> Result<Response, String> {
    let url = url.to_owned();
    // SEC-10: IP literals bypass the connection-time resolver; reject
    // private/reserved literal targets before the first attempt.
    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    // Resolve before every attempt so hostname targets are checked against the
    // task grant as well as literal addresses. The client resolver repeats the
    // policy filter at connection time to close DNS rebinding races.
    network_policy.resolve(&parsed).await?;
    let headers = headers.to_owned();
    with_retry(&RetryPolicy::http_request(), |_attempt| {
        let request = apply_forwarded_headers(client.head(&url), &headers)
            .header(ACCEPT_ENCODING, "identity");
        async move { send_request(request, network_policy).await }
    })
    .await
}

pub(super) async fn send_get_with_retry(
    client: &Client,
    url: &str,
    range: Option<String>,
    if_range: Option<&str>,
    headers: &[(String, String)],
    network_policy: &NetworkPolicy,
) -> Result<Response, String> {
    let url = url.to_owned();
    // SEC-10: literal-authority pre-flight, mirroring send_head_with_retry.
    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    network_policy.resolve(&parsed).await?;
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
        async move { send_request(request, network_policy).await }
    })
    .await
}

pub(crate) fn apply_forwarded_headers(
    mut request: RequestBuilder,
    headers: &[(String, String)],
) -> RequestBuilder {
    for (name, value) in headers {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(mut value) = HeaderValue::from_str(value) else {
            continue;
        };
        value.set_sensitive(crate::models::is_sensitive_request_header(name.as_str()));
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
    let value = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())?
        .trim();
    retry_after_duration_from_value(value)
}

fn retry_after_duration_from_value(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds).min(Duration::from_secs(60)));
    }
    let target = parse_http_date(value)?;
    let delay = (target - Utc::now()).num_milliseconds().max(0);
    Some(Duration::from_millis(
        u64::try_from(delay)
            .ok()?
            .min(Duration::from_secs(60).as_millis() as u64),
    ))
}

/// Return the server's Retry-After deadline in a stable form that can travel
/// with the task error and survive a worker restart. HTTP permits either a
/// delta-seconds value or an RFC 7231 HTTP-date (normally RFC 1123).
pub(super) fn retry_after_at(response: &Response) -> Option<String> {
    let value = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())?
        .trim();
    retry_after_deadline_from_value(value).map(|date| date.to_rfc3339())
}

fn retry_after_deadline_from_value(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(seconds) = value.parse::<i64>() {
        return Utc::now().checked_add_signed(chrono::Duration::try_seconds(seconds.max(0))?);
    }
    parse_http_date(value)
}

fn parse_http_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc2822(value)
        .or_else(|_| DateTime::parse_from_rfc3339(value))
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

/// SEC-11: secrets belong to a full origin (scheme, host and normalized port).
/// Unknown custom headers may carry tokens, so only public profile headers can
/// cross an origin boundary. Unparseable targets fail closed.
pub(crate) fn headers_for_origin(
    headers: &[(String, String)],
    origin: &str,
    target_url: &str,
) -> Vec<(String, String)> {
    let same_origin = url_origin(target_url).is_some_and(|target| target == origin);
    if same_origin {
        headers.to_vec()
    } else {
        headers
            .iter()
            .filter(|(name, _)| !crate::models::is_sensitive_request_header(name))
            .cloned()
            .collect()
    }
}

/// Normalizes a URL to its origin (`scheme://host:port` with default ports
/// resolved) for SEC-11 comparisons; `None` when the URL is unparseable or
/// hostless, which callers must treat as "strip credentials".
pub(crate) fn url_origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    Some(crate::download::network_policy::format_authority_url(
        parsed.scheme(),
        host,
        port,
    ))
}

#[cfg(test)]
mod origin_binding_tests {
    use super::{headers_for_origin, url_origin};

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
        let origin = url_origin("https://origin.example").expect("origin");
        let bound = headers_for_origin(&headers, &origin, "https://origin.example/seg-1.ts");
        assert_eq!(bound.len(), headers.len());
        assert!(bound.iter().any(|(n, _)| n == "Authorization"));
        assert!(bound.iter().any(|(n, _)| n == "Cookie"));
    }

    /// SEC-11: cross-host targets lose Authorization/Cookie but keep the rest.
    #[test]
    fn cross_origin_strips_credentials_only() {
        let headers = sample_headers();
        let origin = url_origin("https://origin.example").expect("origin");
        let bound = headers_for_origin(&headers, &origin, "https://cdn.other.example/seg-1.ts");
        assert!(bound.iter().all(|(n, _)| n != "Authorization"));
        assert!(bound.iter().all(|(n, _)| n != "Cookie"));
        assert!(bound.iter().any(|(n, _)| n == "User-Agent"));
        assert!(bound.iter().any(|(n, _)| n == "Referer"));
    }

    /// SEC-11: a different port on the same host is a different origin and
    /// must not receive credentials either (web origin model).
    #[test]
    fn same_host_different_port_strips_credentials() {
        let headers = sample_headers();
        let origin = url_origin("http://127.0.0.1:8001").expect("origin");
        let bound = headers_for_origin(&headers, &origin, "http://127.0.0.1:8002/seg-1.ts");
        assert!(bound.iter().all(|(n, _)| n != "Authorization"));
        assert!(bound.iter().all(|(n, _)| n != "Cookie"));
        assert!(bound.iter().any(|(n, _)| n == "User-Agent"));
    }

    /// SEC-11: default ports normalize away — `:443` on an https origin is
    /// the same origin as the bare host.
    #[test]
    fn default_ports_normalize_to_same_origin() {
        let headers = sample_headers();
        let origin = url_origin("https://origin.example").expect("origin");
        let bound = headers_for_origin(&headers, &origin, "https://origin.example:443/seg-1.ts");
        assert!(bound.iter().any(|(n, _)| n == "Authorization"));
    }

    #[test]
    fn ipv6_origin_keeps_authority_brackets() {
        assert_eq!(
            url_origin("https://[2001:db8::10]/file").as_deref(),
            Some("https://[2001:db8::10]:443")
        );
    }

    /// SEC-11: fail closed — an unparseable target strips credentials too.
    #[test]
    fn unparseable_target_strips_credentials() {
        let headers = sample_headers();
        let origin = url_origin("https://origin.example").expect("origin");
        let bound = headers_for_origin(&headers, &origin, "not a url");
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

    #[test]
    fn retry_after_accepts_delta_seconds_and_http_date() {
        let before = Utc::now();
        let delta = retry_after_deadline_from_value("45").expect("delta seconds");
        assert!(delta >= before + chrono::Duration::seconds(44));
        assert!(delta <= Utc::now() + chrono::Duration::seconds(45));

        let delayed = retry_after_duration_from_value("3600").expect("capped delta");
        assert_eq!(delayed, Duration::from_secs(60));

        let http_date = retry_after_deadline_from_value("Wed, 21 Oct 2015 07:28:00 GMT")
            .expect("RFC 1123 date");
        assert_eq!(http_date.to_rfc3339(), "2015-10-21T07:28:00+00:00");
        let date_duration = retry_after_duration_from_value("Wed, 21 Oct 2015 07:28:00 GMT")
            .expect("past date has an immediate deadline");
        assert_eq!(date_duration, Duration::ZERO);

        let rfc3339 = parse_http_date("2030-01-02T03:04:05Z");
        assert_eq!(
            rfc3339.map(|value| value.to_rfc3339()),
            Some("2030-01-02T03:04:05+00:00".to_string())
        );
    }
}
