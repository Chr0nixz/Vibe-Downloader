use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use quick_xml::{events::Event, Reader};
use reqwest::{
    header::{HeaderName, HeaderValue},
    Client, RequestBuilder,
};
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use super::{
    engine::EngineFuture,
    http::{
        build_client,
        file::{finalize_download_file, persist_completed_path},
    },
    DownloadContext, DownloadEngine, ProbeOutput, ProbeRequest,
};
use crate::{
    db,
    events::{emit_task_progress, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        AppErrorPayload, EngineCapabilities, ProbedFile, SegmentStatus, TaskKind, TaskProgressPayload,
        TaskRecord, TaskStatus,
    },
    proxy::SharedProxyConfig,
};

const PROTOCOL_DASH: &str = "dash";
const DASH_CONTENT_TYPE: &str = "application/dash+xml";
const DASH_PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct DashEngine {
    proxy_config: SharedProxyConfig,
}

#[derive(Debug, Clone)]
struct DashManifestSummary {
    display_name: String,
    representation_count: i64,
    has_media_description: bool,
}

impl DashEngine {
    pub fn new(proxy_config: SharedProxyConfig) -> Self {
        Self { proxy_config }
    }

    async fn client(&self) -> Result<Client, String> {
        let config = self.proxy_config.read().await;
        build_client(&config)
    }

    async fn probe_dash(
        &self,
        url: &str,
        request_headers: &[(String, String)],
    ) -> Result<DashManifestSummary, String> {
        ensure_ffmpeg_available()?;
        let body = fetch_mpd_text(&self.client().await?, url, request_headers).await?;
        parse_dash_manifest(url, &body)
    }
}

impl DownloadEngine for DashEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_DASH
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "http" | "https" | "file")
    }

    fn probe<'a>(&'a self, request: ProbeRequest) -> EngineFuture<'a, Result<ProbeOutput, String>> {
        Box::pin(async move {
            let summary = self
                .probe_dash(&request.uri, &request.request_headers)
                .await?;
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
                    supports_resume: false,
                    supports_parallel: summary.representation_count > 1
                        || summary.has_media_description,
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
                metalink: None,
            })
        })
    }

    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>> {
        Box::pin(async move { run_dash_download(context).await })
    }
}

async fn run_dash_download(context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel,
        speed_limiter: _,
        connection_limit: _,
        request_headers,
        ..
    } = context;

    ensure_ffmpeg_available()?;
    let input_url = task.final_url.clone().unwrap_or_else(|| task.url.clone());
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
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!(
                "Could not create DASH output folder: {e}"
            ))
            .command_error()
        })?;
    }
    if fs::try_exists(&temp_path).await.unwrap_or(false) {
        fs::remove_file(&temp_path).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!(
                "Could not reset DASH temporary file: {e}"
            ))
            .command_error()
        })?;
    }

    let mut command = Command::new(ffmpeg_path().expect("checked above"));
    command
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-nostats")
        .arg("-y");
    if let Some(headers) = ffmpeg_headers(&request_headers) {
        command.arg("-headers").arg(headers);
    }
    command
        .arg("-i")
        .arg(&input_url)
        .arg("-map")
        .arg("0")
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg(&temp_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    db::insert_task_event(&pool, &task.id, "dash_ffmpeg_started", Some(&sanitize_url(&input_url)))
        .await?;
    let mut child = command
        .spawn()
        .map_err(|e| dash_error("dash_ffmpeg_failed", format!("Could not start ffmpeg: {e}"), true))?;
    drain_child_output(child.stdout.take(), child.stderr.take());

    let mut last_size = 0_i64;
    let mut last_tick = Instant::now();
    let mut last_emit = Instant::now() - DASH_PROGRESS_INTERVAL;
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.start_kill();
            let _ = child.wait().await;
            emit_dash_progress(&app, &pool, &task, temp_size(&temp_path).await, 0, 0).await?;
            return Ok(());
        }

        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("Could not inspect ffmpeg status: {e}"))?
        {
            if !status.success() {
                return Err(dash_error(
                    "dash_ffmpeg_failed",
                    format!("ffmpeg could not download this DASH manifest. Exit status: {status}"),
                    true,
                ));
            }
            break;
        }

        if last_emit.elapsed() >= DASH_PROGRESS_INTERVAL {
            let current_size = temp_size(&temp_path).await;
            let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
            let speed_bps = ((current_size - last_size).max(0) as f64 / elapsed) as i64;
            emit_dash_progress(&app, &pool, &task, current_size, speed_bps, 1).await?;
            last_size = current_size;
            last_tick = Instant::now();
            last_emit = Instant::now();
        }

        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    let downloaded = temp_size(&temp_path).await;
    if downloaded <= 0 {
        return Err(dash_error(
            "dash_empty_output",
            "DASH download completed without producing media output.",
            false,
        ));
    }
    let completed_path = finalize_download_file(&temp_path, &final_path).await?;
    persist_completed_path(&pool, &task.id, &completed_path).await?;
    if task.total_size > 0 {
        db::complete_task(&pool, &task.id).await?;
    } else {
        let segment = db::get_first_segment_record(&pool, &task.id)
            .await?
            .ok_or_else(|| "DASH task segment could not be completed.".to_string())?;
        db::complete_unknown_size_task(&pool, &task.id, &segment.id, downloaded).await?;
    }
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }
    Ok(())
}

