use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
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
use tokio::{fs, io::AsyncWriteExt, process::Command, task::JoinSet};
use uuid::Uuid;

use super::{
    engine::EngineFuture,
    http::build_client,
    DownloadContext, DownloadEngine, ProbeOutput, ProbeRequest,
};
use crate::{
    db,
    events::{emit_task_progress, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        AppErrorPayload, EngineCapabilities, ProbedFile, RequestDiagnosticRecord, SegmentStatus,
        TaskKind, TaskProgressPayload, TaskRecord, TaskStatus,
    },
    proxy::SharedProxyConfig,
};

type Aes128CbcDec = cbc::Decryptor<Aes128>;

const PROTOCOL_HLS: &str = "hls";
const HLS_CONTENT_TYPE: &str = "application/vnd.apple.mpegurl";
const HLS_SEGMENT_RETRIES: i32 = 2;
const HLS_LIVE_MAX_IDLE_POLLS: usize = 6;

#[derive(Debug, Clone)]
pub struct HlsEngine {
    proxy_config: SharedProxyConfig,
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
    key: Option<HlsKey>,
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
    key: Option<HlsKey>,
}

#[derive(Debug)]
struct SegmentDownloadResult {
    media_sequence: i64,
    bytes: i64,
    result: Result<(), String>,
}

impl HlsEngine {
    pub fn new(proxy_config: SharedProxyConfig) -> Self {
        Self { proxy_config }
    }

    async fn client(&self) -> Result<Client, String> {
        let config = self.proxy_config.read().await;
        build_client(&config)
    }

    async fn probe_hls(
        &self,
        url: &str,
        request_headers: &[(String, String)],
    ) -> Result<ProbePlan, String> {
        ensure_ffmpeg_available()?;
        let client = self.client().await?;
        let body = fetch_text(&client, url, request_headers).await?;
        validate_playlist_syntax(&body)?;
        let (media_url, selected_bandwidth, selected_resolution, media_body) =
            if is_master_playlist(&body) {
                let variant = choose_master_variant(&body)?;
                let media_url = resolve_url(url, &variant.uri)?;
                let media_body = fetch_text(&client, &media_url, request_headers).await?;
                validate_playlist_syntax(&media_body)?;
                (
                    media_url,
                    Some(variant.bandwidth),
                    variant
                        .resolution
                        .map(|(width, height)| format!("{width}x{height}")),
                    media_body,
                )
            } else {
                (url.to_string(), None, None, body)
            };
        let media = parse_media_playlist(&media_body)?;
        reject_unsupported_media_playlist(&media_body)?;
        Ok(ProbePlan {
            media_url,
            kind: media.kind,
            target_duration: media.target_duration,
            selected_bandwidth,
            selected_resolution,
            display_name: hls_output_name(url),
        })
    }
}

impl DownloadEngine for HlsEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_HLS
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "http" | "https")
    }

    fn probe<'a>(&'a self, request: ProbeRequest) -> EngineFuture<'a, Result<ProbeOutput, String>> {
        Box::pin(async move {
            let plan = self
                .probe_hls(&request.uri, &request.request_headers)
                .await?;
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
            })
        })
    }

    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>> {
        Box::pin(async move { run_hls_download(self.clone(), context).await })
    }
}

