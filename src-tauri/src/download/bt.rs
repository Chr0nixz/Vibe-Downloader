use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use librqbit::{
    api::{Api, ApiAddTorrentResponse, TorrentDetailsResponse, TorrentIdOrHash},
    limits::LimitsConfig,
    AddTorrent, AddTorrentOptions, ConnectionOptions, Session, SessionOptions,
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
        TaskProgressPayload, TaskStatus, TorrentTrackerStatus,
    },
    proxy::{ResolvedProxyConfig, SharedProxyConfig},
};

const PROTOCOL_BT: &str = "bt";
const SOURCE_BT_PREFIX: &str = "bt:";
const BT_METADATA_STATUS_INTERVAL: Duration = Duration::from_secs(10);
const BT_METADATA_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone)]
pub struct BtEngine {
    sessions: Arc<Mutex<HashMap<String, Arc<Api>>>>,
    _proxy_config: SharedProxyConfig,
}

impl Default for BtEngine {
    fn default() -> Self {
        Self::new(crate::proxy::ResolvedProxyConfig::shared_default())
    }
}

impl BtEngine {
    pub fn new(proxy_config: SharedProxyConfig) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            _proxy_config: proxy_config,
        }
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

    async fn api_for_output_folder(
        &self,
        output_folder: &str,
        download_limit_bps: Option<i64>,
        task_proxy_config: &ResolvedProxyConfig,
    ) -> Result<Arc<Api>, String> {
        let proxy_fingerprint = task_proxy_config.fingerprint();
        let key = PathBuf::from(output_folder)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(output_folder))
            .to_string_lossy()
            .to_string();
        let key = format!("{key}|proxy:{proxy_fingerprint}");

        let mut sessions = self.sessions.lock().await;
        if let Some(api) = sessions.get(&key) {
            sync_session_download_limit(api, download_limit_bps);
            return Ok(api.clone());
        }

        std::fs::create_dir_all(output_folder)
            .map_err(|e| format!("Could not create the torrent download directory: {e}"))?;
        let output_path = output_folder.to_string();
        let mut options = SessionOptions::default();
        if let Some(proxy_url) = task_proxy_config.custom_socks5_url_with_auth() {
            options.connect = Some(ConnectionOptions {
                proxy_url: Some(proxy_url),
                ..Default::default()
            });
        }
        options.ratelimits = LimitsConfig {
            upload_bps: None,
            download_bps: non_zero_u32(download_limit_bps),
        };
        let session = Session::new_with_opts(PathBuf::from(&output_path), options)
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

    let (add, _) = add_torrent_source(uri).await?;
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
        hls_variants: Vec::new(),
        metalink: None,
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
        hls_variants: Vec::new(),
        metalink: None,
    }
}

