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
