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
    pub seeding_enabled: bool,
    pub seed_ratio_limit: Option<f64>,
    pub seed_time_limit_seconds: Option<i64>,
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
            piece_count, private, trackers_json, seeding_enabled, seed_ratio_limit,
            seed_time_limit_seconds, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            info_hash = excluded.info_hash,
            name = excluded.name,
            magnet_uri = excluded.magnet_uri,
            torrent_blob = COALESCE(excluded.torrent_blob, torrent_tasks.torrent_blob),
            piece_length = excluded.piece_length,
            piece_count = excluded.piece_count,
            private = excluded.private,
            trackers_json = excluded.trackers_json,
            seeding_enabled = torrent_tasks.seeding_enabled,
            seed_ratio_limit = COALESCE(torrent_tasks.seed_ratio_limit, excluded.seed_ratio_limit),
            seed_time_limit_seconds = COALESCE(torrent_tasks.seed_time_limit_seconds, excluded.seed_time_limit_seconds),
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
    .bind(if upsert.seeding_enabled { 1 } else { 0 })
    .bind(upsert.seed_ratio_limit)
    .bind(upsert.seed_time_limit_seconds)
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
    pub piece_count: i64,
    pub piece_bitfield_base64: Option<&'a str>,
    pub peer_count: i64,
    pub seed_count: i64,
    pub dht_status: Option<&'a str>,
    pub trackers_json: Option<&'a str>,
    pub upload_bytes: i64,
    pub upload_speed_bps: i64,
    pub ratio: f64,
    pub seeding_enabled: bool,
    pub seeding_state: &'a str,
    pub last_error_code: Option<&'a str>,
    pub last_error_message: Option<&'a str>,
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
            task_id, metadata_status, completed_pieces, verified_pieces, piece_count,
            piece_bitfield_base64, peer_count, seed_count, dht_status, trackers_json,
            upload_bytes, upload_speed_bps, ratio, seeding_enabled, seeding_state,
            last_error_code, last_error_message, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            metadata_status = excluded.metadata_status,
            completed_pieces = excluded.completed_pieces,
            verified_pieces = excluded.verified_pieces,
            piece_count = excluded.piece_count,
            piece_bitfield_base64 = excluded.piece_bitfield_base64,
            peer_count = excluded.peer_count,
            seed_count = excluded.seed_count,
            dht_status = excluded.dht_status,
            trackers_json = excluded.trackers_json,
            upload_bytes = excluded.upload_bytes,
            upload_speed_bps = excluded.upload_speed_bps,
            ratio = excluded.ratio,
            seeding_enabled = excluded.seeding_enabled,
            seeding_state = excluded.seeding_state,
            last_error_code = excluded.last_error_code,
            last_error_message = excluded.last_error_message,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(task_id)
    .bind(snapshot.metadata_status)
    .bind(snapshot.completed_pieces)
    .bind(snapshot.verified_pieces)
    .bind(snapshot.piece_count)
    .bind(snapshot.piece_bitfield_base64)
    .bind(snapshot.peer_count)
    .bind(snapshot.seed_count)
    .bind(snapshot.dht_status)
    .bind(snapshot.trackers_json)
    .bind(snapshot.upload_bytes)
    .bind(snapshot.upload_speed_bps)
    .bind(snapshot.ratio)
    .bind(if snapshot.seeding_enabled { 1 } else { 0 })
    .bind(snapshot.seeding_state)
    .bind(snapshot.last_error_code)
    .bind(snapshot.last_error_message)
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
        SELECT task_id, metadata_status, completed_pieces, verified_pieces, piece_count,
               piece_bitfield_base64, peer_count, seed_count, dht_status, trackers_json,
               upload_bytes, upload_speed_bps, ratio, seeding_enabled, seeding_state,
               last_error_code, last_error_message, updated_at
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
        piece_count: row.get("piece_count"),
        piece_bitfield_base64: row.get("piece_bitfield_base64"),
        peer_count: row.get("peer_count"),
        seed_count: row.get("seed_count"),
        dht_status: row.get("dht_status"),
        trackers_json: row.get("trackers_json"),
        upload_bytes: row.get("upload_bytes"),
        upload_speed_bps: row.get("upload_speed_bps"),
        ratio: row.get("ratio"),
        seeding_enabled: row.get::<i64, _>("seeding_enabled") != 0,
        seeding_state: row.get("seeding_state"),
        last_error_code: row.get("last_error_code"),
        last_error_message: row.get("last_error_message"),
        updated_at: row.get("updated_at"),
    }))
}

pub async fn torrent_seeding_enabled(pool: &SqlitePool, task_id: &str) -> Result<bool, String> {
    let value: Option<i64> =
        sqlx::query_scalar("SELECT seeding_enabled FROM torrent_tasks WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(value.unwrap_or(0) != 0)
}

/// F-1: Reads the configured seed ratio limit for a torrent task.
/// Returns `None` when no limit is set or the task has no torrent row.
pub async fn torrent_seed_ratio_limit(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<f64>, String> {
    let value: Option<Option<f64>> =
        sqlx::query_scalar("SELECT seed_ratio_limit FROM torrent_tasks WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(value.flatten())
}

/// FUN-11: Reads the configured seed time limit (seconds) for a torrent task.
pub async fn torrent_seed_time_limit_seconds(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<i64>, String> {
    let value: Option<Option<i64>> =
        sqlx::query_scalar("SELECT seed_time_limit_seconds FROM torrent_tasks WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(value.flatten().filter(|seconds| *seconds > 0))
}

/// FUN-11: Returns both seeding policy fields so the UI can round-trip without
/// clearing unread limits.
pub async fn torrent_seeding_policy(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<(Option<f64>, Option<i64>), String> {
    let row = sqlx::query(
        r#"
        SELECT seed_ratio_limit, seed_time_limit_seconds
        FROM torrent_tasks
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some(row) = row else {
        return Ok((None, None));
    };
    let ratio: Option<f64> = row.try_get("seed_ratio_limit").unwrap_or(None);
    let time: Option<i64> = row
        .try_get::<Option<i64>, _>("seed_time_limit_seconds")
        .unwrap_or(None)
        .filter(|seconds| *seconds > 0);
    Ok((ratio, time))
}

pub async fn update_torrent_seeding(
    pool: &SqlitePool,
    task_id: &str,
    enabled: bool,
    ratio_limit: Option<f64>,
    time_limit_seconds: Option<i64>,
    update_limits: bool,
) -> Result<(), String> {
    // FUN-11: toggle-only updates must not wipe existing ratio/time policy.
    if update_limits {
        sqlx::query(
            r#"
            UPDATE torrent_tasks
            SET seeding_enabled = ?, seed_ratio_limit = ?, seed_time_limit_seconds = ?, updated_at = ?
            WHERE task_id = ?
            "#,
        )
        .bind(if enabled { 1 } else { 0 })
        .bind(ratio_limit)
        .bind(time_limit_seconds)
        .bind(crate::models::task::now_iso())
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query(
            r#"
            UPDATE torrent_tasks
            SET seeding_enabled = ?, updated_at = ?
            WHERE task_id = ?
            "#,
        )
        .bind(if enabled { 1 } else { 0 })
        .bind(crate::models::task::now_iso())
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
