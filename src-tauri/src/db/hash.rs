use sqlx::SqlitePool;

use crate::models::HashVerificationStatus;

pub async fn update_hash_verification(
    pool: &SqlitePool,
    task_id: &str,
    actual_hash_sha256: Option<&str>,
    status: HashVerificationStatus,
    error_message: Option<&str>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();
    let verified_at = if matches!(
        status,
        HashVerificationStatus::Verified | HashVerificationStatus::Failed
    ) {
        Some(updated_at.as_str())
    } else {
        None
    };

    sqlx::query(
        r#"
        UPDATE tasks
        SET actual_hash_sha256 = ?, hash_status = ?, hash_error = ?,
            hash_verified_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(actual_hash_sha256)
    .bind(status.as_str())
    .bind(error_message)
    .bind(verified_at)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// ARC-46: whether any completed task still has a hash verification in flight
/// (`hash_status = 'pending'`). Both verify paths set Pending before the
/// potentially minutes-long hashing and resolve to Verified/Failed afterwards,
/// so this is the completion-action gate: firing shutdown while the last file
/// is still being hashed leaves hash_status stuck at Pending and forces a
/// manual re-verify.
pub async fn any_completed_task_hash_pending(pool: &SqlitePool) -> Result<bool, String> {
    let pending = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE status = 'completed' AND hash_status = 'pending')",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(pending != 0)
}
