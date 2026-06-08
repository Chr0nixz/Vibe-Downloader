use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use tauri_app_lib::{
    download::GlobalSpeedLimiter,
    download::{DirectDownloadRequest, DirectSegmentedDownloadRequest, HttpEngine},
    models::{SegmentStatus, TaskSegmentRecord},
};

const SAMPLE: &[u8] = b"Vibe Downloader HTTP regression payload.";
const LARGE_PAYLOAD_SHA256: &str =
    "f1808c3366e106973e30f4fa360e5355f36284aa0f299705ef9ee0a0d9648fc3";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_reads_headers_and_range_support() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/file", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.file_name, "sample.bin");
    assert_eq!(probe.total_size, SAMPLE.len() as i64);
    assert!(probe.supports_range);
    assert_eq!(probe.source_host, "127.0.0.1");
    assert_eq!(
        probe.content_type.as_deref(),
        Some("application/octet-stream")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_falls_back_to_get_range_when_head_is_incomplete() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/head-no-length", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.file_name, "fallback.bin");
    assert_eq!(probe.total_size, SAMPLE.len() as i64);
    assert!(probe.supports_range);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_sends_identity_accept_encoding() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/requires-identity", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.file_name, "identity.bin");
    assert!(probe.supports_range);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_allows_unknown_size_single_streams() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/unknown-size", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.total_size, 0);
    assert!(!probe.supports_range);
    assert_eq!(probe.file_name, "unknown-size.bin");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_uses_extended_file_name_sources() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");

    let content_location = engine
        .probe(&format!("{}/content-location-name", server.base_url))
        .await
        .expect("content location probe");
    let query_name = engine
        .probe(&format!(
            "{}/query-name?response-content-disposition=attachment%3B%20filename%3D%22query.zip%22",
            server.base_url
        ))
        .await
        .expect("query probe");
    let encoded_name = engine
        .probe(&format!("{}/encoded-name", server.base_url))
        .await
        .expect("encoded probe");

    assert_eq!(content_location.file_name, "report.pdf");
    assert_eq!(query_name.file_name, "query.zip");
    assert_eq!(encoded_name.file_name, "encoded name.txt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_maps_common_http_failures() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");

    let not_found = engine
        .probe(&format!("{}/status/404", server.base_url))
        .await
        .expect_err("404 should fail");
    let denied = engine
        .probe(&format!("{}/status/403", server.base_url))
        .await
        .expect_err("403 should fail");
    let limited = engine
        .probe(&format!("{}/status/429", server.base_url))
        .await
        .expect_err("429 should fail");

    let not_found: serde_json::Value = serde_json::from_str(&not_found).expect("404 payload");
    let denied: serde_json::Value = serde_json::from_str(&denied).expect("403 payload");
    let limited: serde_json::Value = serde_json::from_str(&limited).expect("429 payload");

    assert_eq!(not_found["code"], "http_not_found");
    assert_eq!(
        not_found["message"],
        "The file was not found on the server."
    );
    assert_eq!(denied["code"], "http_denied");
    assert_eq!(denied["message"], "The server denied access to this file.");
    assert_eq!(limited["code"], "server_rate_limited");
    assert_eq!(
        limited["message"],
        "The server is limiting requests. Try again later."
    );
    assert_eq!(limited["recoverable"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_writes_final_file() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("complete");

    let downloaded = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/file", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: SAMPLE.len() as i64,
                supports_range: true,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("download");

    assert_eq!(downloaded, SAMPLE.len() as i64);
    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
    assert!(!paths.temp.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_unknown_size_download_writes_final_file() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("unknown-complete");

    let downloaded = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/unknown-size", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: 0,
                supports_range: false,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("download");

    assert_eq!(downloaded, SAMPLE.len() as i64);
    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_renames_when_final_path_exists() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("final-conflict");
    let existing = b"existing user file";
    fs::write(&paths.final_path, existing).expect("seed existing final");

    let downloaded = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/file", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: SAMPLE.len() as i64,
                supports_range: true,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("download");

    let renamed = paths
        .final_path
        .parent()
        .expect("parent")
        .join("file (1).bin");
    assert_eq!(downloaded, SAMPLE.len() as i64);
    assert_eq!(
        fs::read(&paths.final_path).expect("read existing"),
        existing
    );
    assert_eq!(fs::read(renamed).expect("read renamed final"), SAMPLE);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_can_resume_from_temp_file() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("resume");
    let cancel = Arc::new(AtomicBool::new(false));

    let first_cancel = cancel.clone();
    let first = tokio::spawn({
        let engine = engine.clone();
        let request = DirectDownloadRequest {
            url: format!("{}/slow", server.base_url),
            temp_path: paths.temp.clone(),
            final_path: paths.final_path.clone(),
            total_size: slow_payload().len() as i64,
            supports_range: true,
        };
        async move { engine.download_direct(request, first_cancel).await }
    });

    tokio::time::sleep(Duration::from_millis(35)).await;
    cancel.store(true, Ordering::SeqCst);
    let partial = first.await.expect("join").expect("partial");
    assert!(partial > 0);
    assert!(partial < slow_payload().len() as i64);
    assert!(paths.temp.exists());

    engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/slow", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: slow_payload().len() as i64,
                supports_range: true,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("resume");

    assert_eq!(
        fs::read(&paths.final_path).expect("read final"),
        slow_payload()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_respects_speed_limiter() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("speed-limit");
    let started = std::time::Instant::now();

    engine
        .download_direct_with_limiter(
            DirectDownloadRequest {
                url: format!("{}/slow", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: slow_payload().len() as i64,
                supports_range: true,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(GlobalSpeedLimiter::new(Some(32 * 1024))),
        )
        .await
        .expect("limited download");

    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(
        fs::read(&paths.final_path).expect("read final"),
        slow_payload()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_resume_fails_when_range_is_unavailable() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("no-range");
    fs::write(&paths.temp, b"partial").expect("write temp");

    let error = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/no-range", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: SAMPLE.len() as i64,
                supports_range: false,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect_err("resume should fail");

    assert_eq!(
        error,
        "Resume unavailable. Restart this download from the beginning."
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_download_writes_all_ranges_to_one_file() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-complete");
    let payload = large_payload();
    let segments = direct_segments("segmented-complete", payload.len() as i64);

    let downloaded = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/large", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_range: true,
                segments,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("segmented download");

    assert_eq!(downloaded, payload.len() as i64);
    let final_bytes = fs::read(&paths.final_path).expect("read final");
    assert_eq!(sha256_hex(&final_bytes), LARGE_PAYLOAD_SHA256);
    assert_eq!(final_bytes, payload);
    assert!(!paths.temp.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_retries_transient_segment_failures() {
    std::env::set_var("VIBE_FAST_RETRY_DELAYS", "1");
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-retry");
    let payload = large_payload();

    engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/transient-segment", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_range: true,
                segments: direct_segments("segmented-retry", payload.len() as i64),
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("segmented retry");

    let final_bytes = fs::read(&paths.final_path).expect("read final");
    assert_eq!(sha256_hex(&final_bytes), LARGE_PAYLOAD_SHA256);
    std::env::remove_var("VIBE_FAST_RETRY_DELAYS");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_resume_skips_completed_ranges() {
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-resume");
    let payload = large_payload();
    let mut segments = direct_segments("segmented-resume", payload.len() as i64);
    let first_end = segments[0].range_end as usize;
    fs::write(&paths.temp, &payload[..=first_end]).expect("write completed range");
    segments[0].downloaded_until = segments[0].range_end + 1;
    segments[0].status = SegmentStatus::Completed;

    engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/large", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_range: true,
                segments,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect("resume segmented download");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_failure_does_not_rename_temp_file() {
    std::env::set_var("VIBE_FAST_RETRY_DELAYS", "1");
    let server = TestServer::start();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-failure");
    let payload = large_payload();

    let error = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/segment-error", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_range: true,
                segments: direct_segments("segmented-failure", payload.len() as i64),
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .expect_err("segment should fail");

    assert_eq!(error, "The server returned HTTP 500.");
    assert!(!paths.final_path.exists());
    std::env::remove_var("VIBE_FAST_RETRY_DELAYS");
}

struct TestServer {
    base_url: String,
    stop: Arc<AtomicBool>,
    _state: Arc<Mutex<HashMap<String, usize>>>,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let state = Arc::new(Mutex::new(HashMap::new()));
        let thread_state = state.clone();
        let (ready_tx, ready_rx) = mpsc::channel();

        thread::spawn(move || {
            listener.set_nonblocking(true).expect("nonblocking");
            let _ = ready_tx.send(());
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let state = thread_state.clone();
                        thread::spawn(move || handle_connection(stream, state));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("test server ready");

        Self {
            base_url: format!("http://{addr}"),
            stop,
            _state: state,
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
    }
}

struct TestPaths {
    temp: PathBuf,
    final_path: PathBuf,
}

impl TestPaths {
    fn new(label: &str) -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("vibe-downloader-{label}-{id}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        Self {
            temp: dir.join("file.bin.vibe-downloading"),
            final_path: dir.join("file.bin"),
        }
    }
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<HashMap<String, usize>>>) {
    let mut buffer = [0_u8; 4096];
    let Ok(read) = stream.read(&mut buffer) else {
        return;
    };
    if read == 0 {
        return;
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut lines = request.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or("/");
    let byte_range = request.lines().find_map(parse_range);
    let accept_encoding_identity = request.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("accept-encoding")
                && value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("identity"))
        })
    });

    match path {
        "/requires-identity" if !accept_encoding_identity => {
            write_response(&mut stream, 400, &[], b"identity required", false)
        }
        "/requires-identity" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "identity.bin",
            false,
        ),
        "/file" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "sample.bin",
            false,
        ),
        "/head-no-length" if method == "HEAD" => {
            write_response(&mut stream, 200, &[("Accept-Ranges", "bytes")], &[], false)
        }
        "/head-no-length" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "fallback.bin",
            false,
        ),
        "/unknown-size" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[("Content-Type", "application/octet-stream")],
        ),
        "/content-location-name" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[
                ("Content-Type", "application/pdf"),
                ("Content-Location", "/exports/report"),
            ],
        ),
        target if target.starts_with("/query-name") => {
            respond_file_without_disposition(&mut stream, method, SAMPLE, byte_range, true, false)
        }
        "/encoded-name" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[
                ("Content-Type", "application/octet-stream"),
                (
                    "Content-Disposition",
                    "attachment; filename*=UTF-8''encoded%20name.txt",
                ),
            ],
        ),
        "/transient-segment" if byte_range.is_some_and(|range| range.start > 0) => {
            let key = format!(
                "transient-{}",
                byte_range.map(|range| range.start).unwrap_or(0)
            );
            let mut state = state.lock().expect("state lock");
            let count = state.entry(key).or_insert(0);
            if *count == 0 {
                *count += 1;
                write_response(&mut stream, 500, &[], b"retry later", false);
            } else {
                drop(state);
                respond_file(
                    &mut stream,
                    method,
                    &large_payload(),
                    byte_range,
                    true,
                    "transient.bin",
                    false,
                );
            }
        }
        "/transient-segment" => respond_file(
            &mut stream,
            method,
            &large_payload(),
            byte_range,
            true,
            "transient.bin",
            false,
        ),
        "/slow" => respond_file(
            &mut stream,
            method,
            &slow_payload(),
            byte_range,
            true,
            "slow.bin",
            true,
        ),
        "/large" => respond_file(
            &mut stream,
            method,
            &large_payload(),
            byte_range,
            true,
            "large.bin",
            false,
        ),
        "/segment-error" if byte_range.is_some_and(|range| range.start > 0) => {
            write_response(&mut stream, 500, &[], b"segment failed", false)
        }
        "/segment-error" => respond_file(
            &mut stream,
            method,
            &large_payload(),
            byte_range,
            true,
            "segment-error.bin",
            false,
        ),
        "/no-range" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            None,
            false,
            "no-range.bin",
            false,
        ),
        "/status/403" => write_response(&mut stream, 403, &[], b"denied", false),
        "/status/404" => write_response(&mut stream, 404, &[], b"missing", false),
        "/status/429" => write_response(&mut stream, 429, &[], b"limited", false),
        _ => write_response(&mut stream, 404, &[], b"missing", false),
    }
}

fn respond_file(
    stream: &mut TcpStream,
    method: &str,
    payload: &[u8],
    byte_range: Option<ByteRange>,
    supports_range: bool,
    file_name: &str,
    slow: bool,
) {
    let start = byte_range
        .map(|range| range.start)
        .unwrap_or(0)
        .min(payload.len());
    let end = byte_range
        .and_then(|range| range.end)
        .unwrap_or_else(|| payload.len().saturating_sub(1))
        .min(payload.len().saturating_sub(1));
    let body = if method == "HEAD" || start > end {
        &[][..]
    } else {
        &payload[start..=end]
    };
    let status = if byte_range.is_some() && supports_range {
        206
    } else {
        200
    };
    let content_length = if method == "HEAD" {
        payload.len().to_string()
    } else {
        body.len().to_string()
    };
    let content_range = format!("bytes {start}-{end}/{}", payload.len());
    let disposition = format!("attachment; filename=\"{file_name}\"");
    let mut headers = vec![
        ("Content-Length", content_length.as_str()),
        ("Content-Type", "application/octet-stream"),
        ("Content-Disposition", disposition.as_str()),
    ];
    if supports_range {
        headers.push(("Accept-Ranges", "bytes"));
    }
    if status == 206 {
        headers.push(("Content-Range", content_range.as_str()));
    }

    write_response(stream, status, &headers, body, slow);
}

fn respond_file_without_disposition(
    stream: &mut TcpStream,
    method: &str,
    payload: &[u8],
    byte_range: Option<ByteRange>,
    supports_range: bool,
    slow: bool,
) {
    let start = byte_range
        .map(|range| range.start)
        .unwrap_or(0)
        .min(payload.len());
    let end = byte_range
        .and_then(|range| range.end)
        .unwrap_or_else(|| payload.len().saturating_sub(1))
        .min(payload.len().saturating_sub(1));
    let body = if method == "HEAD" || start > end {
        &[][..]
    } else {
        &payload[start..=end]
    };
    let status = if byte_range.is_some() && supports_range {
        206
    } else {
        200
    };
    let content_length = if method == "HEAD" {
        payload.len().to_string()
    } else {
        body.len().to_string()
    };
    let content_range = format!("bytes {start}-{end}/{}", payload.len());
    let mut headers = vec![
        ("Content-Length", content_length.as_str()),
        ("Content-Type", "application/octet-stream"),
    ];
    if supports_range {
        headers.push(("Accept-Ranges", "bytes"));
    }
    if status == 206 {
        headers.push(("Content-Range", content_range.as_str()));
    }

    write_response(stream, status, &headers, body, slow);
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
    slow: bool,
) {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let mut response = format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n");
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    if slow {
        for chunk in body.chunks(1024) {
            let _ = stream.write_all(chunk);
            let _ = stream.flush();
            thread::sleep(Duration::from_millis(10));
        }
    } else {
        let _ = stream.write_all(body);
        let _ = stream.flush();
    }
}

fn write_unknown_size_response(
    stream: &mut TcpStream,
    method: &str,
    body: &[u8],
    headers: &[(&str, &str)],
) {
    let mut response = "HTTP/1.1 200 OK\r\nConnection: close\r\n".to_string();
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

fn slow_payload() -> Vec<u8> {
    (0..65_536).map(|index| (index % 251) as u8).collect()
}

fn large_payload() -> Vec<u8> {
    (0..(16 * 1024 * 1024 + 13))
        .map(|index| (index % 251) as u8)
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn direct_segments(task_id: &str, total_size: i64) -> Vec<TaskSegmentRecord> {
    let count = 4_i64;
    let base = total_size / count;
    let remainder = total_size % count;
    let mut start = 0_i64;

    (0..count)
        .map(|index| {
            let length = base + if index < remainder { 1 } else { 0 };
            let end = start + length - 1;
            let segment = TaskSegmentRecord {
                id: format!("{task_id}-segment-{index}"),
                task_id: task_id.to_string(),
                range_start: start,
                range_end: end,
                downloaded_until: start,
                status: SegmentStatus::Pending,
                retry_count: 0,
                last_error: None,
            };
            start = end + 1;
            segment
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ByteRange {
    start: usize,
    end: Option<usize>,
}

fn parse_range(line: &str) -> Option<ByteRange> {
    let (name, value) = line.split_once(':')?;
    if !name.eq_ignore_ascii_case("range") {
        return None;
    }
    let (start, end) = value.trim().strip_prefix("bytes=")?.split_once('-')?;
    Some(ByteRange {
        start: start.parse::<usize>().ok()?,
        end: if end.is_empty() {
            None
        } else {
            Some(end.parse::<usize>().ok()?)
        },
    })
}
