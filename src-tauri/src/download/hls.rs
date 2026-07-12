use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, BlockModeDecrypt, KeyIvInit};
use hls_m3u8::{MasterPlaylist as ParsedMasterPlaylist, MediaPlaylist as ParsedMediaPlaylist};
use reqwest::{
    header::{HeaderName, HeaderValue, ACCEPT_ENCODING, RANGE},
    Client, RequestBuilder, StatusCode,
};
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
    process::Command,
    task::JoinSet,
};
use uuid::Uuid;

use super::{
    engine::EngineFuture, http::HttpEngine, read_with_idle_timeout, url_classify::is_hls_url,
    DownloadContext, DownloadEngine, DownloadError, IdleReadOutcome, ProbeOutput, ProbeRequest,
    READ_IDLE_TIMEOUT,
};
use crate::download::error::engine_error;
use crate::download::retry::RetryPolicy;
use crate::{
    db,
    events::{emit_task_updated_record, DbWriteGate, TaskProgressEmitGate},
    models::{
        AppErrorPayload, EngineCapabilities, HlsMediaTrack, HlsVariant, ProbedFile, SegmentStatus,
        TaskKind, TaskProgressPayload, TaskRecord, TaskStatus,
    },
};

type Aes128CbcDec = cbc::Decryptor<Aes128>;

const PROTOCOL_HLS: &str = "hls";
const HLS_CONTENT_TYPE: &str = "application/vnd.apple.mpegurl";
const HLS_SEGMENT_RETRIES: i32 = 2;
const HLS_LIVE_MAX_IDLE_POLLS: usize = 6;
/// E-2: Maximum allowed size for a single HLS segment. Protects against
/// malicious or malformed playlists that would otherwise buffer an unbounded
/// segment into memory.
const HLS_SEGMENT_MAX_BYTES: usize = 512 * 1024 * 1024;
/// E-2: Maximum allowed size for HLS init maps, keys, and playlists fetched
/// via `fetch_bytes`. These are small control-plane resources; a 64 MiB cap
/// is generous while preventing unbounded reads.
const HLS_INIT_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HlsEngine {
    /// E-4: 共享 `HttpEngine` 的客户端缓存，避免每次 probe/download 新建 Client。
    http: Arc<HttpEngine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlaylistKind {
    Vod,
    Event,
    Live,
}

impl PlaylistKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Vod => "vod",
            Self::Event => "event",
            Self::Live => "live",
        }
    }

    fn is_live_like(&self) -> bool {
        !matches!(self, Self::Vod)
    }
}

#[derive(Debug, Clone)]
struct MasterVariant {
    uri: String,
    bandwidth: i64,
    resolution: Option<(i64, i64)>,
    codecs: Option<String>,
}

#[derive(Debug, Clone)]
struct MediaPlaylist {
    kind: PlaylistKind,
    target_duration: i64,
    media_sequence: i64,
    end_list: bool,
    segments: Vec<HlsSegment>,
}

#[derive(Debug, Clone)]
struct HlsSegment {
    media_sequence: i64,
    discontinuity_sequence: i64,
    uri: String,
    duration_ms: i64,
    byte_range: Option<ByteRange>,
    init_map: Option<HlsInitMap>,
    key: Option<HlsKey>,
}

#[derive(Debug, Clone)]
struct HlsInitMap {
    uri: String,
    byte_range: Option<ByteRange>,
}

#[derive(Debug, Clone)]
struct ByteRange {
    start: Option<i64>,
    length: i64,
}

#[derive(Debug, Clone)]
struct HlsKey {
    method: String,
    uri: Option<String>,
    iv: Option<String>,
}

#[derive(Debug, Clone)]
struct ProbePlan {
    media_url: String,
    kind: PlaylistKind,
    target_duration: i64,
    selected_bandwidth: Option<i64>,
    selected_resolution: Option<String>,
    variants: Vec<HlsVariant>,
    audio_tracks: Vec<HlsMediaTrack>,
    subtitle_tracks: Vec<HlsMediaTrack>,
    display_name: String,
}

#[derive(Debug, Clone)]
struct SegmentDownloadPlan {
    task_id: String,
    id: String,
    media_sequence: i64,
    uri: String,
    local_path: PathBuf,
    byte_range: Option<ByteRange>,
    init_map: Option<ResolvedHlsInitMap>,
    key: Option<HlsKey>,
}

#[derive(Debug, Clone)]
struct ResolvedHlsInitMap {
    uri: String,
    local_path: PathBuf,
    byte_range: Option<ByteRange>,
}

#[derive(Debug)]
struct SegmentDownloadResult {
    media_sequence: i64,
    bytes: i64,
    result: Result<(), String>,
}

impl HlsEngine {
    pub fn new(http: Arc<HttpEngine>) -> Self {
        Self { http }
    }

    async fn client(&self) -> Result<Client, String> {
        self.http.client().await
    }

    async fn probe_hls(
        &self,
        url: &str,
        request_headers: &[(String, String)],
        app: &Option<tauri::AppHandle>,
        request_id: &Option<String>,
    ) -> Result<ProbePlan, String> {
        crate::download::engine::emit_probe_phase(app, request_id, "checking_ffmpeg", Some("hls"));
        ensure_ffmpeg_available()?;
        let client = self.client().await?;
        crate::download::engine::emit_probe_phase(
            app,
            request_id,
            "fetching_manifest",
            Some("hls"),
        );
        let body = fetch_text(&client, url, request_headers).await?;
        validate_playlist_syntax(&body)?;
        crate::download::engine::emit_probe_phase(app, request_id, "parsing_manifest", Some("hls"));
        let media_tracks = parse_ext_x_media(&body);
        let (media_url, selected_bandwidth, selected_resolution, variants, media_body) =
            if is_master_playlist(&body) {
                let variant = choose_master_variant(&body)?;
                let selected_uri = variant.uri.clone();
                let variants = hls_variants_from_master(&body, &selected_uri);
                let media_url = resolve_url(url, &variant.uri)?;
                let media_body = fetch_text(&client, &media_url, request_headers).await?;
                validate_playlist_syntax(&media_body)?;
                (
                    media_url,
                    Some(variant.bandwidth),
                    variant
                        .resolution
                        .map(|(width, height)| format!("{width}x{height}")),
                    variants,
                    media_body,
                )
            } else {
                (url.to_string(), None, None, Vec::new(), body.clone())
            };
        let media = parse_media_playlist(&media_body)?;
        reject_unsupported_media_playlist(&media_body)?;
        let audio_tracks = media_tracks
            .iter()
            .filter(|t| t.kind == "AUDIO")
            .cloned()
            .collect::<Vec<_>>();
        let subtitle_tracks = media_tracks
            .iter()
            .filter(|t| t.kind == "SUBTITLES")
            .cloned()
            .collect::<Vec<_>>();
        Ok(ProbePlan {
            media_url,
            kind: media.kind,
            target_duration: media.target_duration,
            selected_bandwidth,
            selected_resolution,
            variants,
            audio_tracks,
            subtitle_tracks,
            display_name: hls_output_name(url),
        })
    }
}

