mod direct;
mod error;
pub(crate) mod file;
mod probe;
mod request;
mod segmented;

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use hickory_resolver::{config::*, TokioResolver};
use reqwest::{Client, NoProxy, Proxy, StatusCode};
use tokio::sync::RwLock;

use super::GlobalSpeedLimiter;
use crate::{
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
use super::{DownloadContext, DownloadEngine, ProbeOutput, ProbeRequest};

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

    /// Returns a cached `Client` for the current proxy configuration, building
    /// one on first use. Reusing the client avoids repeated TCP/TLS handshakes
    /// when multiple downloads target the same host.
    async fn client(&self) -> Result<Client, String> {
        let config = self.proxy_config.read().await;
        let fingerprint = proxy_fingerprint(&config);
        {
            let cache = self.clients.read().await;
            if let Some(client) = cache.get(&fingerprint) {
                return Ok(client.clone());
            }
        }
        let client = build_client(&config)?;
        self.clients
            .write()
            .await
            .insert(fingerprint, client.clone());
        Ok(client)
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
        let client = self.client().await?;
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
        // Use the cached client (keyed by proxy fingerprint) instead of building
        // a fresh one per download. This reuses TCP/TLS connections across tasks.
        let client = self.client().await?;
        run_segmented_download(segmented::SegmentedDownloadContext {
            client: &client,
            app: context.app,
            pool: context.pool,
            task: context.task,
            cancel: context.cancel,
            cancel_token: context.cancel_token,
            speed_limiter: context.speed_limiter,
            connection_limit: context.connection_limit,
            request_headers: context.request_headers,
        })
        .await
    }

    pub async fn download_direct(
        &self,
        request: DirectDownloadRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        run_direct_download(&client, request, cancel, GlobalSpeedLimiter::disabled()).await
    }

    pub async fn download_direct_with_limiter(
        &self,
        request: DirectDownloadRequest,
        cancel: Arc<AtomicBool>,
        speed_limiter: Arc<GlobalSpeedLimiter>,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        run_direct_download(&client, request, cancel, speed_limiter).await
    }

    pub async fn download_segmented_direct(
        &self,
        request: DirectSegmentedDownloadRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        run_direct_segmented_download(&client, request, cancel, GlobalSpeedLimiter::disabled())
            .await
    }
}

fn proxy_fingerprint(config: &ResolvedProxyConfig) -> String {
    format!(
        "{}|{}|{}|{}",
        config.mode.as_str(),
        config.url.as_deref().unwrap_or(""),
        config.username.as_deref().unwrap_or(""),
        config.no_proxy.as_deref().unwrap_or(""),
    )
}

/// DNS resolver adapter wrapping hickory-resolver to implement reqwest's `Resolve` trait.
/// hickory-resolver provides built-in caching, avoiding repeated system DNS lookups
/// when multiple downloads target the same host.
#[derive(Clone, Debug)]
struct HickoryResolver(TokioResolver);

impl HickoryResolver {
    fn new() -> Self {
        Self(
            TokioResolver::builder_with_config(
                ResolverConfig::default(),
                hickory_resolver::net::runtime::TokioRuntimeProvider::new(),
            )
            .with_options(ResolverOpts::default())
            .build()
            .expect("failed to create DNS resolver"),
        )
    }
}

impl reqwest::dns::Resolve for HickoryResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = self.0.clone();
        Box::pin(async move {
            let lookup = resolver
                .lookup_ip(name.as_str())
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                        as Box<dyn std::error::Error + Send + Sync>
                })?;
            let addrs: Vec<SocketAddr> = lookup.iter().map(|ip| SocketAddr::new(ip, 0)).collect();
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub(crate) fn build_client(config: &ResolvedProxyConfig) -> Result<Client, String> {
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("VibeDownloader/0.1")
        .dns_resolver(Arc::new(HickoryResolver::new()))
        .connect_timeout(Duration::from_secs(30))
        // Overall request timeout: prevents downloads from hanging forever
        // when the network stalls. reqwest applies this to the full request
        // lifecycle including body streaming, so we set it generously (60s)
        // to avoid impacting legitimate large-file transfers.
        .timeout(Duration::from_secs(60));

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

    fn probe<'a>(&'a self, request: ProbeRequest) -> EngineFuture<'a, Result<ProbeOutput, String>> {
        Box::pin(async move {
            let probe =
                HttpEngine::probe_with_headers(self, &request.uri, &request.request_headers)
                    .await?;
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
                metalink: None,
            })
        })
    }

    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>> {
        Box::pin(async move { HttpEngine::download(self, context).await })
    }
}
