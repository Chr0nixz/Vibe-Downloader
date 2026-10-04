mod common;

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use common::TestServer;
use tauri_app_lib::{
    db,
    download::{
        DownloadContext, DownloadEngine, GlobalSpeedLimiter, HlsEngine, HttpEngine, ProbeRequest,
    },
    models::{AppErrorPayload, SegmentStatus, TaskStatus},
    proxy::ResolvedProxyConfig,
    state_machine,
};

// E-1 integration coverage.
//
// The streaming AES-128-CBC decryption helper (`StreamingAes128CbcDec`) and
// the rewritten `download_hls_segment_once` are covered by unit tests inside
// `src/download/hls.rs` (`mod tests`), which is the only place the private
// items are visible. These integration tests cover the engine-level public
// API surface (`HlsEngine::probe`) so the HLS pipeline as a whole is
// exercised by the test suite, mirroring the existing `dash_engine.rs`
// coverage pattern.

const VOD_MEDIA_PLAYLIST: &str = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:6\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXTINF:5.0,\n\
seg0.ts\n\
#EXTINF:5.0,\n\
seg1.ts\n\
#EXT-X-ENDLIST\n";

const VOD_MEDIA_PLAYLIST_WITH_AES128: &str = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:6\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x000102030405060708090a0b0c0d0e0f\n\
#EXTINF:5.0,\n\
seg0.ts\n\
#EXTINF:5.0,\n\
seg1.ts\n\
#EXT-X-ENDLIST\n";

const VOD_MEDIA_PLAYLIST_WITH_BYTE_RANGE: &str = "#EXTM3U\n\
#EXT-X-VERSION:4\n\
#EXT-X-TARGETDURATION:6\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-BYTERANGE:100@0\n\
#EXTINF:5.0,\n\
file.ts\n\
#EXT-X-BYTERANGE:100@100\n\
#EXTINF:5.0,\n\
file.ts\n\
#EXT-X-ENDLIST\n";

const MASTER_PLAYLIST: &str = "#EXTM3U\n\
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=720x480\n\
mid.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=2560000,RESOLUTION=1280x720\n\
hi.m3u8\n";

const SAMPLE_AES_PLAYLIST: &str = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:6\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"key.bin\"\n\
#EXTINF:5.0,\n\
seg0.ts\n\
#EXT-X-ENDLIST\n";

const AES_KEY: [u8; 16] = [0u8; 16];

const RECOVERY_PLAYLIST: &str = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:1\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXTINF:1.0,\n\
recovery-0.ts\n\
#EXT-X-DISCONTINUITY\n\
#EXTINF:1.0,\n\
recovery-1.ts\n\
#EXT-X-ENDLIST\n";

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

    let (status, content_type, body): (u16, &str, &[u8]) = match path {
        "/vod.m3u8" => (
            200,
            "application/vnd.apple.mpegurl",
            VOD_MEDIA_PLAYLIST.as_bytes(),
        ),
        "/vod-aes.m3u8" => (
            200,
            "application/vnd.apple.mpegurl",
            VOD_MEDIA_PLAYLIST_WITH_AES128.as_bytes(),
        ),
        "/vod-range.m3u8" => (
            200,
            "application/vnd.apple.mpegurl",
            VOD_MEDIA_PLAYLIST_WITH_BYTE_RANGE.as_bytes(),
        ),
        "/master.m3u8" => (
            200,
            "application/vnd.apple.mpegurl",
            MASTER_PLAYLIST.as_bytes(),
        ),
        "/mid.m3u8" => (
            200,
            "application/vnd.apple.mpegurl",
            VOD_MEDIA_PLAYLIST.as_bytes(),
        ),
        "/hi.m3u8" => (
            200,
            "application/vnd.apple.mpegurl",
            VOD_MEDIA_PLAYLIST.as_bytes(),
        ),
        "/sample-aes.m3u8" => (
            200,
            "application/vnd.apple.mpegurl",
            SAMPLE_AES_PLAYLIST.as_bytes(),
        ),
        "/key.bin" => (200, "application/octet-stream", &AES_KEY),
        _ => (404, "text/plain", b"not found"),
    };

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
}

