mod common;
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tauri_app_lib::{
    db,
    download::{DashEngine, DownloadEngine, EngineRegistry, HlsEngine, HttpEngine, ProbeRequest},
    models::SegmentStatus,
    proxy::ResolvedProxyConfig,
};
use tokio_util::sync::CancellationToken;

fn ffmpeg() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok()
}
fn media(format: &str, audio: bool) -> Vec<u8> {
    let paths = common::TestPaths::new("b6-media-fixture");
    let output = paths.final_path.with_extension(format);
    let mut command = std::process::Command::new("ffmpeg");
    command.args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
    ]);
    if audio {
        command.args([
            "sine=frequency=1000:sample_rate=44100",
            "-t",
            "1",
            "-c:a",
            "aac",
        ]);
    } else {
        command.args([
            "color=c=blue:size=64x64:rate=5",
            "-t",
            "1",
            "-c:v",
            if format == "ts" {
                "mpeg2video"
            } else {
                "mpeg4"
            },
        ]);
    }
    command.arg(&output);
    assert!(command.status().unwrap().success());
    std::fs::read(output).unwrap()
}
fn server(
    bodies: HashMap<String, (String, Vec<u8>)>,
    seen: Arc<Mutex<Vec<String>>>,
) -> common::TestServer {
    common::TestServer::start(move |mut stream| {
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            return;
        }
        let line = String::from_utf8_lossy(&buf[..n]);
        let path = line
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_string();
        seen.lock().unwrap().push(path.clone());
        let (status, mime, body) = match bodies.get(&path) {
            Some((mime, body)) => (200, mime.as_str(), body.as_slice()),
            None => (404, "text/plain", b"not found".as_slice()),
        };
        let _ = write!(stream, "HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        if !line.starts_with("HEAD ") {
            let _ = stream.write_all(body);
        }
    })
}
fn request(uri: String) -> ProbeRequest {
    ProbeRequest {
        uri,
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
    }
}
fn http() -> Arc<HttpEngine> {
    Arc::new(HttpEngine::with_proxy_config(ResolvedProxyConfig::shared_default()).unwrap())
}

#[cfg(debug_assertions)]
fn creation_state(pool: sqlx::SqlitePool) -> tauri_app_lib::AppState {
    let downloads = Arc::default();
    let request_headers = Arc::default();
    let speed_limiter = tauri_app_lib::download::GlobalSpeedLimiter::disabled();
    let engine_registry = Arc::new(EngineRegistry::new().unwrap());
    let task_runtime_locks = Arc::default();
    let scheduler = Arc::new(tauri_app_lib::scheduler::Scheduler::new(
        Arc::clone(&downloads),
        Arc::clone(&request_headers),
        speed_limiter.clone(),
        engine_registry.clone(),
        Arc::clone(&task_runtime_locks),
    ));
    tauri_app_lib::AppState {
        pool,
        downloads,
        request_headers,
        speed_limiter,
        engine_registry,
        task_runtime_locks,
        scheduler,
        browser_realtime: tauri_app_lib::browser_realtime::BrowserRealtimeState::new(),
        quit_requested: Arc::default(),
        lifecycle_gate: Arc::default(),
        lifecycle: Arc::default(),
        active_supervisors: Arc::default(),
        close_request_pending: Arc::default(),
    }
}
fn inspect(path: &str) -> serde_json::Value {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
            path,
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
const VTT: &str = "WEBVTT\n\n00:00:00.000 --> 00:00:00.800\nHello B6\n";
const PLAYLIST: &str = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1.0,\nvideo.ts\n#EXT-X-ENDLIST\n";

#[tokio::test]
async fn hls_missing_segment_limit_is_bounded_at_persistence_boundary() {
    let (_db, pool) = common::test_pool("hls-limit-boundary").await;
    let paths = common::TestPaths::new("hls-limit-boundary");
    let task = common::download_task(
        "hls-limit-boundary-task",
        "http://127.0.0.1:1/live.m3u8".to_string(),
        "hls",
        "live.mp4",
        0,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task).await.unwrap();
    db::upsert_hls_task(
        &pool,
        db::HlsTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            media_url: &task.url,
            playlist_kind: "live",
            selected_bandwidth: None,
            selected_resolution: None,
            target_duration: 1,
            last_media_sequence: None,
            output_format: "mp4",
            staging_dir: task.temp_path.as_deref().unwrap(),
            selected_audio_track_uris: None,
            selected_subtitle_track_uris: None,
            max_missing_segments: i64::MAX,
            missing_segments_json: None,
            selected_track_catalog_json: None,
        },
    )
    .await
    .unwrap();

    let row = db::get_hls_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(row.max_missing_segments, db::MAX_HLS_MISSING_SEGMENTS);

    sqlx::query("UPDATE hls_tasks SET max_missing_segments = ? WHERE task_id = ?")
        .bind(i64::MAX)
        .bind(&task.id)
        .execute(&pool)
        .await
        .unwrap();
    let restored = db::get_hls_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(restored.max_missing_segments, db::MAX_HLS_MISSING_SEGMENTS);
    pool.close().await;
}

