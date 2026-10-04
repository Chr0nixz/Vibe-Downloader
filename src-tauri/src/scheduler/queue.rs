//! ARC-61: bounded candidate pages keep blocked queue heads from starving other hosts.

use std::collections::{HashMap, VecDeque};

use sqlx::SqlitePool;

use crate::{db, models::TaskRecord};

pub struct DispatchQueue {
    page_size: i64,
    eligible_before: String,
    after: Option<db::QueuedTaskCursor>,
    pending: VecDeque<TaskRecord>,
    exhausted: bool,
}

impl DispatchQueue {
    pub fn new(page_size: i64) -> Self {
        Self {
            page_size: page_size.clamp(1, db::MAX_TASK_PAGE_SIZE),
            eligible_before: crate::models::task::now_iso(),
            after: None,
            pending: VecDeque::new(),
            exhausted: false,
        }
    }

    /// Recheck each candidate because dispatch can fill a host after the page was read.
    pub async fn next(
        &mut self,
        pool: &SqlitePool,
        outside_schedule: bool,
        host_limit: usize,
        host_slots: &HashMap<String, usize>,
    ) -> Result<Option<TaskRecord>, String> {
        loop {
            if let Some(task) = self.pending.pop_front() {
                if outside_schedule && task.obey_schedule {
                    continue;
                }
                if host_slots.get(&task.source_key).copied().unwrap_or(0) >= host_limit {
                    continue;
                }
                return Ok(Some(task));
            }
            if self.exhausted {
                return Ok(None);
            }
            let blocked_hosts = host_slots
                .iter()
                .filter(|(_, used)| **used >= host_limit)
                .map(|(host, _)| host.as_str())
                .collect::<Vec<_>>();
            let page = db::list_queued_task_records_page(
                pool,
                self.page_size,
                self.after.as_ref(),
                &self.eligible_before,
                &blocked_hosts,
                outside_schedule,
            )
            .await?;
            self.exhausted = page.len() < self.page_size as usize;
            self.after = page.last().map(db::QueuedTaskCursor::from);
            self.pending = page.into();
        }
    }
}
