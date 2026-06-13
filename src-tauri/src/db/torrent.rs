use sqlx::{Row, SqlitePool};

pub struct TaskRemoteMetadataUpdate<'a> {
    pub final_url: &'a str,
    pub total_size: i64,
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub supports_resume: bool,
    pub supports_parallel: bool,
    pub supports_multi_file: bool,
    pub source_key: &'a str,
}

pub async fn update_task_remote_metadata(
    pool: &SqlitePool,
    task_id: &str,
    update: TaskRemoteMetadataUpdate<'_>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET final_url = ?, total_size = ?, etag = ?, last_modified = ?,
            content_type = ?, supports_resume = ?, supports_parallel = ?,
            supports_multi_file = ?, source_key = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(update.final_url)
    .bind(update.total_size)
    .bind(update.etag)
    .bind(update.last_modified)
    .bind(update.content_type)
    .bind(if update.supports_resume { 1 } else { 0 })
    .bind(if update.supports_parallel { 1 } else { 0 })
    .bind(if update.supports_multi_file { 1 } else { 0 })
    .bind(update.source_key)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub struct TaskTorrentMetadataUpdate<'a> {
    pub final_url: &'a str,
    pub file_name: &'a str,
    pub total_size: i64,
    pub source_key: &'a str,
    pub supports_multi_file: bool,
}

pub async fn update_task_torrent_metadata(
    pool: &SqlitePool,
    task_id: &str,
    update: TaskTorrentMetadataUpdate<'_>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET final_url = ?, file_name = ?, total_size = ?, content_type = ?,
            supports_resume = 1, supports_parallel = 1, supports_multi_file = ?,
            source_key = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(update.final_url)
    .bind(update.file_name)
    .bind(update.total_size)
    .bind("application/x-bittorrent")
    .bind(if update.supports_multi_file { 1 } else { 0 })
    .bind(update.source_key)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub struct TorrentTaskUpsert<'a> {
    pub info_hash: &'a str,
    pub name: &'a str,
    pub magnet_uri: Option<&'a str>,
    pub torrent_blob: Option<&'a [u8]>,
    pub piece_length: i64,
    pub piece_count: i64,
    pub private: bool,
    pub trackers_json: Option<&'a str>,
}

pub async fn upsert_torrent_task(
    pool: &SqlitePool,
    task_id: &str,
    upsert: TorrentTaskUpsert<'_>,
) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO torrent_tasks (
            task_id, info_hash, name, magnet_uri, torrent_blob, piece_length,
            piece_count, private, trackers_json, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            info_hash = excluded.info_hash,
            name = excluded.name,
            magnet_uri = excluded.magnet_uri,
            torrent_blob = COALESCE(excluded.torrent_blob, torrent_tasks.torrent_blob),
            piece_length = excluded.piece_length,
            piece_count = excluded.piece_count,
            private = excluded.private,
            trackers_json = excluded.trackers_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(task_id)
    .bind(upsert.info_hash)
    .bind(upsert.name)
    .bind(upsert.magnet_uri)
    .bind(upsert.torrent_blob)
    .bind(upsert.piece_length)
    .bind(upsert.piece_count)
    .bind(if upsert.private { 1 } else { 0 })
    .bind(upsert.trackers_json)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub struct TorrentRuntimeSnapshotUpsert<'a> {
    pub metadata_status: &'a str,
    pub completed_pieces: i64,
    pub verified_pieces: i64,
    pub peer_count: i64,
    pub seed_count: i64,
    pub upload_bytes: i64,
    pub upload_speed_bps: i64,
    pub ratio: f64,
}

pub async fn upsert_torrent_runtime_snapshot(
    pool: &SqlitePool,
    task_id: &str,
    snapshot: TorrentRuntimeSnapshotUpsert<'_>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO torrent_runtime_snapshots (
            task_id, metadata_status, completed_pieces, verified_pieces, peer_count,
            seed_count, upload_bytes, upload_speed_bps, ratio, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            metadata_status = excluded.metadata_status,
            completed_pieces = excluded.completed_pieces,
            verified_pieces = excluded.verified_pieces,
            peer_count = excluded.peer_count,
            seed_count = excluded.seed_count,
            upload_bytes = excluded.upload_bytes,
            upload_speed_bps = excluded.upload_speed_bps,
            ratio = excluded.ratio,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(task_id)
    .bind(snapshot.metadata_status)
    .bind(snapshot.completed_pieces)
    .bind(snapshot.verified_pieces)
    .bind(snapshot.peer_count)
    .bind(snapshot.seed_count)
    .bind(snapshot.upload_bytes)
    .bind(snapshot.upload_speed_bps)
    .bind(snapshot.ratio)
    .bind(&updated_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_torrent_runtime_snapshot(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<crate::models::TorrentRuntimeSnapshotRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT task_id, metadata_status, completed_pieces, verified_pieces, peer_count,
               seed_count, upload_bytes, upload_speed_bps, ratio, updated_at
        FROM torrent_runtime_snapshots
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(row.map(|row| crate::models::TorrentRuntimeSnapshotRecord {
        task_id: row.get("task_id"),
        metadata_status: row.get("metadata_status"),
        completed_pieces: row.get("completed_pieces"),
        verified_pieces: row.get("verified_pieces"),
        peer_count: row.get("peer_count"),
        seed_count: row.get("seed_count"),
        upload_bytes: row.get("upload_bytes"),
        upload_speed_bps: row.get("upload_speed_bps"),
        ratio: row.get("ratio"),
        updated_at: row.get("updated_at"),
    }))
}