#[tokio::test]
async fn redirected_media_uses_final_manifest_base_without_forwarding_source_secrets() {
    if !ffmpeg() {
        eprintln!("requires ffmpeg/ffprobe");
        return;
    }
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();
    let bodies = HashMap::from([
        ("/assets/master".to_string(), ("application/vnd.apple.mpegurl", b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\ntracks/video.m3u8\n".to_vec())),
        ("/assets/tracks/video.m3u8".into(), ("application/vnd.apple.mpegurl", PLAYLIST.as_bytes().to_vec())),
        ("/assets/tracks/video.ts".into(), ("video/mp2t", media("ts", false))),
        ("/assets/manifest".into(), ("application/dash+xml", b"<MPD type=\"static\"><Period><AdaptationSet contentType=\"audio\" mimeType=\"audio/mp4\"><Representation id=\"a\" bandwidth=\"128000\"><SegmentList><SegmentURL media=\"audio.m4a\"/></SegmentList></Representation></AdaptationSet></Period></MPD>".to_vec())),
        ("/assets/audio.m4a".into(), ("audio/mp4", media("m4a", true))),
    ]);
    let target = common::TestServer::start(move |mut stream| {
        let mut buffer = [0; 8192];
        let size = stream.read(&mut buffer).unwrap_or(0);
        if size == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&buffer[..size]);
        requests.lock().unwrap().push(text.to_ascii_lowercase());
        let path = text.split_whitespace().nth(1).unwrap();
        let (mime, body) = bodies
            .get(path)
            .expect("relative resources must resolve under /assets");
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        if !text.starts_with("HEAD ") {
            let _ = stream.write_all(body);
        }
    });
    let target_url = target.base_url.clone();
    let source = common::TestServer::start(move |mut stream| {
        let mut buffer = [0; 8192];
        let size = stream.read(&mut buffer).unwrap_or(0);
        if size == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&buffer[..size]);
        assert!(text
            .to_ascii_lowercase()
            .contains("authorization: basic dxnlcjpwyxnz"));
        let path = if text.contains(" /hls ") {
            "master"
        } else {
            "manifest"
        };
        let _ = write!(stream, "HTTP/1.1 302 Found\r\nLocation: {target_url}/assets/{path}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    });
    let (_db, pool) = common::test_pool("b6-redirected-media").await;
    let registry = EngineRegistry::new().unwrap();
    for (path, protocol) in [("hls", "hls"), ("dash", "dash")] {
        let source_url = format!("{}/{path}", source.base_url);
        let mut probe_request = request(source_url.clone());
        probe_request.credentials = Some(db::TaskCredentials {
            username: "user".into(),
            password: "pass".into(),
            private_key_data: None,
            private_key_passphrase: None,
        });
        probe_request.request_headers = vec![("Cookie".into(), "secret=value".into())];
        let probe = registry.probe(probe_request).await.unwrap();
        assert_eq!(probe.protocol, protocol);
        assert!(probe
            .resolved_uri
            .starts_with(&format!("{}/assets/", target.base_url)));
        if protocol == "hls" {
            assert_eq!(
                probe.hls_variants[0].uri,
                format!("{}/assets/tracks/video.m3u8", target.base_url)
            );
        }
        let paths = common::TestPaths::new("b6-redirected-output");
        let mut task =
            common::download_task(path, source_url, protocol, "output.mp4", 0, &paths, false);
        task.final_url = Some(probe.resolved_uri);
        db::insert_task_record(&pool, &task).await.unwrap();
        db::ensure_task_segments(&pool, &task).await.unwrap();
        let mut context =
            common::headless_download_context(pool.clone(), task.clone(), CancellationToken::new());
        context.request_headers = vec![
            ("Authorization".into(), "Basic dXNlcjpwYXNz".into()),
            ("Cookie".into(), "secret=value".into()),
        ];
        registry
            .engine_for_task(&task)
            .unwrap()
            .download(context)
            .await
            .unwrap();
        inspect(paths.final_path.to_str().unwrap());
    }
    {
        let requests = captured.lock().unwrap();
        assert!(requests
            .iter()
            .any(|raw| raw.contains(" /assets/tracks/video.ts ")));
        assert!(requests
            .iter()
            .any(|raw| raw.contains(" /assets/audio.m4a ")));
        assert!(requests
            .iter()
            .all(|raw| !raw.contains("authorization:") && !raw.contains("cookie:")));
    }
    pool.close().await;
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn hls_variant_selection_keeps_the_original_credential_origin() {
    if !ffmpeg() {
        eprintln!("requires ffmpeg/ffprobe");
        return;
    }
    common::install_test_secret_key();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let recorded = captured.clone();
    let video = media("ts", false);
    let destination = common::TestServer::start(move |mut stream| {
        let mut buffer = [0; 8192];
        let size = stream.read(&mut buffer).unwrap_or(0);
        if size == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&buffer[..size]);
        recorded.lock().unwrap().push(text.to_ascii_lowercase());
        let body = if text.contains(" /selected.m3u8 ") {
            PLAYLIST.as_bytes()
        } else {
            &video
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(body);
    });
    let selected = format!("{}/selected.m3u8", destination.base_url);
    let master = format!("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\n{selected}\n#EXT-X-STREAM-INF:BANDWIDTH=2000\nmain.m3u8\n");
    let source = common::TestServer::start(move |mut stream| {
        let mut buffer = [0; 8192];
        let size = stream.read(&mut buffer).unwrap_or(0);
        if size == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&buffer[..size]);
        assert!(text
            .to_ascii_lowercase()
            .contains("authorization: basic dxnlcjpwyxnz"));
        let body = if text.contains(" /master.m3u8 ") {
            master.as_bytes()
        } else {
            PLAYLIST.as_bytes()
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(body);
    });
    let (_db, pool) = common::test_pool("b6-variant-origin").await;
    let state = creation_state(pool.clone());
    let paths = common::TestPaths::new("b6-variant-origin");
    let url = format!("{}/master.m3u8", source.base_url);
    let authorization = db::create_network_authorization(
        &pool,
        tauri_app_lib::download::network_policy::TaskSource::Manual,
        &url,
    )
    .await
    .unwrap();
    let input = serde_json::json!({
        "url": url, "saveDir": paths.final_path.parent().unwrap(), "startPaused": true,
        "networkAuthorizationId": authorization.id, "username": "user", "password": "pass",
        "selectedHlsVariantUri": selected,
        "requestProfile": { "customHeaders": [{ "name": "Cookie", "value": "secret=value" }] },
    });
    let task = tauri_app_lib::commands::tasks::create_task_headless(
        &state,
        serde_json::from_value(input).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(task.url, url);
    assert_eq!(
        db::get_hls_task(&pool, &task.id)
            .await
            .unwrap()
            .unwrap()
            .media_url,
        selected
    );
    sqlx::query("UPDATE tasks SET status = 'downloading' WHERE id = ?")
        .bind(&task.id)
        .execute(&pool)
        .await
        .unwrap();
    let record = db::get_task_record(&pool, &task.id).await.unwrap().unwrap();
    let final_path = record.final_path.clone().unwrap();
    let mut context =
        common::headless_download_context(pool.clone(), record, CancellationToken::new());
    context.request_headers = db::resolve_task_request_profile_headers(&pool, &task.id)
        .await
        .unwrap();
    HlsEngine::new(http()).download(context).await.unwrap();
    inspect(&final_path);
    {
        let requests = captured.lock().unwrap();
        assert!(requests.iter().any(|raw| raw.contains(" /selected.m3u8 ")));
        assert!(requests
            .iter()
            .all(|raw| !raw.contains("authorization:") && !raw.contains("cookie:")));
    }
    pool.close().await;
}

#[tokio::test]
async fn ffmpeg_absence_allows_verified_ts_and_rejects_other_media() {
    const FIXTURE_ENV: &str = "VIBE_B6_FFMPEGLESS_FIXTURE";
    let fixture = match std::env::var_os(FIXTURE_ENV) {
        Some(path) => std::fs::read(path).unwrap(),
        None => {
            if !ffmpeg() {
                eprintln!("requires ffmpeg to generate the parent fixture");
                return;
            }
            let paths = common::TestPaths::new("b6-no-ffmpeg");
            std::fs::write(&paths.final_path, media("ts", false)).unwrap();
            // Change only the child environment; other media tests may run concurrently.
            let output = tokio::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "ffmpeg_absence_allows_verified_ts_and_rejects_other_media",
                    "--nocapture",
                ])
                .env(FIXTURE_ENV, &paths.final_path)
                .env("PATH", "")
                .env_remove("VIBE_FFMPEG_PATH")
                .kill_on_drop(true)
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
    };
    assert!(!ffmpeg(), "child must exercise actual binary absence");
    let server = server(
        HashMap::from([
            ("/safe.m3u8".into(), ("application/vnd.apple.mpegurl".into(), PLAYLIST.as_bytes().to_vec())),
            ("/video.ts".into(), ("video/mp2t".into(), fixture.clone())),
            ("/fragmented.m3u8".into(), ("application/vnd.apple.mpegurl".into(), b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1,\nvideo.m4s\n#EXT-X-ENDLIST\n".to_vec())),
            ("/external.m3u8".into(), ("application/vnd.apple.mpegurl".into(), b"#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"a\"\nsafe.m3u8\n".to_vec())),
        ]),
        Arc::new(Mutex::new(vec![])),
    );
    let hls = HlsEngine::new(http());
    let uri = format!("{}/safe.m3u8", server.base_url);
    let probe = hls.probe(request(uri.clone())).await.unwrap();
    assert!(probe.display_name.ends_with(".ts"));
    for path in ["fragmented.m3u8", "external.m3u8"] {
        let error = hls
            .probe(request(format!("{}/{path}", server.base_url)))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("hls_ffmpeg_missing"), "{error}");
    }
    let dash_error = DashEngine::new(http())
        .probe(request(format!("{}/manifest.mpd", server.base_url)))
        .await
        .unwrap_err()
        .to_string();
    assert!(dash_error.contains("dash_ffmpeg_missing"), "{dash_error}");
    let (_db, pool) = common::test_pool("b6-no-ffmpeg").await;
    let mut paths = common::TestPaths::new("b6-no-ffmpeg-output");
    paths.final_path = paths.final_path.with_extension("ts");
    let task = common::download_task("safe-ts", uri, "hls", "safe.ts", 0, &paths, false);
    db::insert_task_record(&pool, &task).await.unwrap();
    db::ensure_task_segments(&pool, &task).await.unwrap();
    hls.download(common::headless_download_context(
        pool.clone(),
        task.clone(),
        CancellationToken::new(),
    ))
    .await
    .unwrap();
    assert_eq!(std::fs::read(&paths.final_path).unwrap(), fixture);
    assert_eq!(
        db::get_hls_task(&pool, &task.id)
            .await
            .unwrap()
            .unwrap()
            .output_format,
        "ts"
    );
    pool.close().await;
}

async fn hls_row(
    pool: &sqlx::SqlitePool,
    task: &tauri_app_lib::models::TaskRecord,
    kind: &str,
    format: &str,
    subs: Option<&str>,
    catalog: Option<&str>,
    limit: i64,
) {
    db::upsert_hls_task(
        pool,
        db::HlsTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            media_url: &task.url,
            playlist_kind: kind,
            selected_bandwidth: None,
            selected_resolution: None,
            target_duration: 1,
            last_media_sequence: None,
            output_format: format,
            staging_dir: task.temp_path.as_ref().unwrap(),
            selected_audio_track_uris: None,
            selected_subtitle_track_uris: subs,
            max_missing_segments: limit,
            missing_segments_json: None,
            selected_track_catalog_json: catalog,
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn hls_webvtt_is_embedded_as_mov_text_with_language() {
    if !ffmpeg() {
        eprintln!("requires ffmpeg/ffprobe");
        return;
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let server = server(HashMap::from([
        ("/media.m3u8".into(), ("application/vnd.apple.mpegurl".into(), PLAYLIST.as_bytes().to_vec())),
        ("/video.ts".into(), ("video/mp2t".into(), media("ts", false))),
        ("/subs.m3u8".into(), ("application/vnd.apple.mpegurl".into(), b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1.0,\nsubtitle.vtt\n#EXT-X-ENDLIST\n".to_vec())),
        ("/subtitle.vtt".into(), ("text/vtt".into(), VTT.as_bytes().to_vec())),
    ]), seen);
    let (_db, pool) = common::test_pool("b6-hls-subtitle").await;
    let paths = common::TestPaths::new("b6-hls-subtitle");
    let mut task = common::download_task(
        "hls-sub",
        format!("{}/media.m3u8", server.base_url),
        "hls",
        "media.mp4",
        0,
        &paths,
        false,
    );
    task.final_path = Some(
        paths
            .final_path
            .with_extension("mp4")
            .to_string_lossy()
            .into_owned(),
    );
    db::insert_task_record(&pool, &task).await.unwrap();
    db::ensure_task_segments(&pool, &task).await.unwrap();
    let subs = serde_json::to_string(&vec![format!("{}/subs.m3u8", server.base_url)]).unwrap();
    let catalog = serde_json::json!([{ "uri": format!("{}/subs.m3u8", server.base_url), "language": "zh-CN", "name": "Chinese", "groupId": "sub", "kind": "SUBTITLES", "default": true, "autoSelect": true }]).to_string();
    hls_row(&pool, &task, "vod", "mp4", Some(&subs), Some(&catalog), 0).await;
    HlsEngine::new(http())
        .download(common::headless_download_context(
            pool.clone(),
            task.clone(),
            CancellationToken::new(),
        ))
        .await
        .unwrap();
    let result = inspect(
        db::get_task_record(&pool, &task.id)
            .await
            .unwrap()
            .unwrap()
            .final_path
            .as_ref()
            .unwrap(),
    );
    let subtitle = result["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["codec_type"] == "subtitle")
        .unwrap();
    assert_eq!(subtitle["codec_name"], "mov_text");
    assert_eq!(subtitle["tags"]["language"], "zho");
    assert!(result["format"]["format_name"]
        .as_str()
        .unwrap()
        .contains("mp4"));
    pool.close().await;
}

#[tokio::test]
async fn idle_hls_finishes_offline_as_validated_ts_and_rejects_bad_packets() {
    if !ffmpeg() {
        eprintln!("requires ffmpeg for fixture generation only");
        return;
    }
    for valid in [true, false] {
        let (_db, pool) = common::test_pool("b6-offline-ts").await;
        let paths = common::TestPaths::new("b6-offline-ts");
        let task = common::download_task(
            "offline",
            "http://source.invalid/live.m3u8".into(),
            "hls",
            "recording.ts",
            0,
            &paths,
            false,
        );
        db::insert_task_record(&pool, &task).await.unwrap();
        db::ensure_task_segments(&pool, &task).await.unwrap();
        hls_row(&pool, &task, "live", "ts", None, None, 0).await;
        std::fs::create_dir_all(&paths.temp).unwrap();
        let segment_path = paths.temp.join("saved.ts");
        let bytes = if valid {
            media("ts", false)
        } else {
            vec![0; 376]
        };
        std::fs::write(&segment_path, &bytes).unwrap();
        db::upsert_hls_segment(
            &pool,
            db::HlsSegmentUpsert {
                id: "saved",
                task_id: &task.id,
                media_sequence: 12,
                discontinuity_sequence: 0,
                uri: "http://source.invalid/saved.ts",
                local_path: &segment_path.to_string_lossy(),
                duration_ms: 1000,
                byte_range_start: None,
                byte_range_length: None,
                init_map_uri: None,
                init_map_local_path: None,
                init_map_byte_range_start: None,
                init_map_byte_range_length: None,
                key_method: None,
                key_uri: None,
                key_iv: None,
            },
        )
        .await
        .unwrap();
        db::update_hls_segment_status(
            &pool,
            "saved",
            bytes.len() as i64,
            SegmentStatus::Completed,
            0,
            None,
        )
        .await
        .unwrap();
        db::request_hls_finish(&pool, &task.id).await.unwrap();
        let result = HlsEngine::new(http())
            .download(common::headless_download_context(
                pool.clone(),
                task.clone(),
                CancellationToken::new(),
            ))
            .await;
        if valid {
            result.unwrap();
            let completed = db::get_task_record(&pool, &task.id).await.unwrap().unwrap();
            assert!(completed.final_path.as_ref().unwrap().ends_with(".ts"));
            assert_eq!(
                inspect(completed.final_path.as_ref().unwrap())["format"]["format_name"],
                "mpegts"
            );
        } else {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("hls_ffmpeg_missing"));
            assert!(!paths.final_path.with_extension("ts").exists());
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn dash_preserves_selected_quality_language_and_webvtt_on_resume() {
    if !ffmpeg() {
        eprintln!("requires ffmpeg/ffprobe");
        return;
    }
    let mpd = r#"<MPD type="static" mediaPresentationDuration="PT1S"><Period>
      <AdaptationSet contentType="video" mimeType="video/mp4"><Role value="main"/>
        <Representation id="low" bandwidth="100000" width="64" height="64"><SegmentList><SegmentURL media="low.mp4"/></SegmentList></Representation>
        <Representation id="high" bandwidth="200000" width="128" height="128"><SegmentList><SegmentURL media="high.mp4"/></SegmentList></Representation>
      </AdaptationSet>
      <AdaptationSet contentType="audio" mimeType="audio/mp4" lang="en"><Representation id="en" bandwidth="64000"><SegmentList><SegmentURL media="en.m4a"/></SegmentList></Representation></AdaptationSet>
      <AdaptationSet contentType="audio" mimeType="audio/mp4" lang="zh-CN"><Representation id="zh" bandwidth="64000"><SegmentList><SegmentURL media="zh.m4a"/></SegmentList></Representation></AdaptationSet>
      <AdaptationSet mimeType="application/webvtt" lang="en"><Representation id="sub" bandwidth="0"><SegmentList><SegmentURL media="sub.vtt"/></SegmentList></Representation></AdaptationSet>
    </Period></MPD>"#;
    let seen = Arc::new(Mutex::new(vec![]));
    let server = server(
        HashMap::from([
            (
                "/manifest".into(),
                ("application/dash+xml".into(), mpd.as_bytes().to_vec()),
            ),
            ("/low.mp4".into(), ("video/mp4".into(), media("mp4", false))),
            ("/zh.m4a".into(), ("audio/mp4".into(), media("m4a", true))),
            (
                "/sub.vtt".into(),
                ("text/vtt".into(), VTT.as_bytes().to_vec()),
            ),
        ]),
        seen.clone(),
    );
    let registry = EngineRegistry::new().unwrap();
    let url = format!("{}/manifest", server.base_url);
    let probe = registry.probe(request(url.clone())).await.unwrap();
    assert_eq!(probe.protocol, "dash");
    assert_eq!(probe.dash_tracks.len(), 5);
    assert!(probe.dash_tracks.iter().any(|track| track.id == "high"
        && track.selected
        && track.role.as_deref() == Some("main")));
    let (_db, pool) = common::test_pool("b6-dash-selected").await;
    let paths = common::TestPaths::new("b6-dash-selected");
    let mut task = common::download_task("dash", url, "dash", "media.mp4", 0, &paths, false);
    task.final_path = Some(
        paths
            .final_path
            .with_extension("mp4")
            .to_string_lossy()
            .into_owned(),
    );
    db::insert_task_record(&pool, &task).await.unwrap();
    db::ensure_task_segments(&pool, &task).await.unwrap();
    db::upsert_dash_task(
        &pool,
        db::DashTaskUpsert {
            task_id: &task.id,
            input_url: &task.url,
            manifest_url: &task.url,
            staging_dir: task.temp_path.as_ref().unwrap(),
            video_representation_id: Some("low"),
            video_bandwidth: None,
            video_codecs: None,
            audio_representation_id: Some("zh"),
            audio_bandwidth: None,
            audio_codecs: None,
            selected_subtitle_representation_id: Some("sub"),
            track_catalog_json: Some(&serde_json::to_string(&probe.dash_tracks).unwrap()),
            output_format: "mp4",
            segment_count: 0,
        },
    )
    .await
    .unwrap();
    // A new engine instance reads the original persisted choice.
    DashEngine::new(http())
        .download(common::headless_download_context(
            pool.clone(),
            task.clone(),
            CancellationToken::new(),
        ))
        .await
        .unwrap();
    let selected = db::get_dash_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(selected.video_representation_id.as_deref(), Some("low"));
    assert_eq!(selected.audio_representation_id.as_deref(), Some("zh"));
    assert_eq!(
        selected.selected_subtitle_representation_id.as_deref(),
        Some("sub")
    );
    assert!(!seen
        .lock()
        .unwrap()
        .iter()
        .any(|path| path == "/high.mp4" || path == "/en.m4a"));
    let output = inspect(
        db::get_task_record(&pool, &task.id)
            .await
            .unwrap()
            .unwrap()
            .final_path
            .as_ref()
            .unwrap(),
    );
    assert!(output["streams"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["codec_type"] == "audio" && s["tags"]["language"] == "zho"));
    assert!(output["streams"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["codec_name"] == "mov_text" && s["tags"]["language"] == "eng"));
    pool.close().await;
}

#[tokio::test]
async fn live_missing_segment_budget_records_gap_and_default_zero_stops() {
    if !ffmpeg() {
        return;
    }
    let segment = Arc::new(media("ts", false));
    for limit in [0, 1] {
        let polls = Arc::new(AtomicUsize::new(0));
        let segment = segment.clone();
        let server = common::TestServer::start(move |mut stream| {
            let mut buf = [0; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                return;
            }
            let line = String::from_utf8_lossy(&buf[..n]);
            let path = line
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let playlist = format!("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:40\n#EXTINF:1,\nvideo.ts\n#EXTINF:1,\nmissing.ts\n{}", if polls.fetch_add(usize::from(path == "/live.m3u8"), Ordering::SeqCst) > 0 { "#EXT-X-ENDLIST\n" } else { "" });
            let (status, body) = if path == "/live.m3u8" {
                (200, playlist.as_bytes())
            } else if path == "/video.ts" {
                (200, segment.as_slice())
            } else {
                (404, b"gone".as_slice())
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(body);
        });
        let (_db, pool) = common::test_pool("b6-gap").await;
        let paths = common::TestPaths::new("b6-gap");
        let task = common::download_task(
            "gap",
            format!("{}/live.m3u8", server.base_url),
            "hls",
            "recording.mp4",
            0,
            &paths,
            false,
        );
        db::insert_task_record(&pool, &task).await.unwrap();
        db::ensure_task_segments(&pool, &task).await.unwrap();
        hls_row(&pool, &task, "live", "mp4", None, None, limit).await;
        let result = HlsEngine::new(http())
            .download(common::headless_download_context(
                pool.clone(),
                task.clone(),
                CancellationToken::new(),
            ))
            .await;
        if limit == 0 {
            assert!(result.is_err());
            assert!(!paths.final_path.exists());
        } else {
            result.unwrap();
            let row = db::get_hls_task(&pool, &task.id).await.unwrap().unwrap();
            let gaps: serde_json::Value =
                serde_json::from_str(row.missing_segments_json.as_ref().unwrap()).unwrap();
            assert_eq!(gaps[0]["mediaSequence"], 41);
            assert_eq!(gaps[0]["startMs"], 1000);
            assert_eq!(gaps[0]["endMs"], 2000);
            assert!(gaps[0]["reason"].as_str().is_some());
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn suffixless_prefix_routing_stays_bounded_and_protected_dash_is_rejected() {
    let seen = Arc::new(Mutex::new(vec![]));
    let server = server(HashMap::from([
        ("/stream".into(), ("application/octet-stream".into(), PLAYLIST.as_bytes().to_vec())),
        ("/protected".into(), ("application/dash+xml".into(), b"<MPD type=\"static\"><Period><AdaptationSet><ContentProtection schemeIdUri=\"drm\"/></AdaptationSet></Period></MPD>".to_vec())),
    ]), seen);
    let registry = EngineRegistry::new().unwrap();
    assert_eq!(
        registry
            .probe(request(format!("{}/stream", server.base_url)))
            .await
            .unwrap()
            .protocol,
        "hls"
    );
    if ffmpeg() {
        let error = registry
            .probe(request(format!("{}/protected", server.base_url)))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("dash_drm_unsupported"), "{error}");
    }
}

#[tokio::test]
async fn suffixless_torrent_and_metalink_use_auth_and_strip_secrets_on_redirect() {
    const TORRENT: &[u8] =
        b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces20:ABCDEFGHIJKLMNOPQRST6:lengthi1eee";
    let destination_headers = Arc::new(Mutex::new(Vec::new()));
    let recorded = destination_headers.clone();
    let destination = common::TestServer::start(move |mut stream| {
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            return;
        }
        recorded
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase());
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/x-bittorrent\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", TORRENT.len());
        let _ = stream.write_all(TORRENT);
    });
    let target = format!("{}/payload", destination.base_url);
    let source = common::TestServer::start(move |mut stream| {
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&buf[..n]);
        if !text.lines().any(|line| {
            line.split_once(':').is_some_and(|(key, value)| {
                key.eq_ignore_ascii_case("Authorization") && value.trim() == "Basic dXNlcjpwYXNz"
            })
        }) {
            let _ = stream.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        let head = text.starts_with("HEAD ");
        let path = text.split_whitespace().nth(1).unwrap();
        if path == "/redirect" && !head {
            let _ = write!(stream, "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        }
        let metalink = format!("<metalink xmlns=\"urn:ietf:params:xml:ns:metalink\"><file name=\"payload.bin\"><size>1</size><url>{target}</url></file></metalink>");
        let (mime, body) = if path == "/metalink" {
            ("application/metalink4+xml", metalink.as_bytes())
        } else {
            ("application/x-bittorrent", TORRENT)
        };
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        if !head {
            let _ = stream.write_all(body);
        }
    });
    let registry = EngineRegistry::new().unwrap();
    for (path, protocol) in [
        ("torrent", "bt"),
        ("metalink", "metalink"),
        ("redirect", "bt"),
    ] {
        let mut probe_request = request(format!("{}/{path}", source.base_url));
        probe_request.credentials = Some(db::TaskCredentials {
            username: "user".into(),
            password: "pass".into(),
            private_key_data: None,
            private_key_passphrase: None,
        });
        probe_request.request_headers = vec![("Cookie".into(), "secret=value".into())];
        assert_eq!(
            registry.probe(probe_request).await.unwrap().protocol,
            protocol
        );
    }
    let recorded = destination_headers.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert!(!recorded[0].contains("authorization:"));
    assert!(!recorded[0].contains("cookie:"));
}
