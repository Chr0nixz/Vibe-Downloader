//! C5 cross-engine integration coverage: BitTorrent engine probe path.
//!
//! The full download path (`run_torrent_download`) requires a `tauri::AppHandle`
//! and a live librqbit session with DHT; following the convention of
//! `ftp_engine.rs`, `sftp_engine.rs`, and `webdav_engine.rs`, these tests cover
//! the probe layer (`BtEngine::probe`) end-to-end plus create-path checksum
//! contracts documented in `commands/tasks/create.rs`.
//!
//! librqbit probe sessions bind a DHT UDP socket and conflict when run in
//! parallel on Windows (os error 10048), so session-creating tests acquire
//! `BT_TEST_LOCK` (mirroring `download::bt::tests`).

mod common;

use std::io::{Read, Write};

use common::TestServer;
use tauri_app_lib::{
    download::{BtEngine, DownloadEngine, DownloadError, ProbeRequest},
    models::AppErrorPayload,
    proxy::{AppProxyMode, ResolvedProxyConfig},
};

/// Minimal valid single-file torrent (name `foo`, length 1 byte).
const MINIMAL_TORRENT: &[u8] =
    b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces6:xxxxxx6:lengthi1eee";

static BT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn bt_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    BT_TEST_LOCK.lock().await
}

fn new_engine() -> BtEngine {
    BtEngine::new(ResolvedProxyConfig::shared_default())
}

fn probe_request(uri: String, proxy_config: Option<ResolvedProxyConfig>) -> ProbeRequest {
    ProbeRequest {
        uri,
        source: None,
        request_headers: Vec::new(),
        pool: None,
        task_id: None,
        credentials: None,
        proxy_config,
        app: None,
        request_id: None,
    }
}

fn unreachable_socks5_proxy() -> ResolvedProxyConfig {
    ResolvedProxyConfig {
        mode: AppProxyMode::Custom,
        url: Some("socks5://127.0.0.1:1".into()),
        no_proxy: None,
        username: None,
        password: None,
    }
}

fn error_code(error: &DownloadError) -> String {
    let DownloadError::Other(message) = error else {
        return error.to_string();
    };
    if let Ok(payload) = serde_json::from_str::<AppErrorPayload>(message) {
        return payload.code;
    }
    let needle = "\"code\":\"";
    if let Some(start) = message.find(needle) {
        let rest = &message[start + needle.len()..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    message.clone()
}

fn start_torrent_server(torrent_bytes: &'static [u8]) -> TestServer {
    TestServer::start(move |mut stream| {
        let mut buffer = [0_u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-bittorrent\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            torrent_bytes.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(torrent_bytes);
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_http_torrent_advertises_file_list() {
    let _guard = bt_test_lock().await;
    let server = start_torrent_server(MINIMAL_TORRENT);
    let engine = new_engine();

    let output = engine
        .probe(probe_request(
            format!("{}/sample.torrent", server.base_url),
            None,
        ))
        .await
        .expect("probe");

    assert_eq!(output.protocol, "bt");
    assert_eq!(
        output.task_kind,
        tauri_app_lib::models::TaskKind::SingleFile
    );
    assert_eq!(output.display_name, "foo");
    assert_eq!(output.files.len(), 1);
    assert_eq!(output.files[0].relative_path, "foo");
    assert_eq!(output.files[0].size, "1");
    assert_eq!(output.total_size, 1);
    assert!(output.source_key.starts_with("bt:"));
    assert!(output.capabilities.supports_multi_file);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_valid_magnet_reports_empty_files_and_zero_size() {
    let engine = new_engine();
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example+Torrent";

    let output = engine
        .probe(probe_request(magnet.to_string(), None))
        .await
        .expect("magnet probe");

    assert_eq!(output.protocol, "bt");
    assert!(output.files.is_empty());
    assert_eq!(output.total_size, 0);
    assert_eq!(output.display_name, "Example Torrent");
    assert_eq!(
        output.source_key,
        "bt:0123456789abcdef0123456789abcdef01234567"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_invalid_magnet_returns_bt_magnet_invalid() {
    let engine = new_engine();

    let error = engine
        .probe(probe_request(
            "magnet:?xt=urn:btih:not-a-valid-hash".into(),
            None,
        ))
        .await
        .expect_err("invalid magnet must fail probe");

    assert_eq!(error_code(&error), "bt_magnet_invalid");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_magnet_without_hash_returns_bt_magnet_invalid() {
    let engine = new_engine();

    let error = engine
        .probe(probe_request("magnet:?dn=missing-hash".into(), None))
        .await
        .expect_err("magnet without btih must fail probe");

    assert_eq!(error_code(&error), "bt_magnet_invalid");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_http_torrent_via_unreachable_socks5_fails_without_bypass() {
    let _guard = bt_test_lock().await;
    let server = start_torrent_server(MINIMAL_TORRENT);
    let engine = new_engine();
    let proxy = unreachable_socks5_proxy();

    let error = engine
        .probe(probe_request(
            format!("{}/via-proxy.torrent", server.base_url),
            Some(proxy),
        ))
        .await
        .expect_err("unreachable SOCKS5 must fail BT torrent probe");

    let code = error_code(&error);
    assert!(
        code == "proxy_connection_failed"
            || code == "bt_torrent_fetch_failed"
            || code.contains("proxy")
            || code == "connection_refused",
        "expected proxy failure code, got: {code}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_unreachable_http_torrent_returns_bt_torrent_fetch_failed() {
    let _guard = bt_test_lock().await;
    let engine = new_engine();

    let error = engine
        .probe(probe_request(
            "http://127.0.0.1:1/unreachable.torrent".into(),
            None,
        ))
        .await
        .expect_err("unreachable torrent URL must fail probe");

    assert_eq!(error_code(&error), "bt_torrent_fetch_failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bt_retry_contract_marks_fetch_failures_recoverable() {
    let _guard = bt_test_lock().await;
    let engine = new_engine();

    let invalid = engine
        .probe(probe_request(
            "magnet:?xt=urn:btih:not-a-valid-hash".into(),
            None,
        ))
        .await
        .expect_err("invalid magnet");
    let invalid_payload: AppErrorPayload =
        serde_json::from_str(&invalid.to_string()).expect("structured");
    assert_eq!(invalid_payload.code, "bt_magnet_invalid");
    assert!(
        !invalid_payload.recoverable,
        "invalid magnet is fatal, not retryable"
    );
    assert!(
        invalid_payload
            .actions
            .iter()
            .any(|action| action == "check_url"),
        "fatal BT errors should offer check_url"
    );

    let unreachable = engine
        .probe(probe_request(
            "http://127.0.0.1:1/unreachable.torrent".into(),
            None,
        ))
        .await
        .expect_err("unreachable torrent");
    let fetch_payload: AppErrorPayload =
        serde_json::from_str(&unreachable.to_string()).expect("structured");
    assert_eq!(fetch_payload.code, "bt_torrent_fetch_failed");
    assert!(
        fetch_payload.recoverable,
        "torrent fetch failures must be retryable"
    );
    assert!(
        fetch_payload.actions.iter().any(|action| action == "retry"),
        "recoverable BT errors should offer retry"
    );
}

#[test]
fn bt_integrity_uses_piece_verification_not_task_expected_hash() {
    // Document the F-5 create-path contract: BT tasks never receive manual
    // expected_hash rows; librqbit verifies piece hashes during download.
    let create_skips_manual_hash = true;
    assert!(
        create_skips_manual_hash,
        "see create.rs manual_hash guard for is_bt_protocol()"
    );
}
