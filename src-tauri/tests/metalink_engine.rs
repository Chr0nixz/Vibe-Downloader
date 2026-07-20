//! F-2/F-3/F-4 Metalink parallel mirror download — integration coverage.
//!
//! The parallel mirror download path (`download_metalink_file_parallel`,
//! `MetalinkRangeWorker`, `download_metalink_range_from_mirror`,
//! `assemble_metalink_part_files`) lives inside the private `metalink`
//! module. F-4 exposes the two AppHandle-free internal functions
//! (`download_metalink_range_from_mirror` + `assemble_metalink_part_files`)
//! via a `#[doc(hidden)] pub mod testing` so they can be exercised
//! end-to-end here without a full Tauri runtime.
//!
//! These tests cover three layers:
//!
//! 1. **Probe layer** (`MetalinkEngine::probe`): verifies the engine
//!    parses a multi-mirror Metalink4 manifest, surfaces file sizes,
//!    and correctly advertises `supports_parallel` based on whether
//!    every file has a known size.
//!
//! 2. **DB health-tracking layer** (`db::metalink`): verifies the
//!    mirror health columns added by migration 002 behave correctly —
//!    cooldown filtering, range-support flagging, and speed tracking.
//!    These are the functions the parallel dispatcher consults when
//!    deciding which mirrors are eligible for range work.
//!
//! 3. **F-4 Parallel download layer** (`download_metalink_range_from_mirror`
//!    + `assemble_metalink_part_files`): verifies true part-file resume
//!      (F-2), mpsc progress aggregation monotonicity (F-3), file assembly
//!      integrity, and mirror error handling — the exact code paths that
//!      were previously untested.

mod common;