async fn run_torrent_download(engine: BtEngine, context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel,
        speed_limiter,
        proxy_config,
        ..
    } = context;

    let api = engine
        .api_for_output_folder(
            &task.save_dir,
            speed_limiter.current_limit_bps(),
            &proxy_config,
        )
        .await?;
    let (add, private_flag) = add_torrent_source(&task.url).await?;
    let had_file_selection = !db::list_task_file_records(&pool, &task.id)
        .await?
        .is_empty();
    let selected_paths = selected_torrent_paths(&pool, &task).await?;
    mark_torrent_metadata_fetching(&app, &pool, &task).await?;
    let response = wait_for_torrent_metadata(
        &engine,
        api.clone(),
        add,
        torrent_should_start_paused(selected_paths.as_ref()),
        &app,
        &pool,
        &task,
        cancel.clone(),
    )
    .await?;

    let torrent_id = TorrentIdOrHash::try_from(response.details.info_hash.as_str())
        .map_err(|e| format!("Torrent info hash is invalid: {e:#}"))?;
    let selected_indices =
        match selected_torrent_indices(selected_paths.as_ref(), &response.details) {
            Ok(indices) => indices,
            Err(error) => {
                let _ = api.api_torrent_action_forget(torrent_id).await;
                return Err(error);
            }
        };
    if let Some(indices) = &selected_indices {
        if let Err(error) = api
            .api_torrent_action_update_only_files(torrent_id, indices)
            .await
        {
            let _ = api.api_torrent_action_forget(torrent_id).await;
            return Err(format!("Could not select torrent files: {error:#}"));
        }
        db::insert_task_event(&pool, &task.id, "bt_file_selection_applied", None).await?;
        if let Err(error) = api.api_torrent_action_start(torrent_id).await {
            let _ = api.api_torrent_action_forget(torrent_id).await;
            return Err(format!(
                "Could not start torrent after applying file selection: {error:#}"
            ));
        }
    }

    persist_torrent_details(&pool, &task, &response, selected_paths.as_ref(), private_flag).await?;
    if task.url.starts_with("magnet:")
        && !had_file_selection
        && response
            .details
            .files
            .as_ref()
            .is_some_and(|files| files.len() > 1)
    {
        let _ = api.api_torrent_action_pause(torrent_id).await;
        let _ = api.api_torrent_action_forget(torrent_id).await;
        db::update_task_file_selection(&pool, &task.id, &[]).await?;
        db::update_task_status(
            &pool,
            &task.id,
            TaskStatus::NeedsAttention,
            0,
            0,
            Some("Torrent metadata is ready. Choose files before downloading."),
            Some(
                &crate::models::AppErrorPayload {
                    code: "bt_file_selection_required".to_string(),
                    message:
                        "Torrent metadata is ready. Choose at least one file before downloading."
                            .to_string(),
                    recoverable: true,
                    actions: vec!["check_url".to_string()],
                }
                .command_error(),
            ),
        )
        .await?;
        db::insert_task_event(&pool, &task.id, "bt_file_selection_required", None).await?;
        if let Some(current) = db::get_task_record(&pool, &task.id).await? {
            emit_task_updated_record(&app, &pool, &current).await;
        }
        return Ok(());
    }
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }

    let mut last_progress = 0_i64;
    let mut last_health_summary = Some("Fetching torrent metadata".to_string());
    let mut last_tick = Instant::now();

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = api.api_torrent_action_pause(torrent_id).await;
            db::update_task_progress(&pool, &task.id, last_progress, 0, 0, TaskStatus::Paused)
                .await?;
            if let Some(segment) = db::get_first_segment_record(&pool, &task.id).await? {
                db::update_segment_runtime_progress(
                    &pool,
                    &segment.id,
                    last_progress,
                    0,
                    SegmentStatus::Pending,
                )
                .await?;
            }
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
        let peer_stats = stats.live.as_ref().map(|live| &live.snapshot.peer_stats);
        let live_peers = peer_stats.map(|stats| stats.live).unwrap_or(0);
        let connecting_peers = peer_stats.map(|stats| stats.connecting).unwrap_or(0);
        let queued_peers = peer_stats.map(|stats| stats.queued).unwrap_or(0);
        let seen_peers = peer_stats.map(|stats| stats.seen).unwrap_or(0);
        let completed_pieces = stats
            .live
            .as_ref()
            .map(|live| {
                i64::try_from(live.snapshot.downloaded_and_checked_pieces).unwrap_or(i64::MAX)
            })
            .unwrap_or(0);
        let (piece_bitfield_base64, piece_count) = torrent_piece_bitfield(&api, torrent_id);
        let dht_status = torrent_dht_status(&api);
        let trackers_json = torrent_tracker_status_json(&task.url);
        let seeding_enabled = db::torrent_seeding_enabled(&pool, &task.id)
            .await
            .unwrap_or(false);
        let live_peer_count = i32::try_from(live_peers).unwrap_or(i32::MAX);
        let peer_count = i64::from(live_peers)
            .saturating_add(i64::from(connecting_peers))
            .saturating_add(i64::from(queued_peers));
        let metadata_status = torrent_metadata_status(
            speed,
            live_peers,
            connecting_peers,
            queued_peers,
            seen_peers,
        );
        let health_summary = torrent_health_summary(
            speed,
            live_peers,
            connecting_peers,
            queued_peers,
            seen_peers,
        );

        db::update_task_progress(
            &pool,
            &task.id,
            downloaded,
            speed,
            live_peer_count,
            TaskStatus::Downloading,
        )
        .await?;
        if last_health_summary.as_deref() != Some(health_summary.as_str()) {
            db::update_task_health_summary(&pool, &task.id, Some(health_summary.as_str())).await?;
            if let Some(current) = db::get_task_record(&pool, &task.id).await? {
                emit_task_updated_record(&app, &pool, &current).await;
            }
            last_health_summary = Some(health_summary);
        }
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
            db::TorrentRuntimeSnapshotUpsert {
                metadata_status,
                completed_pieces,
                verified_pieces: completed_pieces,
                piece_count,
                piece_bitfield_base64: piece_bitfield_base64.as_deref(),
                peer_count,
                seed_count: 0,
                dht_status: dht_status.as_deref(),
                trackers_json: trackers_json.as_deref(),
                upload_bytes: uploaded,
                upload_speed_bps: upload_speed,
                ratio,
                seeding_enabled,
                seeding_state: if seeding_enabled {
                    "downloading"
                } else {
                    "disabled"
                },
                last_error_code: stats.error.as_ref().map(|_| "bt_runtime_error"),
                last_error_message: stats.error.as_deref(),
            },
        )
        .await?;
        emit_task_progress(
            &app,
            &TaskProgressPayload {
                task_id: task.id.clone(),
                downloaded_bytes: downloaded.to_string(),
                total_size: total.to_string(),
                speed_bps: speed.to_string(),
                connection_count: live_peer_count,
                status: TaskStatus::Downloading,
            },
        );

        if stats.finished || (total > 0 && downloaded >= total) {
            if let Some(segment) = db::get_first_segment_record(&pool, &task.id).await? {
                db::complete_segment(&pool, &segment.id).await?;
            }
            db::complete_task(&pool, &task.id).await?;
            if db::torrent_seeding_enabled(&pool, &task.id)
                .await
                .unwrap_or(false)
            {
                db::upsert_torrent_runtime_snapshot(
                    &pool,
                    &task.id,
                    db::TorrentRuntimeSnapshotUpsert {
                        metadata_status: "seeding",
                        completed_pieces,
                        verified_pieces: completed_pieces,
                        piece_count,
                        piece_bitfield_base64: piece_bitfield_base64.as_deref(),
                        peer_count,
                        seed_count: 0,
                        dht_status: dht_status.as_deref(),
                        trackers_json: trackers_json.as_deref(),
                        upload_bytes: uploaded,
                        upload_speed_bps: upload_speed,
                        ratio,
                        seeding_enabled: true,
                        seeding_state: "seeding",
                        last_error_code: None,
                        last_error_message: None,
                    },
                )
                .await?;
            } else {
                let _ = api.api_torrent_action_forget(torrent_id).await;
            }
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

fn sync_session_download_limit(api: &Api, limit_bps: Option<i64>) {
    api.session()
        .ratelimits
        .set_download_bps(non_zero_u32(limit_bps));
}

fn non_zero_u32(limit_bps: Option<i64>) -> Option<NonZeroU32> {
    let value = limit_bps?;
    if value <= 0 {
        return None;
    }
    let value = u32::try_from(value).unwrap_or(u32::MAX);
    NonZeroU32::new(value)
}

#[allow(clippy::too_many_arguments)]
async fn wait_for_torrent_metadata(
    engine: &BtEngine,
    api: Arc<Api>,
    add: AddTorrent<'static>,
    start_paused: bool,
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<ApiAddTorrentResponse, String> {
    let options = Some(AddTorrentOptions {
        paused: start_paused,
        overwrite: true,
        output_folder: Some(task.save_dir.clone()),
        ..Default::default()
    });
    let add_torrent = api.api_add_torrent(add, options);
    tokio::pin!(add_torrent);

    let started = Instant::now();
    let mut interval = tokio::time::interval(BT_METADATA_STATUS_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            result = &mut add_torrent => {
                return result.map_err(|e| format!("Could not add torrent: {e:#}"));
            }
            _ = interval.tick() => {
                if cancel.load(Ordering::SeqCst) {
                    engine.delete_runtime_task(&task.source_key, false).await;
                    return Err("Torrent metadata fetch was canceled.".to_string());
                }

                let elapsed = started.elapsed();
                if elapsed >= BT_METADATA_TIMEOUT {
                    engine.delete_runtime_task(&task.source_key, false).await;
                    return Err(format!(
                        "Could not fetch torrent metadata after {} seconds. No reachable peers returned metadata through DHT, trackers, or the magnet link.",
                        BT_METADATA_TIMEOUT.as_secs()
                    ));
                }

                mark_torrent_metadata_still_fetching(app, pool, task, elapsed).await?;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if cancel.load(Ordering::SeqCst) {
                    engine.delete_runtime_task(&task.source_key, false).await;
                    return Err("Torrent metadata fetch was canceled.".to_string());
                }
            }
        }
    }
}

fn torrent_should_start_paused(selected_paths: Option<&HashSet<String>>) -> bool {
    selected_paths.is_some()
}

async fn mark_torrent_metadata_fetching(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
) -> Result<(), String> {
    db::update_task_progress(
        pool,
        &task.id,
        task.downloaded_bytes,
        0,
        0,
        TaskStatus::Downloading,
    )
    .await?;
    db::update_task_health_summary(pool, &task.id, Some("Fetching torrent metadata")).await?;
    if let Some(segment) = db::get_first_segment_record(pool, &task.id).await? {
        db::update_segment_runtime_progress(
            pool,
            &segment.id,
            task.downloaded_bytes,
            0,
            SegmentStatus::Downloading,
        )
        .await?;
    }
    let trackers_json = torrent_tracker_status_json(&task.url);
    db::upsert_torrent_runtime_snapshot(
        pool,
        &task.id,
        db::TorrentRuntimeSnapshotUpsert {
            metadata_status: "fetching_metadata",
            completed_pieces: 0,
            verified_pieces: 0,
            piece_count: 0,
            piece_bitfield_base64: None,
            peer_count: 0,
            seed_count: 0,
            dht_status: None,
            trackers_json: trackers_json.as_deref(),
            upload_bytes: 0,
            upload_speed_bps: 0,
            ratio: 0.0,
            seeding_enabled: false,
            seeding_state: "disabled",
            last_error_code: None,
            last_error_message: None,
        },
    )
    .await?;
    db::insert_task_event(pool, &task.id, "bt_metadata_fetching", None).await?;
    emit_task_progress(
        app,
        &TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: task.downloaded_bytes.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: "0".to_string(),
            connection_count: 0,
            status: TaskStatus::Downloading,
        },
    );
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

async fn mark_torrent_metadata_still_fetching(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    elapsed: Duration,
) -> Result<(), String> {
    let elapsed_seconds = elapsed.as_secs().max(1);
    let remaining_seconds = BT_METADATA_TIMEOUT.saturating_sub(elapsed).as_secs().max(1);
    let summary = format!(
        "Fetching torrent metadata via DHT and trackers ({}s elapsed, {}s before timeout)",
        elapsed_seconds, remaining_seconds
    );
    db::update_task_health_summary(pool, &task.id, Some(summary.as_str())).await?;
    let trackers_json = torrent_tracker_status_json(&task.url);
    db::upsert_torrent_runtime_snapshot(
        pool,
        &task.id,
        db::TorrentRuntimeSnapshotUpsert {
            metadata_status: "fetching_metadata",
            completed_pieces: 0,
            verified_pieces: 0,
            piece_count: 0,
            piece_bitfield_base64: None,
            peer_count: 0,
            seed_count: 0,
            dht_status: None,
            trackers_json: trackers_json.as_deref(),
            upload_bytes: 0,
            upload_speed_bps: 0,
            ratio: 0.0,
            seeding_enabled: false,
            seeding_state: "disabled",
            last_error_code: None,
            last_error_message: None,
        },
    )
    .await?;
    emit_task_progress(
        app,
        &TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: task.downloaded_bytes.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: "0".to_string(),
            connection_count: 0,
            status: TaskStatus::Downloading,
        },
    );
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

fn torrent_metadata_status(
    speed_bps: i64,
    live_peers: u32,
    connecting_peers: u32,
    queued_peers: u32,
    seen_peers: u32,
) -> &'static str {
    if speed_bps > 0 {
        "downloading"
    } else if live_peers > 0 {
        "connected"
    } else if connecting_peers > 0 {
        "connecting_peers"
    } else if queued_peers > 0 {
        "queued_peers"
    } else if seen_peers > 0 {
        "waiting_peers"
    } else {
        "searching_peers"
    }
}

