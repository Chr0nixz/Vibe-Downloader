mod common;

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::Arc,
};

use common::TestServer;
use tauri_app_lib::{
    download::{DownloadEngine, HlsEngine, HttpEngine, ProbeRequest},
    proxy::ResolvedProxyConfig,
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
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/");

    let (status, content_type, body): (u16, &str, &[u8]) = match path {
        "/vod.m3u8" => (200, "application/vnd.apple.mpegurl", VOD_MEDIA_PLAYLIST.as_bytes()),
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
        "/master.m3u8" => (200, "application/vnd.apple.mpegurl", MASTER_PLAYLIST.as_bytes()),
        "/mid.m3u8" => (200, "application/vnd.apple.mpegurl", VOD_MEDIA_PLAYLIST.as_bytes()),
        "/hi.m3u8" => (200, "application/vnd.apple.mpegurl", VOD_MEDIA_PLAYLIST.as_bytes()),
        "/sample-aes.m3u8" => (200, "application/vnd.apple.mpegurl", SAMPLE_AES_PLAYLIST.as_bytes()),
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
        app: None,
        request_id: None,
    }
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
        .probe(new_probe_request(format!("{}/vod-aes.m3u8", server.base_url)))
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
        .probe(new_probe_request(format!("{}/vod-range.m3u8", server.base_url)))
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
        .probe(new_probe_request(format!("{}/master.m3u8", server.base_url)))
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
        .probe(new_probe_request(format!("{}/sample-aes.m3u8", server.base_url)))
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
        let Ok(read) = stream.read(&mut buffer) else { return };
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

// ===== E-1 idle-read timeout coverage note =====
//
// The E-1 idle-read timeout helper (`read_with_idle_timeout`) and its 4
// branches (Data/End/Error/IdleTimeout) are covered by unit tests in
// `src/download/mod.rs`. HLS and DASH both wrap `response.chunk()` with
// this helper (error codes `hls_segment_stalled` / `dash_segment_stalled`).
//
// A per-protocol stall integration test is not added here because:
// 1. The full `download()` path requires a `tauri::AppHandle` (not
//    constructible in external test binaries), so only `probe()` is
//    testable at this level.
// 2. The helper is generic over `Future<Output = Result<Option<T>, E>>`;
//    `response.chunk()` satisfies this contract. The SFTP session-level
//    stall test in `sftp_engine.rs` (`sftp_stalled_read_is_detectable_via_idle_timeout`)
//    proves the integration pattern end-to-end for the `AsyncRead::read`
//    family; HTTP chunk uses the same helper with a compatible future.
// 3. Adding `reqwest` as a dev-dependency solely for a chunk-stall test
//    would be disproportionate given the helper's existing unit coverage.
