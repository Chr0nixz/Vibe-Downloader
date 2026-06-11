use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

use librqbit::{
    api::{Api, ApiAddTorrentResponse, TorrentDetailsResponse, TorrentIdOrHash},
    AddTorrent, AddTorrentOptions, Session,
};
use magnet_url::Magnet;
use reqwest::Url;
use tokio::sync::Mutex;

use super::engine::{DownloadContext, DownloadEngine, EngineFuture, ProbeOutput, ProbeRequest};
use crate::{
    db,
    events::{emit_task_progress, emit_task_updated_record},
    models::{
        EngineCapabilities, ProbedFile, SegmentStatus, TaskFileRecord, TaskKind,
        TaskProgressPayload, TaskStatus,
    },
};

const PROTOCOL_BT: &str = "bt";
const SOURCE_BT_PREFIX: &str = "bt:";

#[derive(Clone, Default)]
pub struct BtEngine {
    sessions: Arc<Mutex<HashMap<String, Arc<Api>>>>,
}

impl BtEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn delete_runtime_task(&self, source_key: &str, delete_files: bool) {
        let Some(info_hash) = info_hash_from_source_key(source_key) else {
            return;
        };
        let Ok(id) = TorrentIdOrHash::try_from(info_hash) else {
            return;
        };

        let sessions = self.sessions.lock().await;
        for api in sessions.values() {
            let result = if delete_files {
                api.api_torrent_action_delete(id).await
            } else {
                api.api_torrent_action_forget(id).await
            };
            if result.is_ok() {
                break;
            }
        }
    }

    async fn api_for_output_folder(&self, output_folder: &str) -> Result<Arc<Api>, String> {
        let key = PathBuf::from(output_folder)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(output_folder))
            .to_string_lossy()
            .to_string();

        let mut sessions = self.sessions.lock().await;
        if let Some(api) = sessions.get(&key) {
            return Ok(api.clone());
        }

        std::fs::create_dir_all(&key)
            .map_err(|e| format!("Could not create the torrent download directory: {e}"))?;
        let session = Session::new(PathBuf::from(&key))
            .await
            .map_err(|e| format!("Could not start BitTorrent session: {e:#}"))?;
        let api = Arc::new(Api::new(session, None));
        sessions.insert(key, api.clone());
        Ok(api)
    }
}

impl DownloadEngine for BtEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_BT
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "magnet" | "file")
    }

    fn probe<'a>(&'a self, request: ProbeRequest) -> EngineFuture<'a, Result<ProbeOutput, String>> {
        Box::pin(async move { probe_torrent(&request.uri).await })
    }

    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>> {
        Box::pin(async move { run_torrent_download(self.clone(), context).await })
    }
}

async fn probe_torrent(uri: &str) -> Result<ProbeOutput, String> {
    if uri.trim_start().starts_with("magnet:") {
        return probe_magnet(uri);
    }

    let add = add_torrent_source(uri).await?;
    let probe_dir = std::env::temp_dir().join("vibe-downloader-bt-probe");
    std::fs::create_dir_all(&probe_dir)
        .map_err(|e| format!("Could not create the torrent probe directory: {e}"))?;
    let session = Session::new(probe_dir.clone())
        .await
        .map_err(|e| format!("Could not start BitTorrent probe session: {e:#}"))?;
    let api = Api::new(session, None);
    let response = api
        .api_add_torrent(
            add,
            Some(AddTorrentOptions {
                paused: true,
                list_only: true,
                output_folder: Some(probe_dir.to_string_lossy().to_string()),
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| format!("Could not inspect torrent metadata: {e:#}"))?;
    Ok(probe_from_torrent_details(uri, &response.details))
}

fn probe_magnet(uri: &str) -> Result<ProbeOutput, String> {
    let magnet = Magnet::new(uri).map_err(|_| "Magnet link is invalid.".to_string())?;
    let hash = magnet
        .hash()
        .map(str::to_ascii_lowercase)
        .filter(|hash| hash.len() == 40 || hash.len() == 32)
        .ok_or_else(|| "Magnet link must include a BitTorrent info hash.".to_string())?;
    let display_name = magnet
        .display_name()
        .map(percent_decode_lossy)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("magnet-{hash}"));

    Ok(ProbeOutput {
        protocol: PROTOCOL_BT.to_string(),
        task_kind: TaskKind::MultiFile,
        resolved_uri: uri.to_string(),
        display_name,
        total_size: magnet
            .length()
            .and_then(|value| i64::try_from(value).ok())
            .unwrap_or(0),
        source_key: format!("{SOURCE_BT_PREFIX}{hash}"),
        capabilities: bt_capabilities(),
        files: Vec::new(),
        etag: None,
        last_modified: None,
        content_type: Some("application/x-bittorrent".to_string()),
    })
}

fn probe_from_torrent_details(uri: &str, details: &TorrentDetailsResponse) -> ProbeOutput {
    let files = torrent_files_from_details(details);
    let total_size = files.iter().map(|file| parse_i64(&file.size)).sum::<i64>();
    let display_name = details
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("torrent-{}", details.info_hash));

    ProbeOutput {
        protocol: PROTOCOL_BT.to_string(),
        task_kind: if files.len() > 1 {
            TaskKind::MultiFile
        } else {
            TaskKind::SingleFile
        },
        resolved_uri: format!("{SOURCE_BT_PREFIX}{}", details.info_hash),
        display_name,
        total_size,
        source_key: format!("{SOURCE_BT_PREFIX}{}", details.info_hash),
        capabilities: bt_capabilities(),
        files,
        etag: None,
        last_modified: None,
        content_type: Some(
            if uri.starts_with("magnet:") {
                "application/x-magnet"
            } else {
                "application/x-bittorrent"
            }
            .to_string(),
        ),
    }
}