fn torrent_health_summary(
    speed_bps: i64,
    live_peers: u32,
    connecting_peers: u32,
    queued_peers: u32,
    seen_peers: u32,
) -> String {
    if speed_bps > 0 && live_peers > 0 {
        return format!("Downloading from {}", peer_label(live_peers));
    }
    if live_peers > 0 {
        return format!("Connected to {}; waiting for data", peer_label(live_peers));
    }
    if connecting_peers > 0 {
        return format!("Connecting to {}", peer_label(connecting_peers));
    }
    if queued_peers > 0 {
        return format!("Queued {}; waiting to connect", peer_label(queued_peers));
    }
    if seen_peers > 0 {
        return "Peers found but none are reachable".to_string();
    }
    "Searching DHT and trackers for peers".to_string()
}

fn peer_label(count: u32) -> String {
    if count == 1 {
        "1 peer".to_string()
    } else {
        format!("{count} peers")
    }
}

fn torrent_piece_bitfield(api: &Api, torrent_id: TorrentIdOrHash) -> (Option<String>, i64) {
    match api.api_dump_haves(torrent_id) {
        Ok((bitfield, count)) => (
            Some(STANDARD.encode(bitfield.into_boxed_slice())),
            i64::from(count),
        ),
        Err(error) => {
            tracing::debug!(error = %error, "torrent have bitfield unavailable");
            (None, 0)
        }
    }
}

