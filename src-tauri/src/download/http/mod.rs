mod direct;
mod error;
mod probe;
mod request;
mod segmented;

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use hickory_resolver::{config::*, TokioResolver};
use reqwest::{Client, NoProxy, Proxy, StatusCode};
use tokio::sync::RwLock;

use super::GlobalSpeedLimiter;
use crate::{
    db,
    logging::sanitize_url,
    models::{EngineCapabilities, ProbedFile, TaskKind, TaskSegmentRecord},
    proxy::{AppProxyMode, ResolvedProxyConfig, SharedProxyConfig},
};

use self::{
    direct::run_direct_download,
    direct::run_direct_segmented_download,
    error::format_http_status,
    probe::probe_from_response,
    request::{send_get_with_retry, send_head_with_retry},
    segmented::run_segmented_download,
};

use super::engine::EngineFuture;
use super::{DownloadContext, DownloadEngine, DownloadError, ProbeOutput, ProbeRequest};

/// Maximum idle time between chunk reads before a download is considered stalled.
/// Prevents stalled servers from hanging the scheduler without breaking large
/// file downloads (data flowing resets the timer each chunk).
pub(crate) const HTTP_CHUNK_READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub final_url: String,
    pub file_name: String,
    pub total_size: i64,
    pub supports_resume: bool,
    pub supports_parallel: bool,
    pub supports_multi_file: bool,
    pub source_key: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DirectDownloadRequest {
    pub url: String,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub total_size: i64,
    pub supports_resume: bool,
    pub supports_parallel: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DirectSegmentedDownloadRequest {
    pub url: String,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub total_size: i64,
    pub supports_resume: bool,
    pub supports_parallel: bool,
    pub segments: Vec<TaskSegmentRecord>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpEngine {
    proxy_config: SharedProxyConfig,
    /// Client cache keyed by proxy fingerprint to reuse TCP/TLS connections
    /// across downloads to the same host with the same proxy configuration.
    clients: Arc<RwLock<HashMap<String, Client>>>,
}

impl HttpEngine {
    pub fn new() -> Result<Self, String> {
        Self::with_proxy_config(ResolvedProxyConfig::shared_default())
    }

    pub fn with_proxy_config(proxy_config: SharedProxyConfig) -> Result<Self, String> {
        Ok(Self {
            proxy_config,
            clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Returns a cached `Client` for the current global proxy configuration.
    ///
    /// E-4: Exposes the shared cache to derived engines (HLS / DASH / Metalink / WebDAV),
    /// preventing them from calling `build_client()` to bypass the cache.
    pub async fn client(&self) -> Result<Client, String> {
        let config = self.proxy_config.read().await.clone();
        self.client_for_config(&config).await
    }

    /// FUN-02: Return a cached client for an explicit resolved proxy config.
    /// Task overrides (Inherit/Off/Custom) must use this instead of the global client.
    pub async fn client_for_config(&self, config: &ResolvedProxyConfig) -> Result<Client, String> {
        let fingerprint = config.fingerprint();
        {
            let cache = self.clients.read().await;
            if let Some(client) = cache.get(&fingerprint) {
                return Ok(client.clone());
            }
        }
        let client = build_client(config)?;
        self.clients
            .write()
            .await
            .insert(fingerprint, client.clone());
        Ok(client)
    }

    /// E-4: Returns the current client cache entry count. Used by integration tests to verify
    /// that the shared cache is correctly cleared after `set_proxy_config` (four HTTP-derived engines share the same `Arc<HttpEngine>`).
    pub async fn client_cache_len(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Clear the client cache. Called when proxy configuration changes so
    /// subsequent downloads build fresh clients with the new proxy settings.
    pub async fn invalidate_clients(&self) {
        self.clients.write().await.clear();
    }

    pub async fn probe(&self, url: &str) -> Result<ProbeResult, String> {
        self.probe_with_headers(url, &[]).await
    }

    pub async fn probe_with_headers(
        &self,
        url: &str,
        request_headers: &[(String, String)],
    ) -> Result<ProbeResult, String> {
        self.probe_with_headers_and_proxy(url, request_headers, None)
            .await
    }

    pub async fn probe_with_headers_and_proxy(
        &self,
        url: &str,
        request_headers: &[(String, String)],
        proxy_config: Option<&ResolvedProxyConfig>,
    ) -> Result<ProbeResult, String> {
        let client = if let Some(config) = proxy_config {
            self.client_for_config(config).await?
        } else {
            self.client().await?
        };
        let sanitized = sanitize_url(url);
        let head = send_head_with_retry(&client, url, request_headers).await;
        if let Ok(response) = head {
            if response.status().is_success() {
                let probe = probe_from_response(url, &response, false)?;
                if probe.total_size > 0 {
                    tracing::debug!(
                        url = %sanitized,
                        method = "HEAD",
                        total_size = probe.total_size,
                        supports_parallel = probe.supports_parallel,
                        "probe succeeded"
                    );
                    return Ok(probe);
                }
            }
            tracing::debug!(
                url = %sanitized,
                method = "HEAD",
                status = %response.status(),
                "probe head request incomplete, falling back to ranged GET"
            );
        }

        let response = send_get_with_retry(
            &client,
            url,
            Some("bytes=0-0".to_string()),
            None,
            request_headers,
        )
        .await?;

        if !response.status().is_success() {
            return Err(format_http_status(response.status()));
        }

        let probe = probe_from_response(
            url,
            &response,
            response.status() == StatusCode::PARTIAL_CONTENT,
        )?;
        tracing::debug!(
            url = %sanitized,
            method = "GET",
            status = %response.status(),
            total_size = probe.total_size,
            supports_parallel = probe.supports_parallel,
            "probe succeeded"
        );
        Ok(probe)
    }

    pub async fn download(&self, context: DownloadContext) -> Result<(), String> {
        // FUN-02: use the task-resolved proxy from DownloadContext, not only global.
        let client = self.client_for_config(&context.proxy_config).await?;
        // FUN-01: inject Basic Auth from encrypted task credentials at runtime.
        // Authorization is never persisted in task_request_headers.
        let credentials = db::resolve_task_credentials(&context.pool, &context.task.id).await?;
        let request_headers =
            request::merge_basic_auth_headers(&context.request_headers, credentials.as_ref());
        run_segmented_download(segmented::SegmentedDownloadContext {
            client: &client,
            app: context.app,
            pool: context.pool,
            task: context.task,
            cancel_token: context.cancel_token,
            speed_limiter: context.speed_limiter,
            connection_limit: context.connection_limit,
            request_headers,
        })
        .await
    }

    pub async fn download_direct(
        &self,
        request: DirectDownloadRequest,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        run_direct_download(
            &client,
            request,
            cancel_token,
            GlobalSpeedLimiter::disabled(),
        )
        .await
    }

    pub async fn download_direct_with_limiter(
        &self,
        request: DirectDownloadRequest,
        cancel_token: tokio_util::sync::CancellationToken,
        speed_limiter: Arc<GlobalSpeedLimiter>,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        run_direct_download(&client, request, cancel_token, speed_limiter).await
    }

    pub async fn download_segmented_direct(
        &self,
        request: DirectSegmentedDownloadRequest,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        run_direct_segmented_download(
            &client,
            request,
            cancel_token,
            GlobalSpeedLimiter::disabled(),
        )
        .await
    }
}

/// DNS resolver adapter wrapping hickory-resolver to implement reqwest's `Resolve` trait.
/// hickory-resolver provides built-in caching, avoiding repeated system DNS lookups
/// when multiple downloads target the same host.
#[derive(Clone, Debug)]
struct HickoryResolver(TokioResolver);

impl HickoryResolver {
    /// A-5: Returns `Result` instead of `.expect()` — system DNS configuration
    /// can be missing/corrupted (containers, chroot, minimal images), and a
    /// panic here would crash the worker or the synchronous `set_proxy_config`
    /// path. The caller (`build_client`) propagates the error as a string.
    ///
    /// Uses `TokioResolver::builder_tokio()` to read the OS DNS configuration
    /// (Windows registry / `/etc/resolv.conf`). In hickory-resolver 0.26,
    /// `ResolverConfig::default()` produces an **empty** config with zero name
    /// servers, so every DNS lookup fails with "error sending request for url".
    /// Reading the system config populates real upstream DNS servers.
    fn new() -> Result<Self, String> {
        let resolver = match TokioResolver::builder_tokio() {
            Ok(builder) => builder.build(),
            Err(_) => {
                // Fallback for environments where system DNS config is
                // unavailable (containers, chroot, minimal images).
                TokioResolver::builder_with_config(
                    ResolverConfig::default(),
                    hickory_resolver::net::runtime::TokioRuntimeProvider::new(),
                )
                .with_options(ResolverOpts::default())
                .build()
            }
        }
        .map_err(|e| format!("DNS resolver unavailable: {e}"))?;
        Ok(Self(resolver))
    }
}

impl reqwest::dns::Resolve for HickoryResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = self.0.clone();
        Box::pin(async move {
            let lookup = resolver.lookup_ip(name.as_str()).await.map_err(|e| {
                Box::new(std::io::Error::other(e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;
            // A-2: SSRF connection-time guard — filter out any resolved IP
            // that is private or reserved. If ALL resolved IPs are filtered,
            // return an error so the connection fails rather than falling
            // back to a private address. This is the second layer of defense
            // after the handoff pre-flight DNS check; it also protects
            // non-handoff paths (direct UI/clipboard task creation) where
            // the pre-flight check does not run.
            let addrs: Vec<SocketAddr> = lookup
                .iter()
                .filter(|ip| !crate::download::ssrf::is_private_ip(ip))
                .map(|ip| SocketAddr::new(ip, 0))
                .collect();
            // Return an error (not an empty iterator) when all IPs are filtered. reqwest
            // treats an empty address list as a DNS failure and may retry/hang; an explicit
            // error fails the connection fast with a diagnosable message.
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::other(
                    "SSRF guard: all resolved IPs are private or reserved",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A-2: Custom reqwest redirect policy that re-checks each redirect hop's
/// target URL against the SSRF guard. Without this, a public URL could 302
/// to an internal address (`http://127.0.0.1/...`, `http://169.254.169.254/...`)
/// and reqwest would follow it without question.
///
/// Combined with the connection-time resolver filter (`HickoryResolver::resolve`),
/// this provides defense in depth: even if a redirect slips through to a
/// hostname that resolves to a private IP, the resolver will refuse to
/// connect.
fn ssrf_safe_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        // Limit to 10 hops (matches the prior `Policy::limited(10)` behavior).
        // `previous()` includes the initial URL as the first entry, so
        // `len() > 10` means 10 redirects have already been followed.
        if attempt.previous().len() > 10 {
            return attempt.error(std::io::Error::other("too many redirects"));
        }
        if crate::download::ssrf::is_private_or_reserved_url(attempt.url()) {
            // `stop` returns the 3xx response to the caller rather than
            // following the redirect to a private address. The caller will
            // see the redirect status and handle it as a non-2xx response.
            // The connection-time resolver guard is the authoritative layer;
            // stopping the redirect is sufficient to prevent the request from
            // reaching the internal target.
            return attempt.stop();
        }
        attempt.follow()
    })
}

pub(crate) fn build_client(config: &ResolvedProxyConfig) -> Result<Client, String> {
    let resolver = HickoryResolver::new()?;
    let mut builder = Client::builder()
        .redirect(ssrf_safe_redirect_policy())
        .user_agent(concat!("VibeDownloader/", env!("CARGO_PKG_VERSION")))
        .dns_resolver(Arc::new(resolver))
        // Only set the connection establishment timeout; streaming downloads should not be subject
        // to an overall timeout, otherwise large file downloads would be interrupted after 60s.
        // Application-layer timeouts are set per-engine on the Response as needed.
        .connect_timeout(Duration::from_secs(30))
        // Connection pool optimization (E-10): increase per-host idle connections, set 90s idle timeout,
        // to avoid repeated handshakes when downloading from the same host at short intervals.
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90));

    match config.mode {
        AppProxyMode::Off => {
            builder = builder.no_proxy();
        }
        AppProxyMode::System => {}
        AppProxyMode::Custom => {
            let url = config
                .url
                .as_deref()
                .ok_or_else(|| "Custom proxy URL is not configured.".to_string())?;
            let mut proxy =
                Proxy::all(url).map_err(|_| "Custom proxy URL is invalid.".to_string())?;
            if let Some(no_proxy) = config.no_proxy.as_deref() {
                proxy = proxy.no_proxy(NoProxy::from_string(no_proxy));
            }
            if let Some(username) = &config.username {
                proxy = proxy.basic_auth(username, config.password.as_deref().unwrap_or(""));
            }
            builder = builder.proxy(proxy);
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", sanitize_proxy_error(e)))
}

fn sanitize_proxy_error(error: reqwest::Error) -> String {
    let message = error.to_string();
    if message.contains('@') {
        "proxy configuration is invalid".to_string()
    } else {
        message
    }
}

impl DownloadEngine for HttpEngine {
    fn id(&self) -> &'static str {
        "http"
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "http" | "https")
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
                Some("http"),
            );
            // FUN-01: use ProbeRequest credentials when present; otherwise load
            // from DB when a task_id is available (resume probe path).
            let credentials = if request.credentials.is_some() {
                request.credentials.clone()
            } else if let (Some(pool), Some(task_id)) = (&request.pool, &request.task_id) {
                db::resolve_task_credentials(pool, task_id)
                    .await
                    .map_err(DownloadError::Other)?
            } else {
                None
            };
            let headers =
                request::merge_basic_auth_headers(&request.request_headers, credentials.as_ref());
            let probe = HttpEngine::probe_with_headers_and_proxy(
                self,
                &request.uri,
                &headers,
                request.proxy_config.as_ref(),
            )
            .await
            .map_err(DownloadError::Other)?;
            Ok(ProbeOutput {
                protocol: reqwest::Url::parse(&probe.final_url)
                    .map(|url| url.scheme().to_string())
                    .unwrap_or_else(|_| "http".to_string()),
                task_kind: TaskKind::SingleFile,
                resolved_uri: probe.final_url.clone(),
                display_name: probe.file_name.clone(),
                total_size: probe.total_size,
                source_key: probe.source_key.clone(),
                capabilities: EngineCapabilities {
                    supports_resume: probe.supports_resume,
                    supports_parallel: probe.supports_parallel,
                    supports_multi_file: probe.supports_multi_file,
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
        })
    }

    fn download<'a>(
        &'a self,
        context: DownloadContext,
    ) -> EngineFuture<'a, Result<(), DownloadError>> {
        Box::pin(async move {
            HttpEngine::download(self, context)
                .await
                .map_err(DownloadError::Other)
        })
    }
}
