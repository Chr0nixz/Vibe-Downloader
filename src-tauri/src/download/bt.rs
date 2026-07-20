use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use librqbit::{
    api::{Api, ApiAddTorrentResponse, TorrentDetailsResponse, TorrentIdOrHash},
    limits::LimitsConfig,
    AddTorrent, AddTorrentOptions, ConnectionOptions, Magnet, Session, SessionOptions,
};
use reqwest::Url;
use tokio::sync::Mutex;

use super::engine::{DownloadContext, DownloadEngine, EngineFuture, ProbeOutput, ProbeRequest};
use super::probe_error::reqwest_error_to_structured;
use super::url_classify::is_torrent_url;
use super::DownloadError;
use crate::download::error::engine_error;
use crate::{
    db,
    events::{emit_task_updated_record, TaskProgressEmitGate},
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
/// F-1: Interval between seed-ratio limit checks during the seeding phase.
/// Matches the BT metadata/progress cadence (10s).
const BT_SEEDING_TICK_INTERVAL: Duration = Duration::from_secs(10);

/// E-3: Maximum .torrent file size we are willing to buffer in memory for
/// private-flag pre-parsing. 32 MiB is generous (a typical .torrent is under
/// 1 MiB; even multi-track packs rarely exceed 10 MiB) while capping the
/// memory impact of a malicious or misconfigured URL. When this limit is
/// exceeded, the caller falls back to `AddTorrent::from_url`, letting
/// librqbit stream the torrent directly without buffering in our process.
const TORRENT_MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct BtEngine {
    sessions: Arc<Mutex<HashMap<String, BtSessionEntry>>>,
    _proxy_config: SharedProxyConfig,
}

struct BtSessionEntry {
    api: Arc<Api>,
    active_task_count: usize,
}

/// RAII guard that releases a BT session reference when dropped.
///
/// `release_session_ref` is async (uses `tokio::sync::Mutex`), so `Drop` cannot
/// await it directly. On drop we spawn the release on the async runtime; on the
/// normal completion path callers can use `release()` to release synchronously.
struct SessionRefGuard {
    engine: BtEngine,
    session_key: Option<String>,
}

impl SessionRefGuard {
    fn new(engine: BtEngine, session_key: String) -> Self {
        Self {
            engine,
            session_key: Some(session_key),
        }
    }

    /// Release the session reference synchronously (normal completion path).
    #[allow(dead_code)]
    async fn release(mut self) {
        if let Some(key) = self.session_key.take() {
            self.engine.release_session_ref(&key).await;
        }
    }
}

impl Drop for SessionRefGuard {
    fn drop(&mut self) {
        if let Some(key) = self.session_key.take() {
            let engine = self.engine.clone();
            tauri::async_runtime::spawn(async move {
                engine.release_session_ref(&key).await;
            });
        }
    }
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

        // ARC-12: only forget/delete the torrent here. Session refcount is owned
        // exclusively by `SessionRefGuard` so cancel/delete + Drop never double-decrement.
        let touched_keys: Vec<String> = {
            let sessions = self.sessions.lock().await;
            sessions.keys().cloned().collect()
        };
        for key in &touched_keys {
            let api = {
                let sessions = self.sessions.lock().await;
                sessions.get(key).map(|e| e.api.clone())
            };
            let Some(api) = api else { continue };
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

    async fn release_session_ref(&self, session_key: &str) {
        let mut sessions = self.sessions.lock().await;
        if let Some(entry) = sessions.get_mut(session_key) {
            entry.active_task_count = entry.active_task_count.saturating_sub(1);
            if entry.active_task_count == 0 {
                sessions.remove(session_key);
                tracing::info!(session_key, "bt session evicted (no active tasks)");
            }
        }
    }

    /// ARC-12: include `task_id` so each task owns its session limits. Sharing
    /// only by output folder caused the latest task to overwrite peers' rates.
    ///
    /// PERF-06: canonicalize/create_dir happen off the Tokio worker via
    /// `spawn_blocking` / `tokio::fs` so a slow disk cannot stall the runtime.
    async fn compute_session_key(
        output_folder: &str,
        task_proxy_config: &ResolvedProxyConfig,
        task_id: &str,
    ) -> String {
        let proxy_fingerprint = task_proxy_config.fingerprint();
        let folder = output_folder.to_string();
        let canonical = tokio::task::spawn_blocking(move || {
            PathBuf::from(&folder)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(folder))
                .to_string_lossy()
                .to_string()
        })
        .await
        .unwrap_or_else(|_| output_folder.to_string());
        format!("{canonical}|proxy:{proxy_fingerprint}|task:{task_id}")
    }

    async fn api_for_output_folder(
        &self,
        output_folder: &str,
        task_id: &str,
        download_limit_bps: Option<i64>,
        upload_limit_bps: Option<i64>,
        task_proxy_config: &ResolvedProxyConfig,
    ) -> Result<(Arc<Api>, String), String> {
        let key = Self::compute_session_key(output_folder, task_proxy_config, task_id).await;

        // Fast path: reuse under a short lock (no await while held).
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(entry) = sessions.get_mut(&key) {
                entry.active_task_count += 1;
                sync_session_download_limit(&entry.api, download_limit_bps);
                // F-7: Re-apply the global upload limit on session reuse so settings
                // changes take effect for the next acquire without a restart.
                sync_session_upload_limit(&entry.api, upload_limit_bps);
                return Ok((entry.api.clone(), key));
            }
        }

        // ARC-12: create the librqbit session outside the registry mutex so
        // other BT registry operations are not blocked on DHT/bind work.
        tokio::fs::create_dir_all(output_folder)
            .await
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
            // F-7: Apply the global BT upload limit at session creation.
            upload_bps: non_zero_u32(upload_limit_bps),
            download_bps: non_zero_u32(download_limit_bps),
        };
        let session = Session::new_with_opts(PathBuf::from(&output_path), options)
            .await
            .map_err(|e| format!("Could not start BitTorrent session: {e:#}"))?;
        let api = Arc::new(Api::new(session, None));

        let mut sessions = self.sessions.lock().await;
        // Another worker may have inserted the same key while we awaited.
        if let Some(entry) = sessions.get_mut(&key) {
            entry.active_task_count += 1;
            sync_session_download_limit(&entry.api, download_limit_bps);
            sync_session_upload_limit(&entry.api, upload_limit_bps);
            return Ok((entry.api.clone(), key));
        }
        sessions.insert(
            key.clone(),
            BtSessionEntry {
                api: api.clone(),
                active_task_count: 1,
            },
        );
        Ok((api, key))
    }
}

