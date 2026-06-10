mod direct;
mod error;
mod file;
mod probe;
mod request;
mod segmented;

use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use reqwest::{Client, StatusCode};
use sqlx::SqlitePool;
use tauri::AppHandle;

use super::GlobalSpeedLimiter;
use crate::{
    logging::sanitize_url,
    models::{EngineCapabilities, ProbedFile, TaskKind, TaskRecord, TaskSegmentRecord},
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
}

#[derive(Debug, Clone)]
pub struct HttpEngine {
    client: Client,
}

impl HttpEngine {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent("VibeDownloader/0.1")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

        Ok(Self { client })
    }

    pub async fn probe(&self, url: &str) -> Result<ProbeResult, String> {
        self.probe_with_headers(url, &[]).await
    }

    pub async fn probe_with_headers(
        &self,
        url: &str,
        request_headers: &[(String, String)],
    ) -> Result<ProbeResult, String> {
        let sanitized = sanitize_url(url);
        let head = send_head_with_retry(&self.client, url, request_headers).await;
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
            &self.client,
            url,
            Some("bytes=0-0".to_string()),
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

    #[allow(clippy::too_many_arguments)]
    pub async fn download(
        &self,
        app: AppHandle,
        pool: SqlitePool,
        task: TaskRecord,
        cancel: Arc<AtomicBool>,
        speed_limiter: Arc<GlobalSpeedLimiter>,
        connection_limit: usize,
        request_headers: Vec<(String, String)>,
    ) -> Result<(), String> {
        run_segmented_download(
            &self.client,
            app,
            pool,
            task,
            cancel,
            speed_limiter,
            connection_limit,
            request_headers,
        )
        .await
    }

    pub async fn download_direct(
        &self,
        request: DirectDownloadRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<i64, String> {
        run_direct_download(
            &self.client,
            request,
            cancel,
            GlobalSpeedLimiter::disabled(),
        )
        .await
    }

    pub async fn download_direct_with_limiter(
        &self,
        request: DirectDownloadRequest,
        cancel: Arc<AtomicBool>,
        speed_limiter: Arc<GlobalSpeedLimiter>,
    ) -> Result<i64, String> {
        run_direct_download(&self.client, request, cancel, speed_limiter).await
    }

    pub async fn download_segmented_direct(
        &self,
        request: DirectSegmentedDownloadRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<i64, String> {
        run_direct_segmented_download(
            &self.client,
            request,
            cancel,
            GlobalSpeedLimiter::disabled(),
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
            })
        })
    }

    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>> {
        Box::pin(async move {
            HttpEngine::download(
                self,
                context.app,
                context.pool,
                context.task,
                context.cancel,
                context.speed_limiter,
                context.connection_limit,
                context.request_headers,
            )
            .await
        })
    }
}
