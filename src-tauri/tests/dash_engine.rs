mod common;

use std::{
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use common::TestServer;
use tauri_app_lib::{
    db,
    download::{DashEngine, DownloadEngine, HttpEngine, ProbeRequest},
    models::{AppErrorPayload, SegmentStatus, TaskStatus},
    proxy::ResolvedProxyConfig,
    state_machine,
};

const TEMPLATE_MPD: &str = include_str!("fixtures/dash/supported/template_static.mpd");
const DYNAMIC_MPD: &str = include_str!("fixtures/dash/unsupported/dynamic.mpd");
const TIMELINE_MPD: &str = include_str!("fixtures/dash/unsupported/segment_timeline.mpd");
const MULTI_PERIOD_MPD: &str = include_str!("fixtures/dash/unsupported/multi_period.mpd");
const TIME_TEMPLATE_MPD: &str = include_str!("fixtures/dash/unsupported/time_template.mpd");

const LIST_MPD: &str = r#"
<MPD type="static" mediaPresentationDuration="PT10S" xmlns="urn:mpeg:dash:schema:mpd:2011">
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

const BASE_MPD: &str = r#"
<MPD type="static" mediaPresentationDuration="PT10S" xmlns="urn:mpeg:dash:schema:mpd:2011">
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

const RECOVERY_MPD: &str = r#"
<MPD type="static" mediaPresentationDuration="PT2S" xmlns="urn:mpeg:dash:schema:mpd:2011">
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <Representation id="v0" bandwidth="500000" codecs="avc1.42c01e">
        <SegmentList>
          <SegmentURL media="recovery-0.mp4" />
          <SegmentURL media="recovery-1.mp4" />
        </SegmentList>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>
"#;

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn start_test_server() -> TestServer {
    TestServer::start(handle_connection)
}

fn handle_connection(mut stream: TcpStream) {
    let mut buffer = [0_u8; 4096];
    let Ok(read) = stream.read(&mut buffer) else {
        return;
    };
    if read == 0 {
        return;
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");

    let (status, content_type, body) = match path {
        "/manifest.mpd" => (200, "application/dash+xml", TEMPLATE_MPD.as_bytes()),
        "/live.mpd" => (200, "application/dash+xml", DYNAMIC_MPD.as_bytes()),
        "/timeline.mpd" => (200, "application/dash+xml", TIMELINE_MPD.as_bytes()),
        "/multi-period.mpd" => (200, "application/dash+xml", MULTI_PERIOD_MPD.as_bytes()),
        "/time-template.mpd" => (200, "application/dash+xml", TIME_TEMPLATE_MPD.as_bytes()),
        "/list.mpd" => (200, "application/dash+xml", LIST_MPD.as_bytes()),
        "/base.mpd" => (200, "application/dash+xml", BASE_MPD.as_bytes()),
        _ => (404, "text/plain", b"not found" as &[u8]),
    };

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
}

fn new_engine() -> DashEngine {
    DashEngine::new(Arc::new(
        HttpEngine::with_proxy_config(ResolvedProxyConfig::shared_default())
            .expect("HTTP engine init"),
    ))
}

fn new_probe_request(uri: String) -> ProbeRequest {
    ProbeRequest {
        uri,
        source: None,
        request_headers: Vec::new(),
        pool: None,
        task_id: None,
        credentials: None,
        proxy_config: None,
        app: None,
        request_id: None,
    }
}

fn generate_test_mp4() -> Vec<u8> {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-dash-recovery-{id}.mp4"));
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=5",
            "-t",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
            "-movflags",
            "+faststart",
        ])
        .arg(&path)
        .status()
        .expect("start ffmpeg DASH fixture generation");
    assert!(status.success(), "ffmpeg DASH fixture generation failed");
    let bytes = std::fs::read(&path).expect("read generated DASH fixture");
    let _ = std::fs::remove_file(path);
    bytes
}