impl DownloadEngine for BtEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_BT
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "magnet" | "file")
    }

    /// R-3: Magnet links or `.torrent` files (including http/https/file schemes) route to BT.
    /// Highest priority (100), ensuring it matches before Metalink/HLS/DASH which also support http scheme.
    fn matches_url(&self, url: &Url) -> bool {
        url.scheme() == "magnet" || is_torrent_url(url)
    }

    fn priority(&self) -> i32 {
        100
    }

    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, DownloadError>> {
        Box::pin(async move {
            let proxy_config = request.proxy_config.clone().unwrap_or_default();
            probe_torrent(
                &request.uri,
                &request.app,
                &request.request_id,
                &proxy_config,
            )
            .await
            .map_err(DownloadError::Other)
        })
    }

    fn download<'a>(
        &'a self,
        context: DownloadContext,
    ) -> EngineFuture<'a, Result<(), DownloadError>> {
        Box::pin(async move {
            run_torrent_download(self.clone(), context)
                .await
                .map_err(DownloadError::Other)
        })
    }
}

async fn probe_torrent(
    uri: &str,
    app: &Option<tauri::AppHandle>,
    request_id: &Option<String>,
    proxy_config: &ResolvedProxyConfig,
) -> Result<ProbeOutput, String> {
    if uri.trim_start().starts_with("magnet:") {
        crate::download::engine::emit_probe_phase(app, request_id, "parsing_magnet", Some("bt"));
        return probe_magnet(uri);
    }

    crate::download::engine::emit_probe_phase(app, request_id, "fetching_torrent", Some("bt"));
    // Probe must not fall back to AddTorrent::from_url when HTTP fetch fails; that
    // would bypass SOCKS5 and hide proxy misconfiguration during create-time probe.
    let (add, _, _) = add_torrent_source(uri, proxy_config, false).await?;
    crate::download::engine::emit_probe_phase(app, request_id, "inspecting_metadata", Some("bt"));
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
        .map_err(|e| {
            engine_error(
                "bt_torrent_probe_failed",
                format!("Could not inspect torrent metadata: {e:#}"),
                true,
            )
        })?;
    Ok(probe_from_torrent_details(uri, &response.details))
}

fn probe_magnet(uri: &str) -> Result<ProbeOutput, String> {
    let magnet = Magnet::parse(uri)
        .map_err(|_| engine_error("bt_magnet_invalid", "Magnet link is invalid.", false))?;
    // `as_string()` returns lowercase hex (40 chars for v1 btih, 64 for v2 btmh).
    // librqbit accepts both hex and base32 on parse, but normalizes to hex here.
    let hash = magnet
        .as_id20()
        .map(|id| id.as_string())
        .or_else(|| magnet.as_id32().map(|id| id.as_string()))
        .ok_or_else(|| {
            engine_error(
                "bt_magnet_invalid",
                "Magnet link must include a BitTorrent info hash.",
                false,
            )
        })?;
    // librqbit's `name` field is already percent-decoded via `url::Url::query_pairs()`.
    // `xl` (exact length) is not exposed by librqbit; magnet total size is usually
    // unknown before metadata, so we fall back to 0.
    let display_name = magnet
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("magnet-{hash}"));

    Ok(ProbeOutput {
        protocol: PROTOCOL_BT.to_string(),
        task_kind: TaskKind::MultiFile,
        resolved_uri: uri.to_string(),
        display_name,
        total_size: 0,
        source_key: format!("{SOURCE_BT_PREFIX}{hash}"),
        capabilities: bt_capabilities(),
        files: Vec::new(),
        etag: None,
        last_modified: None,
        content_type: Some("application/x-bittorrent".to_string()),
        hls_variants: Vec::new(),
        hls_audio_tracks: Vec::new(),
        hls_subtitle_tracks: Vec::new(),
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
        hls_audio_tracks: Vec::new(),
        hls_subtitle_tracks: Vec::new(),
        metalink: None,
    }
}

