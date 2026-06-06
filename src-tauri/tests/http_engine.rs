use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use tauri_app_lib::{
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

    assert_eq!(not_found, "The file was not found on the server.");
    assert_eq!(denied, "The server denied access to this file.");
    assert_eq!(limited, "The server is limiting requests. Try again later.");
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
}

struct TestServer {
    base_url: String,
    stop: Arc<AtomicBool>,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        thread::spawn(move || {
            listener.set_nonblocking(true).expect("nonblocking");
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread::spawn(move || handle_connection(stream));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            base_url: format!("http://{addr}"),
            stop,
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

fn handle_connection(mut stream: TcpStream) {
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

    match path {
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