fn start_recovery_server(segment: Arc<Vec<u8>>, requests: Arc<[AtomicUsize; 2]>) -> TestServer {
    TestServer::start(move |mut stream| {
        let mut buffer = [0_u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let request = String::from_utf8_lossy(&buffer[..read]);
        let path = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let (status, content_type, body, delay) = match path {
            "/recovery.mpd" => (
                "200 OK",
                "application/dash+xml",
                RECOVERY_MPD.as_bytes(),
                None,
            ),
            "/recovery-0.mp4" => {
                requests[0].fetch_add(1, Ordering::SeqCst);
                ("200 OK", "video/mp4", segment.as_slice(), None)
            }
            "/recovery-1.mp4" => {
                requests[1].fetch_add(1, Ordering::SeqCst);
                (
                    "200 OK",
                    "video/mp4",
                    segment.as_slice(),
                    Some(Duration::from_millis(750)),
                )
            }
            _ => ("404 Not Found", "text/plain", b"not found".as_slice(), None),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
        let _ = stream.write_all(body);
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_succeeds_on_vod_mpd_with_template() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/manifest.mpd",
            server.base_url
        )))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(output.capabilities.supports_resume);
    assert_eq!(output.content_type.as_deref(), Some("application/dash+xml"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_dynamic_mpd() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let error = engine
        .probe(new_probe_request(format!("{}/live.mpd", server.base_url)))
        .await
        .expect_err("dynamic MPD should be rejected");

    let message = error.to_string();
    assert!(
        message.contains("dash_live_unsupported"),
        "expected dash_live_unsupported in error, got: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_segment_timeline() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let error = engine
        .probe(new_probe_request(format!(
            "{}/timeline.mpd",
            server.base_url
        )))
        .await
        .expect_err("SegmentTimeline MPD should be rejected");

    let message = error.to_string();
    assert!(
        message.contains("dash_segment_timeline_unsupported"),
        "expected dash_segment_timeline_unsupported in error, got: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_multi_period_mpd() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let error = new_engine()
        .probe(new_probe_request(format!(
            "{}/multi-period.mpd",
            server.base_url
        )))
        .await
        .expect_err("multi-Period MPD should be rejected")
        .to_string();
    assert!(
        error.contains("dash_multi_period_unsupported"),
        "expected dash_multi_period_unsupported, got: {error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_time_template_placeholder() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let error = new_engine()
        .probe(new_probe_request(format!(
            "{}/time-template.mpd",
            server.base_url
        )))
        .await
        .expect_err("$Time$ template should be rejected")
        .to_string();
    assert!(
        error.contains("dash_template_unsupported"),
        "expected dash_template_unsupported, got: {error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_handles_segment_list_manifest() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!("{}/list.mpd", server.base_url)))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(!output.display_name.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_handles_segment_base_manifest() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!("{}/base.mpd", server.base_url)))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(output.capabilities.supports_resume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_file_scheme_reads_local_mpd() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let engine = new_engine();

    let dir = std::env::temp_dir().join(format!(
        "vibe-dash-file-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let mpd_path: PathBuf = dir.join("local.mpd");
    std::fs::write(&mpd_path, TEMPLATE_MPD).expect("write mpd");

    let uri = reqwest::Url::from_file_path(&mpd_path)
        .expect("mpd path to file URL")
        .to_string();

    let output = engine
        .probe(new_probe_request(uri))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(output.capabilities.supports_resume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_resumes_staging_without_redownloading_completed_segments() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH staging recovery test: ffmpeg not in PATH");
        return;
    }
    let requests = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let server = start_recovery_server(Arc::new(generate_test_mp4()), requests.clone());
    let pool = common::test_pool("dash-staging-recovery").await;
    let mut paths = common::TestPaths::new("dash-staging-recovery");
    let root = paths
        .final_path
        .parent()
        .expect("DASH test root")
        .to_path_buf();
    paths.temp = root.join("output.mp4.vibe-downloading");
    paths.final_path = root.join("recovery.mp4");
    let task = common::download_task(
        "dash-staging-recovery",
        format!("{}/recovery.mpd", server.base_url),
        "dash",
        "recovery.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert DASH task");
    db::ensure_task_segments(&pool, &task)
        .await
        .expect("insert DASH runtime unit");

    let engine = new_engine();
    let cancel = tokio_util::sync::CancellationToken::new();
    let first_download = tokio::spawn({
        let engine = engine.clone();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });

    loop {
        let segments = db::list_dash_segments(&pool, "dash-staging-recovery")
            .await
            .expect("list DASH segments");
        let first_completed = segments.iter().any(|segment| {
            segment.segment_index == 0 && segment.status == SegmentStatus::Completed
        });
        if first_completed && requests[1].load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cancel.cancel();
    first_download
        .await
        .expect("DASH download task join")
        .expect("DASH cancellation is a clean staging pause");

    let paused = db::get_task_record(&pool, "dash-staging-recovery")
        .await
        .expect("read paused DASH task")
        .expect("paused DASH task exists");
    assert_eq!(paused.status, TaskStatus::Paused);
    let no_app = Option::<tauri::AppHandle>::None;
    let resumed = state_machine::transition_task_with_runtime_state(
        &no_app,
        &pool,
        &paused.id,
        TaskStatus::Downloading,
        paused.downloaded_bytes,
        1,
        Some("Downloading"),
        Some("resumed"),
        None,
        SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("persist DASH resume");

    engine
        .download(common::headless_download_context(
            pool.clone(),
            resumed,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("resume DASH download");

    assert_eq!(
        requests[0].load(Ordering::SeqCst),
        1,
        "completed DASH segment must be reused from staging"
    );
    assert_eq!(
        requests[1].load(Ordering::SeqCst),
        2,
        "interrupted DASH segment must be requested again"
    );
    assert!(paths.final_path.exists());
    assert!(
        std::fs::metadata(&paths.final_path)
            .expect("DASH MP4 metadata")
            .len()
            > 0
    );
    let completed = db::get_task_record(&pool, "dash-staging-recovery")
        .await
        .expect("read completed DASH task")
        .expect("completed DASH task exists");
    assert_eq!(completed.status, TaskStatus::Completed);
}

fn b64_basic(user: &str, pass: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_mpd_returns_401() {
    let server = TestServer::start(|mut stream| {
        let mut buffer = [0_u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let body = b"auth required";
        let _ = write!(
            stream,
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"dash\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(body);
    });
    let error = new_engine()
        .probe(new_probe_request(format!("{}/secure.mpd", server.base_url)))
        .await
        .expect_err("401 must fail probe");
    let payload: AppErrorPayload =
        serde_json::from_str(&error.to_string()).expect("structured http_denied");
    assert_eq!(payload.code, "http_denied");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_uses_persisted_dash_credentials() {
    common::install_test_secret_key();
    let expected = b64_basic("dashuser", "dashpass");
    let observed = Arc::new(std::sync::Mutex::new(None));
    let server = TestServer::start({
        let expected = expected.clone();
        let observed = observed.clone();
        move |mut stream| {
            let mut buffer = [0_u8; 8192];
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            if read == 0 {
                return;
            }
            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request
                .lines()
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            let authorization = request.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("authorization") {
                    Some(value.trim().to_string())
                } else {
                    None
                }
            });
            if let Some(auth) = &authorization {
                *observed.lock().expect("lock") = Some(auth.clone());
            }
            let provided = authorization
                .as_deref()
                .and_then(|value| value.strip_prefix("Basic ").map(str::trim));
            if provided != Some(expected.as_str()) {
                let body = b"auth required";
                let _ = write!(
                    stream,
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
                return;
            }
            let (status, content_type, body) = match path {
                "/secure.mpd" => (200, "application/dash+xml", LIST_MPD.as_bytes()),
                _ => (200, "video/mp4", b"seg" as &[u8]),
            };
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
        }
    });

    let pool = common::test_pool("dash-cred-rotation").await;
    let mut paths = common::TestPaths::new("dash-cred-rotation");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("temp.mp4");
    paths.final_path = root.join("secure.mp4");
    let task = common::download_task(
        "dash-cred-rotation",
        format!("{}/secure.mpd", server.base_url),
        "dash",
        "secure.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task).await.expect("insert");
    db::upsert_task_credentials(&pool, &task.id, "dash", "dashuser", "dashpass", None, None)
        .await
        .expect("creds");

    let cancel = tokio_util::sync::CancellationToken::new();
    let download = tokio::spawn({
        let engine = new_engine();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });
    let started = std::time::Instant::now();
    loop {
        if observed.lock().expect("lock").is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            cancel.cancel();
            let _ = download.await;
            panic!("timed out waiting for Authorization");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    cancel.cancel();
    let _ = download.await;
    let auth = observed.lock().expect("lock").clone().expect("auth");
    assert!(auth.starts_with("Basic "));
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_retries_transient_segment_failures() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH segment retry test: ffmpeg not in PATH");
        return;
    }
    let payload = Arc::new(generate_test_mp4());
    let fail_budget = Arc::new(AtomicUsize::new(2));
    let server = TestServer::start({
        let payload = payload.clone();
        let fail_budget = fail_budget.clone();
        move |mut stream| {
            let mut buffer = [0_u8; 4096];
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            if read == 0 {
                return;
            }
            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request
                .lines()
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .nth(1)
                .unwrap_or("/");
            if path == "/retry.mpd" {
                let body = RECOVERY_MPD.as_bytes();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/dash+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
                return;
            }
            if path.contains("recovery-0") {
                let remaining = fail_budget.fetch_sub(1, Ordering::SeqCst);
                if remaining > 0 {
                    let body = b"temporary";
                    let response = format!(
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(body);
                    return;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&payload);
        }
    });

    let pool = common::test_pool("dash-segment-retry").await;
    let mut paths = common::TestPaths::new("dash-segment-retry");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("temp.mp4");
    paths.final_path = root.join("retry.mp4");
    let task = common::download_task(
        "dash-segment-retry",
        format!("{}/retry.mpd", server.base_url),
        "dash",
        "retry.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task).await.expect("insert");
    db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");

    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("DASH download should succeed after transient 500s");

    let segments = db::list_dash_segments(&pool, "dash-segment-retry")
        .await
        .expect("list");
    assert!(
        segments.iter().any(|segment| segment.retry_count > 0),
        "expected retry_count > 0 after transient failures, got {:?}",
        segments
            .iter()
            .map(|s| (s.segment_index, s.retry_count))
            .collect::<Vec<_>>()
    );
    assert!(paths.final_path.exists());
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_reenters_after_reset_interrupted_tasks() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH process-restart reentry test: ffmpeg not in PATH");
        return;
    }
    let requests = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let server = start_recovery_server(Arc::new(generate_test_mp4()), requests.clone());
    let pool = common::test_pool("dash-process-restart").await;
    let mut paths = common::TestPaths::new("dash-process-restart");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("temp.mp4");
    paths.final_path = root.join("restart.mp4");
    let task = common::download_task(
        "dash-process-restart",
        format!("{}/recovery.mpd", server.base_url),
        "dash",
        "restart.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task).await.expect("insert");
    db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");

    let cancel = tokio_util::sync::CancellationToken::new();
    let first = tokio::spawn({
        let engine = new_engine();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });
    loop {
        let segments = db::list_dash_segments(&pool, "dash-process-restart")
            .await
            .expect("list");
        if segments
            .iter()
            .any(|segment| segment.segment_index == 0 && segment.status == SegmentStatus::Completed)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cancel.cancel();
    first.abort();
    let _ = first.await;

    sqlx::query("UPDATE tasks SET status = 'downloading', updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind("dash-process-restart")
        .execute(&pool)
        .await
        .expect("force downloading");
    db::reset_interrupted_tasks(&pool, true)
        .await
        .expect("reset");
    let queued = db::get_task_record(&pool, "dash-process-restart")
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(queued.status, TaskStatus::Queued);
    let no_app = Option::<tauri::AppHandle>::None;
    let resumed = state_machine::transition_task_with_runtime_state(
        &no_app,
        &pool,
        &queued.id,
        TaskStatus::Downloading,
        queued.downloaded_bytes,
        1,
        Some("Downloading"),
        Some("startup_resume"),
        None,
        SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("Downloading");

    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            resumed,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("cold reentry");
    assert!(paths.final_path.exists());
    pool.close().await;
}
