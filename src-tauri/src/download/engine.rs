use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;
use tauri::AppHandle;

use super::{BtEngine, FtpEngine, GlobalSpeedLimiter, HlsEngine, HttpEngine};
use crate::models::{EngineCapabilities, ProbedFile, TaskKind, TaskRecord};
use crate::proxy::{ResolvedProxyConfig, SharedProxyConfig};

pub(crate) type EngineFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub uri: String,
    pub source: Option<String>,
    pub request_headers: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ProbeOutput {
    pub protocol: String,
    pub task_kind: TaskKind,
    pub resolved_uri: String,
    pub display_name: String,
    pub total_size: i64,
    pub source_key: String,
    pub capabilities: EngineCapabilities,
    pub files: Vec<ProbedFile>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DownloadContext {
    pub app: AppHandle,
    pub pool: SqlitePool,
    pub task: TaskRecord,
    pub cancel: Arc<AtomicBool>,
    pub finish: Arc<AtomicBool>,
    pub speed_limiter: Arc<GlobalSpeedLimiter>,
    pub connection_limit: usize,
    pub request_headers: Vec<(String, String)>,
}

pub trait DownloadEngine: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports_scheme(&self, scheme: &str) -> bool;
    fn probe<'a>(&'a self, request: ProbeRequest) -> EngineFuture<'a, Result<ProbeOutput, String>>;
    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>>;
}

#[derive(Clone)]
pub struct EngineRegistry {
    engines: Vec<Arc<dyn DownloadEngine>>,
    bt_engine: Arc<BtEngine>,
    proxy_config: SharedProxyConfig,
}

impl Default for EngineRegistry {
    fn default() -> Self {
        Self::new().expect("HTTP engine initialization failed")
    }
}

impl EngineRegistry {
    pub fn new() -> Result<Self, String> {
        let proxy_config = ResolvedProxyConfig::shared_default();
        let bt_engine = Arc::new(BtEngine::new(proxy_config.clone()));
        Ok(Self {
            engines: vec![
                bt_engine.clone(),
                Arc::new(HlsEngine::new(proxy_config.clone())),
                Arc::new(HttpEngine::with_proxy_config(proxy_config.clone())?),
                Arc::new(FtpEngine::new(proxy_config.clone())),
            ],
            bt_engine,
            proxy_config,
        })
    }

    pub async fn set_proxy_config(&self, config: ResolvedProxyConfig) {
        *self.proxy_config.write().await = config;
    }

    pub fn engine_for_uri(&self, uri: &str) -> Result<Arc<dyn DownloadEngine>, String> {
        let parsed =
            reqwest::Url::parse(uri.trim()).map_err(|_| "Download URL is invalid.".to_string())?;
        let scheme = parsed.scheme();
        if scheme == "magnet" || scheme == "file" || is_torrent_url(&parsed) {
            return Ok(self.bt_engine.clone());
        }
        if is_hls_url(&parsed) {
            return self
                .engines
                .iter()
                .find(|engine| engine.id() == "hls")
                .cloned()
                .ok_or_else(|| "HLS engine is not available.".to_string());
        }
        self.engines
            .iter()
            .find(|engine| engine.supports_scheme(scheme))
            .cloned()
            .ok_or_else(|| format!("The {scheme} protocol is not supported yet."))
    }

    pub async fn delete_runtime_task(&self, task: &TaskRecord, delete_files: bool) {
        if matches!(task.protocol.as_str(), "bt" | "magnet") {
            self.bt_engine
                .delete_runtime_task(&task.source_key, delete_files)
                .await;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExternalEngineAdapter;

fn is_torrent_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".torrent"))
}

fn is_hls_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".m3u8"))
}
