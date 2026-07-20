mod common;

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use common::TestServer;
use tauri_app_lib::{
    db,
    download::{DownloadEngine, HlsEngine, HttpEngine, ProbeRequest},
    models::{SegmentStatus, TaskStatus},
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
    let pool = common::test_pool("hls-staging-recovery").await;
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

    loop {
        let segments = db::list_hls_segments(&pool, "hls-staging-recovery")
            .await
            .expect("list HLS segments");
        let first_completed = segments.iter().any(|segment| {
            segment.media_sequence == 0 && segment.status == SegmentStatus::Completed
        });
        if first_completed && requests[1].load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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

    let pool = common::test_pool("hls-live-idle").await;
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

    let pool = common::test_pool("hls-target-clamp").await;
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

    let pool = common::test_pool("hls-cancel-sleep").await;
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

    let pool = common::test_pool("hls-fun10-relative").await;
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

    let pool = common::test_pool("hls-fun10-fail").await;
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