impl DownloadEngine for HlsEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_HLS
    }

    /// R-3: HLS 仅靠 `matches_url`（`.m3u8` 后缀）路由，不参与 scheme 兜底，
    /// 避免普通 https URL 被误路由到 HLS。
    fn supports_scheme(&self, _scheme: &str) -> bool {
        false
    }

    /// R-3: `.m3u8` 路径路由到 HLS 引擎。优先级 80（介于 Metalink 90 与 DASH 70 之间）。
    fn matches_url(&self, url: &reqwest::Url) -> bool {
        is_hls_url(url)
    }

    fn priority(&self) -> i32 {
        80
    }

    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, DownloadError>> {
        Box::pin(async move {
            let plan = self
                .probe_hls(
                    &request.uri,
                    &request.request_headers,
                    &request.app,
                    &request.request_id,
                )
                .await
                .map_err(DownloadError::Other)?;
            let source_key = reqwest::Url::parse(&plan.media_url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_else(|| "hls".to_string());
            Ok(ProbeOutput {
                protocol: PROTOCOL_HLS.to_string(),
                task_kind: TaskKind::Manifest,
                resolved_uri: plan.media_url.clone(),
                display_name: plan.display_name.clone(),
                total_size: 0,
                source_key,
                capabilities: EngineCapabilities {
                    supports_resume: true,
                    supports_parallel: true,
                    supports_multi_file: false,
                },
                files: vec![ProbedFile {
                    relative_path: plan.display_name,
                    size: "0".to_string(),
                    content_type: Some("video/mp4".to_string()),
                }],
                etag: None,
                last_modified: None,
                content_type: Some(HLS_CONTENT_TYPE.to_string()),
                hls_variants: plan.variants.clone(),
                hls_audio_tracks: plan.audio_tracks.clone(),
                hls_subtitle_tracks: plan.subtitle_tracks.clone(),
                metalink: None,
            })
        })
    }

    fn download<'a>(
        &'a self,
        context: DownloadContext,
    ) -> EngineFuture<'a, Result<(), DownloadError>> {
        Box::pin(async move {
            run_hls_download(self.clone(), context)
                .await
                .map_err(DownloadError::Other)
        })
    }
}

async fn run_hls_download(engine: HlsEngine, context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel_token,
        finish,
        speed_limiter,
        connection_limit,
        request_headers,
        proxy_config: _proxy_config,
    } = context;
    ensure_ffmpeg_available()?;
    // Capture ffmpeg path once before any awaits to avoid TOCTOU:
    // if ffmpeg were removed between ensure_ffmpeg_available() and run_ffmpeg(),
    // the second call would return None and fail after a long download.
    let ffmpeg = ffmpeg_path().ok_or_else(|| {
        engine_error(
            "hls_ffmpeg_missing",
            "ffmpeg was not found. Install ffmpeg or set VIBE_FFMPEG_PATH before creating HLS tasks.",
            true,
        )
    })?;
    let client = engine.client().await?;
    let staging_dir = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "HLS task is missing a staging path.".to_string())?;
    fs::create_dir_all(&staging_dir).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not create HLS staging folder: {e}"))
            .command_error()
    })?;

    let plan = engine
        .probe_hls(&task.url, &request_headers, &None, &None)
        .await?;
    db::upsert_hls_task(
        &pool,
        db::HlsTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            media_url: &plan.media_url,
            playlist_kind: plan.kind.as_str(),
            selected_bandwidth: plan.selected_bandwidth,
            selected_resolution: plan.selected_resolution.as_deref(),
            target_duration: plan.target_duration,
            last_media_sequence: None,
            output_format: "mp4",
            staging_dir: &staging_dir.to_string_lossy(),
            selected_audio_track_uris: None,
            selected_subtitle_track_uris: None,
        },
    )
    .await?;
    db::insert_task_event(
        &pool,
        &task.id,
        "hls_playlist_resolved",
        Some(&plan.media_url),
    )
    .await?;

    // F-6: Read selected audio/subtitle track URIs from DB (stored by create_task)
    // and download them into staging subdirs for ffmpeg muxing.
    let mut extra_inputs: Vec<PathBuf> = Vec::new();
    if let Ok(Some(hls_task)) = db::get_hls_task(&pool, &task.id).await {
        if let Some(audio_json) = hls_task.selected_audio_track_uris.as_deref() {
            if let Ok(uris) = serde_json::from_str::<Vec<String>>(audio_json) {
                for uri in &uris {
                    if cancel_token.is_cancelled() {
                        break;
                    }
                    match download_external_track(
                        &client,
                        uri,
                        &request_headers,
                        &staging_dir,
                        "audio",
                        uri.rsplit('/').next().unwrap_or("track"),
                        &cancel_token,
                    )
                    .await
                    {
                        Ok(Some(path)) => extra_inputs.push(path),
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!(task_id = %task.id, error = %e, "Failed to download audio track");
                        }
                    }
                }
            }
        }
        if let Some(subtitle_json) = hls_task.selected_subtitle_track_uris.as_deref() {
            if let Ok(uris) = serde_json::from_str::<Vec<String>>(subtitle_json) {
                for uri in &uris {
                    if cancel_token.is_cancelled() {
                        break;
                    }
                    match download_external_track(
                        &client,
                        uri,
                        &request_headers,
                        &staging_dir,
                        "subtitle",
                        uri.rsplit('/').next().unwrap_or("track"),
                        &cancel_token,
                    )
                    .await
                    {
                        Ok(Some(path)) => extra_inputs.push(path),
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!(task_id = %task.id, error = %e, "Failed to download subtitle track");
                        }
                    }
                }
            }
        }
    }

    // E-5a: Single scan of completed HLS segments — derive both the
    // cumulative downloaded byte count and the (disc_seq, media_seq) set
    // from one `list_hls_segments` round-trip instead of two.
    let (mut downloaded_total, mut seen) = existing_hls_progress(&pool, &task.id).await?;
    let mut idle_polls = 0_usize;
    let mut progress_gate = TaskProgressEmitGate::default();
    // E-4: Cache first segment ID once and throttle per-segment DB writes.
    let first_segment_id: Option<String> = db::get_first_segment_record(&pool, &task.id)
        .await?
        .map(|segment| segment.id);
    let mut db_write_gate = DbWriteGate::default();

    loop {
        if cancel_token.is_cancelled() {
            pause_hls_task(&app, &pool, &task, downloaded_total).await?;
            progress_gate.flush(&app);
            return Ok(());
        }
        if finish.load(Ordering::SeqCst) || db::hls_finish_requested(&pool, &task.id).await? {
            break;
        }

        let media_body = fetch_text(&client, &plan.media_url, &request_headers).await?;
        validate_playlist_syntax(&media_body)?;
        reject_unsupported_media_playlist(&media_body)?;
        let media = parse_media_playlist(&media_body)?;
        let _playlist_sequence = media.media_sequence;
        let new_segments = media
            .segments
            .iter()
            .filter(|segment| seen.insert((segment.discontinuity_sequence, segment.media_sequence)))
            .cloned()
            .collect::<Vec<_>>();
        if new_segments.is_empty() {
            idle_polls += 1;
        } else {
            idle_polls = 0;
            let plans = persist_hls_segment_plans(
                &pool,
                &task,
                &staging_dir,
                &plan.media_url,
                new_segments,
            )
            .await?;
            let downloaded = download_hls_segments(
                &app,
                &pool,
                &task,
                &client,
                &request_headers,
                &speed_limiter,
                &cancel_token,
                connection_limit.max(1),
                downloaded_total,
                plans,
                &mut progress_gate,
                first_segment_id.as_deref(),
                &mut db_write_gate,
            )
            .await?;
            downloaded_total = downloaded;
        }

        if media.end_list || (!plan.kind.is_live_like() && idle_polls > 0) {
            break;
        }
        if idle_polls >= HLS_LIVE_MAX_IDLE_POLLS && finish.load(Ordering::SeqCst) {
            break;
        }

        let delay = Duration::from_secs(u64::try_from(media.target_duration.max(1)).unwrap_or(1));
        tokio::time::sleep(delay).await;
    }

    if cancel_token.is_cancelled() {
        pause_hls_task(&app, &pool, &task, downloaded_total).await?;
        progress_gate.flush(&app);
        return Ok(());
    }

    progress_gate.flush(&app);
    finalize_hls_task(
        &app,
        &pool,
        &task,
        &staging_dir,
        downloaded_total,
        &ffmpeg,
        &extra_inputs,
    )
    .await
}

