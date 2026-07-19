use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use quick_xml::{events::Event, Reader};
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
    engine::EngineFuture,
    file_ops::{finalize_download_file, persist_completed_path},
    http::HttpEngine,
    read_with_idle_timeout,
    url_classify::is_dash_url,
    DownloadContext, DownloadEngine, DownloadError, IdleReadOutcome, ProbeOutput, ProbeRequest,
    READ_IDLE_TIMEOUT,
};
use crate::download::error::engine_error;
use crate::download::retry::RetryPolicy;
use crate::{
    db,
    events::{emit_task_updated_record, DbWriteGate, TaskProgressEmitGate},
    logging::sanitize_url,
    models::{
        AppErrorPayload, EngineCapabilities, ProbedFile, SegmentStatus, TaskKind,
        TaskProgressPayload, TaskRecord, TaskStatus,
    },
};

const PROTOCOL_DASH: &str = "dash";
const DASH_CONTENT_TYPE: &str = "application/dash+xml";
const DASH_SEGMENT_RETRIES: i32 = 2;
const TRACK_KIND_VIDEO: &str = "video";
const TRACK_KIND_AUDIO: &str = "audio";

#[derive(Debug, Clone)]
pub struct DashEngine {
    /// E-4: Shares the `HttpEngine` client cache to avoid creating a new Client on every probe/download.
    http: Arc<HttpEngine>,
}

impl DashEngine {
    pub fn new(http: Arc<HttpEngine>) -> Self {
        Self { http }
    }

    async fn client(&self) -> Result<Client, String> {
        self.http.client().await
    }

    async fn client_for_config(
        &self,
        config: &crate::proxy::ResolvedProxyConfig,
    ) -> Result<Client, String> {
        self.http.client_for_config(config).await
    }

    async fn probe_dash(
        &self,
        url: &str,
        request_headers: &[(String, String)],
        pool: Option<&SqlitePool>,
        app: &Option<tauri::AppHandle>,
        request_id: &Option<String>,
    ) -> Result<DashManifestSummary, String> {
        crate::download::engine::emit_probe_phase(app, request_id, "checking_ffmpeg", Some("dash"));
        super::ffmpeg::ensure_ffmpeg_available(
            pool,
            "dash_ffmpeg_missing",
            "ffmpeg was not found. Install ffmpeg, configure a path in Settings → External tools, or set VIBE_FFMPEG_PATH before creating MPEG-DASH tasks.",
        )
        .await?;
        let client = self.client().await?;
        crate::download::engine::emit_probe_phase(
            app,
            request_id,
            "fetching_manifest",
            Some("dash"),
        );
        let body = fetch_mpd_text(&client, url, request_headers).await?;
        crate::download::engine::emit_probe_phase(
            app,
            request_id,
            "parsing_manifest",
            Some("dash"),
        );
        let parsed = parse_dash_manifest(url, &body)?;
        let (_video, _audio) = select_tracks(&parsed)?;
        Ok(DashManifestSummary {
            display_name: dash_output_name(url),
        })
    }
}