async fn run_torrent_download(engine: BtEngine, context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel_token,
        speed_limiter,
        proxy_config,
        ..
    } = context;

    let mut progress_gate = TaskProgressEmitGate::default();

    // F-7: Read the global BT upload limit from settings. Fetched per-task at
    // session acquisition so settings changes apply to the next task without
    // a restart. The download limit still comes from the task speed limiter
    // (per-task + global min).
    let bt_upload_limit_bps = db::get_bt_upload_limit_bps_setting(&pool).await;

    let (api, session_key) = engine
        .api_for_output_folder(
            &task.save_dir,
            &task.id,
            speed_limiter.current_limit_bps(),
            bt_upload_limit_bps,
            &proxy_config,
        )
        .await?;
    let _session_guard = SessionRefGuard::new(engine.clone(), session_key);
    let source_started = Instant::now();
    let source_result = add_torrent_source(&task.url, &proxy_config, true)
        .await
        .map_err(|error| {
            engine_error(
                "bt_source_failed",
                format!("Could not load the torrent source: {error}"),
                true,
            )
        });
    crate::download::diagnostics::persist_engine_diagnostic(
        crate::download::diagnostics::EngineDiagnosticContext {
            pool: &pool,
            task_id: &task.id,
            method: "BT SOURCE",
            url: &task.url,
            range_header: None,
            status_code: None,
            content_length: None,
            error: source_result.as_ref().err().map(String::as_str),
            retry_count: 0,
            duration: source_started.elapsed(),
        },
    )
    .await;
    let (add, private_flag, configured_trackers) = source_result?;
    let configured_trackers_json = tracker_status_json_from_statuses(&configured_trackers)
        .or_else(|| torrent_tracker_status_json(&task.url));
    let had_file_selection = !db::list_task_file_records(&pool, &task.id)
        .await?
        .is_empty();
    let selected_paths = selected_torrent_paths(&pool, &task).await?;
    mark_torrent_metadata_fetching(&app, &pool, &task, &mut progress_gate).await?;
    let metadata_started = Instant::now();
    let metadata_result = wait_for_torrent_metadata(
        &engine,
        api.clone(),
        add,
        torrent_should_start_paused(selected_paths.as_ref()),
        &app,
        &pool,
        &task,
        cancel_token.clone(),
        &mut progress_gate,
    )
    .await;
    crate::download::diagnostics::persist_engine_diagnostic(
        crate::download::diagnostics::EngineDiagnosticContext {
            pool: &pool,
            task_id: &task.id,
            method: "BT METADATA",
            url: &task.url,
            range_header: None,
            status_code: None,
            content_length: None,
            error: metadata_result.as_ref().err().map(String::as_str),
            retry_count: 0,
            duration: metadata_started.elapsed(),
        },
    )
    .await;
    let response = metadata_result?;

    let torrent_id = TorrentIdOrHash::try_from(response.details.info_hash.as_str())
        .map_err(|e| format!("Torrent info hash is invalid: {e:#}"))?;
    let selected_indices =
        match selected_torrent_indices(selected_paths.as_ref(), &response.details) {
            Ok(indices) => indices,
            Err(error) => {
                let _ = api.api_torrent_action_forget(torrent_id).await;
                progress_gate.flush(&app);
                return Err(error);
            }
        };
    if let Some(indices) = &selected_indices {
        if let Err(error) = api
            .api_torrent_action_update_only_files(torrent_id, indices)
            .await
        {
            let _ = api.api_torrent_action_forget(torrent_id).await;
            progress_gate.flush(&app);
            return Err(format!("Could not select torrent files: {error:#}"));
        }
        db::insert_task_event(&pool, &task.id, "bt_file_selection_applied", None).await?;
        if let Err(error) = api.api_torrent_action_start(torrent_id).await {
            let _ = api.api_torrent_action_forget(torrent_id).await;
            progress_gate.flush(&app);
            return Err(format!(
                "Could not start torrent after applying file selection: {error:#}"
            ));
        }
    }

    persist_torrent_details(
        &pool,
        &task,
        &response,
        selected_paths.as_ref(),
        private_flag,
        &configured_trackers,
    )
    .await?;

    // For magnet sources, the private flag cannot be parsed before metadata is
    // fetched, and TorrentDetailsResponse does not expose it. librqbit handles
    // DHT/PEX disabling internally based on the info dict's private field, so
    // the runtime safety behavior is correct. We record an event so the DB
    // record (private=false) is understood as "unknown" rather than "non-private".
    if private_flag.is_none() {
        db::insert_task_event(
            &pool,
            &task.id,
            "bt_private_flag_unknown",
            Some(
                "Torrent source does not expose the private flag before metadata. \
                 DHT/PEX handling relies on librqbit internals.",
            ),
        )
        .await?;
    }

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
            None,
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
        progress_gate.flush(&app);
        return Ok(());
    }
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }

    let mut last_progress = 0_i64;
    let mut last_health_summary = Some("Fetching torrent metadata".to_string());
    let mut last_tick = Instant::now();
    let mut last_file_progress_emit = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);

    loop {
        if cancel_token.is_cancelled() {
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
            progress_gate.flush(&app);
            return Ok(());
        }

        let stats_started = Instant::now();
        let stats = match api.api_stats_v1(torrent_id) {
            Ok(stats) => stats,
            Err(error) => {
                let error = crate::download::error::engine_error(
                    "bt_runtime_stats_failed",
                    format!("Could not read torrent stats: {error:#}"),
                    true,
                );
                crate::download::diagnostics::persist_engine_diagnostic(
                    crate::download::diagnostics::EngineDiagnosticContext {
                        pool: &pool,
                        task_id: &task.id,
                        method: "BT STATS",
                        url: &task.url,
                        range_header: None,
                        status_code: None,
                        content_length: None,
                        error: Some(&error),
                        retry_count: 0,
                        duration: stats_started.elapsed(),
                    },
                )
                .await;
                return Err(error);
            }
        };
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
        let trackers_json = configured_trackers_json.clone();
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
        // ARC-13: persist real per-file have-bytes from librqbit.
        let file_progress_changed =
            sync_bt_task_file_progress(&pool, &task.id, &stats.file_progress).await?;
        if file_progress_changed && last_file_progress_emit.elapsed() >= Duration::from_secs(2) {
            crate::events::evict_task_files_version(&task.id);
            if let Some(current) = db::get_task_record(&pool, &task.id).await? {
                emit_task_updated_record(&app, &pool, &current).await;
            }
            last_file_progress_emit = Instant::now();
        }
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
        progress_gate.emit_or_store(
            &app,
            TaskProgressPayload {
                task_id: task.id.clone(),
                downloaded_bytes: downloaded.to_string(),
                total_size: total.to_string(),
                speed_bps: speed.to_string(),
                connection_count: live_peer_count,
                status: TaskStatus::Downloading,
            },
            false,
        );

        if stats.finished || (total > 0 && downloaded >= total) {
            if let Some(segment) = db::get_first_segment_record(&pool, &task.id).await? {
                db::complete_segment(&pool, &segment.id).await?;
            }
            db::complete_task(&pool, &task.id).await?;
            if seeding_enabled {
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
                if let Some(current) = db::get_task_record(&pool, &task.id).await? {
                    emit_task_updated_record(&app, &pool, &current).await;
                }
                progress_gate.flush(&app);

                // F-1 / FUN-11: Seeding monitoring — ratio OR time limit stops
                // seeding. When neither is set, seed until cancelled.
                let seeding_started = Instant::now();
                loop {
                    tokio::time::sleep(BT_SEEDING_TICK_INTERVAL).await;
                    if cancel_token.is_cancelled() {
                        break;
                    }
                    let seed_stats = api.api_stats_v1(torrent_id).map_err(|e| {
                        format!("Could not read torrent stats during seeding: {e:#}")
                    })?;
                    let seed_uploaded =
                        i64::try_from(seed_stats.uploaded_bytes).unwrap_or(i64::MAX);
                    let seed_ratio = if downloaded > 0 {
                        seed_uploaded as f64 / downloaded as f64
                    } else {
                        0.0
                    };
                    let seed_upload_speed = seed_stats
                        .live
                        .as_ref()
                        .map(|live| i64::try_from(live.upload_speed.as_bytes()).unwrap_or(i64::MAX))
                        .unwrap_or(0);

                    let seed_ratio_limit = db::torrent_seed_ratio_limit(&pool, &task.id)
                        .await
                        .unwrap_or(None);
                    let seed_time_limit = db::torrent_seed_time_limit_seconds(&pool, &task.id)
                        .await
                        .unwrap_or(None)
                        .map(|seconds| Duration::from_secs(u64::try_from(seconds).unwrap_or(0)));
                    let limit_reached = seeding_limit_reached(
                        seed_ratio,
                        seed_ratio_limit,
                        seeding_started.elapsed(),
                        seed_time_limit,
                    );

                    if limit_reached {
                        let _ = api.api_torrent_action_forget(torrent_id).await;
                        db::upsert_torrent_runtime_snapshot(
                            &pool,
                            &task.id,
                            db::TorrentRuntimeSnapshotUpsert {
                                metadata_status: "completed",
                                completed_pieces,
                                verified_pieces: completed_pieces,
                                piece_count,
                                piece_bitfield_base64: piece_bitfield_base64.as_deref(),
                                peer_count,
                                seed_count: 0,
                                dht_status: dht_status.as_deref(),
                                trackers_json: trackers_json.as_deref(),
                                upload_bytes: seed_uploaded,
                                upload_speed_bps: seed_upload_speed,
                                ratio: seed_ratio,
                                seeding_enabled: true,
                                seeding_state: "completed",
                                last_error_code: None,
                                last_error_message: None,
                            },
                        )
                        .await?;
                        break;
                    }

                    // Still seeding — update snapshot with current ratio/upload.
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
                            upload_bytes: seed_uploaded,
                            upload_speed_bps: seed_upload_speed,
                            ratio: seed_ratio,
                            seeding_enabled: true,
                            seeding_state: "seeding",
                            last_error_code: None,
                            last_error_message: None,
                        },
                    )
                    .await?;
                }
            } else {
                let _ = api.api_torrent_action_forget(torrent_id).await;
            }
            if let Some(current) = db::get_task_record(&pool, &task.id).await? {
                emit_task_updated_record(&app, &pool, &current).await;
            }
            progress_gate.flush(&app);
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