async fn persist_hls_segment_plans(
    pool: &SqlitePool,
    task: &TaskRecord,
    staging_dir: &Path,
    media_url: &str,
    segments: Vec<HlsSegment>,
) -> Result<Vec<SegmentDownloadPlan>, String> {
    let mut plans = Vec::new();
    // E-3: Collect upsert rows and persist in a single transaction at the end,
    // avoiding N independent WAL commits for N segments.
    let mut upserts: Vec<db::HlsSegmentUpsert<'_>> = Vec::with_capacity(segments.len());
    // Owned strings that back the `&str` borrows in `upserts`. We keep them
    // alive until the bulk insert completes.
    let mut owned_ids: Vec<String> = Vec::with_capacity(segments.len());
    let mut owned_uris: Vec<String> = Vec::with_capacity(segments.len());
    let mut owned_local_paths: Vec<String> = Vec::with_capacity(segments.len());
    let mut owned_init_map_uris: Vec<Option<String>> = Vec::with_capacity(segments.len());
    let mut owned_init_map_local_paths: Vec<Option<String>> = Vec::with_capacity(segments.len());
    let mut owned_key_methods: Vec<Option<String>> = Vec::with_capacity(segments.len());
    let mut owned_key_uris: Vec<Option<String>> = Vec::with_capacity(segments.len());
    let mut owned_key_ivs: Vec<Option<String>> = Vec::with_capacity(segments.len());
    let mut meta_media_sequence: Vec<i64> = Vec::with_capacity(segments.len());
    let mut meta_discontinuity_sequence: Vec<i64> = Vec::with_capacity(segments.len());
    let mut meta_duration_ms: Vec<i64> = Vec::with_capacity(segments.len());
    let mut meta_byte_range_start: Vec<Option<i64>> = Vec::with_capacity(segments.len());
    let mut meta_byte_range_length: Vec<Option<i64>> = Vec::with_capacity(segments.len());
    let mut meta_init_map_byte_range_start: Vec<Option<i64>> = Vec::with_capacity(segments.len());
    let mut meta_init_map_byte_range_length: Vec<Option<i64>> = Vec::with_capacity(segments.len());
    for segment in segments {
        let id = Uuid::new_v4().to_string();
        let local_name = format!(
            "seg-{}-{}.bin",
            segment.discontinuity_sequence, segment.media_sequence
        );
        let local_path = staging_dir.join(local_name);
        let uri = resolve_url(media_url, &segment.uri)?;
        let init_map = segment
            .init_map
            .as_ref()
            .map(|map| {
                let uri = resolve_url(media_url, &map.uri)?;
                Ok::<_, String>(ResolvedHlsInitMap {
                    local_path: staging_dir
                        .join(init_map_local_name(&uri, map.byte_range.as_ref())),
                    uri,
                    byte_range: map.byte_range.clone(),
                })
            })
            .transpose()?;
        let key_uri = segment
            .key
            .as_ref()
            .and_then(|key| key.uri.as_deref())
            .map(|value| resolve_url(media_url, value))
            .transpose()?;
        let key = segment.key.as_ref().map(|key| HlsKey {
            method: key.method.clone(),
            uri: key_uri.clone(),
            iv: key.iv.clone(),
        });
        let init_map_local_path = init_map
            .as_ref()
            .map(|map| map.local_path.to_string_lossy().to_string());
        meta_media_sequence.push(segment.media_sequence);
        meta_discontinuity_sequence.push(segment.discontinuity_sequence);
        meta_duration_ms.push(segment.duration_ms);
        meta_byte_range_start.push(segment.byte_range.as_ref().and_then(|range| range.start));
        meta_byte_range_length.push(segment.byte_range.as_ref().map(|range| range.length));
        meta_init_map_byte_range_start.push(
            init_map
                .as_ref()
                .and_then(|map| map.byte_range.as_ref())
                .and_then(|range| range.start),
        );
        meta_init_map_byte_range_length.push(
            init_map
                .as_ref()
                .and_then(|map| map.byte_range.as_ref())
                .map(|range| range.length),
        );
        owned_ids.push(id.clone());
        owned_uris.push(uri.clone());
        owned_local_paths.push(local_path.to_string_lossy().to_string());
        owned_init_map_uris.push(init_map.as_ref().map(|map| map.uri.clone()));
        owned_init_map_local_paths.push(init_map_local_path.clone());
        owned_key_methods.push(key.as_ref().map(|key| key.method.clone()));
        owned_key_uris.push(key.as_ref().and_then(|key| key.uri.clone()));
        owned_key_ivs.push(key.as_ref().and_then(|key| key.iv.clone()));
        plans.push(SegmentDownloadPlan {
            task_id: task.id.clone(),
            id,
            media_sequence: segment.media_sequence,
            uri,
            local_path,
            byte_range: segment.byte_range,
            init_map,
            key,
        });
    }
    for idx in 0..owned_ids.len() {
        upserts.push(db::HlsSegmentUpsert {
            id: &owned_ids[idx],
            task_id: &task.id,
            media_sequence: meta_media_sequence[idx],
            discontinuity_sequence: meta_discontinuity_sequence[idx],
            uri: &owned_uris[idx],
            local_path: &owned_local_paths[idx],
            duration_ms: meta_duration_ms[idx],
            byte_range_start: meta_byte_range_start[idx],
            byte_range_length: meta_byte_range_length[idx],
            init_map_uri: owned_init_map_uris[idx].as_deref(),
            init_map_local_path: owned_init_map_local_paths[idx].as_deref(),
            init_map_byte_range_start: meta_init_map_byte_range_start[idx],
            init_map_byte_range_length: meta_init_map_byte_range_length[idx],
            key_method: owned_key_methods[idx].as_deref(),
            key_uri: owned_key_uris[idx].as_deref(),
            key_iv: owned_key_ivs[idx].as_deref(),
        });
    }
    // E-3: Single transaction for all segments in this batch.
    db::bulk_upsert_hls_segments(pool, &upserts).await?;
    Ok(plans)
}

#[allow(clippy::too_many_arguments)]
async fn download_hls_segments(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    connection_limit: usize,
    initial_downloaded: i64,
    plans: Vec<SegmentDownloadPlan>,
    progress_gate: &mut TaskProgressEmitGate,
    first_segment_id: Option<&str>,
    db_write_gate: &mut DbWriteGate,
) -> Result<i64, String> {
    let mut pending = plans.into_iter();
    let mut workers = JoinSet::new();
    let mut active = 0_usize;
    let mut downloaded_total = initial_downloaded;

    loop {
        while active < connection_limit {
            let Some(plan) = pending.next() else {
                break;
            };
            active += 1;
            let client = client.clone();
            let pool = pool.clone();
            let request_headers = request_headers.to_vec();
            let speed_limiter = speed_limiter.clone();
            let cancel_token = cancel_token.clone();
            workers.spawn(async move {
                download_hls_segment(
                    &pool,
                    &client,
                    request_headers,
                    speed_limiter,
                    cancel_token,
                    plan,
                )
                .await
            });
        }

        if active == 0 {
            break;
        }
        let joined = workers
            .join_next()
            .await
            .ok_or_else(|| "HLS segment worker stopped unexpectedly.".to_string())?;
        active = active.saturating_sub(1);
        let result = joined.map_err(|e| format!("A HLS worker stopped unexpectedly: {e}"))?;
        match result.result {
            Ok(()) => {
                downloaded_total = downloaded_total.saturating_add(result.bytes);
                db::update_hls_last_media_sequence(pool, &task.id, result.media_sequence).await?;
                emit_hls_progress(
                    app,
                    pool,
                    task,
                    downloaded_total,
                    active + 1,
                    progress_gate,
                    first_segment_id,
                    db_write_gate,
                    false,
                )
                .await?;
            }
            Err(error) => {
                cancel_token.cancel();
                workers.abort_all();
                progress_gate.flush(app);
                return Err(engine_error(
                    "hls_segment_failed",
                    format!("HLS segment download failed: {error}"),
                    true,
                ));
            }
        }

        if cancel_token.is_cancelled() {
            workers.abort_all();
            break;
        }
    }

    Ok(downloaded_total)
}