impl DownloadEngine for DashEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_DASH
    }

    /// R-3: DASH routes only via `matches_url` (`.mpd` suffix), not via scheme fallback,
    /// to prevent ordinary HTTPS URLs from being misrouted to DASH.
    fn supports_scheme(&self, _scheme: &str) -> bool {
        false
    }

    /// R-3: `.mpd` paths route to the DASH engine. Priority 70.
    fn matches_url(&self, url: &reqwest::Url) -> bool {
        is_dash_url(url)
    }

    fn priority(&self) -> i32 {
        70
    }

    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, DownloadError>> {
        Box::pin(async move {
            let pool = request.pool.as_ref();
            let summary = self
                .probe_dash(
                    &request.uri,
                    &request.request_headers,
                    pool,
                    &request.app,
                    &request.request_id,
                )
                .await
                .map_err(DownloadError::Other)?;
            let source_key = reqwest::Url::parse(&request.uri)
                .ok()
                .and_then(|url| {
                    if url.scheme() == "file" {
                        Some("dash:file".to_string())
                    } else {
                        url.host_str().map(str::to_string)
                    }
                })
                .unwrap_or_else(|| "dash".to_string());
            Ok(ProbeOutput {
                protocol: PROTOCOL_DASH.to_string(),
                task_kind: TaskKind::Manifest,
                resolved_uri: request.uri.clone(),
                display_name: summary.display_name.clone(),
                total_size: 0,
                source_key,
                capabilities: EngineCapabilities {
                    supports_resume: true,
                    supports_parallel: true,
                    supports_multi_file: false,
                },
                files: vec![ProbedFile {
                    relative_path: summary.display_name,
                    size: "0".to_string(),
                    content_type: Some("video/mp4".to_string()),
                }],
                etag: None,
                last_modified: None,
                content_type: Some(DASH_CONTENT_TYPE.to_string()),
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
        Box::pin(async move {
            run_dash_download(self.clone(), context)
                .await
                .map_err(DownloadError::Other)
        })
    }
}

#[derive(Debug, Clone)]
struct DashManifestSummary {
    display_name: String,
}

// ---------------------------------------------------------------------------
// MPD parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ParsedMpd {
    periods: Vec<ParsedPeriod>,
}

#[derive(Debug, Clone)]
struct ParsedPeriod {
    adaptation_sets: Vec<ParsedAdaptationSet>,
}

#[derive(Debug, Clone)]
struct ParsedAdaptationSet {
    content_type: Option<String>,
    mime_type: Option<String>,
    representations: Vec<ParsedRepresentation>,
}

#[derive(Debug, Clone)]
struct ParsedRepresentation {
    id: String,
    bandwidth: i64,
    codecs: Option<String>,
    segment_source: SegmentSource,
}

#[derive(Debug, Clone)]
enum SegmentSource {
    /// SegmentTemplate with `$Number$` template.
    Template {
        media_template: String,
        initialization: Option<String>,
        start_number: i64,
        duration: Option<i64>,
        segment_count: i64,
    },
    /// SegmentList with explicit segment URLs.
    List {
        initialization: Option<String>,
        segments: Vec<ListSegment>,
    },
    /// SegmentBase with a single BaseURL + byte ranges.
    Base {
        base_url: String,
        initialization_range: Option<ByteRange>,
        media_range: Option<ByteRange>,
    },
}

#[derive(Debug, Clone)]
struct ListSegment {
    uri: String,
}

#[derive(Debug, Clone)]
struct ByteRange {
    start: i64,
    length: i64,
}

fn parse_dash_manifest(manifest_url: &str, text: &str) -> Result<ParsedMpd, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut mpd_duration_seconds: Option<f64> = None;
    let mut saw_mpd = false;
    let mut has_segment_timeline = false;
    let mut current_period_duration_seconds: Option<f64> = None;
    let mut periods: Vec<ParsedPeriod> = Vec::new();

    // Stack-based traversal: we track the current path of element names.
    let mut element_stack: Vec<String> = Vec::new();
    let mut current_adaptation: Option<ParsedAdaptationSet> = None;
    let mut current_representation: Option<ParsedRepresentation> = None;
    let mut current_segment_list: Vec<ListSegment> = Vec::new();
    let mut current_init_for_list: Option<String> = None;
    let mut current_base_url: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "mpd" => {
                        saw_mpd = true;
                        if attr_value(&event, "type")
                            .is_some_and(|v| v.eq_ignore_ascii_case("dynamic"))
                        {
                            return Err(engine_error(
                                "dash_live_unsupported",
                                "Live MPEG-DASH MPDs are not supported. Use a static/VOD MPD.",
                                false,
                            ));
                        }
                        if let Some(d) = attr_value(&event, "mediaPresentationDuration") {
                            mpd_duration_seconds = parse_iso8601_duration(&d);
                        }
                    }
                    "period" => {
                        if let Some(d) = attr_value(&event, "duration") {
                            current_period_duration_seconds = parse_iso8601_duration(&d);
                        }
                        periods.push(ParsedPeriod {
                            adaptation_sets: Vec::new(),
                        });
                    }
                    "segmenttimeline" => {
                        has_segment_timeline = true;
                    }
                    "adaptationset" => {
                        let content_type = attr_value(&event, "contentType");
                        let mime_type = attr_value(&event, "mimeType");
                        current_adaptation = Some(ParsedAdaptationSet {
                            content_type,
                            mime_type,
                            representations: Vec::new(),
                        });
                    }
                    "representation" => {
                        let id = attr_value(&event, "id").unwrap_or_default();
                        let bandwidth = attr_value(&event, "bandwidth")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(0);
                        let codecs = attr_value(&event, "codecs");
                        current_representation = Some(ParsedRepresentation {
                            id,
                            bandwidth,
                            codecs,
                            segment_source: SegmentSource::Base {
                                base_url: String::new(),
                                initialization_range: None,
                                media_range: None,
                            },
                        });
                        current_segment_list.clear();
                        current_init_for_list = None;
                        current_base_url = None;
                    }
                    "segmenttemplate" => {
                        let media_template = attr_value(&event, "media").unwrap_or_default();
                        let initialization = attr_value(&event, "initialization");
                        let start_number = attr_value(&event, "startNumber")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(1);
                        let timescale = attr_value(&event, "timescale")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(1);
                        let duration =
                            attr_value(&event, "duration").and_then(|v| v.parse::<i64>().ok());
                        if let Some(rep) = current_representation.as_mut() {
                            // Compute segment count from period duration.
                            let period_seconds =
                                current_period_duration_seconds.or(mpd_duration_seconds);
                            let segment_count = match (duration, period_seconds) {
                                (Some(d), Some(secs)) if d > 0 && timescale > 0 => {
                                    let total = (secs * timescale as f64 / d as f64).ceil() as i64;
                                    total.max(0)
                                }
                                _ => 0,
                            };
                            rep.segment_source = SegmentSource::Template {
                                media_template,
                                initialization,
                                start_number,
                                duration,
                                segment_count,
                            };
                        }
                    }
                    "segmentlist" => {
                        let initialization = attr_value(&event, "initialization");
                        current_init_for_list = initialization;
                        current_segment_list.clear();
                    }
                    "segmenturl" => {
                        if let Some(uri) = attr_value(&event, "media") {
                            current_segment_list.push(ListSegment { uri });
                        }
                    }
                    "segmentbase" => {
                        // Default SegmentBase has no attributes here; Initialization
                        // child element carries the byte range.
                    }
                    "initialization" => {
                        if let Some(range) = attr_value(&event, "range") {
                            let br = parse_byte_range(&range);
                            if let Some(rep) = current_representation.as_mut() {
                                if let SegmentSource::Base {
                                    initialization_range,
                                    ..
                                } = &mut rep.segment_source
                                {
                                    *initialization_range = br;
                                }
                            }
                        }
                    }
                    "baseurl" => {
                        // BaseURL content is captured in Text event below.
                    }
                    _ => {}
                }
                element_stack.push(name);
            }
            Ok(Event::Empty(event)) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "segmenturl" => {
                        if let Some(uri) = attr_value(&event, "media") {
                            current_segment_list.push(ListSegment { uri });
                        }
                    }
                    "initialization" => {
                        if let Some(range) = attr_value(&event, "range") {
                            let br = parse_byte_range(&range);
                            if let Some(rep) = current_representation.as_mut() {
                                if let SegmentSource::Base {
                                    initialization_range,
                                    ..
                                } = &mut rep.segment_source
                                {
                                    *initialization_range = br;
                                }
                            }
                        }
                    }
                    "segmenttemplate" => {
                        let media_template = attr_value(&event, "media").unwrap_or_default();
                        let initialization = attr_value(&event, "initialization");
                        let start_number = attr_value(&event, "startNumber")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(1);
                        let timescale = attr_value(&event, "timescale")
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(1);
                        let duration =
                            attr_value(&event, "duration").and_then(|v| v.parse::<i64>().ok());
                        if let Some(rep) = current_representation.as_mut() {
                            let period_seconds =
                                current_period_duration_seconds.or(mpd_duration_seconds);
                            let segment_count = match (duration, period_seconds) {
                                (Some(d), Some(secs)) if d > 0 && timescale > 0 => {
                                    let total = (secs * timescale as f64 / d as f64).ceil() as i64;
                                    total.max(0)
                                }
                                _ => 0,
                            };
                            rep.segment_source = SegmentSource::Template {
                                media_template,
                                initialization,
                                start_number,
                                duration,
                                segment_count,
                            };
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(text)) if element_stack.last().is_some_and(|name| name == "baseurl") => {
                if let Ok(value) = text.decode().map(|value| value.into_owned()) {
                    current_base_url = Some(value);
                }
            }
            Ok(Event::End(event)) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "adaptationset" => {
                        if let Some(adapt) = current_adaptation.take() {
                            if let Some(period) = periods.last_mut() {
                                period.adaptation_sets.push(adapt);
                            }
                        }
                    }
                    "representation" => {
                        if let Some(mut rep) = current_representation.take() {
                            // If we accumulated a SegmentList, finalize it.
                            if !current_segment_list.is_empty() {
                                rep.segment_source = SegmentSource::List {
                                    initialization: current_init_for_list.take(),
                                    segments: std::mem::take(&mut current_segment_list),
                                };
                            } else if let Some(base) = current_base_url.take() {
                                // SegmentBase with BaseURL.
                                if let SegmentSource::Base {
                                    initialization_range,
                                    media_range,
                                    ..
                                } = &rep.segment_source
                                {
                                    rep.segment_source = SegmentSource::Base {
                                        base_url: base,
                                        initialization_range: initialization_range.clone(),
                                        media_range: media_range.clone(),
                                    };
                                }
                            }
                            if let Some(adapt) = current_adaptation.as_mut() {
                                adapt.representations.push(rep);
                            }
                        }
                    }
                    "period" => {
                        current_period_duration_seconds = None;
                    }
                    _ => {}
                }
                element_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(engine_error(
                    "dash_invalid_manifest",
                    format!("DASH MPD could not be parsed: {error}"),
                    false,
                ))
            }
            _ => {}
        }
        buf.clear();
    }

    if !saw_mpd {
        return Err(engine_error(
            "dash_invalid_manifest",
            "DASH MPD is missing the MPD root element.",
            false,
        ));
    }
    if has_segment_timeline {
        return Err(engine_error(
            "dash_segment_timeline_unsupported",
            "SegmentTimeline is not supported yet. Use a static MPD with SegmentTemplate, SegmentList, or SegmentBase.",
            false,
        ));
    }
    if periods.is_empty() || periods.iter().all(|p| p.adaptation_sets.is_empty()) {
        return Err(engine_error(
            "dash_invalid_manifest",
            "DASH MPD does not contain any adaptation sets.",
            false,
        ));
    }

    let _ = manifest_url; // retained for future relative-URL resolution at parse time
    Ok(ParsedMpd { periods })
}