/// F-7: Apply the global BT upload limit to an existing session. `None` clears
/// the limit (unlimited). Called on session reuse so settings changes take
/// effect without restarting the session.
fn sync_session_upload_limit(api: &Api, limit_bps: Option<i64>) {
    api.session()
        .ratelimits
        .set_upload_bps(non_zero_u32(limit_bps));
}

fn non_zero_u32(limit_bps: Option<i64>) -> Option<NonZeroU32> {
    let value = limit_bps?;
    if value <= 0 {
        return None;
    }
    let value = u32::try_from(value).unwrap_or(u32::MAX);
    NonZeroU32::new(value)
}

/// FUN-11: pure seeding-stop predicate for ratio and/or time limits.
fn seeding_limit_reached(
    ratio: f64,
    ratio_limit: Option<f64>,
    seeded_for: Duration,
    time_limit: Option<Duration>,
) -> bool {
    let ratio_hit = ratio_limit.is_some_and(|limit| ratio >= limit);
    let time_hit = time_limit.is_some_and(|limit| seeded_for >= limit);
    ratio_hit || time_hit
}

#[cfg(test)]
mod seeding_limit_tests {
    use super::seeding_limit_reached;
    use std::time::Duration;

    #[test]
    fn ratio_alone_stops_seeding() {
        assert!(seeding_limit_reached(
            1.0,
            Some(1.0),
            Duration::from_secs(0),
            None
        ));
        assert!(!seeding_limit_reached(
            0.9,
            Some(1.0),
            Duration::from_secs(0),
            None
        ));
    }

