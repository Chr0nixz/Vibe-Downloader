use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use sqlx::{migrate::MigrateError, sqlite::SqlitePoolOptions, SqlitePool};

pub struct DbConnection {
    pub pool: SqlitePool,
    pub data_was_reset: bool,
    pub backup_path: Option<PathBuf>,
}

pub async fn connect(db_path: &Path) -> Result<DbConnection, String> {
    let pool = open_pool(db_path).await?;

    match run_migrations(&pool).await {
        Ok(()) => {
            tracing::info!(db_path = %db_path.display(), "database connected and migrations applied");
            Ok(DbConnection {
                pool,
                data_was_reset: false,
                backup_path: None,
            })
        }
        Err(error) if should_rebuild_database_after_migration_error(&error) => {
            tracing::warn!(
                db_path = %db_path.display(),
                error = %error,
                "rebuilding database after recognized migration history reset"
            );
            pool.close().await;
            let backup_path = backup_database_files(db_path)?;
            tracing::info!(
                db_path = %db_path.display(),
                backup_path = %backup_path.display(),
                "database backed up before rebuild"
            );
            remove_database_files(db_path)?;

            let pool = open_pool(db_path).await?;
            run_migrations(&pool)
                .await
                .map_err(|e| format!("Migration failed after database rebuild: {e}"))?;
            tracing::info!(db_path = %db_path.display(), "database rebuilt and migrations applied");
            Ok(DbConnection {
                pool,
                data_was_reset: true,
                backup_path: Some(backup_path),
            })
        }
        Err(error) => Err(format!("Migration failed: {error}")),
    }
}

async fn open_pool(db_path: &Path) -> Result<SqlitePool, String> {
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    SqlitePoolOptions::new()
        .max_connections(16)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
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

fn should_rebuild_database_after_migration_error(error: &MigrateError) -> bool {
    matches!(error, MigrateError::VersionMissing(_) | MigrateError::VersionMismatch(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilds_only_for_version_history_mismatch() {
        assert!(should_rebuild_database_after_migration_error(
            &MigrateError::VersionMissing(7)
        ));
        assert!(should_rebuild_database_after_migration_error(
            &MigrateError::VersionMismatch(7)
        ));
        assert!(!should_rebuild_database_after_migration_error(
            &MigrateError::Dirty(7)
        ));
        assert!(!should_rebuild_database_after_migration_error(
            &MigrateError::VersionTooOld(3, 7)
        ));
    }
}

fn remove_database_files(db_path: &Path) -> Result<(), String> {
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

fn backup_database_files(db_path: &Path) -> Result<PathBuf, String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_path = db_path.with_extension(format!("db.bak-{timestamp}"));
    std::fs::copy(db_path, &backup_path).map_err(|e| {
        format!(
            "Failed to backup database to {}: {e}",
            backup_path.display()
        )
    })?;
    let wal_path = sqlite_sidecar_path(db_path, "-wal");
    if wal_path.exists() {
        let wal_backup = sqlite_sidecar_path(&backup_path, "-wal");
        let _ = std::fs::copy(&wal_path, &wal_backup);
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