fn select_tracks(
    mpd: &ParsedMpd,
) -> Result<(Option<ParsedRepresentation>, Option<ParsedRepresentation>), String> {
    let mut best_video: Option<ParsedRepresentation> = None;
    let mut best_audio: Option<ParsedRepresentation> = None;
    for period in &mpd.periods {
        for adapt in &period.adaptation_sets {
            let is_video = adapt.content_type.as_deref() == Some("video")
                || adapt
                    .mime_type
                    .as_deref()
                    .is_some_and(|m| m.starts_with("video"));
            let is_audio = adapt.content_type.as_deref() == Some("audio")
                || adapt
                    .mime_type
                    .as_deref()
                    .is_some_and(|m| m.starts_with("audio"));
            if !is_video && !is_audio {
                continue;
            }
            let best = adapt
                .representations
                .iter()
                .max_by_key(|r| r.bandwidth)
                .cloned();
            if is_video {
                best_video = best.or(best_video);
            } else if is_audio {
                best_audio = best.or(best_audio);
            }
        }
    }
    if best_video.is_none() && best_audio.is_none() {
        return Err(engine_error(
            "dash_no_tracks",
            "DASH MPD has no video or audio tracks.",
            false,
        ));
    }
    Ok((best_video, best_audio))
}

/// Generate concrete segment download plans for a selected representation.
fn build_segment_plans(
    manifest_url: &str,
    staging_dir: &Path,
    task_id: &str,
    track_kind: &str,
    rep: &ParsedRepresentation,
) -> Result<Vec<DashSegmentPlan>, String> {
    // E-5b: Parse the manifest base URL once. The previous `resolve_url`
    // helper re-parsed `manifest_url` on every call (up to 5 per
    // representation: init + N segments for Template/List, or the BaseURL
    // for SegmentBase). With thousands of segments this dominated parse
    // cost; we now reuse the parsed `Url` and call `.join()` per segment.
    let parsed_base = reqwest::Url::parse(manifest_url)
        .map_err(|e| format!("Could not parse DASH manifest URL '{manifest_url}': {e}"))?;
    match &rep.segment_source {
        SegmentSource::Template {
            media_template,
            initialization,
            start_number,
            duration,
            segment_count,
        } => {
            if let Some(d) = duration {
                if *d <= 0 || *segment_count <= 0 {
                    return Err(engine_error(
                        "dash_no_duration",
                        "SegmentTemplate is missing a valid duration or the MPD has no mediaPresentationDuration to compute segment count.",
                        false,
                    ));
                }
            } else {
                return Err(engine_error(
                    "dash_no_duration",
                    "SegmentTemplate is missing the duration attribute.",
                    false,
                ));
            }
            let mut plans = Vec::new();
            // Optional initialization segment.
            if let Some(init_rel) = initialization {
                let init_uri = resolve_url_with_base(&parsed_base, init_rel)?;
                let init_local = staging_dir.join(format!("init_{track_kind}.mp4"));
                plans.push(DashSegmentPlan {
                    id: Uuid::new_v4().to_string(),
                    task_id: task_id.to_string(),
                    track_kind: track_kind.to_string(),
                    segment_index: -1, // init segment marker
                    uri: init_uri,
                    local_path: init_local.to_string_lossy().to_string(),
                    byte_range: None,
                    init_segment_uri: None,
                    init_segment_local_path: None,
                });
            }
            for i in 0..*segment_count {
                let number = start_number + i;
                let media_url = media_template.replace("$Number$", &number.to_string());
                let resolved = resolve_url_with_base(&parsed_base, &media_url)?;
                let local_path = staging_dir.join(format!("seg_{track_kind}_{i}.m4s"));
                plans.push(DashSegmentPlan {
                    id: Uuid::new_v4().to_string(),
                    task_id: task_id.to_string(),
                    track_kind: track_kind.to_string(),
                    segment_index: i,
                    uri: resolved,
                    local_path: local_path.to_string_lossy().to_string(),
                    byte_range: None,
                    init_segment_uri: None,
                    init_segment_local_path: None,
                });
            }
            Ok(plans)
        }
        SegmentSource::List {
            initialization,
            segments,
        } => {
            let mut plans = Vec::new();
            if let Some(init_rel) = initialization {
                let init_uri = resolve_url_with_base(&parsed_base, init_rel)?;
                let init_local = staging_dir.join(format!("init_{track_kind}.mp4"));
                plans.push(DashSegmentPlan {
                    id: Uuid::new_v4().to_string(),
                    task_id: task_id.to_string(),
                    track_kind: track_kind.to_string(),
                    segment_index: -1,
                    uri: init_uri,
                    local_path: init_local.to_string_lossy().to_string(),
                    byte_range: None,
                    init_segment_uri: None,
                    init_segment_local_path: None,
                });
            }
            for (i, seg) in segments.iter().enumerate() {
                let resolved = resolve_url_with_base(&parsed_base, &seg.uri)?;
                let local_path = staging_dir.join(format!("seg_{track_kind}_{i}.m4s"));
                plans.push(DashSegmentPlan {
                    id: Uuid::new_v4().to_string(),
                    task_id: task_id.to_string(),
                    track_kind: track_kind.to_string(),
                    segment_index: i as i64,
                    uri: resolved,
                    local_path: local_path.to_string_lossy().to_string(),
                    byte_range: None,
                    init_segment_uri: None,
                    init_segment_local_path: None,
                });
            }
            Ok(plans)
        }
        SegmentSource::Base {
            base_url,
            initialization_range,
            media_range,
        } => {
            let resolved_base = resolve_url_with_base(&parsed_base, base_url)?;
            let mut plans = Vec::new();
            if let Some(init_range) = initialization_range {
                let init_local = staging_dir.join(format!("init_{track_kind}.mp4"));
                plans.push(DashSegmentPlan {
                    id: Uuid::new_v4().to_string(),
                    task_id: task_id.to_string(),
                    track_kind: track_kind.to_string(),
                    segment_index: -1,
                    uri: resolved_base.clone(),
                    local_path: init_local.to_string_lossy().to_string(),
                    byte_range: Some(init_range.clone()),
                    init_segment_uri: None,
                    init_segment_local_path: None,
                });
            }
            let media_local = staging_dir.join(format!("seg_{track_kind}_0.m4s"));
            plans.push(DashSegmentPlan {
                id: Uuid::new_v4().to_string(),
                task_id: task_id.to_string(),
                track_kind: track_kind.to_string(),
                segment_index: 0,
                uri: resolved_base,
                local_path: media_local.to_string_lossy().to_string(),
                byte_range: media_range.clone(),
                init_segment_uri: None,
                init_segment_local_path: None,
            });
            Ok(plans)
        }
    }
}

