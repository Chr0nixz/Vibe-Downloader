mod direct;
mod error;
mod probe;
mod request;
mod segmented;

use std::{path::PathBuf, sync::Arc, time::Duration};

use reqwest::{Client, StatusCode};

use super::net_factory::NetworkClientFactory;
use super::network_policy::NetworkPolicy;
use super::GlobalSpeedLimiter;
use crate::{
    db,
    logging::sanitize_url,
    models::{EngineCapabilities, ProbedFile, TaskKind, TaskSegmentRecord},
    proxy::{ResolvedProxyConfig, SharedProxyConfig},
};

use self::{
    direct::run_direct_download,
    direct::run_direct_segmented_download,
    error::format_http_status_with_retry_after,
    probe::probe_from_response,
    request::{retry_after_at, send_get_with_retry, send_head_with_retry},
    segmented::run_segmented_download,
};

pub(crate) use request::{
    apply_forwarded_headers, headers_for_origin, merge_basic_auth_headers, send_request,
    send_request_with_error_mapper, url_origin,
};

/// Preserve Retry-After metadata for every HTTP-derived engine, including
/// HLS, DASH, and Metalink paths that receive the response outside the core
/// segmented worker.
pub(crate) fn format_http_response_error(response: &reqwest::Response) -> String {
    error::format_http_status_with_retry_after(response.status(), request::retry_after_at(response))
}

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
    /// SEC-03: all clients come from the shared network factory — proxy
    /// policy, SSRF resolver/redirect guards and pooling live there.
    factory: Arc<NetworkClientFactory>,
}

impl HttpEngine {
    pub fn new() -> Result<Self, String> {
        Self::with_proxy_config(ResolvedProxyConfig::shared_default())
    }

    pub fn with_proxy_config(proxy_config: SharedProxyConfig) -> Result<Self, String> {
        Ok(Self {
            proxy_config,
            factory: Arc::new(NetworkClientFactory::new()),
        })
    }

    /// EngineRegistry wiring: engines share one factory (and therefore one
    /// client cache and one invalidation path) across HTTP and BT.
    pub fn with_proxy_config_and_factory(
        proxy_config: SharedProxyConfig,
        factory: Arc<NetworkClientFactory>,
    ) -> Result<Self, String> {
        Ok(Self {
            proxy_config,
            factory,
        })
    }

    /// Returns a cached `Client` for the current global proxy configuration.
    ///
    /// E-4: Exposes the shared cache to derived engines (HLS / DASH / Metalink / WebDAV),
    /// preventing them from building clients that bypass the shared policy stack.
    pub async fn client(&self) -> Result<Client, String> {
        let config = self.proxy_config.read().await.clone();
        self.client_for_config(&config).await
    }

    /// FUN-02: Return a cached client for an explicit resolved proxy config.
    /// Task overrides (Inherit/Off/Custom) must use this instead of the global client.
    pub async fn client_for_config(&self, config: &ResolvedProxyConfig) -> Result<Client, String> {
        self.factory.client_for(config).await
    }

    pub async fn proxy_config(&self) -> ResolvedProxyConfig {
        self.proxy_config.read().await.clone()
    }

    pub async fn client_for_network_policy(
        &self,
        config: &ResolvedProxyConfig,
        network_policy: &NetworkPolicy,
    ) -> Result<Client, String> {
        self.factory.client_for_policy(config, network_policy).await
    }

    /// E-4: Returns the current client cache entry count. Used by integration tests to verify
    /// that the shared cache is correctly cleared after `set_proxy_config` (four HTTP-derived engines share the same `Arc<HttpEngine>`).
    pub async fn client_cache_len(&self) -> usize {
        self.factory.cache_len().await
    }

    /// Clear the client cache. Called when proxy configuration changes so
    /// subsequent downloads build fresh clients with the new proxy settings.
    pub async fn invalidate_clients(&self) {
        self.factory.invalidate().await;
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
        self.probe_with_headers_and_proxy_and_policy(
            url,
            request_headers,
            proxy_config,
            &NetworkPolicy::default(),
        )
        .await
    }

    pub async fn probe_with_headers_and_proxy_and_policy(
        &self,
        url: &str,
        request_headers: &[(String, String)],
        proxy_config: Option<&ResolvedProxyConfig>,
        network_policy: &NetworkPolicy,
    ) -> Result<ProbeResult, String> {
        let client = if let Some(config) = proxy_config {
            self.client_for_network_policy(config, network_policy)
                .await?
        } else {
            let config = self.proxy_config.read().await.clone();
            self.client_for_network_policy(&config, network_policy)
                .await?
        };
        let sanitized = sanitize_url(url);
        let head = send_head_with_retry(&client, url, request_headers, network_policy).await;
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
            network_policy,
        )
        .await?;

        if !response.status().is_success() {
            return Err(format_http_status_with_retry_after(
                response.status(),
                retry_after_at(&response),
            ));
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
        let client = self
            .client_for_network_policy(&context.proxy_config, &context.network_policy)
            .await?;
        // FUN-01: inject Basic Auth from encrypted task credentials at runtime.
        // Authorization is never persisted in task_request_headers.
        let credentials = db::resolve_task_credentials(&context.pool, &context.task.id).await?;
        let request_headers =
            request::merge_basic_auth_headers(&context.request_headers, credentials.as_ref());
        let request_headers = headers_for_origin(
            &request_headers,
            &url_origin(&context.task.url).unwrap_or_default(),
            context
                .task
                .final_url
                .as_deref()
                .unwrap_or(&context.task.url),
        );
        run_segmented_download(segmented::SegmentedDownloadContext {
            client: &client,
            app: context.app,
            pool: context.pool,
            task: context.task,
            cancel_token: context.cancel_token,
            speed_limiter: context.speed_limiter,
            connection_limit: context.connection_limit,
            request_headers,
            network_policy: context.network_policy,
        })
        .await
    }

    pub async fn download_direct(
        &self,
        request: DirectDownloadRequest,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        let network_policy = NetworkPolicy::default();
        run_direct_download(
            &client,
            request,
            cancel_token,
            GlobalSpeedLimiter::disabled(),
            &network_policy,
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
        let network_policy = NetworkPolicy::default();
        run_direct_download(
            &client,
            request,
            cancel_token,
            speed_limiter,
            &network_policy,
        )
        .await
    }

    pub async fn download_segmented_direct(
        &self,
        request: DirectSegmentedDownloadRequest,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<i64, String> {
        let client = self.client().await?;
        let network_policy = NetworkPolicy::default();
        run_direct_segmented_download(
            &client,
            request,
            cancel_token,
            GlobalSpeedLimiter::disabled(),
            &network_policy,
        )
        .await
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
            let probe = HttpEngine::probe_with_headers_and_proxy_and_policy(
                self,
                &request.uri,
                &headers,
                request.proxy_config.as_ref(),
                &request.network_policy,
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
        Box::pin(crate::download::lifecycle::run_owned(async move {
            HttpEngine::download(self, context)
                .await
                .map_err(DownloadError::Other)
        }))
    }
}