async fn run_hls_download(engine: HlsEngine, context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel,
        finish,
        speed_limiter,
        connection_limit,
        request_headers,
    } = context;
    ensure_ffmpeg_available()?;
    let client = engine.client().await?;
    let staging_dir = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "HLS task is missing a staging path.".to_string())?;
    fs::create_dir_all(&staging_dir)
        .await
        .map_err(|e| AppErrorPayload::disk_write_failed(format!("Could not create HLS staging folder: {e}")).command_error())?;

    let plan = engine.probe_hls(&task.url, &request_headers).await?;
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
        },
    )
    .await?;
    db::insert_task_event(&pool, &task.id, "hls_playlist_resolved", Some(&plan.media_url)).await?;

    let mut downloaded_total = existing_hls_downloaded_bytes(&pool, &task.id).await?;
    let mut seen = existing_hls_sequences(&pool, &task.id).await?;
    let mut idle_polls = 0_usize;

    loop {
        if cancel.load(Ordering::SeqCst) {
            pause_hls_task(&app, &pool, &task, downloaded_total).await?;
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
                &cancel,
                connection_limit.max(1),
                downloaded_total,
                plans,
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

    if cancel.load(Ordering::SeqCst) {
        pause_hls_task(&app, &pool, &task, downloaded_total).await?;
        return Ok(());
    }

    finalize_hls_task(&app, &pool, &task, &staging_dir, downloaded_total).await
}

async fn persist_hls_segment_plans(
    pool: &SqlitePool,
    task: &TaskRecord,
    staging_dir: &Path,
    media_url: &str,
    segments: Vec<HlsSegment>,
) -> Result<Vec<SegmentDownloadPlan>, String> {
    let mut plans = Vec::new();
    for segment in segments {
        let id = Uuid::new_v4().to_string();
        let local_name = format!(
            "seg-{}-{}.bin",
            segment.discontinuity_sequence, segment.media_sequence
        );
        let local_path = staging_dir.join(local_name);
        let uri = resolve_url(media_url, &segment.uri)?;
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
        db::upsert_hls_segment(
            pool,
            db::HlsSegmentUpsert {
                id: &id,
                task_id: &task.id,
                media_sequence: segment.media_sequence,
                discontinuity_sequence: segment.discontinuity_sequence,
                uri: &uri,
                local_path: &local_path.to_string_lossy(),
                duration_ms: segment.duration_ms,
                byte_range_start: segment.byte_range.as_ref().and_then(|range| range.start),
                byte_range_length: segment.byte_range.as_ref().map(|range| range.length),
                key_method: key.as_ref().map(|key| key.method.as_str()),
                key_uri: key.as_ref().and_then(|key| key.uri.as_deref()),
                key_iv: key.as_ref().and_then(|key| key.iv.as_deref()),
            },
        )
        .await?;
        plans.push(SegmentDownloadPlan {
            task_id: task.id.clone(),
            id,
            media_sequence: segment.media_sequence,
            uri,
            local_path,
            byte_range: segment.byte_range,
            key,
        });
    }
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
    cancel: &Arc<AtomicBool>,
    connection_limit: usize,
    initial_downloaded: i64,
    plans: Vec<SegmentDownloadPlan>,
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
            let cancel = cancel.clone();
            workers.spawn(async move {
                download_hls_segment(
                    &pool,
                    &client,
                    request_headers,
                    speed_limiter,
                    cancel,
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
                emit_hls_progress(app, pool, task, downloaded_total, active + 1).await?;
            }
            Err(error) => {
                cancel.store(true, Ordering::SeqCst);
                workers.abort_all();
                return Err(hls_error(
                    "hls_segment_failed",
                    format!("HLS segment download failed: {error}"),
                    true,
                ));
            }
        }

        if cancel.load(Ordering::SeqCst) {
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
    cancel: Arc<AtomicBool>,
    plan: SegmentDownloadPlan,
) -> SegmentDownloadResult {
    let mut retry_count = 0;
    let mut last_error = None;
    loop {
        let started = Instant::now();
        let result = download_hls_segment_once(
            pool,
            client,
            &request_headers,
            &speed_limiter,
            &cancel,
            &plan,
        )
        .await;
        persist_hls_diagnostic(
            pool,
            &plan,
            result.as_ref().err().map(String::as_str),
            retry_count,
            started.elapsed(),
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
            Err(error) if cancel.load(Ordering::SeqCst) => {
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
                tokio::time::sleep(Duration::from_millis(200 * u64::try_from(retry_count).unwrap_or(1))).await;
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
    cancel: &Arc<AtomicBool>,
    plan: &SegmentDownloadPlan,
) -> Result<i64, String> {
    db::update_hls_segment_status(
        pool,
        &plan.id,
        0,
        SegmentStatus::Downloading,
        0,
        None,
    )
    .await?;
    if let Some(parent) = plan.local_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| AppErrorPayload::disk_write_failed(format!("Could not create HLS segment folder: {e}")).command_error())?;
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

    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("HLS segment connection failed: {e}"))?
    {
        if cancel.load(Ordering::SeqCst) {
            return Err("Download canceled.".to_string());
        }
        speed_limiter.throttle(chunk.len()).await;
        data.extend_from_slice(&chunk);
    }

    let data = if let Some(key) = &plan.key {
        decrypt_hls_segment(client, request_headers, key, plan.media_sequence, data).await?
    } else {
        data
    };
    let bytes = i64::try_from(data.len()).unwrap_or(i64::MAX);
    let mut file = fs::File::create(&plan.local_path)
        .await
        .map_err(|e| AppErrorPayload::disk_write_failed(format!("Could not create HLS segment file: {e}")).command_error())?;
    file.write_all(&data)
        .await
        .map_err(|e| AppErrorPayload::disk_write_failed(format!("Could not write HLS segment: {e}")).command_error())?;
    file.flush()
        .await
        .map_err(|e| AppErrorPayload::disk_write_failed(format!("Could not flush HLS segment: {e}")).command_error())?;
    Ok(bytes)
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
        return Err(hls_error(
            "hls_unsupported_encryption",
            format!("Unsupported HLS encryption method: {}", key.method),
            false,
        ));
    }
    let uri = key.uri.as_deref().ok_or_else(|| {
        hls_error(
            "hls_unsupported_encryption",
            "AES-128 HLS key is missing a URI.",
            false,
        )
    })?;
    let key_bytes = fetch_bytes(client, uri, request_headers, None).await?;
    if key_bytes.len() != 16 {
        return Err(hls_error(
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

async fn finalize_hls_task(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    staging_dir: &Path,
    downloaded_total: i64,
) -> Result<(), String> {
    db::update_task_health_summary(pool, &task.id, Some("Converting HLS stream to MP4")).await?;
    let local_playlist = write_local_hls_playlist(pool, &task.id, staging_dir).await?;
    let mp4_temp = staging_dir.join("output.mp4");
    run_ffmpeg(&local_playlist, &mp4_temp).await?;
    let final_path = task
        .final_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "HLS task is missing a final path.".to_string())?;
    let completed_path =
        crate::download::http::file::finalize_download_file(&mp4_temp, &final_path).await?;
    crate::download::http::file::persist_completed_path(pool, &task.id, &completed_path).await?;
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
        return Err(hls_error(
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
    text.push_str(&format!("#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n"));
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
    for segment in &completed {
        if segment.discontinuity_sequence != previous_discontinuity {
            text.push_str("#EXT-X-DISCONTINUITY\n");
            previous_discontinuity = segment.discontinuity_sequence;
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

async fn run_ffmpeg(input: &Path, output: &Path) -> Result<(), String> {
    let ffmpeg = ffmpeg_path().ok_or_else(|| {
        hls_error(
            "hls_ffmpeg_missing",
            "ffmpeg was not found. Install ffmpeg or set VIBE_FFMPEG_PATH before creating HLS tasks.",
            true,
        )
    })?;
    if fs::try_exists(output).await.unwrap_or(false) {
        fs::remove_file(output)
            .await
            .map_err(|e| format!("Could not reset HLS MP4 output: {e}"))?;
    }
    let status = Command::new(ffmpeg)
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-allowed_extensions")
        .arg("ALL")
        .arg("-i")
        .arg(input)
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg(output)
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

async fn emit_hls_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    downloaded: i64,
    connection_count: usize,
) -> Result<(), String> {
    db::update_task_progress(
        pool,
        &task.id,
        downloaded,
        0,
        i32::try_from(connection_count).unwrap_or(i32::MAX),
        TaskStatus::Downloading,
    )
    .await?;
    if let Some(segment) = db::get_first_segment_record(pool, &task.id).await? {
        db::update_segment_runtime_progress(
            pool,
            &segment.id,
            downloaded,
            0,
            SegmentStatus::Downloading,
        )
        .await?;
    }
    emit_task_progress(
        app,
        &TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: downloaded.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: "0".to_string(),
            connection_count: i32::try_from(connection_count).unwrap_or(i32::MAX),
            status: TaskStatus::Downloading,
        },
    );
    Ok(())
}

async fn existing_hls_downloaded_bytes(pool: &SqlitePool, task_id: &str) -> Result<i64, String> {
    Ok(db::list_hls_segments(pool, task_id)
        .await?
        .into_iter()
        .filter(|segment| segment.status == SegmentStatus::Completed)
        .map(|segment| segment.downloaded_bytes)
        .sum())
}

async fn existing_hls_sequences(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<HashSet<(i64, i64)>, String> {
    Ok(db::list_hls_segments(pool, task_id)
        .await?
        .into_iter()
        .filter(|segment| segment.status == SegmentStatus::Completed)
        .map(|segment| (segment.discontinuity_sequence, segment.media_sequence))
        .collect())
}

async fn fetch_text(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<String, String> {
    let bytes = fetch_bytes(client, url, headers, None).await?;
    String::from_utf8(bytes).map_err(|_| {
        hls_error(
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
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| format!("Could not read HLS resource: {e}"))
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

async fn persist_hls_diagnostic(
    pool: &SqlitePool,
    plan: &SegmentDownloadPlan,
    error: Option<&str>,
    retry_count: i32,
    duration: Duration,
) {
    let record = RequestDiagnosticRecord {
        task_id: plan.task_id.clone(),
        method: "GET".to_string(),
        url: sanitize_url(&plan.uri),
        range_header: plan.byte_range.as_ref().map(byte_range_header),
        if_range_header: None,
        status_code: if error.is_some() { None } else { Some(200) },
        etag: None,
        last_modified: None,
        content_length: None,
        error_message: error.map(str::to_string),
        retry_count,
        duration_ms: i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
    };
    if let Err(error) = db::insert_request_diagnostic(pool, &record).await {
        tracing::warn!(error = %error, "failed to persist hls request diagnostic");
    }
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
        hls_error(
            "hls_invalid_playlist",
            format!("HLS playlist could not be parsed: {error}"),
            false,
        )
    })
}

fn choose_master_variant(body: &str) -> Result<MasterVariant, String> {
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
            });
        }
    }
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
            hls_error(
                "hls_invalid_playlist",
                "HLS master playlist does not contain a playable variant.",
                false,
            )
        })
}

fn parse_media_playlist(body: &str) -> Result<MediaPlaylist, String> {
    if !body.lines().any(|line| line.trim() == "#EXTM3U") {
        return Err(hls_error(
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
            target_duration = value.trim().parse::<i64>().unwrap_or(target_duration).max(1);
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
        } else if line.starts_with("#EXT-X-MAP:") {
            return Err(hls_error(
                "hls_unsupported_playlist",
                "fMP4 HLS playlists with EXT-X-MAP are not supported in this version.",
                false,
            ));
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
                return Err(hls_error(
                    "hls_unsupported_encryption",
                    format!("Unsupported HLS encryption method: {method}"),
                    false,
                ));
            }
            if attrs
                .get("KEYFORMAT")
                .is_some_and(|value| value != "identity" && value != "\"identity\"")
            {
                return Err(hls_error(
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
        return Err(hls_error(
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
    let (length, start) = value.trim().split_once('@').map_or((value.trim(), None), |(l, s)| {
        (l.trim(), Some(s.trim()))
    });
    Some(ByteRange {
        length: length.parse::<i64>().ok()?.max(0),
        start: start
            .and_then(|value| value.parse::<i64>().ok())
            .map(|value| value.max(0)),
    })
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
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or(&name);
    format!("{stem}.mp4")
}

fn byte_range_header(range: &ByteRange) -> String {
    let start = range.start.unwrap_or(0);
    let end = start.saturating_add(range.length).saturating_sub(1).max(start);
    format!("bytes={start}-{end}")
}

fn parse_hls_iv(value: &str) -> Result<[u8; 16], String> {
    let value = value.trim().trim_start_matches("0x").trim_start_matches("0X");
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
        Err(hls_error(
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

fn hls_error(code: &str, message: impl Into<String>, recoverable: bool) -> String {
    AppErrorPayload::new(
        code,
        message,
        recoverable,
        if recoverable {
            vec!["retry", "check_url"]
        } else {
            vec!["check_url"]
        },
    )
    .command_error()
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
        assert_eq!(media.segments[0].byte_range.as_ref().map(|range| range.start), Some(Some(50)));
        assert_eq!(media.segments[0].byte_range.as_ref().map(|range| range.length), Some(100));
        assert_eq!(media.segments[1].byte_range.as_ref().map(|range| range.start), Some(Some(150)));
        assert_eq!(media.segments[1].byte_range.as_ref().map(|range| range.length), Some(75));
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
}