async fn emit_dash_progress(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
    downloaded: i64,
    speed_bps: i64,
    connection_count: i32,
) -> Result<(), String> {
    db::update_task_runtime_progress(
        pool,
        &task.id,
        downloaded,
        speed_bps,
        connection_count,
        TaskStatus::Downloading,
        Some("Downloading DASH media"),
    )
    .await?;
    if let Some(segment) = db::get_first_segment_record(pool, &task.id).await? {
        db::update_segment_runtime_progress(
            pool,
            &segment.id,
            downloaded,
            speed_bps,
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
            speed_bps: speed_bps.to_string(),
            connection_count,
            status: TaskStatus::Downloading,
        },
    );
    Ok(())
}

async fn temp_size(path: &Path) -> i64 {
    fs::metadata(path)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn drain_child_output(
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
) {
    if let Some(stdout) = stdout {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while lines.next_line().await.ok().flatten().is_some() {}
        });
    }
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Some(line) = lines.next_line().await.ok().flatten() {
                tracing::debug!(line = %line, "ffmpeg dash output");
            }
        });
    }
}

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

fn parse_dash_manifest(manifest_url: &str, text: &str) -> Result<DashManifestSummary, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut saw_mpd = false;
    let mut representation_count = 0_i64;
    let mut has_media_description = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "mpd" => {
                        saw_mpd = true;
                        if attr_value(&event, "type")
                            .is_some_and(|value| value.eq_ignore_ascii_case("dynamic"))
                        {
                            return Err(dash_error(
                                "dash_live_unsupported",
                                "Live MPEG-DASH MPDs are not supported yet. Use a static/VOD MPD.",
                                false,
                            ));
                        }
                    }
                    "representation" => representation_count += 1,
                    "segmenttemplate" | "segmentlist" | "segmentbase" | "baseurl" => {
                        has_media_description = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(dash_error(
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
        return Err(dash_error(
            "dash_invalid_manifest",
            "DASH MPD is missing the MPD root element.",
            false,
        ));
    }
    if representation_count == 0 {
        return Err(dash_error(
            "dash_invalid_manifest",
            "DASH MPD does not contain any representations.",
            false,
        ));
    }
    Ok(DashManifestSummary {
        display_name: dash_output_name(manifest_url),
        representation_count,
        has_media_description,
    })
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
    format!("{stem}.mp4")
}

fn ensure_ffmpeg_available() -> Result<(), String> {
    if ffmpeg_path().is_some() {
        Ok(())
    } else {
        Err(dash_error(
            "dash_ffmpeg_missing",
            "ffmpeg was not found. Install ffmpeg or set VIBE_FFMPEG_PATH before creating MPEG-DASH tasks.",
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

fn ffmpeg_headers(headers: &[(String, String)]) -> Option<String> {
    let mut out = String::new();
    for (name, value) in headers {
        if name.contains(['\r', '\n', ':']) || value.contains(['\r', '\n']) {
            continue;
        }
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    (!out.is_empty()).then_some(out)
}

fn dash_error(code: &str, message: impl Into<String>, recoverable: bool) -> String {
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
    fn parses_static_mpd_summary() {
        let summary = parse_dash_manifest(
            "https://example.com/video/movie.mpd",
            r#"
            <MPD type="static" xmlns="urn:mpeg:dash:schema:mpd:2011">
              <Period>
                <AdaptationSet mimeType="video/mp4">
                  <Representation id="v1" bandwidth="1000">
                    <BaseURL>video.mp4</BaseURL>
                  </Representation>
                </AdaptationSet>
              </Period>
            </MPD>
            "#,
        )
        .expect("manifest");

        assert_eq!(summary.display_name, "movie.mp4");
        assert_eq!(summary.representation_count, 1);
        assert!(summary.has_media_description);
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
    fn builds_ffmpeg_header_block() {
        let headers = ffmpeg_headers(&[
            ("Cookie".to_string(), "a=b".to_string()),
            ("Bad:Name".to_string(), "ignored".to_string()),
        ])
        .expect("headers");

        assert_eq!(headers, "Cookie: a=b\r\n");
    }
}