async fn run_torrent_download(engine: BtEngine, context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel,
        ..
    } = context;

    let api = engine.api_for_output_folder(&task.save_dir).await?;
    let add = add_torrent_source(&task.url).await?;
    let response = api
        .api_add_torrent(
            add,
            Some(AddTorrentOptions {
                paused: false,
                overwrite: true,
                output_folder: Some(task.save_dir.clone()),
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| format!("Could not add torrent: {e:#}"))?;

    persist_torrent_details(&pool, &task, &response).await?;
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }

    let torrent_id = TorrentIdOrHash::try_from(response.details.info_hash.as_str())
        .map_err(|e| format!("Torrent info hash is invalid: {e:#}"))?;
    let mut last_progress = 0_i64;
    let mut last_tick = Instant::now();

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = api.api_torrent_action_pause(torrent_id).await;
            db::update_task_progress(&pool, &task.id, last_progress, 0, 0, TaskStatus::Paused)
                .await?;
            return Ok(());
        }

        let stats = api
            .api_stats_v1(torrent_id)
            .map_err(|e| format!("Could not read torrent stats: {e:#}"))?;
        let downloaded = i64::try_from(stats.progress_bytes).unwrap_or(i64::MAX);
        let total = i64::try_from(stats.total_bytes).unwrap_or(i64::MAX);
        let speed = stats
            .live
            .as_ref()
            .map(|live| i64::try_from(live.download_speed.as_bytes()).unwrap_or(i64::MAX))
            .unwrap_or_else(|| {
                let elapsed = last_tick.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    ((downloaded - last_progress).max(0) as f64 / elapsed) as i64
                } else {
                    0
                }
            });
        let upload_speed = stats
            .live
            .as_ref()
            .map(|live| i64::try_from(live.upload_speed.as_bytes()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        let uploaded = i64::try_from(stats.uploaded_bytes).unwrap_or(i64::MAX);
        let ratio = if downloaded > 0 {
            uploaded as f64 / downloaded as f64
        } else {
            0.0
        };

        db::update_task_progress(
            &pool,
            &task.id,
            downloaded,
            speed,
            task.connection_count.max(1),
            TaskStatus::Downloading,
        )
        .await?;
        if let Some(segment) = db::get_first_segment_record(&pool, &task.id).await? {
            db::update_segment_runtime_progress(
                &pool,
                &segment.id,
                downloaded,
                speed,
                SegmentStatus::Downloading,
            )
            .await?;
        }
        db::upsert_torrent_runtime_snapshot(
            &pool,
            &task.id,
            "ready",
            0,
            0,
            0,
            0,
            uploaded,
            upload_speed,
            ratio,
        )
        .await?;
        emit_task_progress(
            &app,
            &TaskProgressPayload {
                task_id: task.id.clone(),
                downloaded_bytes: downloaded.to_string(),
                total_size: total.to_string(),
                speed_bps: speed.to_string(),
                connection_count: task.connection_count.max(1),
                status: TaskStatus::Downloading,
            },
        );

        if stats.finished || (total > 0 && downloaded >= total) {
            db::complete_task(&pool, &task.id).await?;
            if let Some(current) = db::get_task_record(&pool, &task.id).await? {
                emit_task_updated_record(&app, &pool, &current).await;
            }
            return Ok(());
        }

        last_progress = downloaded;
        last_tick = Instant::now();
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn persist_torrent_details(
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    response: &ApiAddTorrentResponse,
) -> Result<(), String> {
    let details = &response.details;
    let files = torrent_files_from_details(details);
    let total_size = files.iter().map(|file| parse_i64(&file.size)).sum::<i64>();
    let source_key = format!("{SOURCE_BT_PREFIX}{}", details.info_hash);
    let name = details
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| task.file_name.clone());

    db::update_task_torrent_metadata(
        pool,
        &task.id,
        &source_key,
        &name,
        total_size,
        &source_key,
        files.len() > 1,
    )
    .await?;
    db::upsert_torrent_task(
        pool,
        &task.id,
        &details.info_hash,
        &name,
        task.url.starts_with("magnet:").then_some(task.url.as_str()),
        None,
        0,
        i64::from(details.total_pieces),
        false,
        None,
    )
    .await?;

    if !files.is_empty() {
        db::delete_task_files_for_task(pool, &task.id).await?;
        for file in files {
            let record = task_file_record_from_probed_file(task, &file);
            db::insert_task_file_record(pool, &record).await?;
        }
    }
    Ok(())
}

fn task_file_record_from_probed_file(
    task: &crate::models::TaskRecord,
    file: &ProbedFile,
) -> TaskFileRecord {
    let relative = sanitize_relative_path(&file.relative_path);
    let final_path = PathBuf::from(&task.save_dir).join(&relative);
    let file_name = relative
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&file.relative_path)
        .to_string();

    TaskFileRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        file_name,
        save_dir: task.save_dir.clone(),
        temp_path: None,
        final_path: Some(final_path.to_string_lossy().to_string()),
        total_size: parse_i64(&file.size),
        downloaded_bytes: 0,
        selected: true,
        status: TaskStatus::Queued,
        content_type: file.content_type.clone(),
    }
}