fn torrent_dht_status(api: &Api) -> Option<String> {
    api.api_dht_stats()
        .ok()
        .and_then(|stats| serde_json::to_string(&stats).ok())
}

fn torrent_tracker_status_json(uri: &str) -> Option<String> {
    let trackers = tracker_statuses_from_uri(uri);
    if trackers.is_empty() {
        None
    } else {
        serde_json::to_string(&trackers).ok()
    }
}

fn tracker_statuses_from_uri(uri: &str) -> Vec<TorrentTrackerStatus> {
    let Ok(magnet) = Magnet::new(uri) else {
        return Vec::new();
    };
    magnet
        .trackers()
        .iter()
        .map(|url| TorrentTrackerStatus {
            url: percent_decode_lossy(url),
            status: "configured".to_string(),
            last_error: None,
        })
        .collect()
}

async fn persist_torrent_details(
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    response: &ApiAddTorrentResponse,
    selected_paths: Option<&HashSet<String>>,
    private_flag: Option<bool>,
) -> Result<(), String> {
    let details = &response.details;
    let files = torrent_files_from_details(details);
    let total_size = selected_torrent_total_size(&files, selected_paths);
    let source_key = format!("{SOURCE_BT_PREFIX}{}", details.info_hash);
    let name = details
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| task.file_name.clone());

    db::update_task_torrent_metadata(
        pool,
        &task.id,
        db::TaskTorrentMetadataUpdate {
            final_url: &source_key,
            file_name: &name,
            total_size,
            source_key: &source_key,
            supports_multi_file: files.len() > 1,
        },
    )
    .await?;
    let trackers_json = torrent_tracker_status_json(&task.url);
    db::upsert_torrent_task(
        pool,
        &task.id,
        db::TorrentTaskUpsert {
            info_hash: &details.info_hash,
            name: &name,
            magnet_uri: task.url.starts_with("magnet:").then_some(task.url.as_str()),
            torrent_blob: None,
            piece_length: 0,
            piece_count: i64::from(details.total_pieces),
            private: private_flag.unwrap_or(false),
            trackers_json: trackers_json.as_deref(),
            seeding_enabled: false,
            seed_ratio_limit: None,
            seed_time_limit_seconds: None,
        },
    )
    .await?;

    if !files.is_empty() {
        db::delete_task_files_for_task(pool, &task.id).await?;
        for file in files {
            let record = task_file_record_from_probed_file(task, &file, selected_paths);
            db::insert_task_file_record(pool, &record).await?;
        }
    }
    Ok(())
}