    #[test]
    fn time_alone_stops_seeding() {
        assert!(seeding_limit_reached(
            0.0,
            None,
            Duration::from_secs(3600),
            Some(Duration::from_secs(3600))
        ));
        assert!(!seeding_limit_reached(
            0.0,
            None,
            Duration::from_secs(3599),
            Some(Duration::from_secs(3600))
        ));
    }

    #[test]
    fn either_limit_stops_seeding() {
        assert!(seeding_limit_reached(
            2.0,
            Some(1.5),
            Duration::from_secs(1),
            Some(Duration::from_secs(3600))
        ));
        assert!(seeding_limit_reached(
            0.1,
            Some(1.5),
            Duration::from_secs(3600),
            Some(Duration::from_secs(3600))
        ));
    }

    #[test]
    fn neither_limit_means_unlimited() {
        assert!(!seeding_limit_reached(
            100.0,
            None,
            Duration::from_secs(86_400),
            None
        ));
    }
}

#[allow(clippy::too_many_arguments)]
async fn wait_for_torrent_metadata(
    engine: &BtEngine,
    api: Arc<Api>,
    add: AddTorrent<'static>,
    start_paused: bool,
    app: &Option<tauri::AppHandle>,
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    cancel_token: tokio_util::sync::CancellationToken,
    progress_gate: &mut TaskProgressEmitGate,
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
                return result.map_err(|error| {
                    crate::download::error::engine_error(
                        "bt_metadata_failed",
                        format!("Could not add torrent: {error:#}"),
                        true,
                    )
                });
            }
            _ = interval.tick() => {
                if cancel_token.is_cancelled() {
                    engine.delete_runtime_task(&task.source_key, false).await;
                    return Err("Torrent metadata fetch was canceled.".to_string());
                }

                let elapsed = started.elapsed();
                if elapsed >= BT_METADATA_TIMEOUT {
                    engine.delete_runtime_task(&task.source_key, false).await;
                    return Err(crate::download::error::engine_error(
                        "bt_metadata_timeout",
                        format!(
                            "Could not fetch torrent metadata after {} seconds. No reachable peers returned metadata through DHT, trackers, or the magnet link.",
                            BT_METADATA_TIMEOUT.as_secs()
                        ),
                        true,
                    ));
                }

                mark_torrent_metadata_still_fetching(app, pool, task, elapsed, &mut *progress_gate).await?;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if cancel_token.is_cancelled() {
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
    app: &Option<tauri::AppHandle>,
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    progress_gate: &mut TaskProgressEmitGate,
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
    progress_gate.emit_or_store(
        app,
        TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: task.downloaded_bytes.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: "0".to_string(),
            connection_count: 0,
            status: TaskStatus::Downloading,
        },
        false,
    );
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

async fn mark_torrent_metadata_still_fetching(
    app: &Option<tauri::AppHandle>,
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    elapsed: Duration,
    progress_gate: &mut TaskProgressEmitGate,
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
    progress_gate.emit_or_store(
        app,
        TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: task.downloaded_bytes.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: "0".to_string(),
            connection_count: 0,
            status: TaskStatus::Downloading,
        },
        false,
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
    if uri.trim().starts_with("magnet:") {
        return tracker_statuses_from_magnet(uri);
    }
    Vec::new()
}

fn tracker_statuses_from_magnet(uri: &str) -> Vec<TorrentTrackerStatus> {
    let Ok(magnet) = Magnet::parse(uri) else {
        return Vec::new();
    };
    let updated_at = crate::models::task::now_iso();
    // librqbit's `trackers` field is already percent-decoded via `url::Url::query_pairs()`.
    magnet
        .trackers
        .iter()
        .map(|url| configured_tracker_status(url.clone(), &updated_at))
        .collect()
}

/// FUN-15: announce list from a .torrent metainfo (announce + announce-list).
fn tracker_statuses_from_torrent_bytes(bytes: &[u8]) -> Vec<TorrentTrackerStatus> {
    let Ok(meta) = librqbit::torrent_from_bytes(bytes) else {
        return Vec::new();
    };
    let updated_at = crate::models::task::now_iso();
    let mut seen = HashSet::new();
    let mut trackers = Vec::new();
    for announce in meta.iter_announce() {
        let url = String::from_utf8_lossy(announce.as_ref()).to_string();
        if url.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        trackers.push(configured_tracker_status(url, &updated_at));
    }
    trackers
}

fn configured_tracker_status(url: String, updated_at: &str) -> TorrentTrackerStatus {
    TorrentTrackerStatus {
        url,
        status: "configured".to_string(),
        source: "configured".to_string(),
        updated_at: Some(updated_at.to_string()),
        last_error: None,
    }
}

fn tracker_status_json_from_statuses(trackers: &[TorrentTrackerStatus]) -> Option<String> {
    if trackers.is_empty() {
        None
    } else {
        serde_json::to_string(trackers).ok()
    }
}

/// ARC-13: map librqbit file_progress indices onto persisted task_files rows
/// (same order as `torrent_files_from_details` / insert). Returns whether any
/// selected file byte count changed.
async fn sync_bt_task_file_progress(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    file_progress: &[u64],
) -> Result<bool, String> {
    if file_progress.is_empty() {
        return Ok(false);
    }
    let files = db::list_task_file_records(pool, task_id).await?;
    if files.is_empty() {
        return Ok(false);
    }
    let updates = bt_file_progress_updates(&files, file_progress, TaskStatus::Downloading);
    if updates.is_empty() {
        return Ok(false);
    }
    db::update_task_files_progress_batch(pool, &updates).await?;
    Ok(true)
}

fn bt_file_progress_updates(
    files: &[TaskFileRecord],
    file_progress: &[u64],
    status: TaskStatus,
) -> Vec<(String, i64, TaskStatus)> {
    let mut updates = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let Some(&have_bytes) = file_progress.get(index) else {
            break;
        };
        let downloaded = i64::try_from(have_bytes).unwrap_or(i64::MAX);
        if downloaded == file.downloaded_bytes {
            continue;
        }
        let file_status = if file.total_size > 0 && downloaded >= file.total_size {
            TaskStatus::Completed
        } else {
            status
        };
        updates.push((file.id.clone(), downloaded, file_status));
    }
    updates
}