async fn download_hls_segment(
    pool: &SqlitePool,
    client: &Client,
    request_headers: Vec<(String, String)>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: tokio_util::sync::CancellationToken,
    plan: SegmentDownloadPlan,
) -> SegmentDownloadResult {
    let retry_policy = RetryPolicy::hls_segment();
    let mut retry_count = 0;
    let mut last_error = None;
    loop {
        let started = Instant::now();
        let result = download_hls_segment_once(
            pool,
            client,
            &request_headers,
            &speed_limiter,
            &cancel_token,
            &plan,
        )
        .await;
        crate::download::diagnostics::persist_engine_diagnostic(
            crate::download::diagnostics::EngineDiagnosticContext {
                pool,
                task_id: &plan.task_id,
                method: "GET",
                url: &plan.uri,
                range_header: plan.byte_range.as_ref().map(byte_range_header),
                status_code: result.as_ref().ok().map(|_| 200),
                content_length: None,
                error: result.as_ref().err().map(String::as_str),
                retry_count,
                duration: started.elapsed(),
            },
        )
        .await;
        match result {
            Ok(bytes) => {
                let _ = db::update_hls_segment_status(
                    pool,
                    &plan.id,
                    bytes,
                    SegmentStatus::Completed,
                    retry_count,
                    None,
                )
                .await;
                return SegmentDownloadResult {
                    media_sequence: plan.media_sequence,
                    bytes,
                    result: Ok(()),
                };
            }
            Err(error) if cancel_token.is_cancelled() => {
                return SegmentDownloadResult {
                    media_sequence: plan.media_sequence,
                    bytes: 0,
                    result: Err(error),
                };
            }
            Err(error) if retry_count < HLS_SEGMENT_RETRIES => {
                retry_count += 1;
                last_error = Some(error.clone());
                let _ = db::update_hls_segment_status(
                    pool,
                    &plan.id,
                    0,
                    SegmentStatus::Pending,
                    retry_count,
                    Some(&error),
                )
                .await;
                let delay = retry_policy.delay_for_attempt(u32::try_from(retry_count).unwrap_or(1));
                if !delay.is_zero() {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            return SegmentDownloadResult {
                                media_sequence: plan.media_sequence,
                                bytes: 0,
                                result: Err(last_error.unwrap_or(error)),
                            };
                        }
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
            Err(error) => {
                let message = last_error.unwrap_or(error);
                let _ = db::update_hls_segment_status(
                    pool,
                    &plan.id,
                    0,
                    SegmentStatus::Failed,
                    retry_count,
                    Some(&message),
                )
                .await;
                return SegmentDownloadResult {
                    media_sequence: plan.media_sequence,
                    bytes: 0,
                    result: Err(message),
                };
            }
        }
    }
}

async fn download_hls_segment_once(
    pool: &SqlitePool,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    plan: &SegmentDownloadPlan,
) -> Result<i64, String> {
    db::update_hls_segment_status(pool, &plan.id, 0, SegmentStatus::Downloading, 0, None).await?;
    if let Some(parent) = plan.local_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not create HLS segment folder: {e}"))
                .command_error()
        })?;
    }
    if let Some(init_map) = &plan.init_map {
        ensure_hls_init_map(
            client,
            request_headers,
            speed_limiter,
            cancel_token,
            init_map,
        )
        .await?;
    }
    let mut response = apply_forwarded_headers(client.get(&plan.uri), request_headers)
        .header(ACCEPT_ENCODING, "identity");
    if let Some(range) = &plan.byte_range {
        response = response.header(RANGE, byte_range_header(range));
    }
    let mut response = response
        .send()
        .await
        .map_err(|e| format!("Could not request HLS segment: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("server returned {}", response.status()));
    }
    if plan.byte_range.is_some() && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err("server did not honor HLS byte range request".to_string());
    }

    let key = plan.key.as_ref();
    if let Some(key) = key {
        // E-1 + E-2: Encrypted segment — buffer the ciphertext with a safety
        // size limit and idle timeout, then decrypt the full buffer and write
        // to disk. A streaming AES-128-CBC decryptor would avoid the double
        // buffer but requires manual block-alignment and PKCS7 tail handling;
        // the size cap already removes the unbounded-growth risk.
        let mut data: Vec<u8> = Vec::new();
        loop {
            let outcome = tokio::select! {
                _ = cancel_token.cancelled() => {
                    return Err("Download canceled.".to_string());
                }
                outcome = read_with_idle_timeout(response.chunk(), READ_IDLE_TIMEOUT) => outcome,
            };
            let chunk = match outcome {
                IdleReadOutcome::Data(chunk) => chunk,
                IdleReadOutcome::End => break,
                IdleReadOutcome::Error(e) => {
                    return Err(format!("HLS segment connection failed: {e}"));
                }
                IdleReadOutcome::IdleTimeout => {
                    return Err(engine_error(
                        "hls_segment_stalled",
                        "HLS segment stalled: no data received for 60 seconds.",
                        true,
                    ));
                }
            };
            speed_limiter.throttle(chunk.len()).await;
            data.extend_from_slice(&chunk);
            if data.len() > HLS_SEGMENT_MAX_BYTES {
                return Err(engine_error(
                    "hls_segment_too_large",
                    format!("HLS segment exceeds the {HLS_SEGMENT_MAX_BYTES} byte safety limit."),
                    false,
                ));
            }
        }
        let decrypted =
            decrypt_hls_segment(client, request_headers, key, plan.media_sequence, data).await?;
        let bytes = i64::try_from(decrypted.len()).unwrap_or(i64::MAX);
        let mut file = fs::File::create(&plan.local_path).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not create HLS segment file: {e}"))
                .command_error()
        })?;
        file.write_all(&decrypted).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write HLS segment: {e}"))
                .command_error()
        })?;
        file.flush().await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not flush HLS segment: {e}"))
                .command_error()
        })?;
        Ok(bytes)
    } else {
        // E-1 + E-2: Unencrypted segment — stream directly to disk with an
        // idle timeout (prevents stalled servers from holding a worker) and a
        // safety size limit (prevents malicious/malformed playlists from
        // causing unbounded memory growth).
        let file = fs::File::create(&plan.local_path).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not create HLS segment file: {e}"))
                .command_error()
        })?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);
        let mut downloaded: usize = 0;
        loop {
            let outcome = tokio::select! {
                _ = cancel_token.cancelled() => {
                    writer.flush().await.ok();
                    return Err("Download canceled.".to_string());
                }
                outcome = read_with_idle_timeout(response.chunk(), READ_IDLE_TIMEOUT) => outcome,
            };
            let chunk = match outcome {
                IdleReadOutcome::Data(chunk) => chunk,
                IdleReadOutcome::End => break,
                IdleReadOutcome::Error(e) => {
                    writer.flush().await.ok();
                    return Err(format!("HLS segment connection failed: {e}"));
                }
                IdleReadOutcome::IdleTimeout => {
                    writer.flush().await.ok();
                    return Err(engine_error(
                        "hls_segment_stalled",
                        "HLS segment stalled: no data received for 60 seconds.",
                        true,
                    ));
                }
            };
            speed_limiter.throttle(chunk.len()).await;
            downloaded = downloaded.saturating_add(chunk.len());
            if downloaded > HLS_SEGMENT_MAX_BYTES {
                writer.flush().await.ok();
                return Err(engine_error(
                    "hls_segment_too_large",
                    format!("HLS segment exceeds the {HLS_SEGMENT_MAX_BYTES} byte safety limit."),
                    false,
                ));
            }
            writer.write_all(&chunk).await.map_err(|e| {
                AppErrorPayload::disk_write_failed(format!("Could not write HLS segment: {e}"))
                    .command_error()
            })?;
        }
        writer.flush().await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not flush HLS segment: {e}"))
                .command_error()
        })?;
        Ok(i64::try_from(downloaded).unwrap_or(i64::MAX))
    }
}