// ---------------------------------------------------------------------------
// Download execution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct DashSegmentPlan {
    id: String,
    task_id: String,
    track_kind: String,
    segment_index: i64,
    uri: String,
    local_path: String,
    byte_range: Option<ByteRange>,
    init_segment_uri: Option<String>,
    init_segment_local_path: Option<String>,
}

#[derive(Debug)]
struct DashSegmentDownloadResult {
    bytes: i64,
    result: Result<(), String>,
}

async fn run_dash_download(engine: DashEngine, context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel_token,
        speed_limiter,
        connection_limit,
        request_headers,
        proxy_config,
        ..
    } = context;

    let ffmpeg = super::ffmpeg::ensure_ffmpeg_available(
        Some(&pool),
        "dash_ffmpeg_missing",
        "ffmpeg was not found. Install ffmpeg, configure a path in Settings → External tools, or set VIBE_FFMPEG_PATH before creating MPEG-DASH tasks.",
    )
    .await?;
    let manifest_url = task.final_url.clone().unwrap_or_else(|| task.url.clone());
    let temp_path = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "DASH task is missing a temporary path.".to_string())?;
    let final_path = task
        .final_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "DASH task is missing a final path.".to_string())?;

    // ARC-02: isolate staging under save_dir/.vibe-staging/{task_id}.
    let staging_dir = PathBuf::from(&task.save_dir)
        .join(".vibe-staging")
        .join(&task.id);
    fs::create_dir_all(&staging_dir).await.map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not create DASH staging folder: {e}"))
            .command_error()
    })?;
    if fs::try_exists(&temp_path).await.unwrap_or(false) {
        fs::remove_file(&temp_path).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not reset DASH temporary file: {e}"))
                .command_error()
        })?;
    }

    // FUN-02: honor per-task proxy instead of the global SharedProxyConfig client.
    let client = engine.client_for_config(&proxy_config).await?;
    let mpd_text = fetch_mpd_text(&client, &manifest_url, &request_headers).await?;
    let parsed = parse_dash_manifest(&manifest_url, &mpd_text)?;
    let (video_rep, audio_rep) = select_tracks(&parsed)?;

    // Build segment plans for each selected track.
    let mut all_plans: Vec<DashSegmentPlan> = Vec::new();
    if let Some(rep) = &video_rep {
        all_plans.extend(build_segment_plans(
            &manifest_url,
            &staging_dir,
            &task.id,
            TRACK_KIND_VIDEO,
            rep,
        )?);
    }
    if let Some(rep) = &audio_rep {
        all_plans.extend(build_segment_plans(
            &manifest_url,
            &staging_dir,
            &task.id,
            TRACK_KIND_AUDIO,
            rep,
        )?);
    }
    if all_plans.is_empty() {
        return Err(engine_error(
            "dash_no_segments",
            "DASH manifest produced no downloadable segments.",
            false,
        ));
    }

    // Persist dash_task metadata.
    db::upsert_dash_task(
        &pool,
        db::DashTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            manifest_url: &manifest_url,
            staging_dir: &staging_dir.to_string_lossy(),
            video_representation_id: video_rep.as_ref().map(|r| r.id.as_str()),
            video_bandwidth: video_rep.as_ref().map(|r| r.bandwidth),
            video_codecs: video_rep.as_ref().and_then(|r| r.codecs.as_deref()),
            audio_representation_id: audio_rep.as_ref().map(|r| r.id.as_str()),
            audio_bandwidth: audio_rep.as_ref().map(|r| r.bandwidth),
            audio_codecs: audio_rep.as_ref().and_then(|r| r.codecs.as_deref()),
            output_format: "mp4",
            segment_count: i64::try_from(all_plans.len()).unwrap_or(0),
        },
    )
    .await?;
    db::insert_task_event(
        &pool,
        &task.id,
        "dash_manifest_resolved",
        Some(&sanitize_url(&manifest_url)),
    )
    .await?;

    // E-3: Persist all segment rows in a single transaction.
    let upserts: Vec<db::DashSegmentUpsert<'_>> = all_plans
        .iter()
        .map(|plan| db::DashSegmentUpsert {
            id: &plan.id,
            task_id: &plan.task_id,
            track_kind: &plan.track_kind,
            segment_index: plan.segment_index,
            uri: &plan.uri,
            local_path: &plan.local_path,
            byte_range_start: plan.byte_range.as_ref().map(|r| r.start),
            byte_range_length: plan.byte_range.as_ref().map(|r| r.length),
            init_segment_uri: plan.init_segment_uri.as_deref(),
            init_segment_local_path: plan.init_segment_local_path.as_deref(),
            duration_ms: 0,
        })
        .collect();
    db::bulk_upsert_dash_segments(&pool, &upserts).await?;

    // Resume: skip completed segments.
    let seen = db::existing_dash_segment_keys(&pool, &task.id).await?;
    let mut downloaded_total = db::existing_dash_downloaded_bytes(&pool, &task.id).await?;
    let pending_plans: Vec<DashSegmentPlan> = all_plans
        .into_iter()
        .filter(|plan| !seen.contains(&(plan.track_kind.clone(), plan.segment_index)))
        .collect();

    let mut progress_gate = TaskProgressEmitGate::default();
    // E-4: Cache first segment ID once and throttle per-segment DB writes.
    let first_segment_id: Option<String> = db::get_first_segment_record(&pool, &task.id)
        .await?
        .map(|segment| segment.id);
    let mut db_write_gate = DbWriteGate::default();

    if cancel_token.is_cancelled() {
        pause_dash_task(&app, &pool, &task, downloaded_total).await?;
        progress_gate.flush(&app);
        return Ok(());
    }

    if !pending_plans.is_empty() {
        let downloaded = download_dash_segments(
            &app,
            &pool,
            &task,
            &client,
            &request_headers,
            &speed_limiter,
            &cancel_token,
            connection_limit.max(1),
            downloaded_total,
            pending_plans,
            &mut progress_gate,
            first_segment_id.as_deref(),
            &mut db_write_gate,
        )
        .await?;
        downloaded_total = downloaded;
    }

    if cancel_token.is_cancelled() {
        pause_dash_task(&app, &pool, &task, downloaded_total).await?;
        progress_gate.flush(&app);
        return Ok(());
    }

    progress_gate.flush(&app);
    finalize_dash_task(
        &app,
        &pool,
        &task,
        &staging_dir,
        &temp_path,
        &final_path,
        downloaded_total,
        &ffmpeg,
        &cancel_token,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn download_dash_segments(
    app: &Option<AppHandle>,
    pool: &SqlitePool,
    task: &TaskRecord,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    connection_limit: usize,
    initial_downloaded: i64,
    plans: Vec<DashSegmentPlan>,
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
                download_dash_segment(
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
            .ok_or_else(|| "DASH segment worker stopped unexpectedly.".to_string())?;
        active = active.saturating_sub(1);
        let result = joined.map_err(|e| format!("A DASH worker stopped unexpectedly: {e}"))?;
        match result.result {
            Ok(()) => {
                downloaded_total = downloaded_total.saturating_add(result.bytes);
                emit_dash_progress(
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
            Err(_) if cancel_token.is_cancelled() => {
                workers.abort_all();
                break;
            }
            Err(error) => {
                cancel_token.cancel();
                workers.abort_all();
                progress_gate.flush(app);
                return Err(engine_error(
                    "dash_segment_failed",
                    format!("DASH segment download failed: {error}"),
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

async fn download_dash_segment(
    pool: &SqlitePool,
    client: &Client,
    request_headers: Vec<(String, String)>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: tokio_util::sync::CancellationToken,
    plan: DashSegmentPlan,
) -> DashSegmentDownloadResult {
    let retry_policy = RetryPolicy::hls_segment();
    let mut retry_count = 0;
    let mut last_error = None;
    loop {
        let started = Instant::now();
        let result = download_dash_segment_once(
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
                let _ = db::update_dash_segment_status(
                    pool,
                    &plan.id,
                    bytes,
                    SegmentStatus::Completed,
                    retry_count,
                    None,
                )
                .await;
                return DashSegmentDownloadResult {
                    bytes,
                    result: Ok(()),
                };
            }
            Err(error) if cancel_token.is_cancelled() => {
                return DashSegmentDownloadResult {
                    bytes: 0,
                    result: Err(error),
                };
            }
            Err(error) if retry_count < DASH_SEGMENT_RETRIES => {
                retry_count += 1;
                last_error = Some(error.clone());
                let _ = db::update_dash_segment_status(
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
                            return DashSegmentDownloadResult {
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
                let _ = db::update_dash_segment_status(
                    pool,
                    &plan.id,
                    0,
                    SegmentStatus::Failed,
                    retry_count,
                    Some(&message),
                )
                .await;
                return DashSegmentDownloadResult {
                    bytes: 0,
                    result: Err(message),
                };
            }
        }
    }
}

async fn download_dash_segment_once(
    pool: &SqlitePool,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    plan: &DashSegmentPlan,
) -> Result<i64, String> {
    db::update_dash_segment_status(pool, &plan.id, 0, SegmentStatus::Downloading, 0, None).await?;
    if let Some(parent) = Path::new(&plan.local_path).parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not create DASH segment folder: {e}"))
                .command_error()
        })?;
    }

    let mut request = apply_forwarded_headers(client.get(&plan.uri), request_headers)
        .header(ACCEPT_ENCODING, "identity");
    if let Some(range) = &plan.byte_range {
        request = request.header(RANGE, byte_range_header(range));
    }
    let mut response = request
        .send()
        .await
        .map_err(|e| format!("Could not request DASH segment: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("server returned {}", response.status()));
    }
    if plan.byte_range.is_some() && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err("server did not honor DASH byte range request".to_string());
    }

    let file = fs::File::create(&plan.local_path)
        .await
        .map_err(|e| format!("Could not create DASH segment file: {e}"))?;
    let mut writer = BufWriter::with_capacity(256 * 1024, file);
    let mut downloaded: i64 = 0;

    loop {
        // E-1: wrap the chunk read with an idle timeout so a stalled server
        // cannot hold the worker, connection slot, and queue slot forever.
        // The cancel token is raced alongside so a user-initiated pause/cancel
        // interrupts an in-flight (and potentially stalled) read immediately.
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
                return Err(format!("DASH segment connection failed: {e}"));
            }
            IdleReadOutcome::IdleTimeout => {
                return Err(engine_error(
                    "dash_segment_stalled",
                    "DASH segment stalled: no data received for 60 seconds.",
                    true,
                ));
            }
        };
        if speed_limiter
            .throttle(chunk.len(), cancel_token)
            .await
            .is_err()
        {
            writer.flush().await.ok();
            return Err("Download canceled.".to_string());
        }
        writer
            .write_all(&chunk)
            .await
            .map_err(|e| format!("Could not write DASH segment to disk: {e}"))?;
        downloaded = downloaded.saturating_add(i64::try_from(chunk.len()).unwrap_or(0));
    }
    writer
        .flush()
        .await
        .map_err(|e| format!("Could not flush DASH segment to disk: {e}"))?;
    Ok(downloaded)
}

#[allow(clippy::too_many_arguments)]
async fn emit_dash_progress(
    app: &Option<AppHandle>,
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
    // E-4: Throttle DB writes to the same cadence as IPC events.
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

async fn pause_dash_task(
    app: &Option<AppHandle>,
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
async fn finalize_dash_task(
    app: &Option<AppHandle>,
    pool: &SqlitePool,
    task: &TaskRecord,
    staging_dir: &Path,
    temp_path: &Path,
    final_path: &Path,
    downloaded_total: i64,
    ffmpeg: &Path,
    cancel_token: &tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    // ARC-03: do not publish after delete/pause removed the active task.
    let still_active = db::get_task_record(pool, &task.id)
        .await?
        .is_some_and(|current| current.status == TaskStatus::Downloading);
    if !still_active || cancel_token.is_cancelled() {
        return Err("Download canceled.".to_string());
    }
    // Generate ffconcat files for video and audio tracks.
    let segments = db::list_dash_segments(pool, &task.id).await?;
    let video_segments: Vec<_> = segments
        .iter()
        .filter(|s| s.track_kind == TRACK_KIND_VIDEO && s.status == SegmentStatus::Completed)
        .collect();
    let audio_segments: Vec<_> = segments
        .iter()
        .filter(|s| s.track_kind == TRACK_KIND_AUDIO && s.status == SegmentStatus::Completed)
        .collect();

    let video_concat = if !video_segments.is_empty() {
        Some(write_ffconcat(staging_dir, TRACK_KIND_VIDEO, &video_segments).await?)
    } else {
        None
    };
    let audio_concat = if !audio_segments.is_empty() {
        Some(write_ffconcat(staging_dir, TRACK_KIND_AUDIO, &audio_segments).await?)
    } else {
        None
    };

    if video_concat.is_none() && audio_concat.is_none() {
        return Err(engine_error(
            "dash_empty_output",
            "DASH download completed without producing any media segments.",
            false,
        ));
    }

    run_ffmpeg_remux(
        ffmpeg,
        video_concat.as_deref(),
        audio_concat.as_deref(),
        temp_path,
        cancel_token,
    )
    .await?;

    if cancel_token.is_cancelled() {
        let _ = fs::remove_file(temp_path).await;
        return Err("Download canceled.".to_string());
    }
    let still_active = db::get_task_record(pool, &task.id)
        .await?
        .is_some_and(|current| current.status == TaskStatus::Downloading);
    if !still_active {
        let _ = fs::remove_file(temp_path).await;
        return Err("Download canceled.".to_string());
    }

    let downloaded = fs::metadata(temp_path)
        .await
        .map(|m| i64::try_from(m.len()).unwrap_or(downloaded_total))
        .unwrap_or(downloaded_total);
    let completed_path = finalize_download_file(temp_path, final_path).await?;
    persist_completed_path(pool, &task.id, &completed_path).await?;
    if task.total_size > 0 {
        db::complete_task(pool, &task.id).await?;
    } else {
        let segment = db::get_first_segment_record(pool, &task.id)
            .await?
            .ok_or_else(|| "DASH task segment could not be completed.".to_string())?;
        db::complete_unknown_size_task(pool, &task.id, &segment.id, downloaded).await?;
    }
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

async fn write_ffconcat(
    staging_dir: &Path,
    track_kind: &str,
    segments: &[&db::DashSegmentRecord],
) -> Result<PathBuf, String> {
    let mut text = String::from("ffconcat version 1.0\n");
    for segment in segments {
        let file_name = Path::new(&segment.local_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("segment.bin")
            .replace('\\', "/");
        text.push_str(&format!("file '{file_name}'\n"));
    }
    let path = staging_dir.join(format!("{track_kind}.ffconcat"));
    fs::write(&path, text)
        .await
        .map_err(|e| format!("Could not write DASH ffconcat file: {e}"))?;
    Ok(path)
}

async fn run_ffmpeg_remux(
    ffmpeg: &Path,
    video_concat: Option<&Path>,
    audio_concat: Option<&Path>,
    output: &Path,
    cancel_token: &tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    if fs::try_exists(output).await.unwrap_or(false) {
        fs::remove_file(output)
            .await
            .map_err(|e| format!("Could not reset DASH MP4 output: {e}"))?;
    }
    let mut command = Command::new(ffmpeg);
    command
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error");
    // Working directory = staging dir so ffconcat relative paths resolve.
    if let Some(video) = video_concat {
        if let Some(dir) = video.parent() {
            command.current_dir(dir);
        }
        command
            .arg("-i")
            .arg(video.file_name().unwrap_or(video.as_os_str()));
    }
    if let Some(audio) = audio_concat {
        command
            .arg("-i")
            .arg(audio.file_name().unwrap_or(audio.as_os_str()));
    }
    match (video_concat, audio_concat) {
        (Some(_), Some(_)) => {
            command.arg("-map").arg("0:v").arg("-map").arg("1:a");
        }
        _ => {
            command.arg("-map").arg("0");
        }
    }
    command
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-f")
        .arg("mp4")
        .arg(output);
    match super::ffmpeg::run_cancellable(command, cancel_token).await {
        Ok(()) => Ok(()),
        Err(error) if error.contains("canceled") => {
            let _ = fs::remove_file(output).await;
            Err(error)
        }
        Err(error) => {
            let _ = fs::remove_file(output).await;
            Err(engine_error(
                "dash_ffmpeg_failed",
                format!("ffmpeg could not remux DASH segments: {error}"),
                true,
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn fetch_mpd_text(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "DASH MPD URL is invalid.".to_string())?;
    if parsed.scheme() == "file" {
        let path = parsed
            .to_file_path()
            .map_err(|_| "DASH MPD file path is invalid.".to_string())?;
        return fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Could not read DASH MPD file {}: {e}", path.display()));
    }
    let response = apply_forwarded_headers(client.get(url), headers)
        .send()
        .await
        .map_err(|e| format!("Could not request DASH MPD: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("DASH MPD returned {}", response.status()));
    }
    response
        .text()
        .await
        .map_err(|e| format!("Could not read DASH MPD: {e}"))
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

fn attr_value(event: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|attr| local_name(attr.key.as_ref()).eq_ignore_ascii_case(name))
        .map(|attr| String::from_utf8_lossy(attr.value.as_ref()).to_string())
}

fn local_name(name: &[u8]) -> String {
    let value = String::from_utf8_lossy(name);
    value
        .rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(&value)
        .to_ascii_lowercase()
}

fn parse_byte_range(value: &str) -> Option<ByteRange> {
    let (start, length) = value.split_once('-')?;
    let start = start.trim().parse::<i64>().ok()?;
    let end = length.trim().parse::<i64>().ok()?;
    if end < start {
        return None;
    }
    Some(ByteRange {
        start,
        length: end - start + 1,
    })
}

fn byte_range_header(range: &ByteRange) -> String {
    let end = range.start + range.length - 1;
    format!("bytes={}-{}", range.start, end)
}

/// E-5b: Resolve a segment URL against an already-parsed manifest base.
/// The previous `resolve_url(base: &str, ...)` re-parsed the base URL on
/// every call (up to N+1 times per representation inside
/// `build_segment_plans`). `build_segment_plans` now parses `manifest_url`
/// once and passes the `&Url` here so segment loops only pay the cost of
/// `Url::join`.
fn resolve_url_with_base(base: &reqwest::Url, relative: &str) -> Result<String, String> {
    base.join(relative)
        .map(|url| url.to_string())
        .map_err(|e| format!("Could not resolve DASH segment URL '{relative}': {e}"))
}

/// Parse ISO 8601 duration (e.g. "PT60S", "PT1H30M", "PT2.5S") into seconds.
fn parse_iso8601_duration(value: &str) -> Option<f64> {
    let value = value.trim();
    if !value.starts_with("PT") {
        return None;
    }
    let body = &value[2..];
    let mut seconds = 0.0_f64;
    let mut number = String::new();
    for ch in body.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number.push(ch);
        } else {
            let n: f64 = number.parse().ok()?;
            number.clear();
            match ch {
                'H' => seconds += n * 3600.0,
                'M' => seconds += n * 60.0,
                'S' => seconds += n,
                _ => return None,
            }
        }
    }
    Some(seconds)
}

fn dash_output_name(url: &str) -> String {
    let name = reqwest::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("dash-{}", chrono::Utc::now().timestamp()));
    let path = Path::new(&name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&name);
    let sanitized = crate::download::sanitize::sanitize_single_file_name(stem);
    let final_stem = Path::new(&sanitized)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&sanitized);
    format!("{final_stem}.mp4")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE_MPD: &str = r#"
        <MPD type="static" mediaPresentationDuration="PT60S" xmlns="urn:mpeg:dash:schema:mpd:2011">
          <Period>
            <AdaptationSet mimeType="video/mp4" contentType="video">
              <Representation id="v0" bandwidth="500000" codecs="avc1.42c01e">
                <SegmentTemplate media="seg-$Number$.m4s" initialization="init.m4s" startNumber="1" duration="2" timescale="1" />
              </Representation>
              <Representation id="v1" bandwidth="1000000" codecs="avc1.42c01e">
                <SegmentTemplate media="seg-$Number$.m4s" initialization="init.m4s" startNumber="1" duration="2" timescale="1" />
              </Representation>
            </AdaptationSet>
            <AdaptationSet mimeType="audio/mp4" contentType="audio">
              <Representation id="a0" bandwidth="128000" codecs="mp4a.40.2">
                <SegmentTemplate media="aseg-$Number$.m4s" initialization="ainit.m4s" startNumber="1" duration="2" timescale="1" />
              </Representation>
            </AdaptationSet>
          </Period>
        </MPD>
    "#;

    #[test]
    fn parses_segment_template_with_number() {
        let parsed = parse_dash_manifest("https://example.com/video.mpd", TEMPLATE_MPD)
            .expect("manifest parses");
        assert_eq!(parsed.periods.len(), 1);
        let adapt = &parsed.periods[0].adaptation_sets;
        assert_eq!(adapt.len(), 2);
        let video_rep = adapt[0].representations.first().unwrap();
        match &video_rep.segment_source {
            SegmentSource::Template {
                media_template,
                start_number,
                segment_count,
                duration,
                ..
            } => {
                assert_eq!(media_template, "seg-$Number$.m4s");
                assert_eq!(*start_number, 1);
                assert_eq!(*duration, Some(2));
                // 60s * 1 timescale / 2 = 30 segments
                assert_eq!(*segment_count, 30);
            }
            _ => panic!("expected SegmentSource::Template"),
        }
    }

    #[test]
    fn selects_highest_bandwidth_per_track() {
        let parsed = parse_dash_manifest("https://example.com/video.mpd", TEMPLATE_MPD)
            .expect("manifest parses");
        let (video, audio) = select_tracks(&parsed).expect("tracks selected");
        let video = video.expect("video track");
        let audio = audio.expect("audio track");
        assert_eq!(video.bandwidth, 1000000);
        assert_eq!(audio.bandwidth, 128000);
    }

    #[test]
    fn rejects_dynamic_mpd() {
        let error = parse_dash_manifest(
            "https://example.com/live.mpd",
            r#"<MPD type="dynamic"><Period><AdaptationSet><Representation id="1" /></AdaptationSet></Period></MPD>"#,
        )
        .unwrap_err();
        assert!(error.contains("dash_live_unsupported"));
    }

    #[test]
    fn rejects_segment_timeline() {
        let mpd = r#"
            <MPD type="static" mediaPresentationDuration="PT60S">
              <Period>
                <AdaptationSet mimeType="video/mp4">
                  <Representation id="v0" bandwidth="500000">
                    <SegmentTemplate media="seg-$Number$.m4s">
                      <SegmentTimeline>
                        <S t="0" d="2" />
                      </SegmentTimeline>
                    </SegmentTemplate>
                  </Representation>
                </AdaptationSet>
              </Period>
            </MPD>
        "#;
        let error = parse_dash_manifest("https://example.com/video.mpd", mpd).unwrap_err();
        assert!(error.contains("dash_segment_timeline_unsupported"));
    }

    #[test]
    fn parses_iso8601_duration() {
        assert_eq!(parse_iso8601_duration("PT60S"), Some(60.0));
        assert_eq!(parse_iso8601_duration("PT1H30M"), Some(5400.0));
        assert_eq!(parse_iso8601_duration("PT2.5S"), Some(2.5));
        assert_eq!(parse_iso8601_duration("PT"), Some(0.0));
        assert_eq!(parse_iso8601_duration("invalid"), None);
    }

    #[test]
    fn parses_byte_range() {
        let br = parse_byte_range("0-1023").expect("byte range");
        assert_eq!(br.start, 0);
        assert_eq!(br.length, 1024);
        let header = byte_range_header(&br);
        assert_eq!(header, "bytes=0-1023");
        assert!(parse_byte_range("invalid").is_none());
    }

    #[test]
    fn computes_segment_urls_resolves_relative() {
        let parsed = parse_dash_manifest("https://example.com/path/video.mpd", TEMPLATE_MPD)
            .expect("manifest parses");
        let (video, _) = select_tracks(&parsed).expect("tracks selected");
        let video = video.expect("video track");
        let staging = Path::new("/tmp/staging");
        let plans = build_segment_plans(
            "https://example.com/path/video.mpd",
            staging,
            "task-1",
            TRACK_KIND_VIDEO,
            &video,
        )
        .expect("plans built");
        // 1 init + 30 media segments
        assert_eq!(plans.len(), 31);
        assert_eq!(plans[0].segment_index, -1);
        assert_eq!(plans[0].uri, "https://example.com/path/init.m4s");
        assert_eq!(plans[1].segment_index, 0);
        assert_eq!(plans[1].uri, "https://example.com/path/seg-1.m4s");
        assert_eq!(plans[2].uri, "https://example.com/path/seg-2.m4s");
    }

    #[test]
    fn parses_segment_list() {
        let mpd = r#"
            <MPD type="static" mediaPresentationDuration="PT10S">
              <Period>
                <AdaptationSet mimeType="video/mp4" contentType="video">
                  <Representation id="v0" bandwidth="500000">
                    <SegmentList initialization="init.m4s">
                      <SegmentURL media="seg1.m4s" />
                      <SegmentURL media="seg2.m4s" />
                      <SegmentURL media="seg3.m4s" />
                    </SegmentList>
                  </Representation>
                </AdaptationSet>
              </Period>
            </MPD>
        "#;
        let parsed =
            parse_dash_manifest("https://example.com/video.mpd", mpd).expect("manifest parses");
        let (video, audio) = select_tracks(&parsed).expect("tracks selected");
        assert!(audio.is_none());
        let video = video.expect("video track");
        match &video.segment_source {
            SegmentSource::List { segments, .. } => {
                assert_eq!(segments.len(), 3);
                assert_eq!(segments[0].uri, "seg1.m4s");
                assert_eq!(segments[2].uri, "seg3.m4s");
            }
            _ => panic!("expected SegmentSource::List"),
        }
    }

    #[test]
    fn parses_segment_base() {
        let mpd = r#"
            <MPD type="static" mediaPresentationDuration="PT10S">
              <Period>
                <AdaptationSet mimeType="video/mp4" contentType="video">
                  <Representation id="v0" bandwidth="500000">
                    <BaseURL>video.mp4</BaseURL>
                    <SegmentBase>
                      <Initialization range="0-1023" />
                    </SegmentBase>
                  </Representation>
                </AdaptationSet>
              </Period>
            </MPD>
        "#;
        let parsed =
            parse_dash_manifest("https://example.com/video.mpd", mpd).expect("manifest parses");
        let (video, _) = select_tracks(&parsed).expect("tracks selected");
        let video = video.expect("video track");
        match &video.segment_source {
            SegmentSource::Base {
                base_url,
                initialization_range,
                ..
            } => {
                assert_eq!(base_url, "video.mp4");
                let init = initialization_range.as_ref().expect("init range");
                assert_eq!(init.start, 0);
                assert_eq!(init.length, 1024);
            }
            _ => panic!("expected SegmentSource::Base"),
        }
    }

    #[test]
    fn rejects_missing_duration() {
        // SegmentTemplate without duration attribute.
        let mpd = r#"
            <MPD type="static">
              <Period>
                <AdaptationSet mimeType="video/mp4" contentType="video">
                  <Representation id="v0" bandwidth="500000">
                    <SegmentTemplate media="seg-$Number$.m4s" startNumber="1" timescale="1" />
                  </Representation>
                </AdaptationSet>
              </Period>
            </MPD>
        "#;
        let parsed =
            parse_dash_manifest("https://example.com/video.mpd", mpd).expect("manifest parses");
        let (video, _) = select_tracks(&parsed).expect("tracks selected");
        let video = video.expect("video track");
        let staging = Path::new("/tmp/staging");
        let error = build_segment_plans(
            "https://example.com/video.mpd",
            staging,
            "task-1",
            TRACK_KIND_VIDEO,
            &video,
        )
        .unwrap_err();
        assert!(error.contains("dash_no_duration"));
    }
}