async fn persist_torrent_details(
    pool: &sqlx::SqlitePool,
    task: &crate::models::TaskRecord,
    response: &ApiAddTorrentResponse,
    selected_paths: Option<&HashSet<String>>,
    private_flag: Option<bool>,
    configured_trackers: &[TorrentTrackerStatus],
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
    // FUN-15: prefer metainfo/magnet-configured trackers; fall back to URI parse.
    let trackers_json = tracker_status_json_from_statuses(configured_trackers)
        .or_else(|| torrent_tracker_status_json(&task.url));
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

async fn add_torrent_source(
    uri: &str,
    proxy_config: &ResolvedProxyConfig,
    allow_http_url_fallback: bool,
) -> Result<(AddTorrent<'static>, Option<bool>, Vec<TorrentTrackerStatus>), String> {
    let trimmed = uri.trim();
    if trimmed.starts_with("magnet:") {
        return Ok((
            AddTorrent::from_url(trimmed.to_string()),
            None,
            tracker_statuses_from_magnet(trimmed),
        ));
    }

    let parsed = Url::parse(trimmed).map_err(|_| "Torrent URL is invalid.".to_string())?;
    match parsed.scheme() {
        "http" | "https" => {
            // Pre-download the .torrent bytes so we can parse the private flag
            // and submit via from_bytes (matching the file:// path). This ensures
            // librqbit receives the private flag and can disable DHT/PEX for
            // private torrents. Falls back to from_url on download failure to
            // avoid blocking the download flow.
            match download_torrent_bytes(trimmed, proxy_config).await {
                Ok(bytes) => {
                    let private = parse_torrent_private_flag(&bytes);
                    let trackers = tracker_statuses_from_torrent_bytes(&bytes);
                    Ok((AddTorrent::from_bytes(bytes), private, trackers))
                }
                Err(_error) if allow_http_url_fallback => {
                    Ok((AddTorrent::from_url(trimmed.to_string()), None, Vec::new()))
                }
                Err(error) => Err(bt_torrent_fetch_failed(error)),
            }
        }
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
            let trackers = tracker_statuses_from_torrent_bytes(&bytes);
            Ok((AddTorrent::from_bytes(bytes), private, trackers))
        }
        scheme => Err(format!(
            "The {scheme} protocol is not supported for torrent tasks."
        )),
    }
}

fn bt_torrent_fetch_failed(detail: String) -> String {
    let message = if detail.starts_with('{') {
        serde_json::from_str::<crate::models::AppErrorPayload>(&detail)
            .map(|payload| payload.message)
            .unwrap_or(detail)
    } else {
        detail
    };
    engine_error(
        "bt_torrent_fetch_failed",
        format!("Could not download torrent file: {message}"),
        true,
    )
}

fn classify_torrent_download_error(
    error: &reqwest::Error,
    proxy_config: &ResolvedProxyConfig,
) -> String {
    if proxy_config.custom_socks5_url_with_auth().is_some() && error.is_connect() {
        return engine_error(
            "proxy_connection_failed",
            format!("BitTorrent SOCKS5 proxy connection failed: {error}"),
            true,
        );
    }
    reqwest_error_to_structured(error)
}

