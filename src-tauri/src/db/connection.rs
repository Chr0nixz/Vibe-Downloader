use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use sqlx::{migrate::MigrateError, sqlite::SqlitePoolOptions, SqlitePool};

pub struct DbConnection {
    pub pool: SqlitePool,
    pub data_was_reset: bool,
}

pub async fn connect(db_path: &Path) -> Result<DbConnection, String> {
    let pool = open_pool(db_path).await?;

    match run_migrations(&pool).await {
        Ok(()) => {
            tracing::info!(db_path = %db_path.display(), "database connected and migrations applied");
            Ok(DbConnection {
                pool,
                data_was_reset: false,
            })
        }
        Err(error) if should_rebuild_database_after_migration_error(&error) => {
            tracing::warn!(
                db_path = %db_path.display(),
                error = %error,
                "rebuilding development database after migration history reset"
            );
            pool.close().await;
            remove_database_files(db_path)?;

            let pool = open_pool(db_path).await?;
            run_migrations(&pool)
                .await
                .map_err(|e| format!("Migration failed after database rebuild: {e}"))?;
            tracing::info!(db_path = %db_path.display(), "database rebuilt and migrations applied");
            Ok(DbConnection {
                pool,
                data_was_reset: true,
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
    cfg!(debug_assertions)
        && matches!(
            error,
            MigrateError::VersionMissing(_) | MigrateError::VersionMismatch(_)
        )
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
