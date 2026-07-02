use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use quick_xml::{events::Event, Reader};
use reqwest::{
    header::{HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Method, RequestBuilder, Url,
};

use super::{
    engine::EngineFuture, http::HttpEngine, DownloadContext, DownloadEngine, DownloadError,
    ProbeOutput, ProbeRequest,
};
use crate::{
    db,
    download::error::engine_error,
    models::{
        EngineCapabilities, ProbedFile, TaskKind, WebDavDirectoryEntry, WebDavDirectoryProbe,
    },
    proxy::ResolvedProxyConfig,
};

#[derive(Debug, Clone)]
pub struct WebDavEngine {
    /// E-4: 共享 `HttpEngine` 的客户端缓存与 probe/download 实现。WebDAV 把
    /// webdav://webdavs:// 映射为 http://https:// 后委托给 HttpEngine，因此
    /// 直接复用其缓存而非每次构造新实例。
    http: Arc<HttpEngine>,
}

#[derive(Debug, Clone)]
struct WebDavTarget {
    sanitized_uri: String,
    protocol: String,
    http_url: String,
    source_key: String,
    file_name: String,
    credentials: Option<WebDavCredentials>,
}

#[derive(Debug, Clone)]
struct WebDavCredentials {
    username: String,
    password: String,
}

#[derive(Debug, Default)]
struct ParsedDavResponse {
    href: String,
    display_name: Option<String>,
    content_length: Option<i64>,
    content_type: Option<String>,
    is_collection: bool,
}

impl WebDavEngine {
    pub fn new(http: Arc<HttpEngine>) -> Self {
        Self { http }
    }

    async fn probe_target(&self, request: &ProbeRequest) -> Result<ProbeOutput, String> {
        let target = WebDavTarget::parse_file(&request.uri)?;
        let credentials = resolve_probe_credentials(&target, request).await?;
        let headers = webdav_request_headers(&request.request_headers, credentials.as_ref());
        // E-4: 复用共享 HttpEngine 的客户端缓存，而非每次构造新实例。
        let probe = self.http.probe_with_headers(&target.http_url, &headers).await?;
        let final_url = webdav_url_from_http(&probe.final_url, &target.protocol)?;
        Ok(ProbeOutput {
            protocol: target.protocol.clone(),
            task_kind: TaskKind::SingleFile,
            resolved_uri: final_url,
            display_name: probe.file_name.clone(),
            total_size: probe.total_size,
            source_key: target.source_key,
            capabilities: EngineCapabilities {
                supports_resume: probe.supports_resume,
                supports_parallel: probe.supports_parallel,
                supports_multi_file: false,
            },
            files: vec![ProbedFile {
                relative_path: probe.file_name,
                size: probe.total_size.to_string(),
                content_type: probe.content_type.clone(),
            }],
            etag: probe.etag,
            last_modified: probe.last_modified,
            content_type: probe.content_type,
            hls_variants: Vec::new(),
            hls_audio_tracks: Vec::new(),
            hls_subtitle_tracks: Vec::new(),
            metalink: None,
        })
    }

    async fn run_download(&self, context: DownloadContext) -> Result<(), String> {
        let url_target = WebDavTarget::parse_file(&context.task.url)?;
        let final_target = context
            .task
            .final_url
            .as_deref()
            .map(WebDavTarget::parse_file)
            .transpose()?
            .unwrap_or_else(|| url_target.clone());
        let credentials = db::resolve_task_credentials(&context.pool, &context.task.id)
            .await?
            .map(|credentials| WebDavCredentials {
                username: credentials.username,
                password: credentials.password,
            })
            .or_else(|| url_target.credentials.clone())
            .or_else(|| final_target.credentials.clone());
        let request_headers =
            webdav_request_headers(&context.request_headers, credentials.as_ref());
        let mut task = context.task.clone();
        task.url = url_target.http_url;
        task.final_url = Some(final_target.http_url);
        // E-4: 复用共享 HttpEngine 的客户端缓存与下载实现。
        self.http.download(DownloadContext {
            task,
            request_headers,
            ..context
        })
        .await
    }
}

impl DownloadEngine for WebDavEngine {
    fn id(&self) -> &'static str {
        "webdav"
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "webdav" | "webdavs")
    }

    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, DownloadError>> {
        Box::pin(async move {
            crate::download::engine::emit_probe_phase(
                &request.app,
                &request.request_id,
                "connecting",
                Some("webdav"),
            );
            self.probe_target(&request)
                .await
                .map_err(DownloadError::Other)
        })
    }

    fn download<'a>(
        &'a self,
        context: DownloadContext,
    ) -> EngineFuture<'a, Result<(), DownloadError>> {
        Box::pin(async move {
            self.run_download(context)
                .await
                .map_err(DownloadError::Other)
        })
    }
}

