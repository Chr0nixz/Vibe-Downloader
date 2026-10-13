mod common;

use std::time::{Duration, Instant};

#[tokio::test]
async fn database_guard_reclaims_files_for_an_unclosed_pool() {
    let (guard, pool) = common::test_pool("guard-unclosed").await;
    let path = guard.database_path().to_owned();
    let sidecars = [
        path.clone(),
        path.with_extension("sqlite-wal"),
        path.with_extension("sqlite-shm"),
    ];

    // Deliberately omit pool.close(). The guard owns a shared handle and must
    // still release it before deleting the database files.
    drop(pool);
    drop(guard);

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && sidecars.iter().any(|file| file.exists()) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    assert!(
        sidecars.iter().all(|file| !file.exists()),
        "database guard left files behind: {:?}",
        sidecars
    );
}
