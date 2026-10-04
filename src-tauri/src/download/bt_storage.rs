//! ARC-62: librqbit's synchronous storage can survive its session's cancellation signal.

use super::lifecycle::{Lease, Resources};
use super::{GlobalSpeedLimiter, ThrottleError};
use librqbit::{
    storage::{
        filesystem::FilesystemStorageFactory, BoxStorageFactory, StorageFactory, StorageFactoryExt,
        TorrentStorage,
    },
    ManagedTorrentShared, TorrentMetadata,
};
use std::{io::IoSlice, path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(super) struct OwnedFilesystemFactory {
    lease: Option<Arc<Lease>>,
    shared_limiter: Arc<GlobalSpeedLimiter>,
    cancel_token: CancellationToken,
}

impl OwnedFilesystemFactory {
    pub fn new(shared_limiter: Arc<GlobalSpeedLimiter>, cancel_token: CancellationToken) -> Self {
        Self {
            lease: Resources::current().map(|owner| Arc::new(owner.lease())),
            shared_limiter,
            cancel_token,
        }
    }
}

impl StorageFactory for OwnedFilesystemFactory {
    type Storage = OwnedStorage;
    fn create(
        &self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<OwnedStorage> {
        Ok(OwnedStorage {
            inner: Box::new(FilesystemStorageFactory::default().create(shared, metadata)?),
            lease: self.lease.clone(),
            shared_limiter: self.shared_limiter.clone(),
            cancel_token: self.cancel_token.clone(),
        })
    }
    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

pub(super) struct OwnedStorage {
    // Field order closes the actual files before releasing their ownership.
    inner: Box<dyn TorrentStorage>,
    lease: Option<Arc<Lease>>,
    shared_limiter: Arc<GlobalSpeedLimiter>,
    cancel_token: CancellationToken,
}

impl TorrentStorage for OwnedStorage {
    fn init(
        &mut self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        self.inner.init(shared, metadata)
    }
    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        self.inner.pread_exact(file_id, offset, buf)
    }
    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        self.shared_limiter
            .throttle_blocking(buf.len(), &self.cancel_token)
            .map_err(|error| match error {
                ThrottleError::Cancelled => anyhow::anyhow!("BT download throttling cancelled"),
            })?;
        self.inner.pwrite_all(file_id, offset, buf)
    }
    fn pwrite_all_vectored(
        &self,
        file_id: usize,
        offset: u64,
        bufs: [IoSlice<'_>; 2],
    ) -> anyhow::Result<usize> {
        let total = bufs[0].len().saturating_add(bufs[1].len());
        self.shared_limiter
            .throttle_blocking(total, &self.cancel_token)
            .map_err(|error| match error {
                ThrottleError::Cancelled => anyhow::anyhow!("BT download throttling cancelled"),
            })?;
        self.inner.pwrite_all_vectored(file_id, offset, bufs)
    }
    fn remove_file(&self, file_id: usize, filename: &Path) -> anyhow::Result<()> {
        self.inner.remove_file(file_id, filename)
    }
    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        self.inner.remove_directory_if_empty(path)
    }
    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        self.inner.ensure_file_length(file_id, length)
    }
    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Self {
            inner: self.inner.take()?,
            lease: self.lease.clone(),
            shared_limiter: self.shared_limiter.clone(),
            cancel_token: self.cancel_token.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use librqbit::storage::TorrentStorage;
    use std::sync::mpsc;
    use std::time::Duration;

    struct TestStorage;

    impl TorrentStorage for TestStorage {
        fn init(&mut self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> anyhow::Result<()> {
            Ok(())
        }

        fn pread_exact(&self, _: usize, _: u64, _: &mut [u8]) -> anyhow::Result<()> {
            Ok(())
        }

        fn pwrite_all(&self, _: usize, _: u64, _: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        fn remove_file(&self, _: usize, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }

        fn remove_directory_if_empty(&self, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }

        fn ensure_file_length(&self, _: usize, _: u64) -> anyhow::Result<()> {
            Ok(())
        }

        fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
            Ok(Box::new(Self))
        }
    }

    fn test_storage(limiter: Arc<GlobalSpeedLimiter>) -> OwnedStorage {
        OwnedStorage {
            inner: Box::new(TestStorage),
            lease: None,
            shared_limiter: limiter,
            cancel_token: CancellationToken::new(),
        }
    }

    #[test]
    fn independent_bt_storage_instances_share_the_root_budget() {
        // FUN-35: storage writes are the first synchronous boundary after
        // librqbit receives a piece. Two sessions must consume one root bucket
        // instead of receiving one global burst each.
        let limiter = Arc::new(GlobalSpeedLimiter::new(Some(1_000)));
        let first = test_storage(limiter.clone());
        let second = test_storage(limiter);

        first
            .pwrite_all(0, 0, &[0; 1_000])
            .expect("first session consumes initial burst");

        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            second
                .pwrite_all(0, 0, &[0; 1_000])
                .expect("second session waits for shared refill");
            done_tx.send(()).expect("send completion");
        });

        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "the second BT session must not receive a separate initial burst"
        );
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("shared root budget refills");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bt_storage_and_http_waiters_share_the_root_budget() {
        // FUN-35: an HTTP-family acquire must observe bytes committed by BT,
        // proving the two protocol families use the same process-wide bucket.
        let limiter = Arc::new(GlobalSpeedLimiter::new(Some(1_000)));
        let storage = test_storage(limiter.clone());
        storage
            .pwrite_all(0, 0, &[0; 1_000])
            .expect("BT session consumes initial burst");

        let cancel = CancellationToken::new();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), limiter.throttle(1_000, &cancel))
                .await
                .is_err(),
            "HTTP-family traffic must not receive an independent burst"
        );
        tokio::time::timeout(Duration::from_secs(2), limiter.throttle(1_000, &cancel))
            .await
            .expect("shared root budget refills")
            .expect("HTTP-family acquire succeeds");
    }
}