async fn ensure_hls_init_map(
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    init_map: &ResolvedHlsInitMap,
) -> Result<(), String> {
    if fs::try_exists(&init_map.local_path).await.unwrap_or(false) {
        return Ok(());
    }
    let data = fetch_bytes(
        client,
        &init_map.uri,
        request_headers,
        init_map.byte_range.as_ref().map(byte_range_header),
    )
    .await?;
    if cancel_token.is_cancelled() {
        return Err("Download canceled.".to_string());
    }
    speed_limiter.throttle(data.len()).await;
    if let Some(parent) = init_map.local_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not create HLS init map folder: {e}"))
                .command_error()
        })?;
    }
    let mut file = fs::File::create(&init_map.local_path).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not create HLS init map: {e}"))
            .command_error()
    })?;
    file.write_all(&data).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not write HLS init map: {e}"))
            .command_error()
    })?;
    file.flush().await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not flush HLS init map: {e}"))
            .command_error()
    })
}

async fn decrypt_hls_segment(
    client: &Client,
    request_headers: &[(String, String)],
    key: &HlsKey,
    media_sequence: i64,
    data: Vec<u8>,
) -> Result<Vec<u8>, String> {
    if key.method.eq_ignore_ascii_case("NONE") {
        return Ok(data);
    }
    if !key.method.eq_ignore_ascii_case("AES-128") {
        return Err(engine_error(
            "hls_unsupported_encryption",
            format!("Unsupported HLS encryption method: {}", key.method),
            false,
        ));
    }
    let uri = key.uri.as_deref().ok_or_else(|| {
        engine_error(
            "hls_unsupported_encryption",
            "AES-128 HLS key is missing a URI.",
            false,
        )
    })?;
    let key_bytes = fetch_bytes(client, uri, request_headers, None).await?;
    if key_bytes.len() != 16 {
        return Err(engine_error(
            "hls_unsupported_encryption",
            "AES-128 HLS key must be 16 bytes.",
            false,
        ));
    }
    let iv = key
        .iv
        .as_deref()
        .map(parse_hls_iv)
        .transpose()?
        .unwrap_or_else(|| sequence_iv(media_sequence));
    let mut buffer = data;
    let decrypted = Aes128CbcDec::new_from_slices(&key_bytes, &iv)
        .map_err(|_| "Could not initialize AES-128 decryptor.".to_string())?
        .decrypt_padded::<Pkcs7>(&mut buffer)
        .map_err(|_| "Could not decrypt AES-128 HLS segment.".to_string())?
        .to_vec();
    Ok(decrypted)
}

/// F-6: Download an external HLS audio/subtitle track into a staging subdir.
/// Returns the path to a local playlist file that can be passed to ffmpeg as
/// an additional `-i` input for `-map` muxing.
async fn download_external_track(
    client: &Client,
    track_url: &str,
    request_headers: &[(String, String)],
    staging_dir: &Path,
    track_kind: &str,
    track_name: &str,
    cancel_token: &tokio_util::sync::CancellationToken,
) -> Result<Option<PathBuf>, String> {
    let body = fetch_text(client, track_url, request_headers).await?;
    validate_playlist_syntax(&body)?;
    let media = parse_media_playlist(&body)?;
    if media.kind == PlaylistKind::Live {
        return Ok(None);
    }
    let track_dir = staging_dir.join(format!("{track_kind}_{track_name}"));
    fs::create_dir_all(&track_dir).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!(
            "Could not create {track_kind} track folder: {e}"
        ))
        .command_error()
    })?;
    let mut local_segments: Vec<(String, i64)> = Vec::new();
    for segment in &media.segments {
        if cancel_token.is_cancelled() {
            return Ok(None);
        }
        let uri = resolve_url(track_url, &segment.uri)?;
        let local_name = format!("seg-{}.bin", segment.media_sequence);
        let local_path = track_dir.join(&local_name);
        let bytes = fetch_bytes(client, &uri, request_headers, None).await?;
        fs::write(&local_path, &bytes).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write {track_kind} segment: {e}"))
                .command_error()
        })?;
        local_segments.push((local_name, segment.duration_ms));
    }
    if local_segments.is_empty() {
        return Ok(None);
    }
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    let target = local_segments
        .iter()
        .map(|(_, ms)| (ms + 999) / 1000)
        .max()
        .unwrap_or(1)
        .max(1);
    text.push_str(&format!(
        "#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n"
    ));
    for (name, ms) in &local_segments {
        let duration = (*ms as f64) / 1000.0;
        text.push_str(&format!("#EXTINF:{duration:.3},\n{name}\n"));
    }
    text.push_str("#EXT-X-ENDLIST\n");
    let playlist_path = track_dir.join("local.m3u8");
    fs::write(&playlist_path, text)
        .await
        .map_err(|e| format!("Could not write {track_kind} track playlist: {e}"))?;
    Ok(Some(playlist_path))
}

async fn finalize_hls_task(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    staging_dir: &Path,
    downloaded_total: i64,
    ffmpeg: &Path,
    extra_inputs: &[PathBuf],
) -> Result<(), String> {
    db::update_task_health_summary(pool, &task.id, Some("Converting HLS stream to MP4")).await?;
    let local_playlist = write_local_hls_playlist(pool, &task.id, staging_dir).await?;
    let mp4_temp = staging_dir.join("output.mp4");
    run_ffmpeg(&local_playlist, &mp4_temp, ffmpeg, extra_inputs).await?;
    let final_path = task
        .final_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "HLS task is missing a final path.".to_string())?;
    let completed_path =
        crate::download::file_ops::finalize_download_file(&mp4_temp, &final_path).await?;
    crate::download::file_ops::persist_completed_path(pool, &task.id, &completed_path).await?;
    if let Some(segment) = db::get_first_segment_record(pool, &task.id).await? {
        db::complete_unknown_size_task(pool, &task.id, &segment.id, downloaded_total).await?;
    } else {
        db::complete_task(pool, &task.id).await?;
    }
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