pub async fn probe_webdav_directory_url(
    input_url: &str,
    proxy_config: ResolvedProxyConfig,
) -> Result<WebDavDirectoryProbe, String> {
    let target = WebDavTarget::parse_directory(input_url)?;
    let client = super::http::build_client(&proxy_config)?;
    let credentials = target.credentials.clone();
    let headers = webdav_request_headers(&[], credentials.as_ref());
    let mut request = client
        .request(
            Method::from_bytes(b"PROPFIND").expect("PROPFIND method"),
            &target.http_url,
        )
        .header(HeaderName::from_static("depth"), "1")
        .header(CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(
            r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:displayname/>
    <d:resourcetype/>
    <d:getcontentlength/>
    <d:getcontenttype/>
  </d:prop>
</d:propfind>"#,
        );
    request = apply_forwarded_headers(request, &headers);
    let response = request.send().await.map_err(|e| {
        engine_error(
            "webdav_propfind_failed",
            format!("WebDAV PROPFIND failed: {e}"),
            true,
        )
    })?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("Could not read WebDAV directory response: {e}"))?;
    if !status.is_success() {
        return Err(engine_error(
            "webdav_propfind_failed",
            format!("WebDAV directory returned {status}."),
            true,
        ));
    }
    let entries = parse_webdav_multistatus(&target, &text)?;
    Ok(WebDavDirectoryProbe {
        input_url: input_url.to_string(),
        directory_url: target.sanitized_uri,
        entries,
        diagnostics: vec![format!("PROPFIND returned {status}")],
    })
}

async fn resolve_probe_credentials(
    target: &WebDavTarget,
    request: &ProbeRequest,
) -> Result<Option<WebDavCredentials>, String> {
    if target.credentials.is_some() {
        return Ok(target.credentials.clone());
    }
    let Some(pool) = request.pool.as_ref() else {
        return Ok(None);
    };
    let Some(task_id) = request.task_id.as_deref() else {
        return Ok(None);
    };
    Ok(db::resolve_task_credentials(pool, task_id)
        .await?
        .map(|credentials| WebDavCredentials {
            username: credentials.username,
            password: credentials.password,
        }))
}

fn webdav_request_headers(
    base_headers: &[(String, String)],
    credentials: Option<&WebDavCredentials>,
) -> Vec<(String, String)> {
    let mut headers = base_headers.to_vec();
    if let Some(credentials) = credentials {
        let has_authorization = headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(AUTHORIZATION.as_str()));
        if !has_authorization {
            let token =
                STANDARD.encode(format!("{}:{}", credentials.username, credentials.password));
            headers.push(("Authorization".to_string(), format!("Basic {token}")));
        }
    }
    headers
}