fn task_file_record_from_probed_file(
    task: &crate::models::TaskRecord,
    file: &ProbedFile,
    selected_paths: Option<&HashSet<String>>,
) -> TaskFileRecord {
    let relative = sanitize_relative_path(&file.relative_path);
    let relative_path = relative.to_string_lossy().replace('\\', "/");
    let final_path = PathBuf::from(&task.save_dir).join(&relative);
    let file_name = relative
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&file.relative_path)
        .to_string();

    TaskFileRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        relative_path: relative_path.clone(),
        file_name,
        save_dir: task.save_dir.clone(),
        temp_path: None,
        final_path: Some(final_path.to_string_lossy().to_string()),
        total_size: parse_i64(&file.size),
        downloaded_bytes: 0,
        selected: selected_paths
            .map(|paths| paths.contains(&relative_path))
            .unwrap_or(true),
        status: TaskStatus::Queued,
        content_type: file.content_type.clone(),
    }
}

async fn selected_torrent_paths(
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
) -> Result<Option<HashSet<String>>, String> {
    let files = db::list_task_file_records(pool, &task.id).await?;
    if files.len() <= 1 {
        return Ok(None);
    }

    let all = files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect::<HashSet<_>>();
    let selected = files
        .iter()
        .filter(|file| file.selected)
        .map(|file| file.relative_path.clone())
        .collect::<HashSet<_>>();
    if selected.is_empty() {
        return Err("Select at least one torrent file to download.".to_string());
    }
    if selected.len() == all.len() {
        return Ok(None);
    }
    Ok(Some(selected))
}