async fn write_local_hls_playlist(
    pool: &SqlitePool,
    task_id: &str,
    staging_dir: &Path,
) -> Result<PathBuf, String> {
    let segments = db::list_hls_segments(pool, task_id).await?;
    let completed = segments
        .into_iter()
        .filter(|segment| segment.status == SegmentStatus::Completed)
        .collect::<Vec<_>>();
    if completed.is_empty() {
        return Err(engine_error(
            "hls_segment_failed",
            "No completed HLS segments are available to convert.",
            true,
        ));
    }
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    let target = completed
        .iter()
        .map(|segment| (segment.duration_ms + 999) / 1000)
        .max()
        .unwrap_or(1)
        .max(1);
    text.push_str(&format!(
        "#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n"
    ));
    if let Some(first) = completed.first() {
        if first.discontinuity_sequence > 0 {
            text.push_str(&format!(
                "#EXT-X-DISCONTINUITY-SEQUENCE:{}\n",
                first.discontinuity_sequence
            ));
        }
    }
    let mut previous_discontinuity = completed
        .first()
        .map(|segment| segment.discontinuity_sequence)
        .unwrap_or(0);
    let mut previous_init_map: Option<String> = None;
    for segment in &completed {
        if segment.discontinuity_sequence != previous_discontinuity {
            text.push_str("#EXT-X-DISCONTINUITY\n");
            previous_discontinuity = segment.discontinuity_sequence;
        }
        if segment.init_map_local_path != previous_init_map {
            if let Some(init_map_path) = segment.init_map_local_path.as_deref() {
                let path = PathBuf::from(init_map_path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("init.mp4")
                    .replace('\\', "/");
                text.push_str(&format!("#EXT-X-MAP:URI=\"{path}\"\n"));
            }
            previous_init_map = segment.init_map_local_path.clone();
        }
        let duration = (segment.duration_ms.max(0) as f64) / 1000.0;
        let path = PathBuf::from(&segment.local_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("segment.bin")
            .replace('\\', "/");
        text.push_str(&format!("#EXTINF:{duration:.3},\n{path}\n"));
    }
    text.push_str("#EXT-X-ENDLIST\n");
    let path = staging_dir.join("local.m3u8");
    fs::write(&path, text)
        .await
        .map_err(|e| format!("Could not write local HLS playlist: {e}"))?;
    Ok(path)
}

async fn run_ffmpeg(
    input: &Path,
    output: &Path,
    ffmpeg: &Path,
    extra_inputs: &[PathBuf],
) -> Result<(), String> {
    if fs::try_exists(output).await.unwrap_or(false) {
        fs::remove_file(output)
            .await
            .map_err(|e| format!("Could not reset HLS MP4 output: {e}"))?;
    }
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-allowed_extensions")
        .arg("ALL")
        .arg("-i")
        .arg(input);
    // F-6: Add extra audio/subtitle track inputs.
    for extra in extra_inputs {
        cmd.arg("-i").arg(extra);
    }
    cmd.arg("-c").arg("copy").arg("-movflags").arg("+faststart");
    // F-6: Map all streams from all inputs so audio/subtitle tracks are muxed.
    if !extra_inputs.is_empty() {
        for i in 0..=extra_inputs.len() {
            cmd.arg("-map").arg(format!("{i}"));
        }
    }
    let status = cmd
        .status()
        .await
        .map_err(|e| format!("Could not start ffmpeg: {e}"))?;
    if !status.success() {
        return Err(format!("ffmpeg failed with status {status}."));
    }
    Ok(())
}

async fn pause_hls_task(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    downloaded_total: i64,
) -> Result<(), String> {
    db::update_task_progress(pool, &task.id, downloaded_total, 0, 0, TaskStatus::Paused).await?;
    if let Some(segment) = db::get_first_segment_record(pool, &task.id).await? {
        db::update_segment_runtime_progress(
            pool,
            &segment.id,
            downloaded_total,
            0,
            SegmentStatus::Pending,
        )
        .await?;
    }
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn emit_hls_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    downloaded: i64,
    connection_count: usize,
    progress_gate: &mut TaskProgressEmitGate,
    first_segment_id: Option<&str>,
    db_write_gate: &mut DbWriteGate,
    force: bool,
) -> Result<(), String> {
    let connection_count_i32 = i32::try_from(connection_count).unwrap_or(i32::MAX);
    // E-4: Throttle DB writes to the same cadence as IPC events. The `force`
    // path (pause/cancel/finalize) always writes. The last pending state is
    // flushed by the caller via `db_write_gate.flush_pending()`.
    if db_write_gate.should_write(force) {
        db::update_task_progress(
            pool,
            &task.id,
            downloaded,
            0,
            connection_count_i32,
            TaskStatus::Downloading,
        )
        .await?;
        if let Some(segment_id) = first_segment_id {
            db::update_segment_runtime_progress(
                pool,
                segment_id,
                downloaded,
                0,
                SegmentStatus::Downloading,
            )
            .await?;
        }
    }
    progress_gate.emit_or_store(
        app,
        TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: downloaded.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: "0".to_string(),
            connection_count: connection_count_i32,
            status: TaskStatus::Downloading,
        },
        force,
    );
    Ok(())
}

/// E-5a: Replaces the previous `existing_hls_downloaded_bytes` +
/// `existing_hls_sequences` pair. Both called `db::list_hls_segments`
/// independently and each materialized the full segment Vec; this version
/// fetches the list once and derives both the cumulative completed-byte
/// total and the `(discontinuity_sequence, media_sequence)` set from the
/// same Vec.
async fn existing_hls_progress(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<(i64, HashSet<(i64, i64)>), String> {
    let mut downloaded_total = 0_i64;
    let mut seen = HashSet::new();
    for segment in db::list_hls_segments(pool, task_id).await? {
        if segment.status == SegmentStatus::Completed {
            downloaded_total += segment.downloaded_bytes;
            seen.insert((segment.discontinuity_sequence, segment.media_sequence));
        }
    }
    Ok((downloaded_total, seen))
}

async fn fetch_text(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<String, String> {
    let bytes = fetch_bytes(client, url, headers, None).await?;
    String::from_utf8(bytes).map_err(|_| {
        engine_error(
            "hls_invalid_playlist",
            "HLS playlist is not valid UTF-8.",
            false,
        )
    })
}

async fn fetch_bytes(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
    range: Option<String>,
) -> Result<Vec<u8>, String> {
    let mut request = apply_forwarded_headers(client.get(url), headers);
    if let Some(range) = range {
        request = request.header(RANGE, range);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Could not request HLS resource: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HLS resource returned {}", response.status()));
    }
    // E-2: Pre-check Content-Length against HLS_INIT_MAX_BYTES so a malicious
    // or malformed server cannot trigger an unbounded memory read.
    if let Some(content_length) = response.content_length() {
        if content_length as usize > HLS_INIT_MAX_BYTES {
            return Err(engine_error(
                "hls_init_too_large",
                format!("HLS resource exceeds the {HLS_INIT_MAX_BYTES} byte safety limit."),
                false,
            ));
        }
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Could not read HLS resource: {e}"))?;
    // E-2: Post-read guard — chunked-encoding responses omit Content-Length.
    if bytes.len() > HLS_INIT_MAX_BYTES {
        return Err(engine_error(
            "hls_init_too_large",
            format!("HLS resource exceeds the {HLS_INIT_MAX_BYTES} byte safety limit."),
            false,
        ));
    }
    Ok(bytes.to_vec())
}

fn apply_forwarded_headers(
    mut request: RequestBuilder,
    headers: &[(String, String)],
) -> RequestBuilder {
    for (name, value) in headers {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(value) = HeaderValue::from_str(value) else {
            continue;
        };
        request = request.header(name, value);
    }
    request
}

fn is_master_playlist(body: &str) -> bool {
    body.lines()
        .any(|line| line.trim_start().starts_with("#EXT-X-STREAM-INF"))
}

fn validate_playlist_syntax(body: &str) -> Result<(), String> {
    let parsed = if is_master_playlist(body) {
        ParsedMasterPlaylist::try_from(body).map(|_| ())
    } else {
        ParsedMediaPlaylist::try_from(body).map(|_| ())
    };
    parsed.map_err(|error| {
        engine_error(
            "hls_invalid_playlist",
            format!("HLS playlist could not be parsed: {error}"),
            false,
        )
    })
}

fn choose_master_variant(body: &str) -> Result<MasterVariant, String> {
    let variants = parse_master_variants(body);
    variants
        .into_iter()
        .max_by_key(|variant| {
            (
                variant.bandwidth,
                variant
                    .resolution
                    .map(|(width, height)| width.saturating_mul(height))
                    .unwrap_or(0),
            )
        })
        .ok_or_else(|| {
            engine_error(
                "hls_invalid_playlist",
                "HLS master playlist does not contain a playable variant.",
                false,
            )
        })
}

fn hls_variants_from_master(body: &str, selected_uri: &str) -> Vec<HlsVariant> {
    parse_master_variants(body)
        .into_iter()
        .map(|variant| HlsVariant {
            selected: variant.uri == selected_uri,
            uri: variant.uri,
            bandwidth: variant.bandwidth.to_string(),
            resolution: variant
                .resolution
                .map(|(width, height)| format!("{width}x{height}")),
            codecs: variant.codecs,
        })
        .collect()
}

fn parse_master_variants(body: &str) -> Vec<MasterVariant> {
    let mut variants = Vec::new();
    let mut pending_attrs: Option<HashMap<String, String>> = None;
    for raw in body.lines() {
        let line = raw.trim();
        if line.starts_with("#EXT-X-STREAM-INF:") {
            pending_attrs = Some(parse_attributes(
                line.trim_start_matches("#EXT-X-STREAM-INF:"),
            ));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(attrs) = pending_attrs.take() {
            let bandwidth = attrs
                .get("BANDWIDTH")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0);
            let resolution = attrs.get("RESOLUTION").and_then(|value| {
                let (w, h) = value.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            });
            variants.push(MasterVariant {
                uri: line.to_string(),
                bandwidth,
                resolution,
                codecs: attrs.get("CODECS").cloned(),
            });
        }
    }
    variants
}

fn parse_media_playlist(body: &str) -> Result<MediaPlaylist, String> {
    if !body.lines().any(|line| line.trim() == "#EXTM3U") {
        return Err(engine_error(
            "hls_invalid_playlist",
            "HLS playlist is missing #EXTM3U.",
            false,
        ));
    }
    let mut playlist_type = None;
    let mut target_duration = 6_i64;
    let mut media_sequence = 0_i64;
    let mut discontinuity_sequence = 0_i64;
    let mut next_duration = None;
    let mut current_key: Option<HlsKey> = None;
    let mut current_init_map: Option<HlsInitMap> = None;
    let mut current_byte_range: Option<ByteRange> = None;
    let mut next_byte_range_start: Option<i64> = None;
    let mut segments = Vec::new();
    let mut end_list = false;
    let mut segment_index = 0_i64;

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            target_duration = value
                .trim()
                .parse::<i64>()
                .unwrap_or(target_duration)
                .max(1);
        } else if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = value.trim().parse::<i64>().unwrap_or(0).max(0);
        } else if let Some(value) = line.strip_prefix("#EXT-X-PLAYLIST-TYPE:") {
            playlist_type = Some(value.trim().to_ascii_uppercase());
        } else if let Some(value) = line.strip_prefix("#EXT-X-KEY:") {
            current_key = parse_key(value)?;
        } else if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            current_byte_range = parse_byte_range(value);
        } else if let Some(value) = line.strip_prefix("#EXTINF:") {
            next_duration = parse_extinf_duration(value);
        } else if line == "#EXT-X-DISCONTINUITY" {
            discontinuity_sequence += 1;
            next_byte_range_start = None;
        } else if line == "#EXT-X-ENDLIST" {
            end_list = true;
        } else if let Some(value) = line.strip_prefix("#EXT-X-MAP:") {
            current_init_map = Some(parse_init_map(value)?);
        } else if !line.starts_with('#') {
            let sequence = media_sequence + segment_index;
            let byte_range = current_byte_range.take().map(|range| {
                let start = range.start.or(next_byte_range_start).unwrap_or(0);
                next_byte_range_start = Some(start.saturating_add(range.length));
                ByteRange {
                    start: Some(start),
                    length: range.length,
                }
            });
            if byte_range.is_none() {
                next_byte_range_start = None;
            }
            segments.push(HlsSegment {
                media_sequence: sequence,
                discontinuity_sequence,
                uri: line.to_string(),
                duration_ms: next_duration.take().unwrap_or(0),
                byte_range,
                init_map: current_init_map.clone(),
                key: current_key.clone(),
            });
            segment_index += 1;
        }
    }

    let kind = match playlist_type.as_deref() {
        Some("VOD") => PlaylistKind::Vod,
        Some("EVENT") => PlaylistKind::Event,
        _ if end_list => PlaylistKind::Vod,
        _ => PlaylistKind::Live,
    };
    Ok(MediaPlaylist {
        kind,
        target_duration,
        media_sequence,
        end_list,
        segments,
    })
}

