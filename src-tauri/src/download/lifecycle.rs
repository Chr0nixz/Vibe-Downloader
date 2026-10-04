//! ARC-62: keep a run alive until its child tasks and submitted file I/O have exited.

use futures_util::FutureExt;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::sync::Notify;

tokio::task_local! {
    static CURRENT: Resources;
}

#[derive(Clone, Default)]
pub struct Resources(Arc<ResourceState>);

#[derive(Default)]
struct ResourceState {
    active: AtomicUsize,
    changed: Notify,
}

pub struct Lease {
    resources: Resources,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.resources.0.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.resources.0.changed.notify_waiters();
        }
    }
}

impl Resources {
    pub fn current() -> Option<Self> {
        CURRENT.try_with(Clone::clone).ok()
    }

    pub fn lease(&self) -> Lease {
        self.0.active.fetch_add(1, Ordering::AcqRel);
        Lease {
            resources: self.clone(),
        }
    }

    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub async fn drain(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.0.active.load(Ordering::Acquire) == 0 {
                return;
            }
            changed.await;
        }
    }
}

/// Nested protocol delegation shares the outer owner instead of waiting for itself.
pub async fn run_owned<F: Future>(future: F) -> F::Output {
    if Resources::current().is_some() {
        return future.await;
    }
    let resources = Resources::default();
    let result = resources
        .scope(std::panic::AssertUnwindSafe(future).catch_unwind())
        .await;
    resources.drain().await;
    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Child leases are acquired before spawn, including children not yet polled.
pub struct JoinSet<T: 'static>(tokio::task::JoinSet<T>);

impl<T: Send + 'static> Default for JoinSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> JoinSet<T> {
    pub fn new() -> Self {
        Self(tokio::task::JoinSet::new())
    }

    pub fn spawn<F>(&mut self, future: F) -> tokio::task::AbortHandle
    where
        F: Future<Output = T> + Send + 'static,
    {
        let resources = Resources::current();
        let lease = resources.as_ref().map(Resources::lease);
        self.0.spawn(async move {
            let _lease = lease;
            match resources {
                Some(resources) => resources.scope(future).await,
                None => future.await,
            }
        })
    }

    pub async fn join_next(&mut self) -> Option<Result<T, tokio::task::JoinError>> {
        self.0.join_next().await
    }

    pub fn abort_all(&mut self) {
        self.0.abort_all();
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A submitted blocking operation keeps its lease even if its awaiting future is dropped.
pub async fn blocking<F, T>(operation: F) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let lease = Resources::current().map(|resources| resources.lease());
    tokio::task::spawn_blocking(move || {
        let _lease = lease;
        operation()
    })
    .await
}