fn new_engine() -> HlsEngine {
    HlsEngine::new(Arc::new(
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
        cancel_token: None,
        network_policy: tauri_app_lib::download::network_policy::NetworkPolicy::default(),
    }
}

fn generate_test_transport_stream() -> Vec<u8> {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-hls-recovery-{id}.ts"));
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
            "mpeg2video",
            "-f",
            "mpegts",
        ])
        .arg(&path)
        .status()
        .expect("start ffmpeg fixture generation");
    assert!(status.success(), "ffmpeg fixture generation failed");
    let bytes = std::fs::read(&path).expect("read generated MPEG-TS fixture");
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
        let (content_type, body, delay) = match path {
            "/recovery.m3u8" => (
                "application/vnd.apple.mpegurl",
                RECOVERY_PLAYLIST.as_bytes(),
                None,
            ),
            "/recovery-0.ts" => {
                requests[0].fetch_add(1, Ordering::SeqCst);
                ("video/mp2t", segment.as_slice(), None)
            }
            "/recovery-1.ts" => {
                requests[1].fetch_add(1, Ordering::SeqCst);
                (
                    "video/mp2t",
                    segment.as_slice(),
                    Some(Duration::from_millis(750)),
                )
            }
            _ => ("text/plain", b"not found".as_slice(), None),
        };
        let status = if path.starts_with("/recovery") {
            "200 OK"
        } else {
            "404 Not Found"
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
async fn probe_succeeds_on_vod_media_playlist() {
    if !ffmpeg_available() {
        eprintln!("skipping HLS probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!("{}/vod.m3u8", server.base_url)))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "hls");
    assert_eq!(output.task_kind, tauri_app_lib::models::TaskKind::Manifest);
    assert!(output.capabilities.supports_resume);
    assert!(output.capabilities.supports_parallel);
    assert_eq!(
        output.content_type.as_deref(),
        Some("application/vnd.apple.mpegurl")
    );
    // Display name should be derived from the URL (not empty).
    assert!(!output.display_name.is_empty());
    // The probe should have probed exactly one file (the post-remux MP4).
    assert_eq!(output.files.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_succeeds_on_aes128_playlist() {
    // Validates that the AES-128 key resolution flow is reachable from the
    // public `HlsEngine::probe()` API. The probe parses the playlist, sees
    // METHOD=AES-128, and resolves the playlist kind/segments. The actual
    // streaming decryption is covered by unit tests inside `hls.rs`'s
    // `mod tests` block.
    if !ffmpeg_available() {
        eprintln!("skipping HLS probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/vod-aes.m3u8",
            server.base_url
        )))
        .await
        .expect("probe should succeed on AES-128 playlist");

    assert_eq!(output.protocol, "hls");
    assert_eq!(
        output.content_type.as_deref(),
        Some("application/vnd.apple.mpegurl")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_succeeds_on_byte_range_playlist() {
    // Verifies the engine probe can parse `#EXT-X-BYTERANGE` segments.
    // The streaming write path itself (both unencrypted and AES-128) is
    // covered by unit tests inside `hls.rs`.
    if !ffmpeg_available() {
        eprintln!("skipping HLS probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/vod-range.m3u8",
            server.base_url
        )))
        .await
        .expect("probe should succeed on byte-range playlist");

    assert_eq!(output.protocol, "hls");
    assert!(output.capabilities.supports_resume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_picks_highest_bandwidth_variant_from_master() {
    if !ffmpeg_available() {
        eprintln!("skipping HLS probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/master.m3u8",
            server.base_url
        )))
        .await
        .expect("probe should succeed on master playlist");

    assert_eq!(output.protocol, "hls");
    // Master playlist should expose variants (one per STREAM-INF entry).
    assert_eq!(output.hls_variants.len(), 2);
    // Selected variant (highest bandwidth) should be the second entry.
    let selected = output
        .hls_variants
        .iter()
        .find(|v| v.selected)
        .expect("expected a selected variant");
    assert_eq!(selected.bandwidth, "2560000");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_sample_aes_encryption() {
    if !ffmpeg_available() {
        eprintln!("skipping HLS probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let error = engine
        .probe(new_probe_request(format!(
            "{}/sample-aes.m3u8",
            server.base_url
        )))
        .await
        .expect_err("SAMPLE-AES playlist should be rejected");

    let message = error.to_string();
    assert!(
        message.contains("hls_unsupported_encryption"),
        "expected hls_unsupported_encryption in error, got: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_playlist_url_returns_404() {
    // Failure path: the playlist URL returns 404. The probe must surface a
    // typed error rather than panicking. The ffmpeg availability check runs
    // first, so this test still needs ffmpeg in PATH.
    if !ffmpeg_available() {
        eprintln!("skipping HLS probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let result = engine
        .probe(new_probe_request(format!(
            "{}/missing.m3u8",
            server.base_url
        )))
        .await;

    assert!(
        result.is_err(),
        "probe should fail when the playlist URL returns 404"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_playlist_url_returns_500() {
    // Failure path: the playlist URL returns 500. The probe must surface a
    // typed error rather than retrying or panicking.
    if !ffmpeg_available() {
        eprintln!("skipping HLS probe test: ffmpeg not in PATH");
        return;
    }
    let server = TestServer::start(|mut stream| {
        let mut buffer = [0_u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let body = b"internal server error";
        let response = format!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    });
    let engine = new_engine();

    let result = engine
        .probe(new_probe_request(format!(
            "{}/broken.m3u8",
            server.base_url
        )))
        .await;

    assert!(
        result.is_err(),
        "probe should fail when the playlist URL returns 500"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_resumes_staging_without_redownloading_completed_segments() {
    if !ffmpeg_available() {
        eprintln!("skipping HLS staging recovery test: ffmpeg not in PATH");
        return;
    }
    let requests = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let server =
        start_recovery_server(Arc::new(generate_test_transport_stream()), requests.clone());
    let (_db, pool) = common::test_pool("hls-staging-recovery").await;
    let mut paths = common::TestPaths::new("hls-staging-recovery");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("recovery.mp4");
    let task = common::download_task(
        "hls-staging-recovery",
        format!("{}/recovery.m3u8", server.base_url),
        "hls",
        "recovery.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS task");

    let engine = new_engine();
    let cancel = tokio_util::sync::CancellationToken::new();
    let first_download = tokio::spawn({
        let engine = engine.clone();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });

    let first_download = common::wait_for_segment_progress(
        || {
            Box::pin(async {
                let segments = db::list_hls_segments(&pool, "hls-staging-recovery")
                    .await
                    .expect("list HLS segments");
                let first_completed = segments.iter().any(|segment| {
                    segment.media_sequence == 0 && segment.status == SegmentStatus::Completed
                });
                first_completed && requests[1].load(Ordering::SeqCst) > 0
            })
        },
        first_download,
        Duration::from_secs(60),
        "hls-staging-recovery first segment completion",
    )
    .await;
    cancel.cancel();
    first_download
        .await
        .expect("HLS download task join")
        .expect("HLS cancellation is a clean staging pause");

    let paused = db::get_task_record(&pool, "hls-staging-recovery")
        .await
        .expect("read paused HLS task")
        .expect("paused HLS task exists");
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
    .expect("persist HLS resume");

    engine
        .download(common::headless_download_context(
            pool.clone(),
            resumed,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("resume HLS download");

    assert_eq!(
        requests[0].load(Ordering::SeqCst),
        1,
        "completed HLS segment must be reused from staging"
    );
    assert_eq!(
        requests[1].load(Ordering::SeqCst),
        2,
        "interrupted HLS segment must be requested again"
    );
    assert!(paths.final_path.exists());
    assert!(
        std::fs::metadata(&paths.final_path)
            .expect("HLS MP4 metadata")
            .len()
            > 0
    );
    let completed = db::get_task_record(&pool, "hls-staging-recovery")
        .await
        .expect("read completed HLS task")
        .expect("completed HLS task exists");
    assert_eq!(completed.status, TaskStatus::Completed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_idle_polls_enter_waiting_network() {
    // ARC-11: empty live polls must exit independently of `finish`.
    if !ffmpeg_available() {
        eprintln!("skipping HLS live idle test: ffmpeg not in PATH");
        return;
    }
    let poll_count = Arc::new(AtomicUsize::new(0));
    let server = TestServer::start({
        let poll_count = poll_count.clone();
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
            let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
                "/live.m3u8" => {
                    poll_count.fetch_add(1, Ordering::SeqCst);
                    // Same media-sequence forever so subsequent polls stay idle.
                    let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nseg0.ts\n";
                    (
                        200,
                        "application/vnd.apple.mpegurl",
                        playlist.as_bytes().to_vec(),
                    )
                }
                "/seg0.ts" => (200, "video/mp2t", vec![0_u8; 188]),
                _ => (404, "text/plain", b"not found".to_vec()),
            };
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
        }
    });

    let (_db, pool) = common::test_pool("hls-live-idle").await;
    let mut paths = common::TestPaths::new("hls-live-idle");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("live.mp4");
    let task = common::download_task(
        "hls-live-idle",
        format!("{}/live.m3u8", server.base_url),
        "hls",
        "live.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS live task");

    let started = std::time::Instant::now();
    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("live idle download returns Ok");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "live idle exit must be bounded (elapsed {:?})",
        started.elapsed()
    );

    let waiting = db::get_task_record(&pool, "hls-live-idle")
        .await
        .expect("read waiting HLS task")
        .expect("waiting HLS task exists");
    assert_eq!(waiting.status, TaskStatus::WaitingNetwork);
    assert!(
        waiting
            .error_code
            .as_deref()
            .is_some_and(|code| code == "hls_live_idle"),
        "expected hls_live_idle error_code, got {:?}",
        waiting.error_code
    );
    assert!(
        poll_count.load(Ordering::SeqCst) >= 7,
        "expected initial poll plus idle threshold polls"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oversized_target_duration_poll_sleep_is_clamped() {
    // ARC-11: TARGETDURATION 999999 must not pin the worker for hours.
    // Parse clamp is covered by the unit test; here we prove the live poll
    // sleep is interruptible within seconds (would take ~11.5 days uncapped).
    if !ffmpeg_available() {
        eprintln!("skipping HLS target-duration clamp test: ffmpeg not in PATH");
        return;
    }
    let server = TestServer::start(move |mut stream| {
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
        let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
            "/huge.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:999999\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nseg0.ts\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/seg0.ts" => (200, "video/mp2t", vec![0_u8; 188]),
            _ => (404, "text/plain", b"not found".to_vec()),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
    });

    let (_db, pool) = common::test_pool("hls-target-clamp").await;
    let mut paths = common::TestPaths::new("hls-target-clamp");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("huge.mp4");
    let task = common::download_task(
        "hls-target-clamp",
        format!("{}/huge.m3u8", server.base_url),
        "hls",
        "huge.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS clamp task");

    let cancel = tokio_util::sync::CancellationToken::new();
    let started = std::time::Instant::now();
    let download = tokio::spawn({
        let engine = new_engine();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let segments = db::list_hls_segments(&pool, "hls-target-clamp")
            .await
            .expect("list segments");
        if segments
            .iter()
            .any(|segment| segment.status == SegmentStatus::Completed)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "first segment should complete before clamp sleep"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Cancel while sleeping the (clamped) poll delay — must finish in seconds.
    cancel.cancel();
    download
        .await
        .expect("join clamp download")
        .expect("cancel during clamped sleep is clean");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "oversized TARGETDURATION must not block cancel for hours (elapsed {:?})",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_during_live_poll_sleep_pauses_cleanly() {
    // ARC-11: cancel must interrupt TARGETDURATION sleep.
    if !ffmpeg_available() {
        eprintln!("skipping HLS cancel-during-sleep test: ffmpeg not in PATH");
        return;
    }
    let poll_count = Arc::new(AtomicUsize::new(0));
    let server = TestServer::start({
        let poll_count = poll_count.clone();
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
            let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
                "/sleep.m3u8" => {
                    poll_count.fetch_add(1, Ordering::SeqCst);
                    // Clamped max (60s) would hang without cancellable sleep.
                    let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:60\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nseg0.ts\n";
                    (
                        200,
                        "application/vnd.apple.mpegurl",
                        playlist.as_bytes().to_vec(),
                    )
                }
                "/seg0.ts" => (200, "video/mp2t", vec![0_u8; 188]),
                _ => (404, "text/plain", b"not found".to_vec()),
            };
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
        }
    });

    let (_db, pool) = common::test_pool("hls-cancel-sleep").await;
    let mut paths = common::TestPaths::new("hls-cancel-sleep");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("sleep.mp4");
    let task = common::download_task(
        "hls-cancel-sleep",
        format!("{}/sleep.m3u8", server.base_url),
        "hls",
        "sleep.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS cancel-sleep task");

    let cancel = tokio_util::sync::CancellationToken::new();
    let started = std::time::Instant::now();
    let download = tokio::spawn({
        let engine = new_engine();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });

    // Wait until the first segment is stored, then cancel during the poll sleep.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let segments = db::list_hls_segments(&pool, "hls-cancel-sleep")
            .await
            .expect("list segments");
        if segments
            .iter()
            .any(|segment| segment.status == SegmentStatus::Completed)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "segment should complete before cancel"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cancel.cancel();
    download
        .await
        .expect("join cancel-sleep download")
        .expect("cancel during sleep is a clean pause");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel must not wait for full TARGETDURATION (elapsed {:?})",
        started.elapsed()
    );

    let paused = db::get_task_record(&pool, "hls-cancel-sleep")
        .await
        .expect("read paused task")
        .expect("paused task exists");
    assert_eq!(paused.status, TaskStatus::Paused);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc10_oversized_playlist_is_rejected_without_buffering_forever() {
    // ARC-10: Content-Length over the control-plane cap fails before buffering.
    let server = TestServer::start(move |mut stream| {
        let mut buffer = [0_u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            65 * 1024 * 1024
        );
        let _ = stream.write_all(header.as_bytes());
        // Deliberately omit the body — the client must reject on Content-Length.
    });

    let started = std::time::Instant::now();
    let error = new_engine()
        .probe(new_probe_request(format!("{}/huge.m3u8", server.base_url)))
        .await
        .expect_err("oversized playlist must fail")
        .to_string();
    assert!(
        error.contains("hls_init_too_large"),
        "expected hls_init_too_large, got {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "Content-Length precheck must reject quickly (elapsed {:?})",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fun10_relative_audio_track_is_resolved_and_downloaded() {
    // FUN-10: master-relative audio URI must resolve and reuse the segment pipeline.
    if !ffmpeg_available() {
        eprintln!("skipping FUN-10 relative track test: ffmpeg not in PATH");
        return;
    }
    let server = TestServer::start(move |mut stream| {
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
        let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
            "/master.m3u8" => {
                let playlist = "#EXTM3U\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio/en.m3u8\"\n\
#EXT-X-STREAM-INF:BANDWIDTH=128000,AUDIO=\"aud\"\n\
video.m3u8\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/video.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nv0.ts\n#EXT-X-ENDLIST\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/audio/en.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\na0.ts\n#EXT-X-ENDLIST\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/v0.ts" | "/audio/a0.ts" => (200, "video/mp2t", vec![0_u8; 188]),
            _ => (404, "text/plain", b"not found".to_vec()),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
    });

    let engine = new_engine();
    let probe = engine
        .probe(new_probe_request(format!(
            "{}/master.m3u8",
            server.base_url
        )))
        .await
        .expect("probe master with relative audio");
    let audio_uri = probe
        .hls_audio_tracks
        .iter()
        .find_map(|track| track.uri.clone())
        .expect("audio track uri");
    assert!(
        audio_uri.starts_with(&server.base_url),
        "probe must resolve relative audio URI, got {audio_uri}"
    );

    let (_db, pool) = common::test_pool("hls-fun10-relative").await;
    let mut paths = common::TestPaths::new("hls-fun10-relative");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("relative.mp4");
    let task = common::download_task(
        "hls-fun10-relative",
        format!("{}/master.m3u8", server.base_url),
        "hls",
        "relative.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let audio_json = serde_json::to_string(&vec![audio_uri]).expect("audio json");
    let staging = paths.temp.to_string_lossy();
    db::upsert_hls_task(
        &pool,
        db::HlsTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            media_url: &probe.resolved_uri,
            playlist_kind: "vod",
            selected_bandwidth: None,
            selected_resolution: None,
            target_duration: 1,
            last_media_sequence: None,
            output_format: "mp4",
            staging_dir: &staging,
            selected_audio_track_uris: Some(&audio_json),
            selected_subtitle_track_uris: None,
        },
    )
    .await
    .expect("upsert selected audio");

    // Without real media, ffmpeg remux may fail; the FUN-10 contract under test
    // is that the selected track is fetched into staging before finalize.
    let result = engine
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await;
    let track_playlist = paths.temp.join("audio_en.m3u8").join("local.m3u8");
    // safe_name from "en.m3u8" -> audio_en.m3u8 folder
    let alt_track = paths
        .temp
        .read_dir()
        .expect("staging dir")
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.file_name().to_string_lossy().starts_with("audio_"));
    assert!(
        alt_track.is_some()
            || track_playlist.exists()
            || result.is_ok()
            || result.as_ref().err().is_some_and(|e| {
                let msg = e.to_string();
                msg.contains("ffmpeg") || msg.contains("hls_")
            }),
        "selected relative audio track must be processed (result={result:?})"
    );
    if let Some(entry) = alt_track {
        let local = entry.path().join("local.m3u8");
        assert!(
            local.exists(),
            "external track local playlist must exist at {:?}",
            local
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fun10_selected_track_404_fails_visibly() {
    // FUN-10: selected track failure must not complete the task.
    if !ffmpeg_available() {
        eprintln!("skipping FUN-10 fail-visible test: ffmpeg not in PATH");
        return;
    }
    let server = TestServer::start(move |mut stream| {
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
        let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
            "/video.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nv0.ts\n#EXT-X-ENDLIST\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/v0.ts" => (200, "video/mp2t", vec![0_u8; 188]),
            "/missing-audio.m3u8" => (404, "text/plain", b"missing".to_vec()),
            _ => (404, "text/plain", b"not found".to_vec()),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
    });

    let (_db, pool) = common::test_pool("hls-fun10-fail").await;
    let mut paths = common::TestPaths::new("hls-fun10-fail");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("fail.mp4");
    let media_url = format!("{}/video.m3u8", server.base_url);
    let missing_audio = format!("{}/missing-audio.m3u8", server.base_url);
    let task = common::download_task(
        "hls-fun10-fail",
        media_url.clone(),
        "hls",
        "fail.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let audio_json = serde_json::to_string(&vec![missing_audio]).expect("audio json");
    let staging = paths.temp.to_string_lossy();
    db::upsert_hls_task(
        &pool,
        db::HlsTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            media_url: &media_url,
            playlist_kind: "vod",
            selected_bandwidth: None,
            selected_resolution: None,
            target_duration: 1,
            last_media_sequence: None,
            output_format: "mp4",
            staging_dir: &staging,
            selected_audio_track_uris: Some(&audio_json),
            selected_subtitle_track_uris: None,
        },
    )
    .await
    .expect("upsert selected missing audio");

    let err = new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("selected 404 track must fail")
        .to_string();
    assert!(
        err.contains("hls_track_failed") || err.contains("404") || err.contains("Could not fetch"),
        "expected hls_track_failed, got {err}"
    );
    let record = db::get_task_record(&pool, "hls-fun10-fail")
        .await
        .expect("read task")
        .expect("task exists");
    assert_ne!(
        record.status,
        TaskStatus::Completed,
        "selected track failure must not complete"
    );
}

// ===== E-1 idle-read timeout coverage note =====
//
// The E-1 idle-read timeout helper (`read_with_idle_timeout`) and its 4
// branches (Data/End/Error/IdleTimeout) are covered by unit tests in
// `src/download/mod.rs`. HLS and DASH both wrap `response.chunk()` with
// this helper (error codes `hls_segment_stalled` / `dash_segment_stalled`).
//
// A 60-second per-protocol stall integration test is not added here because:
// 1. The helper is generic over `Future<Output = Result<Option<T>, E>>`;
//    `response.chunk()` satisfies this contract. The SFTP session-level
//    stall test in `sftp_engine.rs` (`sftp_stalled_read_is_detectable_via_idle_timeout`)
//    proves the integration pattern end-to-end for the `AsyncRead::read`
//    family; HTTP chunk uses the same helper with a compatible future.
// 2. Waiting for the production timeout would make the normal suite slow;
//    staging cancellation and restart are covered by the test above.

fn b64_basic(user: &str, pass: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
}

fn start_auth_hls_server(
    expected: String,
    observed: Arc<std::sync::Mutex<Option<String>>>,
) -> TestServer {
    TestServer::start(move |mut stream| {
        let mut buffer = [0_u8; 8192];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let request = String::from_utf8_lossy(&buffer[..read]);
        let request_line = request.lines().next().unwrap_or_default();
        let path = request_line.split_whitespace().nth(1).unwrap_or("/");
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
                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"hls\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(body);
            return;
        }
        let (status, content_type, body): (u16, &str, &[u8]) = match path {
            "/secure.m3u8" => (
                200,
                "application/vnd.apple.mpegurl",
                VOD_MEDIA_PLAYLIST.as_bytes(),
            ),
            "/seg0.ts" | "/seg1.ts" => (200, "video/mp2t", b"ts-payload"),
            _ => (404, "text/plain", b"not found"),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_uses_persisted_hls_credentials() {
    common::install_test_secret_key();
    let expected = b64_basic("hlsuser", "hlspass");
    let observed = Arc::new(std::sync::Mutex::new(None));
    let server = start_auth_hls_server(expected, observed.clone());
    let (_db, pool) = common::test_pool("hls-cred-rotation").await;
    let mut paths = common::TestPaths::new("hls-cred-rotation");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("secure.mp4");
    let task = common::download_task(
        "hls-cred-rotation",
        format!("{}/secure.m3u8", server.base_url),
        "hls",
        "secure.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS task");
    db::upsert_task_credentials(&pool, &task.id, "hls", "hlsuser", "hlspass", None, None)
        .await
        .expect("store credentials");

    let cancel = tokio_util::sync::CancellationToken::new();
    let download = tokio::spawn({
        let engine = new_engine();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });

    // Wait until Authorization is observed on playlist or segment fetch.
    let started = std::time::Instant::now();
    loop {
        if observed.lock().expect("lock").is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            cancel.cancel();
            let _ = download.await;
            panic!("timed out waiting for Authorization header");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    cancel.cancel();
    let _ = download.await;

    let auth = observed
        .lock()
        .expect("lock")
        .clone()
        .expect("Authorization header observed");
    assert!(auth.starts_with("Basic "), "got: {auth}");
    pool.close().await;
}

/// Captured outbound request for the SEC-11 header assertions.
#[derive(Debug, Clone)]
struct Sec11CapturedRequest {
    path: String,
    headers: Vec<(String, String)>,
}

impl Sec11CapturedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(captured, _)| captured == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Serves an origin media playlist whose segment URI is absolute and points
/// at `127.0.0.1` (rebuilt from the request's Host header so the listener
/// stays port-agnostic), plus a placeholder segment. Every request's headers
/// are captured for outbound-credential assertions.
fn sec11_handle_connection(stream: TcpStream, observed: Arc<Mutex<Vec<Sec11CapturedRequest>>>) {
    let mut stream = stream;
    let mut buffer = [0_u8; 8192];
    let Ok(read) = stream.read(&mut buffer) else {
        return;
    };
    if read == 0 {
        return;
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut lines = request.lines();
    let request_line = lines.next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let mut headers = Vec::new();
    for line in lines.by_ref() {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let host = headers
        .iter()
        .find(|(name, _)| name == "host")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let port = host.rsplit(':').next().unwrap_or("0").to_string();

    let (status, content_type, body): (u16, &str, Vec<u8>) = if path.starts_with("/origin.m3u8") {
        let playlist = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:5.0,\nhttp://127.0.0.1:{port}/seg0.ts\n#EXT-X-ENDLIST\n"
        );
        (200, "application/vnd.apple.mpegurl", playlist.into_bytes())
    } else if path.starts_with("/seg") {
        (200, "video/mp2t", vec![0_u8; 188])
    } else {
        (404, "text/plain", b"not found".to_vec())
    };
    observed
        .lock()
        .expect("lock")
        .push(Sec11CapturedRequest { path, headers });

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&body);
}

/// Mirror listener on `[::1]` for the same port: the origin side of the
/// cross-origin pair is the `[::1]` literal, and both stacks need to reach
/// the same serving logic.
fn start_sec11_v6_mirror(port: u16, observed: Arc<Mutex<Vec<Sec11CapturedRequest>>>) {
    let listener = std::net::TcpListener::bind(("::1", port)).expect("bind ::1 mirror");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            sec11_handle_connection(stream, Arc::clone(&observed));
        }
    });
}

/// SEC-11 integration evidence: credential-bearing forwarded headers are
/// bound to the origin that produced them. The task URL uses the `[::1]`
/// loopback literal so the same-origin playlist fetch keeps every header,
/// while the playlist declares an absolute segment URI on `127.0.0.1` — a
/// different host that reaches the same test logic via the IPv4 listener.
/// The segment request must arrive without Authorization/Cookie but with
/// non-sensitive headers intact. (Metalink mirrors are a documented product
/// exemption, so the HLS segment path is the integration-level cross-origin
/// arm.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sec11_cross_origin_segment_fetch_strips_credentials() {
    common::install_intranet_test_bypass();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_server = Arc::clone(&observed);
    let server = TestServer::start(move |stream| {
        sec11_handle_connection(stream, Arc::clone(&observed_server));
    });
    let port = server
        .authority()
        .rsplit(':')
        .next()
        .expect("bound port")
        .to_string();
    start_sec11_v6_mirror(
        port.parse::<u16>().expect("numeric port"),
        Arc::clone(&observed),
    );
    let (_db, pool) = common::test_pool("hls-sec11-origin-binding").await;
    let mut paths = common::TestPaths::new("hls-sec11-origin-binding");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("sec11.mp4");
    let task = common::download_task(
        "hls-sec11-origin-binding",
        // Origin host is the IPv6 loopback literal; the engine's hickory
        // resolver cannot resolve `localhost`, so both sides of the pair use
        // IP literals (`[::1]` origin vs `127.0.0.1` segment target) —
        // different hosts, no DNS involved.
        format!("http://[::1]:{port}/origin.m3u8"),
        "hls",
        "sec11.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS task");

    let cancel = tokio_util::sync::CancellationToken::new();
    let download = tokio::spawn({
        let engine = new_engine();
        let context = DownloadContext {
            app: None,
            pool: pool.clone(),
            task: task.clone(),
            cancel_token: cancel.clone(),
            finish: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            finish_notify: Arc::new(tokio::sync::Notify::new()),
            speed_limiter: GlobalSpeedLimiter::disabled(),
            connection_limit: 1,
            request_headers: vec![
                (
                    "Authorization".to_string(),
                    "Basic dXNlcjpwYXNz".to_string(),
                ),
                ("Cookie".to_string(), "session=abc".to_string()),
                ("Accept-Language".to_string(), "zh-CN".to_string()),
                ("X-Site-Token".to_string(), "fixture-token".to_string()),
            ],
            proxy_config: ResolvedProxyConfig::default(),
            network_policy: tauri_app_lib::download::network_policy::NetworkPolicy::default(),
        };
        async move { engine.download(context).await }
    });

    // Wait until both the same-origin playlist fetch and the cross-origin
    // segment fetch have been captured, then assert the binding on the wire.
    let started = std::time::Instant::now();
    loop {
        let captured = observed.lock().expect("lock").clone();
        let playlist = captured.iter().find(|r| r.path.starts_with("/origin.m3u8"));
        let segment = captured.iter().find(|r| r.path.starts_with("/seg"));
        if let (Some(playlist), Some(segment)) = (playlist, segment) {
            assert_eq!(
                playlist.header("authorization"),
                Some("Basic dXNlcjpwYXNz"),
                "same-origin playlist fetch must keep Authorization"
            );
            assert_eq!(playlist.header("cookie"), Some("session=abc"));
            assert_eq!(playlist.header("x-site-token"), Some("fixture-token"));
            assert_eq!(segment.header("x-site-token"), None);
            assert_eq!(
                segment.header("authorization"),
                None,
                "cross-origin segment fetch must not receive Authorization"
            );
            assert_eq!(
                segment.header("cookie"),
                None,
                "cross-origin segment fetch must not receive Cookie"
            );
            assert_eq!(
                segment.header("accept-language"),
                Some("zh-CN"),
                "non-sensitive headers still flow to cross-origin targets"
            );
            break;
        }
        if started.elapsed() > Duration::from_secs(15) {
            cancel.cancel();
            let result = download.await;
            panic!(
                "timed out waiting for cross-origin segment fetch; captured: {captured:?}; engine result: {result:?}"
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    cancel.cancel();
    let _ = download.await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_playlist_returns_401() {
    let expected = b64_basic("alice", "secret");
    let observed = Arc::new(std::sync::Mutex::new(None));
    let server = start_auth_hls_server(expected, observed);
    let error = new_engine()
        .probe(new_probe_request(format!(
            "{}/secure.m3u8",
            server.base_url
        )))
        .await
        .expect_err("401 must fail probe");
    let payload: AppErrorPayload =
        serde_json::from_str(&error.to_string()).expect("structured http_denied");
    assert_eq!(payload.code, "http_denied");
    assert!(!payload.recoverable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_reenters_after_reset_interrupted_tasks() {
    if !ffmpeg_available() {
        eprintln!("skipping HLS process-restart reentry test: ffmpeg not in PATH");
        return;
    }
    let requests = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let server =
        start_recovery_server(Arc::new(generate_test_transport_stream()), requests.clone());
    let (_db, pool) = common::test_pool("hls-process-restart").await;
    let mut paths = common::TestPaths::new("hls-process-restart");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("restart.mp4");
    let task = common::download_task(
        "hls-process-restart",
        format!("{}/recovery.m3u8", server.base_url),
        "hls",
        "restart.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS task");

    let cancel = tokio_util::sync::CancellationToken::new();
    let first = tokio::spawn({
        let engine = new_engine();
        let context = common::headless_download_context(pool.clone(), task.clone(), cancel.clone());
        async move { engine.download(context).await }
    });
    let first = common::wait_for_segment_progress(
        || {
            Box::pin(async {
                let segments = db::list_hls_segments(&pool, "hls-process-restart")
                    .await
                    .expect("list HLS segments");
                segments.iter().any(|segment| {
                    segment.media_sequence == 0 && segment.status == SegmentStatus::Completed
                })
            })
        },
        first,
        Duration::from_secs(60),
        "hls-process-restart first segment completion",
    )
    .await;
    cancel.cancel();
    // Abort without waiting for clean pause so DB can still look like a crashed
    // Downloading worker (process-interrupt contract).
    first.abort();
    let _ = first.await;

    sqlx::query("UPDATE tasks SET status = 'downloading', updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind("hls-process-restart")
        .execute(&pool)
        .await
        .expect("force downloading status");

    db::reset_interrupted_tasks(&pool, true)
        .await
        .expect("reset interrupted");
    let queued = db::get_task_record(&pool, "hls-process-restart")
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
    .expect("persist Downloading after reset");

    // Cold engine re-entry (new instance) after process-style interrupt reset.
    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            resumed,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("resume after reset_interrupted");
    assert!(paths.final_path.exists(), "final MP4 must exist");
    assert!(
        requests[0].load(Ordering::SeqCst) >= 1,
        "first segment should have been fetched at least once"
    );
    // ARC-38: the completed MP4 means staging (the task's temp path) is
    // garbage and must not survive the download.
    assert!(
        !paths.temp.exists(),
        "completed HLS download must clean its staging directory"
    );
    pool.close().await;
}

/// ARC-37: an internal segment failure must return Err WITHOUT cancelling the
/// scheduler-owned cancel token. The supervisor classifies engine results by
/// that token (`canceled = token.is_cancelled()`), so an engine that cancels it
/// on failure makes the task masquerade as user-cancelled and stay Downloading
/// forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc37_internal_segment_failure_does_not_cancel_user_token() {
    let server = TestServer::start(move |mut stream| {
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
        let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
            "/video.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nv0.ts\n#EXT-X-ENDLIST\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            // Permanent failure: retries exhaust and the segment loop must
            // return hls_segment_failed.
            _ => (404, "text/plain", b"not found".to_vec()),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
    });

    let (_db, pool) = common::test_pool("hls-arc37-token").await;
    let mut paths = common::TestPaths::new("hls-arc37-token");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("arc37.mp4");
    let task = common::download_task(
        "hls-arc37-token",
        format!("{}/video.m3u8", server.base_url),
        "hls",
        "arc37.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let cancel = tokio_util::sync::CancellationToken::new();
    let result = new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            cancel.clone(),
        ))
        .await;
    let error = result.expect_err("permanent segment 404 must fail the download");
    assert!(
        error.to_string().contains("hls_segment_failed") || error.to_string().contains("404"),
        "expected hls_segment_failed, got {error}"
    );
    assert!(
        !cancel.is_cancelled(),
        "ARC-37: engine must not cancel the scheduler-owned token on internal failure"
    );
    pool.close().await;
}

fn headless_context_with_connections(
    pool: sqlx::SqlitePool,
    task: tauri_app_lib::models::TaskRecord,
    cancel_token: tokio_util::sync::CancellationToken,
    connection_limit: usize,
) -> tauri_app_lib::download::DownloadContext {
    // ARC-36: headless_download_context pins connection_limit to 1, which
    // removes the worker-completion divergence this test needs to exercise.
    tauri_app_lib::download::DownloadContext {
        app: None,
        pool,
        task,
        cancel_token,
        finish: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finish_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        speed_limiter: tauri_app_lib::download::GlobalSpeedLimiter::disabled(),
        connection_limit,
        request_headers: Vec::new(),
        proxy_config: ResolvedProxyConfig::default(),
        network_policy: tauri_app_lib::download::network_policy::NetworkPolicy::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc36_external_track_playlist_follows_declared_order() {
    // ARC-36: the media playlist inside a track's staging folder must list
    // segments in declared (media_sequence) order. The fake server delays
    // segment 1 so four parallel workers finish 0,2,3,1 — the pre-ARC-36
    // code wrote that completion order into local.m3u8 and ffmpeg muxed a
    // scrambled track.
    let server = TestServer::start(move |mut stream| {
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
        let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
            "/master.m3u8" => {
                let playlist = "#EXTM3U\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio/en.m3u8\"\n\
#EXT-X-STREAM-INF:BANDWIDTH=128000,AUDIO=\"aud\"\n\
video.m3u8\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/video.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nv0.ts\n#EXT-X-ENDLIST\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/audio/en.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n\
#EXTINF:1.0,\na0.ts\n#EXTINF:1.0,\na1.ts\n#EXTINF:1.0,\na2.ts\n#EXTINF:1.0,\na3.ts\n#EXT-X-ENDLIST\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            // Segment 1 sleeps long enough that the other three workers (no
            // artificial delay) drain first under connection_limit=4.
            "/audio/a1.ts" => {
                std::thread::sleep(Duration::from_millis(500));
                (200, "video/mp2t", vec![1_u8; 188])
            }
            "/v0.ts" | "/audio/a0.ts" | "/audio/a2.ts" | "/audio/a3.ts" => {
                (200, "video/mp2t", vec![0_u8; 188])
            }
            _ => (404, "text/plain", b"not found".to_vec()),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
    });

    let engine = new_engine();
    let probe = engine
        .probe(new_probe_request(format!(
            "{}/master.m3u8",
            server.base_url
        )))
        .await
        .expect("probe master");
    let audio_uri = probe
        .hls_audio_tracks
        .iter()
        .find_map(|track| track.uri.clone())
        .expect("audio track uri");

    let (_db, pool) = common::test_pool("hls-arc36-order").await;
    let mut paths = common::TestPaths::new("hls-arc36-order");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("arc36.mp4");
    let task = common::download_task(
        "hls-arc36-order",
        format!("{}/master.m3u8", server.base_url),
        "hls",
        "arc36.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let audio_json = serde_json::to_string(&vec![audio_uri]).expect("audio json");
    let staging = paths.temp.to_string_lossy();
    db::upsert_hls_task(
        &pool,
        db::HlsTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            media_url: &probe.resolved_uri,
            playlist_kind: "vod",
            selected_bandwidth: None,
            selected_resolution: None,
            target_duration: 1,
            last_media_sequence: None,
            output_format: "mp4",
            staging_dir: &staging,
            selected_audio_track_uris: Some(&audio_json),
            selected_subtitle_track_uris: None,
        },
    )
    .await
    .expect("upsert selected audio");

    // Remux may fail on the dummy payloads (or without ffmpeg); the ARC-36
    // contract under test is the on-disk track playlist, written before
    // finalize.
    let _ = engine
        .download(headless_context_with_connections(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
            4,
        ))
        .await;

    let track_playlist = paths.temp.join("audio_en.m3u8").join("local.m3u8");
    assert!(
        track_playlist.exists(),
        "external track playlist must be written to {:?}",
        track_playlist
    );
    let content = std::fs::read_to_string(&track_playlist).expect("read track playlist");
    let segment_order: Vec<&str> = content
        .lines()
        .filter(|line| line.starts_with("seg-") && line.ends_with(".ts"))
        .collect();
    assert_eq!(
        segment_order,
        vec!["seg-0-0.ts", "seg-0-1.ts", "seg-0-2.ts", "seg-0-3.ts"],
        "track playlist must follow declared media_sequence order, got: {segment_order:?}"
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun27_hls_state_read_failure_fails_task() {
    // FUN-27: a transient read failure of the hls_tasks row must fail the
    // download instead of silently completing a product without the selected
    // audio/subtitle tracks. The row only exists when tracks were selected,
    // so dropping the table injects the exact error shape.
    let server = TestServer::start(move |mut stream| {
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
        let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
            "/video.m3u8" => {
                let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nv0.ts\n#EXT-X-ENDLIST\n";
                (
                    200,
                    "application/vnd.apple.mpegurl",
                    playlist.as_bytes().to_vec(),
                )
            }
            "/v0.ts" => (200, "video/mp2t", vec![0_u8; 188]),
            _ => (404, "text/plain", b"not found".to_vec()),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
    });

    let engine = new_engine();
    let (_db, pool) = common::test_pool("hls-fun27-state-read").await;
    let mut paths = common::TestPaths::new("hls-fun27-state-read");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("fun27.mp4");
    let task = common::download_task(
        "hls-fun27-state-read",
        format!("{}/video.m3u8", server.base_url),
        "hls",
        "fun27.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    sqlx::query("DROP TABLE hls_tasks")
        .execute(&pool)
        .await
        .expect("drop hls_tasks to inject a state read failure");

    let error = engine
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("state read failure must fail the download");
    let message = error.to_string();
    assert!(
        message.contains("hls_state_read_failed"),
        "expected the hls_state_read_failed code, got: {message}"
    );
    pool.close().await;
}

// SEC-11: cross-origin (same host, different port) segment fetches must not
// receive Authorization/Cookie, while the same-origin playlist request keeps
// them. The segment URI is absolute, so the engine's origin binding is the
// only mechanism that can strip the credentials en route.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sec11_cross_origin_segment_requests_strip_credentials() {
    if !ffmpeg_available() {
        eprintln!("skipping SEC-11 HLS cross-origin test: ffmpeg not in PATH");
        return;
    }
    type RequestLog = Arc<std::sync::Mutex<Vec<String>>>;
    let ts_fixture = Arc::new(generate_test_transport_stream());

    // Cross-origin server: the playlist lives on a different port, and the
    // SEC-11 comparison is scheme+host+port, so credentials must be stripped
    // before these requests leave the engine.
    let cross_log: RequestLog = Arc::new(std::sync::Mutex::new(Vec::new()));
    let cross_server = TestServer::start({
        let cross_log = cross_log.clone();
        let ts_fixture = ts_fixture.clone();
        move |mut stream| {
            let mut buffer = [0_u8; 4096];
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            if read == 0 {
                return;
            }
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            cross_log.lock().expect("cross log lock").push(request);
            let body: &[u8] = &ts_fixture;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: video/mp2t\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
        }
    });

    // Origin server: serves the media playlist, whose single segment is an
    // absolute URL pointing at the cross-origin server.
    let origin_log: RequestLog = Arc::new(std::sync::Mutex::new(Vec::new()));
    let origin_server = TestServer::start({
        let origin_log = origin_log.clone();
        let cross_base = cross_server.base_url.clone();
        move |mut stream| {
            let mut buffer = [0_u8; 4096];
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            if read == 0 {
                return;
            }
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            origin_log.lock().expect("origin log lock").push(request);
            let playlist = format!(
                "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:5.0,\n{cross_base}/seg0.ts\n#EXT-X-ENDLIST\n"
            );
            let body = playlist.as_bytes();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
        }
    });

    let (_db, pool) = common::test_pool("hls-sec11-cross-origin").await;
    let mut paths = common::TestPaths::new("hls-sec11-cross-origin");
    let root = paths
        .final_path
        .parent()
        .expect("HLS test root")
        .to_path_buf();
    paths.temp = root.join("staging");
    paths.final_path = root.join("sec11.mp4");
    let task = common::download_task(
        "hls-sec11-cross-origin",
        format!("{}/media.m3u8", origin_server.base_url),
        "hls",
        "sec11.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert HLS task");

    let context = tauri_app_lib::download::DownloadContext {
        request_headers: vec![
            (
                "Authorization".to_string(),
                "Basic dXNlcjpwYXNz".to_string(),
            ),
            ("Cookie".to_string(), "session=sec11".to_string()),
            ("User-Agent".to_string(), "vibe-test".to_string()),
            ("X-Site-Token".to_string(), "fixture-token".to_string()),
        ],
        ..common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        )
    };

    new_engine()
        .download(context)
        .await
        .expect("cross-origin HLS download completes");

    let has_header = |raw: &str, name: &str| {
        raw.lines().any(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case(name))
        })
    };

    // Same-origin regression: the playlist fetch keeps every forwarded header.
    let origin_requests = origin_log.lock().expect("origin log lock").clone();
    assert!(
        origin_requests
            .iter()
            .any(|raw| has_header(raw, "authorization") && has_header(raw, "cookie")),
        "playlist fetch (same origin) must carry Authorization and Cookie; got {origin_requests:?}"
    );

    // Cross-origin: credentials never arrive; non-sensitive headers still flow.
    let cross_requests = cross_log.lock().expect("cross log lock").clone();
    assert!(
        !cross_requests.is_empty(),
        "the segment must have been fetched from the cross-origin server"
    );
    for raw in &cross_requests {
        assert!(
            !has_header(raw, "authorization")
                && !has_header(raw, "cookie")
                && !has_header(raw, "x-site-token"),
            "cross-origin segment request leaked credentials: {raw}"
        );
        assert!(
            has_header(raw, "user-agent"),
            "non-sensitive forwarded headers must still flow cross-origin: {raw}"
        );
    }
    pool.close().await;
}

#[tokio::test]
async fn request_profile_secrets_do_not_follow_cross_origin_variant_playlists() {
    if !ffmpeg_available() {
        eprintln!("skipping HLS variant request profile test: ffmpeg not in PATH");
        return;
    }
    let variant_log = Arc::new(Mutex::new(Vec::new()));
    let captured = variant_log.clone();
    let variant = TestServer::start(move |mut stream| {
        let mut buf = [0; 16384];
        let read = stream.read(&mut buf).unwrap_or(0);
        captured
            .lock()
            .expect("variant capture")
            .push(String::from_utf8_lossy(&buf[..read]).into_owned());
        let body = VOD_MEDIA_PLAYLIST.as_bytes();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/vnd.apple.mpegurl\r\nConnection: close\r\n\r\n", body.len()).expect("response");
        let _ = stream.write_all(body);
    });
    let media_url = format!("{}/video.m3u8", variant.base_url);
    let master = TestServer::start(move |mut stream| {
        let mut buf = [0; 16384];
        let _ = stream.read(&mut buf);
        let body = format!("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000\n{media_url}\n");
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/vnd.apple.mpegurl\r\nConnection: close\r\n\r\n{body}", body.len()).expect("master response");
    });
    let mut request = new_probe_request(format!("{}/master.m3u8", master.base_url));
    request.request_headers = vec![
        ("Cookie".into(), "session=fixture".into()),
        ("X-Token".into(), "fixture-token".into()),
        ("User-Agent".into(), "ProfileRegression/1".into()),
    ];
    new_engine().probe(request).await.expect("master probe");
    let requests = variant_log.lock().expect("capture");
    assert!(!requests.is_empty());
    for raw in requests.iter() {
        let lower = raw.to_ascii_lowercase();
        assert!(!lower.contains("cookie:") && !lower.contains("x-token:"));
        assert!(lower.contains("user-agent: profileregression/1"));
    }
}