fn reject_unsupported_media_playlist(body: &str) -> Result<(), String> {
    for line in body.lines().map(str::trim) {
        if line.starts_with("#EXT-X-KEY:") {
            let attrs = parse_attributes(line.trim_start_matches("#EXT-X-KEY:"));
            let method = attrs.get("METHOD").map(String::as_str).unwrap_or("NONE");
            if !matches!(method, "NONE" | "AES-128") {
                return Err(engine_error(
                    "hls_unsupported_encryption",
                    format!("Unsupported HLS encryption method: {method}"),
                    false,
                ));
            }
            if attrs
                .get("KEYFORMAT")
                .is_some_and(|value| value != "identity" && value != "\"identity\"")
            {
                return Err(engine_error(
                    "hls_unsupported_encryption",
                    "Only identity HLS AES-128 keys are supported.",
                    false,
                ));
            }
        }
    }
    Ok(())
}

fn parse_key(value: &str) -> Result<Option<HlsKey>, String> {
    let attrs = parse_attributes(value);
    let method = attrs
        .get("METHOD")
        .cloned()
        .unwrap_or_else(|| "NONE".to_string());
    if method == "NONE" {
        return Ok(None);
    }
    if method != "AES-128" {
        return Err(engine_error(
            "hls_unsupported_encryption",
            format!("Unsupported HLS encryption method: {method}"),
            false,
        ));
    }
    Ok(Some(HlsKey {
        method,
        uri: attrs.get("URI").cloned(),
        iv: attrs.get("IV").cloned(),
    }))
}

fn parse_init_map(value: &str) -> Result<HlsInitMap, String> {
    let attrs = parse_attributes(value);
    let uri = attrs.get("URI").cloned().ok_or_else(|| {
        engine_error(
            "hls_invalid_playlist",
            "HLS EXT-X-MAP is missing a URI.",
            false,
        )
    })?;
    Ok(HlsInitMap {
        uri,
        byte_range: attrs
            .get("BYTERANGE")
            .and_then(|value| parse_byte_range(value)),
    })
}

fn parse_ext_x_media(body: &str) -> Vec<HlsMediaTrack> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("#EXT-X-MEDIA:")?;
            let attrs = parse_attributes(rest);
            let kind = attrs.get("TYPE").cloned()?;
            // Skip CLOSED-CAPTIONS — we don't handle CEA-608/708 embedded in video.
            if kind.eq_ignore_ascii_case("CLOSED-CAPTIONS") {
                return None;
            }
            Some(HlsMediaTrack {
                kind,
                group_id: attrs.get("GROUP-ID").cloned().unwrap_or_default(),
                name: attrs.get("NAME").cloned().unwrap_or_default(),
                language: attrs.get("LANGUAGE").cloned(),
                default: attrs
                    .get("DEFAULT")
                    .is_some_and(|v| v.eq_ignore_ascii_case("YES")),
                auto_select: attrs
                    .get("AUTOSELECT")
                    .is_some_and(|v| v.eq_ignore_ascii_case("YES")),
                uri: attrs.get("URI").cloned(),
            })
        })
        .collect()
}

fn parse_attributes(value: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let mut key = String::new();
    let mut current = String::new();
    let mut in_key = true;
    let mut in_quote = false;
    for ch in value.chars() {
        match ch {
            '=' if in_key => {
                key = current.trim().to_ascii_uppercase();
                current.clear();
                in_key = false;
            }
            '"' => {
                in_quote = !in_quote;
            }
            ',' if !in_key && !in_quote => {
                attrs.insert(key.clone(), current.trim().trim_matches('"').to_string());
                key.clear();
                current.clear();
                in_key = true;
            }
            ch => current.push(ch),
        }
    }
    if !key.is_empty() {
        attrs.insert(key, current.trim().trim_matches('"').to_string());
    }
    attrs
}

fn parse_extinf_duration(value: &str) -> Option<i64> {
    let duration = value.split(',').next()?.trim().parse::<f64>().ok()?;
    Some((duration.max(0.0) * 1000.0).round() as i64)
}

fn parse_byte_range(value: &str) -> Option<ByteRange> {
    let (length, start) = value
        .trim()
        .split_once('@')
        .map_or((value.trim(), None), |(l, s)| (l.trim(), Some(s.trim())));
    Some(ByteRange {
        length: length.parse::<i64>().ok()?.max(0),
        start: start
            .and_then(|value| value.parse::<i64>().ok())
            .map(|value| value.max(0)),
    })
}

fn init_map_local_name(uri: &str, byte_range: Option<&ByteRange>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut hasher);
    if let Some(range) = byte_range {
        range.start.hash(&mut hasher);
        range.length.hash(&mut hasher);
    }
    format!("init-{:016x}.mp4", hasher.finish())
}

