use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use sqlx::{migrate::MigrateError, sqlite::SqlitePoolOptions, SqlitePool, Transaction};

pub struct DbConnection {
    pub pool: SqlitePool,
    pub data_was_reset: bool,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DatabaseRecovery {
    pub reason: String,
    pub message: String,
    pub database_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub backup_verified: bool,
}

pub enum DatabaseConnectOutcome {
    Ready(DbConnection),
    RecoveryRequired(DatabaseRecovery),
}

pub async fn connect(db_path: &Path) -> Result<DbConnection, String> {
    match connect_for_startup(db_path).await? {
        DatabaseConnectOutcome::Ready(connection) => Ok(connection),
        DatabaseConnectOutcome::RecoveryRequired(recovery) => Err(recovery.message),
    }
}

/// ARC-06: Begin a write transaction that acquires the reserved lock immediately.
///
/// Default `pool.begin()` is `BEGIN` (DEFERRED). A deferred reader that later
/// upgrades to a writer can fail with `SQLITE_BUSY_SNAPSHOT` when another
/// connection commits between the snapshot read and the first write. State
/// transitions always write, so IMMEDIATE removes that upgrade race.
pub async fn begin_immediate(
    pool: &SqlitePool,
) -> Result<Transaction<'static, sqlx::Sqlite>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

pub async fn connect_for_startup(db_path: &Path) -> Result<DatabaseConnectOutcome, String> {
    let pool = open_pool(db_path).await?;

    match run_migrations(&pool).await {
        Ok(()) => {
            tracing::info!(db_path = %db_path.display(), "database connected and migrations applied");
            Ok(DatabaseConnectOutcome::Ready(DbConnection {
                pool,
                data_was_reset: false,
                backup_path: None,
            }))
        }
        Err(error) if requires_database_recovery(&error) => {
            tracing::warn!(
                db_path = %db_path.display(),
                error = %error,
                "database migration requires explicit recovery"
            );
            let reason = migration_recovery_reason(&error).to_string();
            let message = format!("Database migration requires recovery: {error}");
            let backup = create_verified_backup(&pool, db_path).await;
            pool.close().await;
            let (backup_path, backup_verified) = match backup {
                Ok(path) => (Some(path), true),
                Err(backup_error) => {
                    tracing::error!(error = %backup_error, "database recovery backup failed");
                    (None, false)
                }
            };
            Ok(DatabaseConnectOutcome::RecoveryRequired(DatabaseRecovery {
                reason,
                message,
                database_path: db_path.to_path_buf(),
                backup_path,
                backup_verified,
            }))
        }
        // Note: as of sqlx 0.9.0, `VersionTooOld` is declared on `MigrateError`
        // but is never constructed by `Migrator::run_direct`. The actual
        // "user downgraded the app" scenario fires `VersionMissing(n)` for
        // each applied migration not present in the embedded set, which is
        // already covered by the rebuild arm above. We keep this explicit
        // `Err(...)` arm as defensive programming: if a future sqlx release
        // starts returning `VersionTooOld`, we surface a clear actionable
        // error rather than silently dropping into the generic catch-all
        // below (which would otherwise describe it as a generic migration
        // failure with no upgrade guidance).
        Err(MigrateError::VersionTooOld(current, target)) => Err(format!(
            "This Vibe Downloader version is older than the database on disk \
             (database reports migration {current}, this build only ships up to {target}). \
             Please upgrade the app instead of resetting the database — resetting would \
             silently downgrade and lose newer-version data."
        )),
        Err(error) => Err(format!("Migration failed: {error}")),
    }
}

fn migration_recovery_reason(error: &MigrateError) -> &'static str {
    match error {
        MigrateError::VersionMissing(_) => "migration_missing",
        MigrateError::VersionMismatch(_) => "migration_mismatch",
        MigrateError::Dirty(_) => "migration_dirty",
        _ => "migration_failed",
    }
}

async fn open_pool(db_path: &Path) -> Result<SqlitePool, String> {
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    SqlitePoolOptions::new()
        .max_connections(16)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                // WAL + synchronous=NORMAL is the standard high-throughput SQLite config (durable across
                // crashes, may lose the last txn on power loss). busy_timeout=5000ms lets writers wait
                // out lock contention from the scheduler/worker pool instead of failing with SQLITE_BUSY.
                sqlx::query("PRAGMA foreign_keys = ON")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("PRAGMA journal_mode = WAL")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("PRAGMA busy_timeout = 5000")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("PRAGMA synchronous = NORMAL")
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .map_err(|e| format!("Database connection failed: {e}"))
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
    sqlx::migrate!("./src/db/migrations").run(pool).await
}

fn requires_database_recovery(error: &MigrateError) -> bool {
    // History mismatches and interrupted migrations are recoverable only after
    // the original file is preserved and the user explicitly approves reset.
    // VersionTooOld remains a hard upgrade error rather than offering downgrade.
    matches!(
        error,
        MigrateError::VersionMissing(_) | MigrateError::VersionMismatch(_) | MigrateError::Dirty(_)
    )
}

pub fn reset_database_files(db_path: &Path) -> Result<(), String> {
    for path in sqlite_database_paths(db_path) {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Failed to remove stale database file {}: {error}",
                    path.display()
                ));
            }
        }
    }

    Ok(())
}

async fn create_verified_backup(pool: &SqlitePool, db_path: &Path) -> Result<PathBuf, String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_path = db_path.with_extension(format!("db.bak-{timestamp}"));
    if backup_path.exists() {
        std::fs::remove_file(&backup_path)
            .map_err(|e| format!("Failed to replace stale database backup: {e}"))?;
    }
    sqlx::query("VACUUM INTO ?")
        .bind(backup_path.to_string_lossy().to_string())
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to create a consistent database backup: {e}"))?;

    let backup_url = format!("sqlite:{}?mode=ro", backup_path.display());
    let verification_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&backup_url)
        .await
        .map_err(|e| format!("Failed to open the database backup for verification: {e}"))?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&verification_pool)
        .await
        .map_err(|e| format!("Failed to verify the database backup: {e}"))?;
    verification_pool.close().await;
    if integrity != "ok" {
        let _ = std::fs::remove_file(&backup_path);
        return Err(format!(
            "Database backup integrity check failed: {integrity}"
        ));
    }
    Ok(backup_path)
}

pub async fn wal_checkpoint(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await
        .map_err(|e| format!("WAL checkpoint failed: {e}"))?;
    Ok(())
}

pub fn wal_file_size_bytes(db_path: &Path) -> u64 {
    let wal_path = sqlite_sidecar_path(db_path, "-wal");
    std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0)
}

fn sqlite_database_paths(db_path: &Path) -> [PathBuf; 4] {
    [
        db_path.to_path_buf(),
        sqlite_sidecar_path(db_path, "-wal"),
        sqlite_sidecar_path(db_path, "-shm"),
        sqlite_sidecar_path(db_path, "-journal"),
    ]
}

fn sqlite_sidecar_path(db_path: &Path, suffix: &str) -> PathBuf {
    let mut path = OsString::from(db_path.as_os_str());
    path.push(suffix);
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_recovery_for_version_history_mismatch_and_dirty() {
        assert!(requires_database_recovery(&MigrateError::VersionMissing(7)));
        assert!(requires_database_recovery(&MigrateError::VersionMismatch(
            7
        )));
        assert!(requires_database_recovery(&MigrateError::Dirty(7)));
        assert!(!requires_database_recovery(&MigrateError::VersionTooOld(
            3, 7
        )));
    }
}
