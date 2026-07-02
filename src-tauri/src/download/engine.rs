use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;
use tauri::AppHandle;

use super::{
    BtEngine, DashEngine, FtpEngine, GlobalSpeedLimiter, HlsEngine, HttpEngine, MetalinkEngine,
    SftpEngine, WebDavEngine,
};
use crate::db::TaskCredentials;
use crate::models::{
    EngineCapabilities, HlsVariant, MetalinkProbeData, ProbedFile, TaskKind, TaskRecord,
};
use crate::proxy::{ResolvedProxyConfig, SharedProxyConfig};

pub(crate) type EngineFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub uri: String,
    pub source: Option<String>,
    pub request_headers: Vec<(String, String)>,
    pub pool: Option<SqlitePool>,
    pub task_id: Option<String>,
    pub credentials: Option<TaskCredentials>,
    /// UX-6: When `Some`, engines emit `probe-phase` events for real-time
    /// stage feedback in the NewDownloadDialog. `None` for internal callers
    /// (create_task, import_urls, resume validation) that don't need UI feedback.
    pub app: Option<AppHandle>,
    /// UX-6: Correlates phase events to a specific probe invocation.
    /// The frontend passes this via `ProbeTaskInput.request_id`.
    pub request_id: Option<String>,
}

/// UX-6: Emit a probe-phase event if `app` and `request_id` are both `Some`.
/// Engines call this at real stage transitions during `probe()`.
/// No-ops silently for internal callers that pass `None`.
pub(crate) fn emit_probe_phase(
    app: &Option<AppHandle>,
    request_id: &Option<String>,
    kind: &str,
    protocol: Option<&str>,
) {
    let Some(app) = app else { return };
    let Some(request_id) = request_id else { return };
    crate::events::emit_probe_phase(
        app,
        &crate::events::ProbePhasePayload {
            request_id: request_id.clone(),
            kind: kind.to_string(),
            protocol: protocol.map(|p| p.to_string()),
        },
    );
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
    pub hls_variants: Vec<HlsVariant>,
    /// F-6: Audio renditions from `#EXT-X-MEDIA TYPE=AUDIO`.
    pub hls_audio_tracks: Vec<crate::models::HlsMediaTrack>,
    /// F-6: Subtitle renditions from `#EXT-X-MEDIA TYPE=SUBTITLES`.
    pub hls_subtitle_tracks: Vec<crate::models::HlsMediaTrack>,
    pub metalink: Option<MetalinkProbeData>,
}

#[derive(Debug, Clone)]
pub struct DownloadContext {
    pub app: AppHandle,
    pub pool: SqlitePool,
    pub task: TaskRecord,
    pub cancel_token: tokio_util::sync::CancellationToken,
    pub finish: Arc<AtomicBool>,
    pub speed_limiter: Arc<GlobalSpeedLimiter>,
    pub connection_limit: usize,
    pub request_headers: Vec<(String, String)>,
    pub proxy_config: ResolvedProxyConfig,
}

pub trait DownloadEngine: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports_scheme(&self, scheme: &str) -> bool;
    /// R-3: URL 内容级匹配（如 `is_hls_url` / `is_metalink_url`）。
    ///
    /// 默认返回 `false`，表示该引擎仅靠 `supports_scheme` 兜底。需要按 URL
    /// 路径后缀或 scheme 精确匹配的引擎应覆盖此方法，并相应提升 `priority`，
    /// 以便在 `supports_scheme` 兜底之前被 `engine_for_uri` 选中。
    fn matches_url(&self, _url: &reqwest::Url) -> bool {
        false
    }
    /// R-3: 路由优先级，数值越大越优先。默认 `0`。
    ///
    /// URL 内容匹配型引擎（BT/HLS/DASH/Metalink）应返回正数，确保在
    /// `supports_scheme` 兜底之前被检查。同优先级引擎按注册顺序遍历。
    fn priority(&self) -> i32 {
        0
    }
    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, super::error::DownloadError>>;
    fn download<'a>(
        &'a self,
        context: DownloadContext,
    ) -> EngineFuture<'a, Result<(), super::error::DownloadError>>;
}

#[derive(Clone)]
pub struct EngineRegistry {
    engines: Vec<Arc<dyn DownloadEngine>>,
    bt_engine: Arc<BtEngine>,
    http_engine: Arc<HttpEngine>,
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
        let http_engine = Arc::new(HttpEngine::with_proxy_config(proxy_config.clone())?);
        Ok(Self {
            engines: vec![
                bt_engine.clone(),
                // E-4: HLS/DASH/Metalink/WebDAV 共享同一 `Arc<HttpEngine>`，
                // 复用其客户端缓存与 `invalidate_clients` 失效路径。
                Arc::new(MetalinkEngine::new(http_engine.clone())),
                Arc::new(HlsEngine::new(http_engine.clone())),
                Arc::new(DashEngine::new(http_engine.clone())),
                Arc::new(WebDavEngine::new(http_engine.clone())),
                http_engine.clone(),
                Arc::new(FtpEngine::new(proxy_config.clone())),
                Arc::new(SftpEngine::new(proxy_config.clone())),
            ],
            bt_engine,
            http_engine,
            proxy_config,
        })
    }

    pub async fn set_proxy_config(&self, config: ResolvedProxyConfig) {
        *self.proxy_config.write().await = config;
        self.http_engine.invalidate_clients().await;
    }

    /// E-4: 暴露共享 HTTP 引擎引用，供集成测试验证客户端缓存共享与失效。
    pub fn http_engine(&self) -> &Arc<HttpEngine> {
        &self.http_engine
    }

    pub async fn proxy_config(&self) -> ResolvedProxyConfig {
        self.proxy_config.read().await.clone()
    }

    pub fn engine_for_uri(&self, uri: &str) -> Result<Arc<dyn DownloadEngine>, String> {
        let parsed =
            reqwest::Url::parse(uri.trim()).map_err(|_| "Download URL is invalid.".to_string())?;
        let scheme = parsed.scheme();
        // R-3: 引擎自描述 matches_url + priority，按优先级降序遍历，命中即返回。
        // sort_by_key 是稳定排序，同优先级引擎保持注册顺序。
        // Clone the Vec (bumps Arc refcounts) so iteration yields owned `Arc<dyn>`
        // and `.clone()` resolves to `<Arc<dyn> as Clone>::clone` returning `Arc<dyn>`.
        let mut sorted = self.engines.clone();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.priority()));
        for engine in &sorted {
            if engine.matches_url(&parsed) {
                return Ok(Arc::clone(engine));
            }
        }
        // 兜底：supports_scheme（HTTP/FTP/SFTP/WebDAV 等仅按 scheme 匹配的引擎）
        sorted
            .into_iter()
            .find(|engine| engine.supports_scheme(scheme))
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