use std::{
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use common::TestServer;
use tauri_app_lib::{
    db,
    download::{
        testing::{
            assemble_metalink_part_files, download_metalink_range_from_mirror, part_file_path,
            run_metalink_range_worker_with_failover, MetalinkWorkerProgress,
        },
        DownloadEngine, GlobalSpeedLimiter, HttpEngine, MetalinkEngine, ProbeRequest,
    },
    models::{
        task::now_iso, AppErrorPayload, ChecksumAlgorithm, HashVerificationStatus, SegmentStatus,
        TaskChecksumRecord, TaskFileRecord, TaskKind, TaskPriority, TaskRecord, TaskStatus,
    },
    proxy::{AppProxyMode, ResolvedProxyConfig},
    state_machine,
};

// --- Manifests --------------------------------------------------------------

/// A valid Metalink4 manifest with two mirrors at different priorities.
/// Both mirrors point at the test server, which serves the same payload
/// for either URL.
const MULTI_MIRROR_META4: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="payload.bin">
    <size>1048576</size>
    <url priority="1" location="us">http://127.0.0.1/mirror1/payload.bin</url>
    <url priority="2" location="de">http://127.0.0.1/mirror2/payload.bin</url>
  </file>
</metalink>
"#;

/// Manifest where one file has size 0 (unknown). This must disable
/// parallel support at the task level — the engine cannot plan byte
/// ranges for a file whose length is unknown.
const UNKNOWN_SIZE_META4: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="unknown.bin">
    <size>0</size>
    <url priority="1">http://127.0.0.1/unknown.bin</url>
  </file>
</metalink>
"#;

/// Multi-file manifest. Used to verify probe output surfaces every file
/// and aggregates the total size correctly.
const MULTI_FILE_META4: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="part1.bin">
    <size>1024</size>
    <url priority="1">http://127.0.0.1/part1.bin</url>
  </file>
  <file name="part2.bin">
    <size>2048</size>
    <url priority="1">http://127.0.0.1/part2.bin</url>
  </file>
</metalink>
"#;

/// Manifest carrying a sha-256 checksum element. Used to verify the probe
/// parses hash metadata so the download path can later verify the file.
/// The hash value below is the sha-256 of the literal string "vibe-test"
/// (32 bytes), which is irrelevant to the probe test — the probe only
/// validates the hash format, not the content.
const CHECKSUM_META4: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="hashed.bin">
    <size>9</size>
    <hash type="sha-256">9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a</hash>
    <url priority="1">http://127.0.0.1/hashed.bin</url>
  </file>
</metalink>
"#;

// --- Probe tests ------------------------------------------------------------

fn start_manifest_server() -> TestServer {
    TestServer::start(handle_manifest_connection)
}

/// Counts how many times each mirror URL was requested. Used by the
/// probe tests to assert that `probe()` only fetches the manifest, not
/// the individual mirrors.
fn start_counting_server() -> (TestServer, Arc<Mutex<Vec<String>>>) {
    let hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let hits_clone = hits.clone();
    let server = TestServer::start(move |mut stream| {
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
        {
            let mut guard = hits_clone.lock().expect("hits lock");
            guard.push(path.to_string());
        }
        let (status, content_type, body): (u16, &str, &[u8]) = match path {
            "/manifest.meta4" => (
                200,
                "application/metalink4+xml",
                MULTI_MIRROR_META4.as_bytes(),
            ),
            p if p.starts_with("/mirror1/") || p.starts_with("/mirror2/") => {
                (200, "application/octet-stream", b"payload bytes")
            }
            _ => (404, "text/plain", b"not found"),
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    });
    (server, hits)
}

fn handle_manifest_connection(mut stream: TcpStream) {
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
        "/multi-mirror.meta4" => (
            200,
            "application/metalink4+xml",
            MULTI_MIRROR_META4.as_bytes(),
        ),
        "/unknown-size.meta4" => (
            200,
            "application/metalink4+xml",
            UNKNOWN_SIZE_META4.as_bytes(),
        ),
        "/multi-file.meta4" => (
            200,
            "application/metalink4+xml",
            MULTI_FILE_META4.as_bytes(),
        ),
        "/checksum.meta4" => (200, "application/metalink4+xml", CHECKSUM_META4.as_bytes()),
        _ => (404, "text/plain", b"not found"),
    };
    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
}

fn new_engine() -> MetalinkEngine {
    MetalinkEngine::new(Arc::new(
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_parses_metalink4_manifest_with_multiple_mirrors() {
    let server = start_manifest_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/multi-mirror.meta4",
            server.base_url
        )))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "metalink");
    assert_eq!(output.task_kind, TaskKind::Manifest);
    assert_eq!(
        output.content_type.as_deref(),
        Some("application/metalink4+xml")
    );
    assert_eq!(output.files.len(), 1, "expected exactly one file entry");
    assert_eq!(output.files[0].relative_path, "payload.bin");
    assert_eq!(output.files[0].size, "1048576");

    let metalink = output
        .metalink
        .as_ref()
        .expect("probe output should carry MetalinkProbeData");
    assert_eq!(metalink.files.len(), 1);
    let file = &metalink.files[0];
    assert_eq!(file.relative_path, "payload.bin");
    assert_eq!(file.size, 1_048_576);
    assert_eq!(
        file.resources.len(),
        2,
        "expected both mirrors parsed from the manifest"
    );
    // Resources keep manifest order (priority-sorted at download time).
    assert_eq!(file.resources[0].priority, 1);
    assert_eq!(file.resources[0].location.as_deref(), Some("us"));
    assert_eq!(file.resources[1].priority, 2);
    assert_eq!(file.resources[1].location.as_deref(), Some("de"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_advertises_parallel_when_all_files_have_known_size() {
    let server = start_manifest_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/multi-mirror.meta4",
            server.base_url
        )))
        .await
        .expect("probe should succeed");

    assert!(
        output.capabilities.supports_parallel,
        "supports_parallel must be true when every file has a known size > 0"
    );
    assert!(output.capabilities.supports_resume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_disables_parallel_when_any_file_has_unknown_size() {
    let server = start_manifest_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/unknown-size.meta4",
            server.base_url
        )))
        .await
        .expect("probe should succeed");

    assert!(
        !output.capabilities.supports_parallel,
        "supports_parallel must be false when any file has size 0"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_aggregates_multi_file_metadata() {
    let server = start_manifest_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/multi-file.meta4",
            server.base_url
        )))
        .await
        .expect("probe should succeed");

    assert_eq!(
        output.files.len(),
        2,
        "expected both files surfaced by probe"
    );
    assert_eq!(output.total_size, 1024 + 2048);
    assert!(output.capabilities.supports_multi_file);
    assert!(
        output.capabilities.supports_parallel,
        "all files have known sizes -> parallel eligible"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_does_not_fetch_mirror_urls() {
    // `probe()` must only fetch the manifest itself. Pulling mirror URLs
    // during probe would defeat the purpose of Metalink (deferred mirror
    // selection at download time, after probe returns).
    let (server, hits) = start_counting_server();
    let engine = new_engine();

    let _ = engine
        .probe(new_probe_request(format!(
            "{}/manifest.meta4",
            server.base_url
        )))
        .await
        .expect("probe should succeed");

    let hits = hits.lock().expect("hits lock").clone();
    assert_eq!(
        hits.len(),
        1,
        "probe must only fetch the manifest, got: {hits:?}"
    );
    assert_eq!(hits[0], "/manifest.meta4");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_non_xml_manifest() {
    // Server returns a 200 with a non-XML body. The XML parser must
    // surface a metalink_invalid_manifest error rather than panicking.
    let server = TestServer::start(|mut stream| {
        let mut buffer = [0_u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let body = b"this is not xml";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/metalink4+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    });
    let engine = new_engine();

    let error = engine
        .probe(new_probe_request(format!(
            "{}/garbage.meta4",
            server.base_url
        )))
        .await
        .expect_err("non-XML manifest should fail probe");

    let message = error.to_string();
    assert!(
        message.contains("metalink_invalid_manifest"),
        "expected metalink_invalid_manifest in error, got: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_manifest_url_returns_404() {
    // Failure path: the manifest URL returns 404. The probe must surface a
    // typed error rather than panicking.
    let server = start_manifest_server();
    let engine = new_engine();

    let result = engine
        .probe(new_probe_request(format!(
            "{}/missing.meta4",
            server.base_url
        )))
        .await;

    assert!(
        result.is_err(),
        "probe should fail when the manifest URL returns 404"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_manifest_url_returns_500() {
    // Failure path: the manifest URL returns 500. The probe must surface a
    // typed error rather than retrying or panicking.
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
            "{}/broken.meta4",
            server.base_url
        )))
        .await;

    assert!(
        result.is_err(),
        "probe should fail when the manifest URL returns 500"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_parses_checksum_metadata_from_manifest() {
    // Checksum metadata: the manifest carries a `<hash type="sha-256">`
    // element. The probe must surface it on the parsed MetalinkFile so the
    // download path can later verify the file integrity.
    let server = start_manifest_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!(
            "{}/checksum.meta4",
            server.base_url
        )))
        .await
        .expect("probe should succeed");

    let metalink = output
        .metalink
        .as_ref()
        .expect("probe output should carry MetalinkProbeData");
    assert_eq!(metalink.files.len(), 1);
    let file = &metalink.files[0];
    assert_eq!(file.relative_path, "hashed.bin");
    assert_eq!(file.size, 9);
    assert_eq!(
        file.checksums.len(),
        1,
        "expected exactly one checksum parsed from the manifest"
    );
    let checksum = &file.checksums[0];
    assert_eq!(
        checksum.algorithm,
        tauri_app_lib::models::ChecksumAlgorithm::Sha256
    );
    assert_eq!(
        checksum.value,
        "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"
    );
}

// --- DB health-tracking tests ----------------------------------------------

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-metalink-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

/// Seed a task + file + N mirror resources. Returns the file_id and the
/// list of resource ids, in insertion order.
async fn seed_mirrors(
    pool: &sqlx::SqlitePool,
    label: &str,
    mirror_urls: &[&str],
) -> (String, Vec<String>) {
    let now = now_iso();
    let task_id = format!("metalink-test-task-{label}");
    let task = TaskRecord {
        id: task_id.clone(),
        url: "https://example.com/manifest.meta4".to_string(),
        final_url: Some("https://example.com/manifest.meta4".to_string()),
        protocol: "metalink".to_string(),
        task_kind: TaskKind::Manifest,
        file_name: "payload.bin".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 0,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("metalink-test-{label}"),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: Some("Queued".to_string()),
        error_message: None,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.clone(),
        updated_at: now,
        files_version: 0,
    };
    db::insert_task_record(pool, &task)
        .await
        .expect("insert task");

    let file_id = format!("metalink-test-file-{label}");
    db::insert_task_file_record(
        pool,
        &TaskFileRecord {
            id: file_id.clone(),
            task_id: task_id.clone(),
            relative_path: "payload.bin".to_string(),
            file_name: "payload.bin".to_string(),
            save_dir: std::env::temp_dir().to_string_lossy().to_string(),
            temp_path: Some(
                std::env::temp_dir()
                    .join("payload.bin.tmp")
                    .to_string_lossy()
                    .to_string(),
            ),
            final_path: Some(
                std::env::temp_dir()
                    .join("payload.bin")
                    .to_string_lossy()
                    .to_string(),
            ),
            total_size: 0,
            downloaded_bytes: 0,
            status: TaskStatus::Queued,
            selected: true,
            content_type: None,
        },
    )
    .await
    .expect("insert task_file");

    let mut resource_ids = Vec::with_capacity(mirror_urls.len());
    for (index, url) in mirror_urls.iter().enumerate() {
        let resource_id = format!("metalink-test-resource-{label}-{index}");
        db::insert_metalink_resource(
            pool,
            db::MetalinkResourceInsert {
                id: &resource_id,
                task_id: &task_id,
                file_id: &file_id,
                url,
                priority: index as i64,
                location: None,
            },
        )
        .await
        .expect("insert metalink_resource");
        resource_ids.push(resource_id);
    }

    (file_id, resource_ids)
}

#[tokio::test]
async fn list_healthy_mirrors_returns_all_initial_mirrors_in_priority_order() {
    let pool = test_pool("healthy-initial").await;
    let (file_id, resource_ids) = seed_mirrors(
        &pool,
        "healthy-initial",
        &[
            "http://mirror-a.example.com/file.bin",
            "http://mirror-b.example.com/file.bin",
            "http://mirror-c.example.com/file.bin",
        ],
    )
    .await;

    let healthy = db::list_healthy_mirrors_for_file(&pool, &file_id)
        .await
        .expect("list healthy");
    assert_eq!(healthy.len(), 3, "all three mirrors should be eligible");
    assert_eq!(healthy[0].id, resource_ids[0], "priority 0 first");
    assert_eq!(healthy[1].id, resource_ids[1], "priority 1 second");
    assert_eq!(healthy[2].id, resource_ids[2], "priority 2 third");
    assert!(
        healthy.iter().all(|m| m.supports_range),
        "all fresh mirrors advertise Range support"
    );

    pool.close().await;
}

#[tokio::test]
async fn list_healthy_mirrors_excludes_those_in_cooldown() {
    let pool = test_pool("cooldown").await;
    let (file_id, resource_ids) = seed_mirrors(
        &pool,
        "cooldown",
        &[
            "http://mirror-a.example.com/file.bin",
            "http://mirror-b.example.com/file.bin",
        ],
    )
    .await;

    // Mark the first mirror as failed — this stamps a 30s cooldown.
    db::mark_metalink_resource_failed(&pool, &resource_ids[0], "transient network error")
        .await
        .expect("mark failed");

    let healthy = db::list_healthy_mirrors_for_file(&pool, &file_id)
        .await
        .expect("list healthy");
    assert_eq!(
        healthy.len(),
        1,
        "cooled-down mirror should be filtered out, got: {healthy:?}"
    );
    assert_eq!(healthy[0].id, resource_ids[1]);

    // Verify the failed mirror retains its cooldown stamp and attempt time.
    let all = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list all");
    let failed = all
        .iter()
        .find(|r| r.id == resource_ids[0])
        .expect("failed mirror present");
    assert!(
        failed.cooldown_until.is_some(),
        "cooldown_until must be set"
    );
    assert!(
        failed.last_attempt_at.is_some(),
        "last_attempt_at must be set"
    );
    assert_eq!(failed.failure_count, 1);
    assert_eq!(failed.status, "failed");

    pool.close().await;
}

#[tokio::test]
async fn list_healthy_mirrors_excludes_unsupported_range_mirrors() {
    let pool = test_pool("no-range").await;
    let (file_id, resource_ids) = seed_mirrors(
        &pool,
        "no-range",
        &[
            "http://mirror-a.example.com/file.bin",
            "http://mirror-b.example.com/file.bin",
        ],
    )
    .await;

    // Simulate a 416 from the first mirror: the engine would call this
    // to flag it as not supporting Range requests.
    db::mark_mirror_unsupported_range(&pool, &resource_ids[0])
        .await
        .expect("mark unsupported");

    let healthy = db::list_healthy_mirrors_for_file(&pool, &file_id)
        .await
        .expect("list healthy");
    assert_eq!(
        healthy.len(),
        1,
        "Range-unsupported mirror must be filtered out"
    );
    assert_eq!(healthy[0].id, resource_ids[1]);

    // The unsupported mirror is still returned by the broad `for_file`
    // query so the serial full-file fallback path can use it.
    let all = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list all");
    let unsupported = all
        .iter()
        .find(|r| r.id == resource_ids[0])
        .expect("unsupported mirror still listed in broad query");
    assert!(!unsupported.supports_range);

    pool.close().await;
}

#[tokio::test]
async fn set_metalink_mirror_cooldown_overrides_default_window() {
    let pool = test_pool("explicit-cooldown").await;
    let (file_id, resource_ids) = seed_mirrors(
        &pool,
        "explicit-cooldown",
        &["http://mirror-a.example.com/file.bin"],
    )
    .await;

    // Set a cooldown 1 hour in the future — far beyond the default 30s.
    let future = now_iso_plus_hours(1);
    db::set_metalink_mirror_cooldown(&pool, &resource_ids[0], &future)
        .await
        .expect("set cooldown");

    let healthy = db::list_healthy_mirrors_for_file(&pool, &file_id)
        .await
        .expect("list healthy");
    assert!(
        healthy.is_empty(),
        "mirror with explicit future cooldown must be filtered out"
    );

    let all = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list all");
    assert_eq!(all[0].cooldown_until.as_deref(), Some(future.as_str()));

    pool.close().await;
}

#[tokio::test]
async fn mark_metalink_resource_completed_clears_cooldown() {
    let pool = test_pool("complete-clears").await;
    let (file_id, resource_ids) = seed_mirrors(
        &pool,
        "complete-clears",
        &["http://mirror-a.example.com/file.bin"],
    )
    .await;

    // Stamp a cooldown, then mark the mirror as completed.
    db::mark_metalink_resource_failed(&pool, &resource_ids[0], "transient")
        .await
        .expect("mark failed");
    db::mark_metalink_resource_completed(&pool, &resource_ids[0])
        .await
        .expect("mark completed");

    let all = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list all");
    let completed = &all[0];
    assert_eq!(completed.status, "completed");
    assert!(
        completed.cooldown_until.is_none(),
        "completion must clear the cooldown window"
    );
    assert!(
        completed.last_attempt_at.is_some(),
        "last_attempt_at should survive completion"
    );

    // Completed mirrors are filtered out by `list_healthy_mirrors_for_file`
    // so they are not re-dispatched by the parallel worker loop.
    let healthy = db::list_healthy_mirrors_for_file(&pool, &file_id)
        .await
        .expect("list healthy");
    assert!(
        healthy.is_empty(),
        "completed mirror must not be re-dispatched"
    );

    pool.close().await;
}

#[tokio::test]
async fn update_mirror_speed_persists_observed_value() {
    let pool = test_pool("speed").await;
    let (file_id, resource_ids) =
        seed_mirrors(&pool, "speed", &["http://mirror-a.example.com/file.bin"]).await;

    db::update_mirror_speed(&pool, &resource_ids[0], 5_242_880)
        .await
        .expect("update speed");

    let all = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list all");
    assert_eq!(
        all[0].avg_speed_bps, 5_242_880,
        "avg_speed_bps should reflect the observed value"
    );

    pool.close().await;
}

#[tokio::test]
async fn mark_metalink_resource_attempted_stamps_last_attempt_without_failing() {
    let pool = test_pool("attempted").await;
    let (file_id, resource_ids) = seed_mirrors(
        &pool,
        "attempted",
        &["http://mirror-a.example.com/file.bin"],
    )
    .await;

    // The engine calls `mark_metalink_resource_attempted` right before
    // sending a Range request so subsequent worker iterations see the
    // mirror as in-flight. It must NOT increment failure_count or set
    // a cooldown.
    db::mark_metalink_resource_attempted(&pool, &resource_ids[0])
        .await
        .expect("mark attempted");

    let all = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list all");
    let mirror = &all[0];
    assert!(
        mirror.last_attempt_at.is_some(),
        "last_attempt_at should be set"
    );
    assert!(
        mirror.cooldown_until.is_none(),
        "attempted must NOT set a cooldown"
    );
    assert_eq!(
        mirror.failure_count, 0,
        "attempted must NOT increment failure_count"
    );
    assert_ne!(mirror.status, "failed", "attempted must NOT mark as failed");

    // The mirror is still eligible for the parallel path.
    let healthy = db::list_healthy_mirrors_for_file(&pool, &file_id)
        .await
        .expect("list healthy");
    assert_eq!(healthy.len(), 1, "attempted mirror is still healthy");

    pool.close().await;
}

#[tokio::test]
async fn list_healthy_mirrors_orders_by_priority_then_failure_count_then_speed() {
    // Mirror B has lower priority number (better) than A, but A has a
    // higher observed speed. The expected ordering is:
    //   1. priority ASC wins first -> B (priority 0)
    //   2. then A (priority 1)
    // The speed tiebreaker only kicks in when priority is equal.
    let pool = test_pool("ordering").await;
    let (_file_id, resource_ids) = seed_mirrors(
        &pool,
        "ordering",
        &[
            "http://mirror-a.example.com/file.bin", // priority 0
            "http://mirror-b.example.com/file.bin", // priority 1
            "http://mirror-c.example.com/file.bin", // priority 2
        ],
    )
    .await;

    // Give the lowest-priority mirror the highest observed speed — it
    // must NOT jump ahead of higher-priority mirrors.
    db::update_mirror_speed(&pool, &resource_ids[2], 10_000_000)
        .await
        .expect("speed c");
    db::update_mirror_speed(&pool, &resource_ids[0], 100_000)
        .await
        .expect("speed a");

    // Bump failure_count on the second mirror — it should still come
    // second because priority dominates.
    db::mark_metalink_resource_failed(&pool, &resource_ids[1], "first failure")
        .await
        .expect("fail b");
    // Wait briefly so the cooldown window elapses, making B eligible again.
    thread::sleep(Duration::from_millis(1100));
    // Override cooldown to the past so B is re-included.
    let past = now_iso_plus_hours(-1);
    db::set_metalink_mirror_cooldown(&pool, &resource_ids[1], &past)
        .await
        .expect("clear cooldown");

    let healthy = db::list_healthy_mirrors_for_file(&pool, &_file_id)
        .await
        .expect("list healthy");
    assert_eq!(healthy.len(), 3, "all three mirrors should be eligible");
    assert_eq!(
        healthy[0].id, resource_ids[0],
        "priority 0 first despite slower observed speed"
    );
    assert_eq!(
        healthy[1].id, resource_ids[1],
        "priority 1 second despite higher failure_count"
    );
    assert_eq!(healthy[2].id, resource_ids[2], "priority 2 third");

    // Failure_count on the second mirror should be preserved.
    assert_eq!(healthy[1].failure_count, 1);

    pool.close().await;
}

// --- Helpers ----------------------------------------------------------------

fn now_iso_plus_hours(hours: i64) -> String {
    use chrono::Utc;
    let now = Utc::now();
    let adjusted = now + chrono::Duration::hours(hours);
    adjusted.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

// --- F-4: Parallel mirror download end-to-end tests -------------------------

/// Fake HTTP server that serves `payload`, honors Range requests, and logs
/// every received Range header into `range_log`. If `error_status` is set,
/// responds with that status instead of the payload. When `slow` is true,
/// drips the body 1KB/10ms so progress messages fire during download.
fn start_mirror_server(
    payload: Arc<Vec<u8>>,
    range_log: Arc<Mutex<Vec<String>>>,
    error_status: Option<u16>,
    slow: bool,
) -> TestServer {
    TestServer::start(move |mut stream| {
        let payload = payload.clone();
        let log = range_log.clone();
        let err = error_status;
        let slow_flag = slow;
        let mut buffer = [0u8; 8192];
        let Some(request) = common::http::read_request_text(&mut stream, &mut buffer) else {
            return;
        };
        let request_line = request.lines().next().unwrap_or_default();
        let method = request_line.split_whitespace().next().unwrap_or("GET");
        let range_header = common::http::extract_header(&request, "range")
            .unwrap_or("")
            .to_string();
        {
            let mut guard = log.lock().expect("range log");
            guard.push(range_header);
        }
        if let Some(status) = err {
            let body = b"server error";
            let response = format!(
                "HTTP/1.1 {status} Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            return;
        }
        let byte_range = common::http::extract_byte_range(&request);
        common::http::respond_file(
            &mut stream,
            method,
            &payload,
            byte_range,
            true,
            "payload.bin",
            slow_flag,
        );
    })
}

/// Build a reqwest client with no redirect following (fake server is single-hop).
fn test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("reqwest client")
}

/// Unique temp path for a part file, avoiding collisions between tests.
fn unique_temp_path(label: &str) -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("vibe-f4-{label}-{id}.bin.tmp"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn f4_parallel_range_download_assembles_complete_file() {
    // 30-byte payload split across 3 workers (ranges 0-9, 10-19, 20-29).
    let payload: Vec<u8> = (0..30u8).collect();
    let payload = Arc::new(payload);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server(payload.clone(), range_log, None, false);

    let pool = test_pool("f4-assemble").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, _resource_ids) = seed_mirrors(&pool, "f4-assemble", &[&mirror_url]).await;
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors");
    let mirror = &mirrors[0];
    let task_id = "metalink-test-task-f4-assemble".to_string();

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("assemble");

    let ranges: [(u64, u64); 3] = [(0, 9), (10, 19), (20, 29)];
    for (index, (range_start, range_end)) in ranges.iter().enumerate() {
        let part_path = part_file_path(&temp_path, index);
        let (progress_tx, _progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();
        let downloaded = download_metalink_range_from_mirror(
            &pool,
            &task_id,
            &client,
            &[],
            &speed_limiter,
            &cancel_token,
            index,
            &progress_tx,
            *range_start,
            *range_end,
            mirror,
            &part_path,
        )
        .await
        .expect("range download");
        assert_eq!(downloaded, 10, "worker {index} should download 10 bytes");
    }

    assemble_metalink_part_files(&temp_path, 3)
        .await
        .expect("assemble");

    let assembled = tokio::fs::read(&temp_path).await.expect("read assembled");
    assert_eq!(assembled, *payload, "assembled file must match original");

    let _ = tokio::fs::remove_file(&temp_path).await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn f4_parallel_download_progress_is_monotonic() {
    // 200KB payload dripped at 10ms/KB (~2s) so multiple 300ms-throttled
    // progress messages fire during the download.
    let payload = Arc::new(vec![0xAB_u8; 200 * 1024]);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server(payload.clone(), range_log, None, true);

    let pool = test_pool("f4-monotonic").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, _resource_ids) = seed_mirrors(&pool, "f4-monotonic", &[&mirror_url]).await;
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors");
    let mirror = &mirrors[0];
    let task_id = "metalink-test-task-f4-monotonic".to_string();

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("monotonic");
    let part_path = part_file_path(&temp_path, 0);

    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();
    let downloaded = download_metalink_range_from_mirror(
        &pool,
        &task_id,
        &client,
        &[],
        &speed_limiter,
        &cancel_token,
        0,
        &progress_tx,
        0,
        (200 * 1024 - 1) as u64,
        mirror,
        &part_path,
    )
    .await
    .expect("download");
    drop(progress_tx);

    let mut messages = Vec::new();
    while let Some(msg) = progress_rx.recv().await {
        messages.push(msg);
    }
    assert!(
        messages.len() >= 2,
        "expected at least 2 progress messages, got {}",
        messages.len()
    );
    let mut prev: i64 = 0;
    for msg in &messages {
        assert!(
            msg.downloaded >= prev,
            "progress must be monotonic non-decreasing: prev={prev}, got={}",
            msg.downloaded
        );
        prev = msg.downloaded;
    }
    assert_eq!(downloaded, (200 * 1024) as i64);

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn f4_parallel_download_resumes_from_existing_part_files() {
    // 10-byte payload, range 0-9. Pre-write 5 bytes to the part file.
    // F-2: the function must skip cleanup, append, and send Range: bytes=5-9.
    let payload: Vec<u8> = (0..10u8).collect();
    let payload = Arc::new(payload);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server(payload.clone(), range_log.clone(), None, false);

    let pool = test_pool("f4-resume").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, _resource_ids) = seed_mirrors(&pool, "f4-resume", &[&mirror_url]).await;
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors");
    let mirror = &mirrors[0];
    let task_id = "metalink-test-task-f4-resume".to_string();

    // FUN-09: same-mirror resume requires persisted validators (or a primary
    // checksum). Seed an ETag so the pre-written part is treated as trusted.
    db::update_metalink_resource_validators(&pool, &mirror.id, Some("\"resume-v1\""), None)
        .await
        .expect("seed validators");
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("reload mirrors");
    let mirror = &mirrors[0];

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("resume");
    let part_path = part_file_path(&temp_path, 0);

    // Pre-write 5 bytes to the part file (simulate partial download).
    tokio::fs::write(&part_path, &payload[0..5])
        .await
        .expect("pre-write part");

    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();
    let downloaded = download_metalink_range_from_mirror(
        &pool,
        &task_id,
        &client,
        &[],
        &speed_limiter,
        &cancel_token,
        0,
        &progress_tx,
        0,
        9,
        mirror,
        &part_path,
    )
    .await
    .expect("resume download");

    // F-2: return value includes pre-existing bytes (5 + 5 = 10).
    assert_eq!(downloaded, 10, "resume should report full range size");

    // F-2: part file must be 10 bytes (5 pre-existing + 5 downloaded).
    let part_data = tokio::fs::read(&part_path).await.expect("read part");
    assert_eq!(part_data.len(), 10, "part file must be complete");
    assert_eq!(part_data, *payload, "part file content must match payload");

    // F-2: server must have received Range: bytes=5-9 (offset by 5).
    let ranges = range_log.lock().expect("range log").clone();
    assert_eq!(ranges.len(), 1, "expected exactly 1 request");
    assert!(
        ranges[0].contains("bytes=5-9"),
        "expected Range: bytes=5-9 (offset by 5 pre-existing bytes), got: {}",
        ranges[0]
    );

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn f4_parallel_download_returns_error_on_mirror_failure() {
    // Mirror returns 500. The function must surface an error so the
    // MetalinkRangeWorker can pull the next mirror from the failover queue.
    let payload = Arc::new(vec![0u8; 10]);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server(payload, range_log, Some(500), false);

    let pool = test_pool("f4-error").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, _resource_ids) = seed_mirrors(&pool, "f4-error", &[&mirror_url]).await;
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors");
    let mirror = &mirrors[0];
    let task_id = "metalink-test-task-f4-error".to_string();

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("error");
    let part_path = part_file_path(&temp_path, 0);

    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();
    let result = download_metalink_range_from_mirror(
        &pool,
        &task_id,
        &client,
        &[],
        &speed_limiter,
        &cancel_token,
        0,
        &progress_tx,
        0,
        9,
        mirror,
        &part_path,
    )
    .await;

    let error = result.expect_err("500 mirror must surface an error");
    assert!(
        error.contains("500") || error.contains("returned") || error.contains("http"),
        "error should mention the status, got: {error}"
    );

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

// --- G-1: MetalinkRangeWorker failover orchestration ------------------------

/// G-1: When the primary mirror returns 500, the MetalinkRangeWorker must
/// exhaust retries on it, mark it failed in the DB, then pull the next
/// mirror from the failover queue and complete the download from the
/// healthy mirror. This exercises the full failover orchestration path
/// (`download_metalink_file_parallel` → `MetalinkRangeWorker::run`) that
/// was previously unreachable without an `AppHandle`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn g1_metalink_worker_failovers_to_healthy_mirror() {
    let payload: Vec<u8> = (0..20u8).collect();
    let payload_arc = Arc::new(payload.clone());

    // Server 1: always returns 500.
    let failing_server = start_mirror_server(
        payload_arc.clone(),
        Arc::new(Mutex::new(Vec::new())),
        Some(500),
        false,
    );
    // Server 2: serves the payload correctly.
    let healthy_server = start_mirror_server(
        payload_arc.clone(),
        Arc::new(Mutex::new(Vec::new())),
        None,
        false,
    );

    let pool = test_pool("g1-failover").await;

    let mirror_urls = [
        format!("{}/payload.bin", failing_server.base_url),
        format!("{}/payload.bin", healthy_server.base_url),
    ];
    let mirror_urls_ref: Vec<&str> = mirror_urls.iter().map(|s| s.as_str()).collect();
    let (file_id, _resource_ids) = seed_mirrors(&pool, "g1-failover", &mirror_urls_ref).await;

    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors");
    assert_eq!(mirrors.len(), 2, "expected 2 mirrors seeded");

    let primary_mirror = mirrors[0].clone();
    let failover_mirror = mirrors[1].clone();
    let primary_mirror_id = primary_mirror.id.clone();

    let task_id = "metalink-test-task-g1-failover".to_string();
    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("g1-failover");
    let part_path = part_file_path(&temp_path, 0);
    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();

    let result = run_metalink_range_worker_with_failover(
        pool.clone(),
        task_id,
        file_id.clone(),
        client,
        vec![],
        speed_limiter,
        cancel_token,
        0,
        0,
        19, // range_end = payload_size - 1
        primary_mirror,
        vec![failover_mirror],
        part_path.clone(),
        progress_tx,
    )
    .await;

    let downloaded = result.expect("worker should failover to healthy mirror and succeed");
    assert_eq!(
        downloaded, 20,
        "should download all 20 bytes from failover mirror"
    );

    // The part file must contain the correct payload data.
    let part_data = tokio::fs::read(&part_path).await.expect("read part file");
    assert_eq!(
        part_data,
        (0..20u8).collect::<Vec<u8>>(),
        "part file data should match payload"
    );

    // The primary mirror must be marked as 'failed' in the DB.
    let resources = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors after failover");
    let failed_mirror = resources
        .iter()
        .find(|r| r.id == primary_mirror_id)
        .expect("primary mirror should still exist");
    assert_eq!(
        failed_mirror.status, "failed",
        "primary mirror should be marked as failed, got status: {}",
        failed_mirror.status
    );
    assert!(
        failed_mirror.failure_count > 0,
        "primary mirror should have failure_count > 0, got: {}",
        failed_mirror.failure_count
    );

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

/// G-1: When all mirrors fail (primary + all failover mirrors), the worker
/// must return an error rather than hanging or silently succeeding.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn g1_metalink_worker_returns_error_when_all_mirrors_fail() {
    let payload = Arc::new(vec![0u8; 10]);

    // Both servers return 500.
    let failing_server_1 = start_mirror_server(
        payload.clone(),
        Arc::new(Mutex::new(Vec::new())),
        Some(500),
        false,
    );
    let failing_server_2 = start_mirror_server(
        payload.clone(),
        Arc::new(Mutex::new(Vec::new())),
        Some(500),
        false,
    );

    let pool = test_pool("g1-all-fail").await;

    let mirror_urls = [
        format!("{}/payload.bin", failing_server_1.base_url),
        format!("{}/payload.bin", failing_server_2.base_url),
    ];
    let mirror_urls_ref: Vec<&str> = mirror_urls.iter().map(|s| s.as_str()).collect();
    let (file_id, _resource_ids) = seed_mirrors(&pool, "g1-all-fail", &mirror_urls_ref).await;

    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors");
    assert_eq!(mirrors.len(), 2);

    let primary_mirror = mirrors[0].clone();
    let failover_mirror = mirrors[1].clone();

    let task_id = "metalink-test-task-g1-all-fail".to_string();
    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("g1-all-fail");
    let part_path = part_file_path(&temp_path, 0);
    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();

    let result = run_metalink_range_worker_with_failover(
        pool.clone(),
        task_id,
        file_id.clone(),
        client,
        vec![],
        speed_limiter,
        cancel_token,
        0,
        0,
        9,
        primary_mirror,
        vec![failover_mirror],
        part_path.clone(),
        progress_tx,
    )
    .await;

    let error = result.expect_err("worker should return error when all mirrors fail");
    assert!(
        error.contains("exhausted") || error.contains("mirrors"),
        "error should mention mirror exhaustion, got: {error}"
    );

    // Both mirrors should be marked as failed.
    let resources = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list mirrors after all fail");
    for resource in &resources {
        assert_eq!(
            resource.status, "failed",
            "all mirrors should be marked as failed, got: {} for {}",
            resource.status, resource.id
        );
    }

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun09_same_mirror_resume_persists_validators() {
    let payload: Vec<u8> = (0..10u8).collect();
    let payload = Arc::new(payload);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server_with_etag(payload.clone(), range_log.clone(), "\"etag-1\"");

    let pool = test_pool("fun09-validators").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, resource_ids) = seed_mirrors(&pool, "fun09-validators", &[&mirror_url]).await;
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list");
    let mirror = &mirrors[0];

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("fun09-validators");
    let part_path = part_file_path(&temp_path, 0);
    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();

    download_metalink_range_from_mirror(
        &pool,
        "metalink-test-task-fun09-validators",
        &client,
        &[],
        &speed_limiter,
        &cancel_token,
        0,
        &progress_tx,
        0,
        9,
        mirror,
        &part_path,
    )
    .await
    .expect("download");

    let updated = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("reload")
        .into_iter()
        .find(|row| row.id == resource_ids[0])
        .expect("resource");
    assert_eq!(updated.etag.as_deref(), Some("\"etag-1\""));

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun09_cross_mirror_without_checksum_restarts_part() {
    let payload: Vec<u8> = (0..10u8).collect();
    let payload = Arc::new(payload);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server(payload.clone(), range_log.clone(), None, false);

    let pool = test_pool("fun09-cross-nohash").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, _) = seed_mirrors(&pool, "fun09-cross-nohash", &[&mirror_url]).await;
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list");
    let mirror = &mirrors[0];

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("fun09-cross-nohash");
    let part_path = part_file_path(&temp_path, 0);
    tokio::fs::write(&part_path, &payload[0..5])
        .await
        .expect("pre-write");

    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();
    let downloaded = download_metalink_range_from_mirror(
        &pool,
        "metalink-test-task-fun09-cross-nohash",
        &client,
        &[],
        &speed_limiter,
        &cancel_token,
        0,
        &progress_tx,
        0,
        9,
        mirror,
        &part_path,
    )
    .await
    .expect("restart download");

    assert_eq!(downloaded, 10);
    let ranges = range_log.lock().expect("range log").clone();
    assert!(
        ranges.iter().any(|r| r.contains("bytes=0-9")),
        "without checksum/validators, part must restart from 0; got {ranges:?}"
    );
    let part_data = tokio::fs::read(&part_path).await.expect("read part");
    assert_eq!(part_data, *payload);

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun09_cross_mirror_with_checksum_allows_resume() {
    let payload: Vec<u8> = (0..10u8).collect();
    let payload = Arc::new(payload);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server(payload.clone(), range_log.clone(), None, false);

    let pool = test_pool("fun09-cross-hash").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, _) = seed_mirrors(&pool, "fun09-cross-hash", &[&mirror_url]).await;
    let now = now_iso();
    db::insert_task_checksum_record(
        &pool,
        &TaskChecksumRecord {
            id: "fun09-cross-hash-ck".to_string(),
            task_id: "metalink-test-task-fun09-cross-hash".to_string(),
            file_id: Some(file_id.clone()),
            algorithm: ChecksumAlgorithm::Sha256,
            expected_hash: "ab".repeat(32),
            actual_hash: None,
            status: HashVerificationStatus::Pending,
            source_kind: "metalink".to_string(),
            source_url: None,
            source_label: None,
            is_primary: true,
            weak: false,
            error_message: None,
            discovered_at: Some(now.clone()),
            verified_at: None,
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await
    .expect("insert checksum");

    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list");
    let mirror = &mirrors[0];

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("fun09-cross-hash");
    let part_path = part_file_path(&temp_path, 0);
    tokio::fs::write(&part_path, &payload[0..5])
        .await
        .expect("pre-write");

    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();
    let downloaded = download_metalink_range_from_mirror(
        &pool,
        "metalink-test-task-fun09-cross-hash",
        &client,
        &[],
        &speed_limiter,
        &cancel_token,
        0,
        &progress_tx,
        0,
        9,
        mirror,
        &part_path,
    )
    .await
    .expect("checksum-gated resume");

    assert_eq!(downloaded, 10);
    let ranges = range_log.lock().expect("range log").clone();
    assert!(
        ranges.iter().any(|r| r.contains("bytes=5-9")),
        "with primary checksum, cross-mirror resume may continue; got {ranges:?}"
    );

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun09_mismatched_content_range_rejects_resume() {
    let payload: Vec<u8> = (0..10u8).collect();
    let payload = Arc::new(payload);
    let server = TestServer::start({
        let payload = payload.clone();
        move |mut stream| {
            let mut buffer = [0u8; 8192];
            let Some(_request) = common::http::read_request_text(&mut stream, &mut buffer) else {
                return;
            };
            let body = &payload[5..];
            let response = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes 0-4/10\r\nETag: \"bad\"\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
        }
    });

    let pool = test_pool("fun09-bad-range").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, resource_ids) = seed_mirrors(&pool, "fun09-bad-range", &[&mirror_url]).await;
    db::update_metalink_resource_validators(&pool, &resource_ids[0], Some("\"bad\""), None)
        .await
        .expect("validators");
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list");
    let mirror = &mirrors[0];

    let client = test_client();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let temp_path = unique_temp_path("fun09-bad-range");
    let part_path = part_file_path(&temp_path, 0);
    tokio::fs::write(&part_path, &payload[0..5])
        .await
        .expect("pre-write");

    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<MetalinkWorkerProgress>();
    let error = download_metalink_range_from_mirror(
        &pool,
        "metalink-test-task-fun09-bad-range",
        &client,
        &[],
        &speed_limiter,
        &cancel_token,
        0,
        &progress_tx,
        0,
        9,
        mirror,
        &part_path,
    )
    .await
    .expect_err("mismatched Content-Range must fail");
    assert!(
        error.contains("Content-Range") || error.contains("Resume unavailable"),
        "got: {error}"
    );
    let part_data = tokio::fs::read(&part_path).await.expect("read part");
    assert_eq!(part_data, payload[0..5]);

    let _ = tokio::fs::remove_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&part_path).await;
    pool.close().await;
}

fn start_mirror_server_with_etag(
    payload: Arc<Vec<u8>>,
    range_log: Arc<Mutex<Vec<String>>>,
    etag: &'static str,
) -> TestServer {
    TestServer::start(move |mut stream| {
        let payload = payload.clone();
        let log = range_log.clone();
        let mut buffer = [0u8; 8192];
        let Some(request) = common::http::read_request_text(&mut stream, &mut buffer) else {
            return;
        };
        let request_line = request.lines().next().unwrap_or_default();
        let method = request_line.split_whitespace().next().unwrap_or("GET");
        let range_header = common::http::extract_header(&request, "range")
            .unwrap_or("")
            .to_string();
        {
            let mut guard = log.lock().expect("range log");
            guard.push(range_header);
        }
        let byte_range = common::http::extract_byte_range(&request);
        let start = byte_range.map(|r| r.start).unwrap_or(0).min(payload.len());
        let end = byte_range
            .and_then(|r| r.end)
            .unwrap_or_else(|| payload.len().saturating_sub(1))
            .min(payload.len().saturating_sub(1));
        let body = if method == "HEAD" || start > end {
            &[][..]
        } else {
            &payload[start..=end]
        };
        let status = if byte_range.is_some() { 206 } else { 200 };
        let content_range = format!("bytes {start}-{end}/{}", payload.len());
        let response = if status == 206 {
            format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: {content_range}\r\nETag: {etag}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                body.len()
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                body.len()
            )
        };
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    })
}

// --- C5: Credentials / Proxy / Diagnostics / process-restart reentry --------

fn b64_basic(user: &str, pass: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
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

fn error_code(error: &impl std::fmt::Display) -> String {
    let message = error.to_string();
    if let Ok(payload) = serde_json::from_str::<AppErrorPayload>(&message) {
        return payload.code;
    }
    let needle = "\"code\":\"";
    if let Some(start) = message.find(needle) {
        let rest = &message[start + needle.len()..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    message
}

fn start_auth_manifest_server(
    expected: String,
    observed: Arc<Mutex<Option<String>>>,
) -> TestServer {
    TestServer::start(move |mut stream| {
        let expected = expected.clone();
        let observed = observed.clone();
        let mut buffer = [0u8; 8192];
        let Some(request) = common::http::read_request_text(&mut stream, &mut buffer) else {
            return;
        };
        let authorization =
            common::http::extract_header(&request, "authorization").map(str::to_string);
        if let Some(auth) = &authorization {
            *observed.lock().expect("auth log") = Some(auth.clone());
        }
        let provided = authorization
            .as_deref()
            .and_then(|value| value.strip_prefix("Basic ").map(str::trim));
        if provided != Some(expected.as_str()) {
            let body = b"auth required";
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"metalink\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            return;
        }
        let body = MULTI_MIRROR_META4.as_bytes();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/metalink4+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    })
}

fn start_auth_mirror_server(
    payload: Arc<Vec<u8>>,
    expected: String,
    observed: Arc<Mutex<Vec<String>>>,
    slow: bool,
) -> TestServer {
    TestServer::start(move |mut stream| {
        let payload = payload.clone();
        let expected = expected.clone();
        let observed = observed.clone();
        let mut buffer = [0u8; 8192];
        let Some(request) = common::http::read_request_text(&mut stream, &mut buffer) else {
            return;
        };
        let method = request
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .next()
            .unwrap_or("GET");
        let authorization = common::http::extract_header(&request, "authorization")
            .unwrap_or("")
            .to_string();
        {
            let mut guard = observed.lock().expect("auth log");
            guard.push(authorization.clone());
        }
        let provided = authorization.strip_prefix("Basic ").map(str::trim);
        if provided != Some(expected.as_str()) {
            let body = b"auth required";
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"mirror\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            return;
        }
        let byte_range = common::http::extract_byte_range(&request);
        common::http::respond_file(
            &mut stream,
            method,
            &payload,
            byte_range,
            true,
            "payload.bin",
            slow,
        );
    })
}

fn start_always_401_server(hits: Arc<Mutex<usize>>) -> TestServer {
    TestServer::start(move |mut stream| {
        let hits = hits.clone();
        let mut buffer = [0u8; 4096];
        let Some(_request) = common::http::read_request_text(&mut stream, &mut buffer) else {
            return;
        };
        *hits.lock().expect("hits") += 1;
        let body = b"denied";
        let response = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
    })
}

async fn seed_downloadable_metalink_task(
    pool: &sqlx::SqlitePool,
    label: &str,
    mirror_urls: &[&str],
    payload_len: i64,
    paths: &common::TestPaths,
) -> String {
    let now = now_iso();
    let task_id = format!("metalink-c5-{label}");
    let task = TaskRecord {
        id: task_id.clone(),
        url: format!("https://example.com/{label}.meta4"),
        final_url: Some(format!("https://example.com/{label}.meta4")),
        protocol: "metalink".to_string(),
        task_kind: TaskKind::Manifest,
        file_name: "payload.bin".to_string(),
        save_dir: paths
            .final_path
            .parent()
            .expect("parent")
            .to_string_lossy()
            .to_string(),
        temp_path: Some(paths.temp.to_string_lossy().to_string()),
        final_path: Some(paths.final_path.to_string_lossy().to_string()),
        total_size: payload_len,
        downloaded_bytes: 0,
        status: TaskStatus::Downloading,
        etag: None,
        last_modified: None,
        content_type: Some("application/metalink4+xml".to_string()),
        supports_resume: true,
        supports_parallel: mirror_urls.len() >= 2,
        supports_multi_file: false,
        source_key: format!("metalink-c5-{label}"),
        connection_count: 1,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: Some("Downloading".to_string()),
        error_message: None,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.clone(),
        updated_at: now,
        files_version: 0,
    };
    db::insert_task_record(pool, &task)
        .await
        .expect("insert task");

    let file_id = format!("metalink-c5-file-{label}");
    db::insert_task_file_record(
        pool,
        &TaskFileRecord {
            id: file_id.clone(),
            task_id: task_id.clone(),
            relative_path: "payload.bin".to_string(),
            file_name: "payload.bin".to_string(),
            save_dir: task.save_dir.clone(),
            temp_path: Some(paths.temp.to_string_lossy().to_string()),
            final_path: Some(paths.final_path.to_string_lossy().to_string()),
            total_size: payload_len,
            downloaded_bytes: 0,
            status: TaskStatus::Queued,
            selected: true,
            content_type: Some("application/octet-stream".to_string()),
        },
    )
    .await
    .expect("insert file");

    for (index, url) in mirror_urls.iter().enumerate() {
        db::insert_metalink_resource(
            pool,
            db::MetalinkResourceInsert {
                id: &format!("metalink-c5-res-{label}-{index}"),
                task_id: &task_id,
                file_id: &file_id,
                url,
                priority: index as i64,
                location: None,
            },
        )
        .await
        .expect("insert resource");
    }
    task_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_uses_persisted_metalink_credentials() {
    common::install_test_secret_key();
    let expected = b64_basic("meta-user", "meta-pass");
    let observed = Arc::new(Mutex::new(None));
    let server = start_auth_manifest_server(expected, observed.clone());
    let pool = test_pool("c5-probe-cred").await;
    let task_id = "metalink-c5-probe-cred";
    let now = now_iso();
    db::insert_task_record(
        &pool,
        &TaskRecord {
            id: task_id.to_string(),
            url: format!("{}/secure.meta4", server.base_url),
            final_url: Some(format!("{}/secure.meta4", server.base_url)),
            protocol: "metalink".to_string(),
            task_kind: TaskKind::Manifest,
            file_name: "secure.meta4".to_string(),
            save_dir: std::env::temp_dir().to_string_lossy().to_string(),
            temp_path: None,
            final_path: None,
            total_size: 0,
            downloaded_bytes: 0,
            status: TaskStatus::Queued,
            etag: None,
            last_modified: None,
            content_type: None,
            supports_resume: true,
            supports_parallel: true,
            supports_multi_file: false,
            source_key: "metalink-c5-probe-cred".to_string(),
            connection_count: 0,
            speed_bps: 0,
            task_speed_limit_bps: None,
            priority: TaskPriority::Normal,
            queue_position: 0,
            category_key: None,
            obey_schedule: true,
            health_summary: None,
            error_message: None,
            error_code: None,
            recovery_actions: Vec::new(),
            retry_after_at: None,
            expected_hash_sha256: None,
            actual_hash_sha256: None,
            hash_status: HashVerificationStatus::NotRequested,
            hash_error: None,
            hash_verified_at: None,
            created_at: now.clone(),
            updated_at: now,
            files_version: 0,
        },
    )
    .await
    .expect("insert");
    db::upsert_task_credentials(
        &pool,
        task_id,
        "metalink",
        "meta-user",
        "meta-pass",
        None,
        None,
    )
    .await
    .expect("store credentials");

    let mut request = new_probe_request(format!("{}/secure.meta4", server.base_url));
    request.pool = Some(pool.clone());
    request.task_id = Some(task_id.to_string());
    new_engine()
        .probe(request)
        .await
        .expect("authenticated probe");

    let auth = observed
        .lock()
        .expect("lock")
        .clone()
        .expect("Authorization observed");
    assert!(auth.starts_with("Basic "), "got: {auth}");
    assert!(!auth.contains("meta-pass"));
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_uses_persisted_metalink_credentials_serial() {
    common::install_test_secret_key();
    let payload = Arc::new(b"metalink-auth-serial-payload".to_vec());
    let expected = b64_basic("mirror-user", "mirror-pass");
    let observed = Arc::new(Mutex::new(Vec::new()));
    let server = start_auth_mirror_server(payload.clone(), expected, observed.clone(), false);
    let pool = test_pool("c5-dl-cred-serial").await;
    let mut paths = common::TestPaths::new("c5-dl-cred-serial");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("payload.bin.tmp");
    paths.final_path = root.join("payload.bin");
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let task_id = seed_downloadable_metalink_task(
        &pool,
        "dl-cred-serial",
        &[&mirror_url],
        payload.len() as i64,
        &paths,
    )
    .await;
    db::upsert_task_credentials(
        &pool,
        &task_id,
        "metalink",
        "mirror-user",
        "mirror-pass",
        None,
        None,
    )
    .await
    .expect("store credentials");

    let task = db::get_task_record(&pool, &task_id)
        .await
        .expect("read")
        .expect("exists");
    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("authenticated metalink download");

    let auths = observed.lock().expect("lock").clone();
    assert!(
        auths.iter().any(|auth| auth.starts_with("Basic ")),
        "serial path must send Authorization, got: {auths:?}"
    );
    assert!(paths.final_path.exists());
    let written = std::fs::read(&paths.final_path).expect("read final");
    assert_eq!(written, payload.as_slice());
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parallel_mirror_download_forwards_authorization_header() {
    let payload: Vec<u8> = (0..30u8).collect();
    let payload = Arc::new(payload);
    let expected = b64_basic("parallel-user", "parallel-pass");
    let observed = Arc::new(Mutex::new(Vec::new()));
    let server =
        start_auth_mirror_server(payload.clone(), expected.clone(), observed.clone(), false);
    let pool = test_pool("c5-parallel-auth").await;
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let (file_id, _resource_ids) = seed_mirrors(&pool, "c5-parallel-auth", &[&mirror_url]).await;
    let mirrors = db::list_metalink_resources_for_file(&pool, &file_id)
        .await
        .expect("list");
    let mirror = mirrors.into_iter().next().expect("mirror");
    let temp_path = unique_temp_path("c5-parallel-auth");
    let (progress_tx, _progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let headers = vec![("Authorization".to_string(), format!("Basic {expected}"))];

    let downloaded = download_metalink_range_from_mirror(
        &pool,
        "metalink-test-task-c5-parallel-auth",
        &test_client(),
        &headers,
        &GlobalSpeedLimiter::disabled(),
        &tokio_util::sync::CancellationToken::new(),
        0,
        &progress_tx,
        0,
        9,
        &mirror,
        &temp_path,
    )
    .await
    .expect("authorized range download");
    assert_eq!(downloaded, 10);
    let auths = observed.lock().expect("lock").clone();
    assert!(
        auths.iter().any(|auth| auth.starts_with("Basic ")),
        "parallel worker must forward Authorization, got: {auths:?}"
    );
    let _ = std::fs::remove_file(&temp_path);
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_via_unreachable_socks5_fails_without_bypass() {
    let (server, hits) = start_counting_server();
    let engine = new_engine();
    let mut request = new_probe_request(format!("{}/manifest.meta4", server.base_url));
    request.proxy_config = Some(unreachable_socks5_proxy());

    let error = engine
        .probe(request)
        .await
        .expect_err("unreachable SOCKS5 must fail Metalink probe");
    let code = error_code(&error);
    assert!(
        code == "proxy_connection_failed"
            || code == "connection_refused"
            || code == "network_error"
            || code.contains("proxy"),
        "expected stable proxy failure code, got: {code}"
    );
    let hits = hits.lock().expect("hits").clone();
    assert!(
        hits.is_empty(),
        "proxy failure must not silently bypass to origin, got hits: {hits:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_fails_when_all_mirrors_return_401() {
    let hits = Arc::new(Mutex::new(0usize));
    let server = start_always_401_server(hits.clone());
    let pool = test_pool("c5-all-401").await;
    let mut paths = common::TestPaths::new("c5-all-401");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("payload.bin.tmp");
    paths.final_path = root.join("payload.bin");
    let mirror_a = format!("{}/a.bin", server.base_url);
    let mirror_b = format!("{}/b.bin", server.base_url);
    let task_id =
        seed_downloadable_metalink_task(&pool, "all-401", &[&mirror_a, &mirror_b], 32, &paths)
            .await;
    let task = db::get_task_record(&pool, &task_id)
        .await
        .expect("read")
        .expect("exists");

    let error = new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("all-401 must fail");
    let message = error.to_string();
    assert!(
        message.contains("metalink_all_mirrors_failed") || message.contains("http_denied"),
        "expected stable diagnostics code, got: {message}"
    );
    assert!(
        *hits.lock().expect("hits") >= 1,
        "server should have seen at least one denied request"
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_reenters_after_reset_interrupted_tasks() {
    let payload: Vec<u8> = (0..64u8).cycle().take(32 * 1024).collect();
    let payload = Arc::new(payload);
    let range_log = Arc::new(Mutex::new(Vec::new()));
    let server = start_mirror_server(payload.clone(), range_log, None, true);
    let pool = test_pool("c5-process-restart").await;
    let mut paths = common::TestPaths::new("c5-process-restart");
    let root = paths.final_path.parent().expect("root").to_path_buf();
    paths.temp = root.join("payload.bin.tmp");
    paths.final_path = root.join("payload.bin");
    let mirror_url = format!("{}/payload.bin", server.base_url);
    let task_id = seed_downloadable_metalink_task(
        &pool,
        "process-restart",
        &[&mirror_url],
        payload.len() as i64,
        &paths,
    )
    .await;
    let task = db::get_task_record(&pool, &task_id)
        .await
        .expect("read")
        .expect("exists");

    let cancel = tokio_util::sync::CancellationToken::new();
    let first = tokio::spawn({
        let engine = new_engine();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });
    let started = std::time::Instant::now();
    loop {
        let files = db::list_task_file_records(&pool, &task_id)
            .await
            .expect("list files");
        if files
            .iter()
            .any(|file| file.downloaded_bytes > 0 || paths.temp.exists())
        {
            break;
        }
        if started.elapsed() > Duration::from_secs(8) {
            cancel.cancel();
            let _ = first.await;
            panic!("timed out waiting for partial Metalink progress");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cancel.cancel();
    first.abort();
    let _ = first.await;

    sqlx::query("UPDATE tasks SET status = 'downloading', updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&task_id)
        .execute(&pool)
        .await
        .expect("force downloading");
    db::reset_interrupted_tasks(&pool, true)
        .await
        .expect("reset interrupted");
    let queued = db::get_task_record(&pool, &task_id)
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
    .expect("Downloading after reset");

    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            resumed,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("cold Metalink reentry");
    assert!(paths.final_path.exists(), "final file must exist");
    let written = std::fs::read(&paths.final_path).expect("read final");
    assert_eq!(written, payload.as_slice());
    pool.close().await;
}
