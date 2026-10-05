//! ARC-60: real librqbit state errors must exit the production engine loop.

use super::*;
use librqbit::storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use std::sync::atomic::AtomicBool;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
enum Failure {
    Disk,
    Opaque,
    None,
}

#[derive(Clone)]
struct TestStorage(Failure);

impl StorageFactory for TestStorage {
    type Storage = Self;

    fn create(&self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> anyhow::Result<Self> {
        Ok(self.clone())
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

impl TorrentStorage for TestStorage {
    fn init(&mut self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> anyhow::Result<()> {
        Ok(())
    }

    fn pread_exact(&self, _: usize, _: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        buf.fill(0);
        Ok(())
    }

    fn pwrite_all(&self, _: usize, _: u64, _: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    fn remove_file(&self, _: usize, _: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn remove_directory_if_empty(&self, _: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn ensure_file_length(&self, _: usize, _: u64) -> anyhow::Result<()> {
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        // Initialization runs asynchronously after add_torrent returns. This
        // fails that live state machine, not the metadata or source parser.
        match self.0 {
            Failure::Disk => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected storage failure",
            )
            .into()),
            Failure::Opaque => Err(anyhow::anyhow!("opaque failure, unrelated to disk full")),
            Failure::None => Ok(Box::new(self.clone())),
        }
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("vibe-arc60-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn run_case(failure: Failure, expected_code: Option<&str>) {
    let directory = TestDirectory::new();
    let pool = db::connect(&directory.0.join("tasks.sqlite"))
        .await
        .unwrap()
        .pool;
    let torrent_path = directory.0.join("fixture.torrent");
    std::fs::write(
        &torrent_path,
        b"d4:infod4:name3:foo12:piece lengthi16384e6:pieces20:ABCDEFGHIJKLMNOPQRST6:lengthi1eee",
    )
    .unwrap();
    let url = Url::from_file_path(&torrent_path).unwrap().to_string();
    let output = probe_torrent(
        &url,
        &None,
        &None,
        &ResolvedProxyConfig::default(),
        &crate::download::NetworkClientFactory::new(),
        &crate::download::network_policy::NetworkPolicy::default(),
    )
    .await;
    let output = output.expect("valid local torrent");
    let now = crate::models::task::now_iso();
    let task = crate::models::TaskRecord {
        id: "arc60".to_string(),
        url,
        final_url: None,
        protocol: "bt".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: "foo".to_string(),
        save_dir: directory.0.to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 1,
        downloaded_bytes: 0,
        status: TaskStatus::Downloading,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: false,
        supports_multi_file: false,
        source_key: output.source_key,
        connection_count: 1,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: crate::models::TaskPriority::Normal,
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
        hash_status: crate::models::HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.clone(),
        updated_at: now,
        files_version: 0,
    };
    db::insert_task_record(&pool, &task).await.unwrap();
    db::ensure_task_segments(&pool, &task).await.unwrap();
    let engine = BtEngine::default();
    let session = Session::new_with_opts(
        directory.0.clone(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            default_storage_factory: Some(TestStorage(failure).boxed()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let api = Arc::new(Api::new(session.clone(), None));
    let key =
        BtEngine::compute_session_key(&task.save_dir, &ResolvedProxyConfig::default(), &task.id)
            .await;
    engine.sessions.lock().await.insert(
        key,
        BtSessionEntry {
            api: api.clone(),
            active_task_count: 0,
        },
    );
    let cancel = CancellationToken::new();
    if matches!(failure, Failure::None) {
        // Cancel before the download starts. The 1-byte fixture can complete
        // faster than any sleep-based timer on fast machines, which would
        // leave the task Completed instead of Paused.
        cancel.cancel();
    }
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        engine.download(DownloadContext {
            app: None,
            pool: pool.clone(),
            task: task.clone(),
            cancel_token: cancel.clone(),
            finish: Arc::new(AtomicBool::new(false)),
            finish_notify: Arc::new(tokio::sync::Notify::new()),
            speed_limiter: GlobalSpeedLimiter::disabled(),
            connection_limit: 1,
            request_headers: Vec::new(),
            proxy_config: ResolvedProxyConfig::default(),
            network_policy: crate::download::network_policy::NetworkPolicy::default(),
        }),
    )
    .await
    .expect("BT runtime must exit instead of retaining its slot")
    .map_err(String::from);
    if let Some(expected_code) = expected_code {
        let error = result.as_ref().expect_err("injected runtime failure");
        let payload: AppErrorPayload = serde_json::from_str(error).unwrap();
        assert_eq!(payload.code, expected_code);
        assert!(
            !cancel.is_cancelled(),
            "runtime failure is not user cancellation"
        );
        assert!(
            engine.sessions.lock().await.is_empty(),
            "release the session before returning the error"
        );
        assert!(api.api_torrent_list().torrents.is_empty());
        let diagnostics = db::list_request_diagnostics_page(&pool, &task.id, None, 20)
            .await
            .unwrap();
        assert!(diagnostics.iter().any(|entry| entry.method == "BT RUNTIME"
            && entry
                .error_message
                .as_deref()
                .is_some_and(|message| message.contains(expected_code))));
        if expected_code == "disk_write_failed" {
            assert!(payload
                .actions
                .iter()
                .any(|action| action == "free_disk_space"));
        }
    } else {
        result.as_ref().unwrap();
    }
    let downloads = Arc::new(Mutex::new(HashMap::from([(
        task.id.clone(),
        crate::DownloadControl {
            cancel_token: cancel.clone(),
            finish: Arc::new(AtomicBool::new(false)),
            finish_notify: Arc::new(tokio::sync::Notify::new()),
            speed_limiter: crate::download::GlobalSpeedLimiter::disabled(),
            handle: None,
            source_key: task.source_key.clone(),
            connection_slots: 1,
        },
    )])));
    let headers = Arc::new(Mutex::new(HashMap::new()));
    crate::scheduler::converge_download_outcome(
        &downloads,
        &headers,
        &Arc::new(crate::TaskRuntimeLocks::default()),
        None,
        &pool,
        &task.id,
        cancel.is_cancelled(),
        result,
    )
    .await;
    assert!(downloads.lock().await.is_empty());
    let current = db::get_task_record(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(
        current.status,
        if expected_code.is_some() {
            TaskStatus::Failed
        } else {
            TaskStatus::Paused
        }
    );
    if let Some(expected_code) = expected_code {
        assert_eq!(current.error_code.as_deref(), Some(expected_code));
    }
    session.stop().await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc60_disk_error_exits_the_engine_and_releases_the_slot() {
    run_case(Failure::Disk, Some("disk_write_failed")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc60_unknown_runtime_error_is_not_classified_by_english_words() {
    run_case(Failure::Opaque, Some("bt_runtime_failed")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc60_cancel_remains_paused_without_runtime_failure() {
    run_case(Failure::None, None).await;
}