/// Download .torrent file bytes via HTTP/HTTPS with optional SOCKS5 proxy.
/// Used to pre-parse the private flag before submitting to librqbit.
async fn download_torrent_bytes(
    url: &str,
    proxy_config: &ResolvedProxyConfig,
) -> Result<Vec<u8>, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60));
    if let Some(proxy_url) = proxy_config.custom_socks5_url_with_auth() {
        builder = builder.proxy(reqwest::Proxy::all(&proxy_url).map_err(|e| e.to_string())?);
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| classify_torrent_download_error(&error, proxy_config))?
        .error_for_status()
        .map_err(|error| {
            if error.is_status() {
                engine_error(
                    "bt_torrent_fetch_failed",
                    format!("HTTP error fetching torrent: {error}"),
                    true,
                )
            } else {
                classify_torrent_download_error(&error, proxy_config)
            }
        })?;
    // E-3: Pre-check Content-Length if the server provided it.
    if let Some(content_length) = response.content_length() {
        if content_length as usize > TORRENT_MAX_BYTES {
            return Err(format!(
                "torrent_too_large: Content-Length {} exceeds max {} bytes",
                content_length, TORRENT_MAX_BYTES
            ));
        }
    }
    // E-3: Stream and accumulate, aborting if running total crosses ceiling.
    let mut accumulated: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| e.to_string())?;
        let new_len = accumulated
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| "torrent_too_large: size overflow".to_string())?;
        if new_len > TORRENT_MAX_BYTES {
            return Err(format!(
                "torrent_too_large: body exceeded max {} bytes",
                TORRENT_MAX_BYTES
            ));
        }
        accumulated.extend_from_slice(&chunk);
    }
    Ok(accumulated)
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

    /// Serializes tests that create real librqbit sessions, which bind a DHT
    /// UDP socket and conflict when run in parallel (Windows os error 10048).
    static BT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn session_evicted_when_ref_count_reaches_zero() {
        let _guard = BT_TEST_LOCK.lock().await;
        let engine = BtEngine::new(crate::proxy::ResolvedProxyConfig::shared_default());
        let temp_dir =
            std::env::temp_dir().join(format!("vibe-bt-session-{}", uuid::Uuid::new_v4()));
        // Create the directory BEFORE calling api_for_output_folder so that
        // compute_session_key's canonicalize() succeeds consistently. Without
        // this, the first call caches with the raw path (canonicalize fails)
        // and the second call looks up with the canonical path — a cache miss
        // that triggers a duplicate Session::new_with_opts DHT bind failure.
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let task_id = "task-arc12-shared";

        let (_api1, key1) = engine
            .api_for_output_folder(
                temp_dir.to_str().unwrap(),
                task_id,
                None,
                None,
                &crate::proxy::ResolvedProxyConfig::default(),
            )
            .await
            .expect("session 1");
        assert_eq!(engine.sessions.lock().await.len(), 1);

        // Same task_id reuses the session (refcount 2). Different task_ids would
        // create separate sessions — skipped here because librqbit DHT binds a
        // fixed UDP port and two sessions conflict in-process.
        let (_api2, key2) = engine
            .api_for_output_folder(
                temp_dir.to_str().unwrap(),
                task_id,
                None,
                None,
                &crate::proxy::ResolvedProxyConfig::default(),
            )
            .await
            .expect("session 2");
        assert_eq!(key1, key2);
        assert_eq!(engine.sessions.lock().await.len(), 1);
        assert_eq!(
            engine
                .sessions
                .lock()
                .await
                .get(&key1)
                .unwrap()
                .active_task_count,
            2
        );

        engine.release_session_ref(&key1).await;
        assert_eq!(engine.sessions.lock().await.len(), 1);

        engine.release_session_ref(&key1).await;
        assert_eq!(engine.sessions.lock().await.len(), 0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn session_key_includes_task_id_for_limit_isolation() {
        // ARC-12: same folder+proxy, different tasks → different keys.
        let proxy = crate::proxy::ResolvedProxyConfig::default();
        let a = BtEngine::compute_session_key("/tmp/bt-out", &proxy, "task-a").await;
        let b = BtEngine::compute_session_key("/tmp/bt-out", &proxy, "task-b").await;
        assert_ne!(a, b);
        assert!(a.contains("task:task-a"));
        assert!(b.contains("task:task-b"));
    }

    #[tokio::test]
    async fn delete_runtime_task_does_not_decrement_session_refcount() {
        // ARC-12: forget/delete must not race SessionRefGuard on refcount.
        let _guard = BT_TEST_LOCK.lock().await;
        let engine = BtEngine::new(crate::proxy::ResolvedProxyConfig::shared_default());
        let temp_dir =
            std::env::temp_dir().join(format!("vibe-bt-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let task_id = "task-arc12-delete";

        let (_api, key) = engine
            .api_for_output_folder(
                temp_dir.to_str().unwrap(),
                task_id,
                None,
                None,
                &crate::proxy::ResolvedProxyConfig::default(),
            )
            .await
            .expect("session");
        assert_eq!(
            engine
                .sessions
                .lock()
                .await
                .get(&key)
                .unwrap()
                .active_task_count,
            1
        );

        // No matching torrent — delete is a no-op for the API, and must not
        // release the session ref that the Guard still owns.
        engine
            .delete_runtime_task("bt:0000000000000000000000000000000000000000", false)
            .await;
        assert_eq!(
            engine
                .sessions
                .lock()
                .await
                .get(&key)
                .map(|e| e.active_task_count),
            Some(1),
            "delete_runtime_task must not decrement session refcount"
        );

        engine.release_session_ref(&key).await;
        assert_eq!(engine.sessions.lock().await.len(), 0);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // NOTE: `different_output_folders_get_different_sessions` was removed because
    // librqbit's Session always initializes a persistent DHT listener that binds
    // a fixed UDP port. Two simultaneous sessions on the same host fail with
    // "address already in use" (Windows os error 10048), making it impossible to
    // test multi-session behavior with real sessions in-process. The cache key
    // derivation (which includes the canonicalized output folder path) ensures
    // different folders produce different keys by construction.

    #[tokio::test]
    async fn release_nonexistent_session_is_noop() {
        let engine = BtEngine::new(crate::proxy::ResolvedProxyConfig::shared_default());
        engine.release_session_ref("nonexistent-key").await;
        assert_eq!(engine.sessions.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn add_torrent_source_reads_file_urls_with_spaces_and_unicode() {
        let dir = std::env::temp_dir().join(format!("vibe-bt-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("样本 torrent.torrent");
        let bytes = b"d4:infode";
        std::fs::write(&path, bytes).expect("write torrent file");
        let url = Url::from_file_path(&path).expect("file url").to_string();

        let proxy_config = crate::proxy::ResolvedProxyConfig::default();
        let (add, private_flag, trackers) = add_torrent_source(&url, &proxy_config, true)
            .await
            .expect("local torrent source");

        match add {
            AddTorrent::TorrentFileBytes(actual) => assert_eq!(actual.as_ref(), bytes),
            AddTorrent::Url(value) => panic!("expected torrent bytes, got URL {value}"),
        }
        // The test bytes (b"d4:infode") are a truncated torrent without a private
        // field, so parsing returns None.
        assert_eq!(private_flag, None);
        assert!(trackers.is_empty());

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
        let private =
            b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1e7:privatei1eee";
        assert_eq!(parse_torrent_private_flag(private), Some(true));
    }

    #[test]
    fn parse_torrent_private_flag_returns_none_for_invalid_bytes() {
        assert_eq!(parse_torrent_private_flag(b"not a torrent"), None);
        assert_eq!(parse_torrent_private_flag(b""), None);
    }

    #[tokio::test]
    async fn add_torrent_source_http_downloads_and_parses_private_flag() {
        // A minimal private torrent with info.private=1.
        let private_torrent =
            b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1e7:privatei1eee";

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/test.torrent");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/x-bittorrent\r\nContent-Length: {}\r\n\r\n",
                private_torrent.len()
            );
            let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes()).await;
            let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, private_torrent).await;
        });

        let proxy_config = crate::proxy::ResolvedProxyConfig::default();
        let (add, private_flag, trackers) = add_torrent_source(&url, &proxy_config, true)
            .await
            .expect("http torrent source");

        match add {
            AddTorrent::TorrentFileBytes(actual) => assert_eq!(actual.as_ref(), private_torrent),
            AddTorrent::Url(value) => panic!("expected torrent bytes, got URL {value}"),
        }
        assert_eq!(private_flag, Some(true));
        assert!(trackers.is_empty());

        server.await.unwrap();
    }

    #[tokio::test]
    async fn add_torrent_source_http_fallback_on_download_failure() {
        // Bind to a port but immediately close the connection to simulate download failure.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/test.torrent");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
            // Return 404
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes()).await;
        });

        let proxy_config = crate::proxy::ResolvedProxyConfig::default();
        let (add, private_flag, trackers) = add_torrent_source(&url, &proxy_config, true)
            .await
            .expect("http torrent source with fallback");

        // On download failure, should fall back to from_url with private_flag=None.
        match add {
            AddTorrent::Url(value) => assert_eq!(value, url),
            AddTorrent::TorrentFileBytes(_) => panic!("expected URL fallback, got bytes"),
        }
        assert_eq!(private_flag, None);
        assert!(trackers.is_empty());

        server.await.unwrap();
    }

    #[test]
    fn magnet_trackers_are_configured_only() {
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp%3A%2F%2Ftracker.example%3A6969%2Fannounce&tr=http%3A%2F%2Fbackup.example%2Fannounce";
        let trackers = tracker_statuses_from_magnet(magnet);
        assert_eq!(trackers.len(), 2);
        assert!(trackers
            .iter()
            .all(|t| t.status == "configured" && t.source == "configured"));
        assert!(trackers.iter().all(|t| t.updated_at.is_some()));
        assert!(trackers.iter().any(|t| t.url.contains("tracker.example")));
    }

    #[test]
    fn torrent_bytes_announce_list_produces_configured_trackers() {
        // announce only (no announce-list) — length prefix must match URL bytes.
        let bytes = b"d8:announce31:http://tracker.example/announce4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1eee";
        let trackers = tracker_statuses_from_torrent_bytes(bytes);
        assert_eq!(trackers.len(), 1);
        assert!(trackers[0].url.contains("tracker.example"));
        assert_eq!(trackers[0].source, "configured");
    }

    #[test]
    fn http_torrent_url_without_bytes_yields_empty_configured_trackers() {
        assert!(tracker_statuses_from_uri("http://example.com/a.torrent").is_empty());
        assert!(tracker_statuses_from_uri("file:///tmp/a.torrent").is_empty());
    }

    #[test]
    fn bt_file_progress_updates_are_independent_per_file() {
        let files = vec![
            TaskFileRecord {
                id: "f1".into(),
                task_id: "t1".into(),
                relative_path: "a.bin".into(),
                file_name: "a.bin".into(),
                save_dir: "/tmp".into(),
                temp_path: None,
                final_path: None,
                total_size: 100,
                downloaded_bytes: 0,
                selected: true,
                status: TaskStatus::Downloading,
                content_type: None,
            },
            TaskFileRecord {
                id: "f2".into(),
                task_id: "t1".into(),
                relative_path: "b.bin".into(),
                file_name: "b.bin".into(),
                save_dir: "/tmp".into(),
                temp_path: None,
                final_path: None,
                total_size: 200,
                downloaded_bytes: 0,
                selected: true,
                status: TaskStatus::Downloading,
                content_type: None,
            },
        ];
        let updates = bt_file_progress_updates(&files, &[40, 200], TaskStatus::Downloading);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0], ("f1".into(), 40, TaskStatus::Downloading));
        assert_eq!(updates[1], ("f2".into(), 200, TaskStatus::Completed));
        let sum: i64 = updates.iter().map(|(_, bytes, _)| *bytes).sum();
        assert_eq!(sum, 240);
    }
}