fn resolve_url(base: &str, value: &str) -> Result<String, String> {
    let base = reqwest::Url::parse(base).map_err(|_| "HLS base URL is invalid.".to_string())?;
    base.join(value)
        .map(|url| url.to_string())
        .map_err(|_| "HLS playlist contains an invalid relative URL.".to_string())
}

fn hls_output_name(url: &str) -> String {
    let name = reqwest::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("hls-{}", chrono::Utc::now().timestamp()));
    let path = Path::new(&name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&name);
    // Sanitize the stem (URL path segments may carry reserved chars or `..`),
    // then re-extract the final path component so any residual `/` or `\`
    // left after replacement cannot escape the save directory.
    let sanitized = crate::download::sanitize::sanitize_single_file_name(stem);
    let final_stem = Path::new(&sanitized)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&sanitized);
    format!("{final_stem}.mp4")
}

fn byte_range_header(range: &ByteRange) -> String {
    let start = range.start.unwrap_or(0);
    let end = start
        .saturating_add(range.length)
        .saturating_sub(1)
        .max(start);
    format!("bytes={start}-{end}")
}

fn parse_hls_iv(value: &str) -> Result<[u8; 16], String> {
    let value = value
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    if value.len() > 32 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("HLS AES-128 IV is invalid.".to_string());
    }
    let padded = format!("{value:0>32}");
    let mut out = [0_u8; 16];
    for index in 0..16 {
        out[index] = u8::from_str_radix(&padded[index * 2..index * 2 + 2], 16)
            .map_err(|_| "HLS AES-128 IV is invalid.".to_string())?;
    }
    Ok(out)
}

fn sequence_iv(sequence: i64) -> [u8; 16] {
    let mut out = [0_u8; 16];
    out[8..].copy_from_slice(&(sequence.max(0) as u64).to_be_bytes());
    out
}

fn ensure_ffmpeg_available() -> Result<(), String> {
    if ffmpeg_path().is_some() {
        Ok(())
    } else {
        Err(engine_error(
            "hls_ffmpeg_missing",
            "ffmpeg was not found. Install ffmpeg or set VIBE_FFMPEG_PATH before creating HLS tasks.",
            true,
        ))
    }
}

fn ffmpeg_path() -> Option<PathBuf> {
    std::env::var_os("VIBE_FFMPEG_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .or_else(|| executable_in_path("ffmpeg"))
}

fn executable_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        #[cfg(target_os = "windows")]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_highest_bandwidth_variant() {
        let variant = choose_master_variant(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100,RESOLUTION=640x360\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=200,RESOLUTION=1280x720\nhi.m3u8\n",
        )
        .expect("variant");
        assert_eq!(variant.uri, "hi.m3u8");
        assert_eq!(variant.resolution, Some((1280, 720)));
    }

    #[test]
    fn parses_live_media_sequence_and_segments() {
        let media = parse_media_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:42\n#EXTINF:5.5,\nseg42.ts\n#EXTINF:5,\nseg43.ts\n",
        )
        .expect("media");
        assert_eq!(media.kind, PlaylistKind::Live);
        assert_eq!(media.media_sequence, 42);
        assert_eq!(media.segments[0].media_sequence, 42);
        assert_eq!(media.segments[0].duration_ms, 5500);
    }

    #[test]
    fn resolves_relative_hls_byte_ranges() {
        let media = parse_media_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-BYTERANGE:100@50\n#EXTINF:5,\nfile.ts\n#EXT-X-BYTERANGE:75\n#EXTINF:5,\nfile.ts\n",
        )
        .expect("media");
        assert_eq!(
            media.segments[0]
                .byte_range
                .as_ref()
                .map(|range| range.start),
            Some(Some(50))
        );
        assert_eq!(
            media.segments[0]
                .byte_range
                .as_ref()
                .map(|range| range.length),
            Some(100)
        );
        assert_eq!(
            media.segments[1]
                .byte_range
                .as_ref()
                .map(|range| range.start),
            Some(Some(150))
        );
        assert_eq!(
            media.segments[1]
                .byte_range
                .as_ref()
                .map(|range| range.length),
            Some(75)
        );
    }

    #[test]
    fn tracks_hls_discontinuity_sequences() {
        let media = parse_media_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:5,\nseg0.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:5,\nseg1.ts\n",
        )
        .expect("media");
        assert_eq!(media.segments[0].discontinuity_sequence, 0);
        assert_eq!(media.segments[1].discontinuity_sequence, 1);
    }

    #[test]
    fn derives_aes_iv_from_sequence() {
        let iv = sequence_iv(258);
        assert_eq!(&iv[14..], &[1, 2]);
    }

    #[test]
    fn rejects_sample_aes() {
        let error = reject_unsupported_media_playlist(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"key\"\n",
        )
        .unwrap_err();
        assert!(error.contains("hls_unsupported_encryption"));
    }

    // E-2: Safety-limit constants must stay pinned at their documented values.
    // Accidentally raising or removing these caps would reintroduce the
    // unbounded-memory-growth risk the caps are meant to prevent.

    #[test]
    fn hls_segment_max_bytes_is_512_mib() {
        assert_eq!(HLS_SEGMENT_MAX_BYTES, 512 * 1024 * 1024);
    }

    #[test]
    fn hls_init_max_bytes_is_64_mib() {
        assert_eq!(HLS_INIT_MAX_BYTES, 64 * 1024 * 1024);
    }

    #[test]
    fn parse_ext_x_media_extracts_audio_and_subtitle_tracks() {
        let master = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud1\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,URI=\"en.m3u8\"\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud1\",NAME=\"Spanish\",LANGUAGE=\"es\",DEFAULT=NO,AUTOSELECT=YES,URI=\"es.m3u8\"\n\
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"sub1\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,URI=\"en-subs.m3u8\"\n\
#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID=\"cc1\",NAME=\"CC\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES\n\
#EXT-X-STREAM-INF:BANDWIDTH=1000000,AUDIO=\"aud1\",SUBTITLES=\"sub1\"\n\
video.m3u8\n";
        let tracks = parse_ext_x_media(master);
        assert_eq!(
            tracks.len(),
            3,
            "should parse 3 tracks (2 audio + 1 subtitle, skip CLOSED-CAPTIONS)"
        );
        let audio: Vec<_> = tracks.iter().filter(|t| t.kind == "AUDIO").collect();
        let subs: Vec<_> = tracks.iter().filter(|t| t.kind == "SUBTITLES").collect();
        assert_eq!(audio.len(), 2);
        assert_eq!(subs.len(), 1);
        // English audio (default)
        assert_eq!(audio[0].group_id, "aud1");
        assert_eq!(audio[0].name, "English");
        assert_eq!(audio[0].language.as_deref(), Some("en"));
        assert!(audio[0].default);
        assert!(audio[0].auto_select);
        assert_eq!(audio[0].uri.as_deref(), Some("en.m3u8"));
        // Spanish audio (not default)
        assert_eq!(audio[1].name, "Spanish");
        assert!(!audio[1].default);
        // Subtitles
        assert_eq!(subs[0].kind, "SUBTITLES");
        assert_eq!(subs[0].uri.as_deref(), Some("en-subs.m3u8"));
        assert!(subs[0].default);
    }

    #[test]
    fn parse_ext_x_media_handles_embedded_tracks_with_null_uri() {
        let master = "#EXTM3U\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud1\",NAME=\"Audio\",DEFAULT=YES,AUTOSELECT=YES\n\
#EXT-X-STREAM-INF:BANDWIDTH=500000,AUDIO=\"aud1\"\n\
video.m3u8\n";
        let tracks = parse_ext_x_media(master);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].kind, "AUDIO");
        assert!(
            tracks[0].uri.is_none(),
            "embedded track should have null URI"
        );
    }

    #[test]
    fn parse_ext_x_media_returns_empty_for_media_playlist() {
        let media = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:6\n\
#EXTINF:6.0,\n\
seg1.ts\n\
#EXT-X-ENDLIST\n";
        let tracks = parse_ext_x_media(media);
        assert!(tracks.is_empty());
    }
}