fn selected_torrent_indices(
    selected_paths: Option<&HashSet<String>>,
    details: &TorrentDetailsResponse,
) -> Result<Option<HashSet<usize>>, String> {
    let Some(selected_paths) = selected_paths else {
        return Ok(None);
    };
    let mut indices = HashSet::new();
    for (index, file) in torrent_files_from_details(details).iter().enumerate() {
        let relative = sanitize_relative_path(&file.relative_path)
            .to_string_lossy()
            .replace('\\', "/");
        if selected_paths.contains(&relative) {
            indices.insert(index);
        }
    }
    if indices.is_empty() {
        return Err("Selected torrent files are no longer available in metadata.".to_string());
    }
    Ok(Some(indices))
}

fn selected_torrent_total_size(
    files: &[ProbedFile],
    selected_paths: Option<&HashSet<String>>,
) -> i64 {
    files
        .iter()
        .filter(|file| {
            selected_paths.is_none_or(|paths| {
                let relative = sanitize_relative_path(&file.relative_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                paths.contains(&relative)
            })
        })
        .map(|file| parse_i64(&file.size))
        .sum()
}

async fn add_torrent_source(uri: &str) -> Result<(AddTorrent<'static>, Option<bool>), String> {
    let trimmed = uri.trim();
    if trimmed.starts_with("magnet:") {
        return Ok((AddTorrent::from_url(trimmed.to_string()), None));
    }

    let parsed = Url::parse(trimmed).map_err(|_| "Torrent URL is invalid.".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok((AddTorrent::from_url(trimmed.to_string()), None)),
        "file" => {
            let path = parsed
                .to_file_path()
                .map_err(|_| "Torrent file path is invalid.".to_string())?;
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("Could not read torrent file {}: {e}", path.display()))?;
            // Parse the private flag from the torrent's info dict so it can be
            // persisted accurately. librqbit handles DHT/PEX disabling internally
            // based on this flag, but TorrentDetailsResponse does not expose it.
            let private = parse_torrent_private_flag(&bytes);
            Ok((AddTorrent::from_bytes(bytes), private))
        }
        scheme => Err(format!(
            "The {scheme} protocol is not supported for torrent tasks."
        )),
    }
}

