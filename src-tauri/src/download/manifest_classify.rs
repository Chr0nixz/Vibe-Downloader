//! B6: bounded manifest identification on the same origin-safe HTTP path as downloads.

use super::{EngineRegistry, ProbeRequest};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE, RANGE};

const PREFIX_BYTES: usize = 4096;

pub(super) async fn identify(
    registry: &EngineRegistry,
    request: &ProbeRequest,
) -> Result<Option<&'static str>, String> {
    let proxy = match request.proxy_config.as_ref() {
        Some(proxy) => proxy.clone(),
        None => registry.proxy_config().await,
    };
    let client = registry
        .http_engine()
        .client_for_network_policy(&proxy, &request.network_policy)
        .await?;
    let headers = super::http::merge_basic_auth_headers(
        &request.request_headers,
        request.credentials.as_ref(),
    );
    let mut head = client
        .head(&request.uri)
        .header(ACCEPT_ENCODING, "identity");
    for (name, value) in &headers {
        head = head.header(name, value);
    }
    let response = super::http::send_request(head, &request.network_policy).await?;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if response.status().is_success() {
        if let Some(kind) = from_content_type(&content_type) {
            return Ok(Some(kind));
        }
        if !matches!(
            content_type.as_str(),
            "" | "application/octet-stream" | "binary/octet-stream"
        ) {
            return Ok(None);
        }
    } else if !matches!(response.status().as_u16(), 405 | 501) {
        return Ok(None);
    }
    // Even if a server ignores Range, only consume this prefix and drop the
    // response. Never buffer an arbitrary page or the whole download here.
    let mut get = client
        .get(&request.uri)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes=0-{}", PREFIX_BYTES - 1));
    for (name, value) in &headers {
        get = get.header(name, value);
    }
    let mut response = super::http::send_request(get, &request.network_policy).await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if let Some(kind) = from_content_type(&content_type) {
        return Ok(Some(kind));
    }
    if !matches!(
        content_type.as_str(),
        "" | "application/octet-stream" | "binary/octet-stream"
    ) {
        return Ok(None);
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prefix = Vec::with_capacity(PREFIX_BYTES);
    while prefix.len() < PREFIX_BYTES {
        let chunk = tokio::time::timeout_at(deadline, response.chunk())
            .await
            .map_err(|_| {
                super::error::engine_error("probe_timeout", "Manifest prefix probe timed out", true)
            })?
            .map_err(|e| e.to_string())?;
        let Some(chunk) = chunk else {
            break;
        };
        prefix.extend_from_slice(&chunk[..chunk.len().min(PREFIX_BYTES - prefix.len())]);
    }
    Ok(from_prefix(&prefix))
}

fn from_content_type(value: &str) -> Option<&'static str> {
    match value {
        "application/vnd.apple.mpegurl"
        | "application/x-mpegurl"
        | "audio/mpegurl"
        | "audio/x-mpegurl" => Some("hls"),
        "application/dash+xml" => Some("dash"),
        "application/x-bittorrent" => Some("bt"),
        "application/metalink4+xml" | "application/metalink+xml" => Some("metalink"),
        _ => None,
    }
}

fn from_prefix(prefix: &[u8]) -> Option<&'static str> {
    let text = String::from_utf8_lossy(prefix);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with("#EXTM3U") {
        return Some("hls");
    }
    if prefix.starts_with(b"d") && prefix.windows(6).any(|w| w == b"4:info") {
        return Some("bt");
    }
    // Only inspect XML's first start element, allowing a declaration/comments.
    let mut reader = quick_xml::Reader::from_str(text);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)) => {
                return match e.local_name().as_ref() {
                    b"MPD" => Some("dash"),
                    b"metalink" => Some("metalink"),
                    _ => None,
                };
            }
            Ok(quick_xml::events::Event::Decl(_) | quick_xml::events::Event::Comment(_)) => {}
            Ok(quick_xml::events::Event::Text(e)) if e.iter().all(u8::is_ascii_whitespace) => {}
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_only_supported_manifests() {
        assert_eq!(from_content_type("application/dash+xml"), Some("dash"));
        assert_eq!(from_prefix(b"#EXTM3U\n#EXTINF:1"), Some("hls"));
        assert_eq!(
            from_prefix(b"<?xml version=\"1.0\"?><MPD type=\"static\">"),
            Some("dash")
        );
        assert_eq!(
            from_prefix(b"<metalink xmlns=\"urn:ietf:params:xml:ns:metalink\">"),
            Some("metalink")
        );
        assert_eq!(from_prefix(b"<!doctype html><html><MPD>"), None);
        assert_eq!(from_prefix(b"random video.mp4 <MPD>"), None);
    }
}