async fn add_torrent_source(uri: &str) -> Result<AddTorrent<'static>, String> {
    let trimmed = uri.trim();
    if trimmed.starts_with("magnet:") {
        return Ok(AddTorrent::from_url(trimmed.to_string()));
    }

    let parsed = Url::parse(trimmed).map_err(|_| "Torrent URL is invalid.".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok(AddTorrent::from_url(trimmed.to_string())),
        "file" => {
            let path = parsed
                .to_file_path()
                .map_err(|_| "Torrent file path is invalid.".to_string())?;
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("Could not read torrent file {}: {e}", path.display()))?;
            Ok(AddTorrent::from_bytes(bytes))
        }
        scheme => Err(format!(
            "The {scheme} protocol is not supported for torrent tasks."
        )),
    }
}

fn torrent_files_from_details(details: &TorrentDetailsResponse) -> Vec<ProbedFile> {
    details
        .files
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|file| ProbedFile {
            relative_path: file.name.clone(),
            size: file.length.to_string(),
            content_type: content_type_for_path(&file.name),
        })
        .collect()
}

fn bt_capabilities() -> EngineCapabilities {
    EngineCapabilities {
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: true,
    }
}

fn info_hash_from_source_key(source_key: &str) -> Option<&str> {
    source_key.strip_prefix(SOURCE_BT_PREFIX)
}

fn parse_i64(value: &str) -> i64 {
    value.parse::<i64>().unwrap_or(0)
}

fn percent_decode_lossy(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(hex as char);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out.replace('+', " ")
}

fn sanitize_relative_path(value: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for component in value
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|component| !component.is_empty() && *component != "." && *component != "..")
    {
        path.push(sanitize_file_name(component));
    }
    if path.as_os_str().is_empty() {
        path.push("torrent-file");
    }
    path
}

fn sanitize_file_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "torrent-file".to_string()
    } else {
        trimmed
    }
}

fn content_type_for_path(path: &str) -> Option<String> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    let value = match extension.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "video/*",
        "mp3" | "flac" | "m4a" | "wav" | "ogg" => "audio/*",
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" => "application/archive",
        "pdf" => "application/pdf",
        "jpg" | "jpeg" | "png" | "gif" | "webp" => "image/*",
        _ => return None,
    };
    Some(value.to_string())
}