fn apply_forwarded_headers(
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

impl WebDavTarget {
    fn parse_file(input: &str) -> Result<Self, String> {
        let target = Self::parse(input)?;
        if target.file_name.is_empty() {
            return Err("WebDAV URL must point to a file, not a directory.".to_string());
        }
        Ok(target)
    }

    fn parse_directory(input: &str) -> Result<Self, String> {
        Self::parse(input)
    }

    fn parse(input: &str) -> Result<Self, String> {
        let parsed = Url::parse(input.trim()).map_err(|_| "WebDAV URL is invalid.".to_string())?;
        let protocol = parsed.scheme().to_ascii_lowercase();
        if !matches!(protocol.as_str(), "webdav" | "webdavs") {
            return Err(format!(
                "The {} protocol is not supported by the WebDAV engine.",
                protocol
            ));
        }
        let host = parsed
            .host_str()
            .filter(|host| !host.is_empty())
            .ok_or_else(|| "WebDAV URL is missing a host.".to_string())?
            .to_string();
        let port = parsed
            .port()
            .unwrap_or(if protocol == "webdavs" { 443 } else { 80 });
        let sanitized_uri = sanitized_webdav_url(&parsed)?;
        let http_url = http_url_from_webdav(&parsed, &protocol)?;
        let path = percent_decode_lossy(parsed.path());
        let file_name = if path.ends_with('/') {
            String::new()
        } else {
            let raw_name = path
                .rsplit('/')
                .find(|part| !part.is_empty())
                .unwrap_or("download")
                .to_string();
            crate::download::sanitize::sanitize_single_file_name(&raw_name)
        };
        let credentials = credentials_from_url(&parsed);
        let source_key = format!("{protocol}://{host}:{port}");
        Ok(Self {
            sanitized_uri,
            protocol,
            http_url,
            source_key,
            file_name,
            credentials,
        })
    }
}

fn credentials_from_url(parsed: &Url) -> Option<WebDavCredentials> {
    if parsed.username().is_empty() && parsed.password().is_none() {
        return None;
    }
    Some(WebDavCredentials {
        username: percent_decode_lossy(parsed.username()),
        password: parsed
            .password()
            .map(percent_decode_lossy)
            .unwrap_or_default(),
    })
}

fn sanitized_webdav_url(parsed: &Url) -> Result<String, String> {
    let mut sanitized = parsed.clone();
    sanitized
        .set_username("")
        .map_err(|_| "WebDAV URL credentials could not be removed.".to_string())?;
    let _ = sanitized.set_password(None);
    Ok(sanitized.to_string())
}

fn http_url_from_webdav(parsed: &Url, protocol: &str) -> Result<String, String> {
    let http_scheme = if protocol == "webdavs" {
        "https"
    } else {
        "http"
    };
    let (_, rest) = parsed
        .as_str()
        .split_once(':')
        .ok_or_else(|| "WebDAV URL is invalid.".to_string())?;
    let mut http =
        Url::parse(&format!("{http_scheme}:{rest}")).map_err(|_| "WebDAV URL is invalid.")?;
    http.set_username("")
        .map_err(|_| "WebDAV URL credentials could not be removed.".to_string())?;
    let _ = http.set_password(None);
    Ok(http.to_string())
}

fn webdav_url_from_http(http_url: &str, original_protocol: &str) -> Result<String, String> {
    let parsed =
        Url::parse(http_url).map_err(|_| "WebDAV redirected URL is invalid.".to_string())?;
    let protocol = match parsed.scheme() {
        "http" => "webdav",
        "https" => "webdavs",
        _ => original_protocol,
    };
    let (_, rest) = parsed
        .as_str()
        .split_once(':')
        .ok_or_else(|| "WebDAV redirected URL is invalid.".to_string())?;
    Ok(format!("{protocol}:{rest}"))
}

fn parse_webdav_multistatus(
    target: &WebDavTarget,
    text: &str,
) -> Result<Vec<WebDavDirectoryEntry>, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut current: Option<ParsedDavResponse> = None;
    let mut text_target: Option<String> = None;
    let mut responses = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "response" => current = Some(ParsedDavResponse::default()),
                    "href" | "displayname" | "getcontentlength" | "getcontenttype" => {
                        text_target = Some(name);
                    }
                    "collection" => {
                        if let Some(current) = current.as_mut() {
                            current.is_collection = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(event))
                if local_name(event.name().as_ref()) == "collection" =>
            {
                if let Some(current) = current.as_mut() {
                    current.is_collection = true;
                }
            }
            Ok(Event::Text(event)) => {
                if let (Some(current), Some(target)) = (current.as_mut(), text_target.as_deref()) {
                    let value = event
                        .decode()
                        .map(|value| value.into_owned())
                        .unwrap_or_default();
                    match target {
                        "href" => current.href = value.trim().to_string(),
                        "displayname" => {
                            let value = value.trim();
                            if !value.is_empty() {
                                current.display_name = Some(percent_decode_lossy(value));
                            }
                        }
                        "getcontentlength" => {
                            current.content_length = value.trim().parse::<i64>().ok();
                        }
                        "getcontenttype" => {
                            let value = value.trim();
                            if !value.is_empty() {
                                current.content_type = Some(value.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(event)) => {
                let name = local_name(event.name().as_ref());
                if matches!(
                    name.as_str(),
                    "href" | "displayname" | "getcontentlength" | "getcontenttype"
                ) {
                    text_target = None;
                }
                if name == "response" {
                    if let Some(response) = current.take() {
                        responses.push(response);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(engine_error(
                    "webdav_invalid_multistatus",
                    format!("WebDAV directory response could not be parsed: {error}"),
                    false,
                ))
            }
            _ => {}
        }
        buf.clear();
    }

    let base_http =
        Url::parse(&target.http_url).map_err(|_| "WebDAV directory URL is invalid.".to_string())?;
    let base_norm = normalize_url_for_compare(base_http.as_str());
    Ok(responses
        .into_iter()
        .filter_map(|response| response.into_entry(target, &base_http, &base_norm))
        .take(200)
        .collect())
}

impl ParsedDavResponse {
    fn into_entry(
        self,
        target: &WebDavTarget,
        base_http: &Url,
        base_norm: &str,
    ) -> Option<WebDavDirectoryEntry> {
        if self.href.trim().is_empty() {
            return None;
        }
        let href_http = if let Ok(url) = Url::parse(&self.href) {
            url
        } else {
            base_http.join(&self.href).ok()?
        };
        if normalize_url_for_compare(href_http.as_str()) == base_norm {
            return None;
        }
        let href = webdav_url_from_http(href_http.as_str(), &target.protocol).ok()?;
        let probable_file_url = (!self.is_collection).then_some(href.clone());
        let name = self
            .display_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| name_from_href(&href_http));
        Some(WebDavDirectoryEntry {
            name,
            href,
            is_collection: self.is_collection,
            size: self
                .content_length
                .filter(|size| *size >= 0)
                .map(|size| size.to_string()),
            content_type: self.content_type,
            probable_file_url,
        })
    }
}

fn normalize_url_for_compare(value: &str) -> String {
    value.trim_end_matches('/').to_ascii_lowercase()
}

fn name_from_href(url: &Url) -> String {
    let path = percent_decode_lossy(url.path())
        .trim_end_matches('/')
        .to_string();
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("resource")
        .to_string()
}

fn local_name(name: &[u8]) -> String {
    let value = String::from_utf8_lossy(name);
    value
        .rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(&value)
        .to_ascii_lowercase()
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(hex);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_webdav_url_to_http_without_credentials() {
        let target = WebDavTarget::parse_file("webdavs://alice:s3cret@example.com/dir/file.bin")
            .expect("target");

        assert_eq!(target.http_url, "https://example.com/dir/file.bin");
        assert_eq!(target.sanitized_uri, "webdavs://example.com/dir/file.bin");
        assert_eq!(target.file_name, "file.bin");
        assert_eq!(target.credentials.expect("credentials").username, "alice");
    }

    #[test]
    fn parses_multistatus_entries() {
        let target = WebDavTarget::parse_directory("webdavs://example.com/files/").expect("target");
        let entries = parse_webdav_multistatus(
            &target,
            r#"
            <d:multistatus xmlns:d="DAV:">
              <d:response>
                <d:href>/files/</d:href>
                <d:propstat><d:prop><d:resourcetype><d:collection /></d:resourcetype></d:prop></d:propstat>
              </d:response>
              <d:response>
                <d:href>/files/movie.mp4</d:href>
                <d:propstat><d:prop>
                  <d:displayname>movie.mp4</d:displayname>
                  <d:getcontentlength>42</d:getcontentlength>
                  <d:getcontenttype>video/mp4</d:getcontenttype>
                </d:prop></d:propstat>
              </d:response>
              <d:response>
                <d:href>/files/sub/</d:href>
                <d:propstat><d:prop><d:resourcetype><d:collection /></d:resourcetype></d:prop></d:propstat>
              </d:response>
            </d:multistatus>
            "#,
        )
        .expect("entries");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "movie.mp4");
        assert_eq!(
            entries[0].probable_file_url.as_deref(),
            Some("webdavs://example.com/files/movie.mp4")
        );
        assert!(entries[1].is_collection);
    }
}
