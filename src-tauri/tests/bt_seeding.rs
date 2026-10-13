mod common;

use sha1::{Digest, Sha1};
use std::time::{Duration, Instant};
use tauri_app_lib::{
    db,
    download::{BtEngine, DownloadEngine, GlobalSpeedLimiter, ProbeRequest},
    models::TaskStatus,
    proxy::ResolvedProxyConfig,
};
use tokio_util::sync::CancellationToken;

async fn fixture(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> (common::TestPaths, tauri_app_lib::models::TaskRecord) {
    let paths = common::TestPaths::new(id);
    let dir = paths.final_path.parent().unwrap();
    let payload = vec![42_u8; 4096];
    let name = format!("{id}.bin");
    std::fs::write(dir.join(&name), &payload).unwrap();
    let mut torrent = format!(
        "d4:infod6:lengthi{}e4:name{}:{}12:piece lengthi16384e6:pieces20:",
        payload.len(),
        name.len(),
        name
    )
    .into_bytes();
    torrent.extend_from_slice(&Sha1::digest(&payload));
    torrent.extend_from_slice(b"ee");
    let source = dir.join("fixture.torrent");
    std::fs::write(&source, &torrent).unwrap();
    let url = reqwest::Url::from_file_path(source).unwrap().to_string();
    let probe = BtEngine::default()
        .probe(ProbeRequest {
            uri: url.clone(),
            source: None,
            request_headers: vec![],
            pool: None,
            task_id: None,
            credentials: None,
            proxy_config: None,
            app: None,
            request_id: None,
            cancel_token: None,
            network_policy: Default::default(),
        })
        .await
        .unwrap();
    let mut task = common::download_task(id, url, "bt", &name, payload.len() as i64, &paths, true);
    task.source_key = probe.source_key.clone();
    task.final_path = Some(dir.join(&name).to_string_lossy().into_owned());
    db::insert_task_record(pool, &task).await.unwrap();
    db::ensure_task_segments(pool, &task).await.unwrap();
    db::upsert_torrent_task(
        pool,
        id,
        db::TorrentTaskUpsert {
            info_hash: probe.source_key.trim_start_matches("bt:"),
            name: &name,
            magnet_uri: None,
            torrent_blob: Some(&torrent),
            piece_length: 16384,
            piece_count: 1,
            private: false,
            trackers_json: None,
            seeding_enabled: true,
            seed_ratio_limit: None,
            seed_time_limit_seconds: None,
        },
    )
    .await
    .unwrap();
    (paths, task)
}

async fn download(
    engine: &BtEngine,
    pool: &sqlx::SqlitePool,
    task: tauri_app_lib::models::TaskRecord,
) {
    tokio::time::timeout(
        Duration::from_secs(20),
        engine.download(common::headless_download_context(
            pool.clone(),
            task,
            CancellationToken::new(),
        )),
    )
    .await
    .expect("the download worker must release its slot while seeding")
    .unwrap();
}

#[tokio::test]
async fn two_seeds_release_download_workers_and_stop_before_files_are_reused() {
    let (_db, pool) = common::test_pool("b6-seeds").await;
    let engine = BtEngine::default();
    let (_a, a) = fixture(&pool, "seed-a").await;
    let (_b, b) = fixture(&pool, "seed-b").await;
    let ((), ()) = tokio::join!(
        download(&engine, &pool, a.clone()),
        download(&engine, &pool, b.clone())
    );
    assert_eq!(engine.seeding_manager().active_count().await, 2);
    assert_eq!(
        db::get_task_record(&pool, &a.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Completed
    );
    // The two background seeds remain active while two HTTP workers share one root budget.
    let server = common::TestServer::start(|mut stream| {
        use std::io::{Read, Write};
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        let body = vec![7; 3000];
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: 3000\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(&body);
    });
    let http = tauri_app_lib::download::HttpEngine::with_proxy_config(
        ResolvedProxyConfig::shared_default(),
    )
    .unwrap();
    let root = std::sync::Arc::new(GlobalSpeedLimiter::new(Some(1000)));
    let mut http_paths = Vec::new();
    let mut http_tasks = Vec::new();
    for index in 0..2 {
        let paths = common::TestPaths::new("http-alongside-seeds");
        let task = common::download_task(
            &format!("http-{index}"),
            format!("{}/file", server.base_url),
            "http",
            "file.bin",
            3000,
            &paths,
            false,
        );
        db::insert_task_record(&pool, &task).await.unwrap();
        db::ensure_task_segments(&pool, &task).await.unwrap();
        http_tasks.push(task);
        http_paths.push(paths);
    }
    let mut first = common::headless_download_context(
        pool.clone(),
        http_tasks[0].clone(),
        CancellationToken::new(),
    );
    let mut second = common::headless_download_context(
        pool.clone(),
        http_tasks[1].clone(),
        CancellationToken::new(),
    );
    first.speed_limiter = root.clone();
    second.speed_limiter = root;
    let started = Instant::now();
    let (first, second) = tokio::join!(http.download(first), http.download(second));
    first.unwrap();
    second.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(4500),
        "HTTP workers must consume one shared global bucket"
    );
    assert_eq!(engine.seeding_manager().active_count().await, 2);
    for paths in &http_paths {
        assert_eq!(std::fs::read(&paths.final_path).unwrap(), vec![7; 3000]);
    }
    // A stopped task must leave the other isolated session alive.
    engine
        .seeding_manager()
        .stop(&a.id, "manual", None)
        .await
        .unwrap();
    assert!(engine.seeding_manager().contains(&b.id).await);
    assert_eq!(
        db::bt_seeding_state(&pool, &a.id)
            .await
            .unwrap()
            .unwrap()
            .stop_reason
            .as_deref(),
        Some("manual")
    );
    std::fs::rename(
        a.final_path.as_ref().unwrap(),
        format!("{}.moved", a.final_path.as_ref().unwrap()),
    )
    .unwrap();
    engine
        .seeding_manager()
        .drain(Some(Duration::from_secs(10)))
        .await
        .unwrap();
    assert_eq!(engine.seeding_manager().active_count().await, 0);
    assert_eq!(
        db::bt_seeding_state(&pool, &b.id)
            .await
            .unwrap()
            .unwrap()
            .stop_reason
            .as_deref(),
        Some("shutdown")
    );
    pool.close().await;
}

#[tokio::test]
async fn completed_torrent_verifies_local_files_and_restores_only_enabled_nonmanual_tasks() {
    let (_db, pool) = common::test_pool("b6-restore").await;
    let engine = BtEngine::default();
    let (_paths, task) = fixture(&pool, "restart-seed").await;
    download(&engine, &pool, task.clone()).await;
    engine.seeding_manager().drain(None).await.unwrap();
    let task = db::get_task_record(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(
        db::bt_seeding_restore_candidates(&pool)
            .await
            .unwrap()
            .len(),
        1
    );
    let next = BtEngine::default();
    next.start_completed_seeding(
        None,
        pool.clone(),
        task.clone(),
        ResolvedProxyConfig::default(),
        GlobalSpeedLimiter::disabled(),
    )
    .await
    .unwrap();
    next.seeding_manager()
        .stop(&task.id, "manual", None)
        .await
        .unwrap();
    assert!(db::bt_seeding_restore_candidates(&pool)
        .await
        .unwrap()
        .is_empty());
    // Same-size corruption must fail piece verification, rather than seed bad data.
    std::fs::write(task.final_path.as_ref().unwrap(), vec![1; 4096]).unwrap();
    let error = next
        .start_completed_seeding(
            None,
            pool.clone(),
            task,
            ResolvedProxyConfig::default(),
            GlobalSpeedLimiter::disabled(),
        )
        .await
        .unwrap_err();
    assert!(error.contains("bt_seeding_files_invalid"), "{error}");
    assert_eq!(next.seeding_manager().active_count().await, 0);
    pool.close().await;
}

#[tokio::test]
async fn cumulative_checkpoints_are_monotonic_and_limits_survive_a_restart() {
    let (_db, pool) = common::test_pool("b6-limits").await;
    let engine = BtEngine::default();
    let (_paths, task) = fixture(&pool, "limited-seed").await;
    download(&engine, &pool, task.clone()).await;
    engine.seeding_manager().drain(None).await.unwrap();
    db::checkpoint_bt_seeding(&pool, &task.id, 8192, 30, Some("limit"))
        .await
        .unwrap();
    db::checkpoint_bt_seeding(&pool, &task.id, 100, 1, Some("limit"))
        .await
        .unwrap();
    db::update_torrent_seeding(&pool, &task.id, true, Some(1.5), Some(60), true)
        .await
        .unwrap();
    let seed = db::bt_seeding_state(&pool, &task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((seed.uploaded_bytes, seed.elapsed_seconds), (8192, 30));
    assert!(seed.started_at.is_some());
    let task = db::get_task_record(&pool, &task.id).await.unwrap().unwrap();
    let error = engine
        .start_completed_seeding(
            None,
            pool.clone(),
            task.clone(),
            ResolvedProxyConfig::default(),
            GlobalSpeedLimiter::disabled(),
        )
        .await
        .unwrap_err();
    assert!(error.contains("bt_seeding_limit_reached"));
    db::update_torrent_seeding(&pool, &task.id, true, None, Some(10), true)
        .await
        .unwrap();
    let error = engine
        .start_completed_seeding(
            None,
            pool.clone(),
            task,
            ResolvedProxyConfig::default(),
            GlobalSpeedLimiter::disabled(),
        )
        .await
        .unwrap_err();
    assert!(error.contains("bt_seeding_limit_reached"));
    pool.close().await;
}

#[tokio::test]
async fn idle_seeding_snapshots_do_not_rewrite_piece_bitmaps_or_grow_wal_each_tick() {
    let (_db, pool) = common::test_pool("b6-writes").await;
    let engine = BtEngine::default();
    let (_paths, task) = fixture(&pool, "idle-seed").await;
    download(&engine, &pool, task.clone()).await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let first = db::get_torrent_runtime_snapshot(&pool, &task.id)
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let next = db::get_torrent_runtime_snapshot(&pool, &task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.piece_bitfield_base64, next.piece_bitfield_base64);
    engine.seeding_manager().drain(None).await.unwrap();
    let snapshot = db::get_torrent_runtime_snapshot(&pool, &task.id)
        .await
        .unwrap()
        .unwrap();
    for _ in 0..10 {
        db::upsert_torrent_runtime_snapshot(
            &pool,
            &task.id,
            db::TorrentRuntimeSnapshotUpsert {
                metadata_status: &snapshot.metadata_status,
                completed_pieces: snapshot.completed_pieces,
                verified_pieces: snapshot.verified_pieces,
                piece_count: snapshot.piece_count,
                piece_bitfield_base64: None,
                peer_count: snapshot.peer_count,
                seed_count: snapshot.seed_count,
                dht_status: snapshot.dht_status.as_deref(),
                trackers_json: None,
                upload_bytes: snapshot.upload_bytes,
                upload_speed_bps: snapshot.upload_speed_bps,
                ratio: snapshot.ratio,
                seeding_enabled: snapshot.seeding_enabled,
                seeding_state: &snapshot.seeding_state,
                last_error_code: snapshot.last_error_code.as_deref(),
                last_error_message: snapshot.last_error_message.as_deref(),
            },
        )
        .await
        .unwrap();
    }
    let unchanged = db::get_torrent_runtime_snapshot(&pool, &task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.updated_at, unchanged.updated_at,
        "identical snapshots must not write SQLite"
    );
    pool.close().await;
}

#[tokio::test]
#[ignore = "explicit local 2/5/10-session performance baseline"]
async fn isolated_sessions_performance_baseline() {
    for count in [2, 5, 10] {
        let paths = common::TestPaths::new(&format!("bt-perf-{count}"));
        let db_path = paths.final_path.parent().unwrap().join("perf.sqlite");
        let pool = db::connect(&db_path).await.unwrap().pool;
        let engine = BtEngine::default();
        let mut fixtures = Vec::new();
        for index in 0..count {
            fixtures.push(fixture(&pool, &format!("perf-{count}-{index}")).await);
        }
        // Row triggers count actual writes across every pooled connection.
        sqlx::query("CREATE TABLE perf_write_count(n INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO perf_write_count VALUES(0)")
            .execute(&pool)
            .await
            .unwrap();
        for table in ["torrent_tasks", "torrent_runtime_snapshots"] {
            sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER count_{table} AFTER UPDATE ON {table} BEGIN UPDATE perf_write_count SET n=n+1; END"))).execute(&pool).await.unwrap();
        }
        let started = Instant::now();
        let mut workers = tokio::task::JoinSet::new();
        for (_, task) in &fixtures {
            let engine = engine.clone();
            let pool = pool.clone();
            let task = task.clone();
            workers.spawn(async move { download(&engine, &pool, task).await });
        }
        while let Some(result) = workers.join_next().await {
            result.unwrap();
        }
        let startup_ms = started.elapsed().as_millis();
        let before: i64 = sqlx::query_scalar("SELECT n FROM perf_write_count")
            .fetch_one(&pool)
            .await
            .unwrap();
        let wal = || {
            std::fs::metadata(format!("{}-wal", db_path.display()))
                .map(|m| m.len())
                .unwrap_or(0)
        };
        let wal_start = wal();
        tokio::time::sleep(Duration::from_secs(10)).await;
        let writes: i64 = sqlx::query_scalar("SELECT n FROM perf_write_count")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mut dht = Vec::new();
        for (_, task) in &fixtures {
            dht.push(
                db::get_torrent_runtime_snapshot(&pool, &task.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .dht_status,
            );
        }
        #[cfg(windows)]
        let rss = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {}).WorkingSet64", std::process::id()),
            ])
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .and_then(|text| text.trim().parse::<u64>().ok());
        #[cfg(not(windows))]
        let rss: Option<u64> = None;
        println!(
            "B6_BT_BASELINE {}",
            serde_json::json!({ "sessions": count, "startup_ms": startup_ms, "rss_bytes": rss, "wal_growth_bytes": wal().saturating_sub(wal_start), "runtime_updates_10s": writes - before, "dht": dht })
        );
        engine.seeding_manager().drain(None).await.unwrap();
        pool.close().await;
    }
}
