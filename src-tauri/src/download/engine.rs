use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;
use tauri::AppHandle;

use super::{GlobalSpeedLimiter, HttpEngine};
use crate::models::{EngineCapabilities, ProbedFile, TaskKind, TaskRecord};

pub(crate) type EngineFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub uri: String,
    pub source: Option<String>,
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
    pub speed_limiter: Arc<GlobalSpeedLimiter>,
    pub connection_limit: usize,
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
}

impl Default for EngineRegistry {
    fn default() -> Self {
        Self::new().expect("HTTP engine initialization failed")
    }
}

impl EngineRegistry {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            engines: vec![Arc::new(HttpEngine::new()?)],
        })
    }

    pub fn engine_for_uri(&self, uri: &str) -> Result<Arc<dyn DownloadEngine>, String> {
        let parsed =
            reqwest::Url::parse(uri.trim()).map_err(|_| "Download URL is invalid.".to_string())?;
        let scheme = parsed.scheme();
        self.engines
            .iter()
            .find(|engine| engine.supports_scheme(scheme))
            .cloned()
            .ok_or_else(|| format!("The {scheme} protocol is not supported yet."))
    }
}

#[derive(Debug, Clone)]
pub struct ExternalEngineAdapter;
