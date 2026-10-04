//! ARC-59: hold task ownership through worker shutdown, file cleanup and row deletion.

use std::collections::{HashMap, HashSet};

use sqlx::SqlitePool;
use tokio::sync::Mutex;

use super::actions::{delete_paths_off_runtime, FileDeleteRequest};
use crate::{
    db,
    download::EngineRegistry,
    events::evict_task_files_version,
    models::{AppErrorPayload, TaskStatus},
    DownloadControl, RequestHeaders, TaskRuntimeLocks,
};

pub struct TaskDeletion<'a> {
    pub pool: &'a SqlitePool,
    pub downloads: &'a Mutex<HashMap<String, DownloadControl>>,
    pub request_headers: &'a Mutex<HashMap<String, RequestHeaders>>,
    pub runtime_locks: &'a TaskRuntimeLocks,
    pub engines: &'a EngineRegistry,
    pub drain_grace: std::time::Duration,
}

impl TaskDeletion<'_> {
    pub async fn delete(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let guard = self.runtime_locks.lock(id).await;
        let result = self.delete_locked(id, delete_file).await;
        drop(guard);
        let stopping = self
            .downloads
            .lock()
            .await
            .get(id)
            .is_some_and(|control| control.cancel_token.is_cancelled());
        if !stopping {
            self.runtime_locks.evict(id).await;
        }
        result
    }

    pub async fn delete_many(&self, ids: &[String], delete_file: bool) -> Result<u32, String> {
        let mut seen = HashSet::new();
        let mut deleted = 0;
        let mut first_error = None;
        for id in ids {
            if !seen.insert(id) {
                continue;
            }
            match self.delete(id, delete_file).await {
                Ok(()) => deleted += 1,
                Err(error) => {
                    tracing::warn!(task_id = %id, error = %error, "task deletion failed; record retained");
                    first_error.get_or_insert(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(deleted),
        }
    }

    async fn delete_locked(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let task = db::get_task_record(self.pool, id).await?;
        crate::remove_and_drain_control(self.downloads, self.request_headers, id, self.drain_grace)
            .await?;
        let Some(task) = task else {
            return Ok(());
        };
        // BT also stops its session before filesystem cleanup. Do not ask the
        // library to delete files itself: failures must retain our task record.
        self.engines.delete_runtime_task(&task, false).await;
        // The writer may publish under a new name while draining. Its original
        // target can now belong to another file, so only clean refreshed paths.
        let Some(task) = db::get_task_record(self.pool, id).await? else {
            return Ok(());
        };
        if matches!(
            task.status,
            TaskStatus::Queued
                | TaskStatus::Downloading
                | TaskStatus::Retrying
                | TaskStatus::WaitingNetwork
        ) {
            db::update_task_status(
                self.pool,
                id,
                TaskStatus::Paused,
                Some(task.status),
                0,
                0,
                Some("Paused"),
                None,
            )
            .await?;
        }

        let files = db::list_task_file_records(self.pool, id).await?;
        let use_trash = delete_file && db::delete_to_trash_enabled(self.pool).await.unwrap_or(true);
        let mut requests = Vec::new();
        let mut add = |path: Option<&str>, use_trash| {
            if let Some(path) = path.filter(|path| !path.trim().is_empty()) {
                requests.push(FileDeleteRequest {
                    path: path.to_string(),
                    use_trash,
                });
            }
        };
        if delete_file {
            for file in &files {
                add(file.temp_path.as_deref(), false);
                add(file.final_path.as_deref(), use_trash);
            }
            add(task.temp_path.as_deref(), false);
            add(task.final_path.as_deref(), use_trash);
        }
        // ARC-38: metadata-only deletion still reclaims owned staging and parts.
        let file_temps = files
            .iter()
            .filter_map(|file| file.temp_path.clone())
            .collect::<Vec<_>>();
        for artifact in
            crate::download::artifacts::task_auxiliary_artifacts(&task, &file_temps).await
        {
            add(artifact.to_str(), false);
        }
        let outcomes = delete_paths_off_runtime(requests).await;
        let failures = outcomes
            .into_iter()
            .filter_map(|outcome| {
                outcome
                    .result
                    .err()
                    .map(|error| format!("{}: {error}", outcome.path))
            })
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            let error = AppErrorPayload::new(
                "storage_cleanup_failed",
                failures.join("\n"),
                true,
                vec!["open_folder"],
            )
            .command_error();
            db::insert_task_event(self.pool, id, "cleanup_failed", Some(&error)).await?;
            return Err(error);
        }
        db::delete_task_record(self.pool, id).await?;
        evict_task_files_version(id);
        Ok(())
    }
}