/// Parse the `info.private` flag from a .torrent file's bencoded bytes.
/// Returns `None` if parsing fails or the field is absent (defaults to false per BEP 3).
fn parse_torrent_private_flag(bytes: &[u8]) -> Option<bool> {
    let meta = librqbit::torrent_from_bytes(bytes).ok()?;
    Some(meta.info.data.private)
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

#[cfg(test)]
mod tests {
    use super::*;
    use librqbit::AddTorrent;

    #[tokio::test]
    async fn add_torrent_source_reads_file_urls_with_spaces_and_unicode() {
        let dir = std::env::temp_dir().join(format!("vibe-bt-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("样本 torrent.torrent");
        let bytes = b"d4:infode";
        std::fs::write(&path, bytes).expect("write torrent file");
        let url = Url::from_file_path(&path).expect("file url").to_string();

        let (add, private_flag) = add_torrent_source(&url)
            .await
            .expect("local torrent source");

        match add {
            AddTorrent::TorrentFileBytes(actual) => assert_eq!(actual.as_ref(), bytes),
            AddTorrent::Url(value) => panic!("expected torrent bytes, got URL {value}"),
        }
        // The test bytes (b"d4:infode") are a truncated torrent without a private
        // field, so parsing returns None.
        assert_eq!(private_flag, None);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn selected_torrent_total_size_uses_sanitized_selected_paths() {
        let files = vec![
            ProbedFile {
                relative_path: "folder/keep.bin".to_string(),
                size: "100".to_string(),
                content_type: None,
            },
            ProbedFile {
                relative_path: "folder/skip.bin".to_string(),
                size: "250".to_string(),
                content_type: None,
            },
        ];
        let selected = HashSet::from(["folder/keep.bin".to_string()]);

        assert_eq!(selected_torrent_total_size(&files, Some(&selected)), 100);
        assert_eq!(selected_torrent_total_size(&files, None), 350);
    }

    #[test]
    fn non_zero_u32_ignores_empty_limits_and_clamps_large_values() {
        assert_eq!(non_zero_u32(None), None);
        assert_eq!(non_zero_u32(Some(0)), None);
        assert_eq!(non_zero_u32(Some(-1)), None);
        assert_eq!(non_zero_u32(Some(1024)).map(NonZeroU32::get), Some(1024));
        assert_eq!(
            non_zero_u32(Some(i64::MAX)).map(NonZeroU32::get),
            Some(u32::MAX)
        );
    }

    #[test]
    fn torrent_starts_paused_only_for_file_subset_selection() {
        assert!(!torrent_should_start_paused(None));
        let selected = HashSet::from(["video/main.mkv".to_string()]);
        assert!(torrent_should_start_paused(Some(&selected)));
    }

    #[test]
    fn parse_torrent_private_flag_detects_private_and_non_private() {
        // A minimal non-private torrent: info dict with name + piece length + pieces + length.
        // b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1eee"
        let non_private = b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1eee";
        assert_eq!(parse_torrent_private_flag(non_private), Some(false));

        // Same torrent but with private=1 in the info dict.
        // b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1e7:privatei1eee"
        let private = b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1e7:privatei1eee";
        assert_eq!(parse_torrent_private_flag(private), Some(true));
    }

    #[test]
    fn parse_torrent_private_flag_returns_none_for_invalid_bytes() {
        assert_eq!(parse_torrent_private_flag(b"not a torrent"), None);
        assert_eq!(parse_torrent_private_flag(b""), None);
    }
}
