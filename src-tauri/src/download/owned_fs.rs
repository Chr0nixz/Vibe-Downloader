//! ARC-62: Tokio file writes can outlive a dropped future; retain ownership until they settle.

use super::lifecycle::{Lease, Resources};
use std::{
    future::Future,
    io,
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

pub use tokio::fs::{metadata, read, read_dir, try_exists};

async fn submitted<F, T>(operation: F) -> io::Result<T>
where
    F: Future<Output = io::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let Some(resources) = Resources::current() else {
        return operation.await;
    };
    let lease = resources.lease();
    tokio::spawn(async move {
        let _lease = lease;
        resources.scope(operation).await
    })
    .await
    .map_err(io::Error::other)?
}

pub struct File {
    inner: Option<tokio::fs::File>,
    lease: Option<Lease>,
}

impl File {
    fn owned(file: tokio::fs::File) -> Self {
        Self {
            inner: Some(file),
            lease: Resources::current().map(|owner| owner.lease()),
        }
    }

    pub async fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        submitted(async move { tokio::fs::File::create(path).await.map(Self::owned) }).await
    }

    pub async fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        submitted(async move { tokio::fs::File::open(path).await.map(Self::owned) }).await
    }

    pub async fn sync_all(&self) -> io::Result<()> {
        self.inner.as_ref().unwrap().sync_all().await
    }
    pub async fn set_len(&self, size: u64) -> io::Result<()> {
        self.inner.as_ref().unwrap().set_len(size).await
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let file = self.inner.take().unwrap();
        let lease = self.lease.take();
        match file.try_into_std() {
            Ok(file) => {
                drop(file);
                drop(lease);
            }
            Err(file) => {
                // into_std waits for Tokio's already-submitted blocking I/O.
                // The lease predates the write, so drain cannot race this handoff.
                tokio::spawn(async move {
                    drop(file.into_std().await);
                    drop(lease);
                });
            }
        }
    }
}

impl AsyncRead for File {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().inner.as_mut().unwrap()).poll_read(cx, buf)
    }
}

impl AsyncWrite for File {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.get_mut().inner.as_mut().unwrap()).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().inner.as_mut().unwrap()).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().inner.as_mut().unwrap()).poll_shutdown(cx)
    }
}

impl AsyncSeek for File {
    fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        Pin::new(self.get_mut().inner.as_mut().unwrap()).start_seek(position)
    }
    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Pin::new(self.get_mut().inner.as_mut().unwrap()).poll_complete(cx)
    }
}

#[derive(Clone, Default)]
pub struct OpenOptions(tokio::fs::OpenOptions);

impl OpenOptions {
    pub fn new() -> Self {
        Self(tokio::fs::OpenOptions::new())
    }
    pub fn read(&mut self, value: bool) -> &mut Self {
        self.0.read(value);
        self
    }
    pub fn write(&mut self, value: bool) -> &mut Self {
        self.0.write(value);
        self
    }
    pub fn append(&mut self, value: bool) -> &mut Self {
        self.0.append(value);
        self
    }
    pub fn truncate(&mut self, value: bool) -> &mut Self {
        self.0.truncate(value);
        self
    }
    pub fn create(&mut self, value: bool) -> &mut Self {
        self.0.create(value);
        self
    }
    pub fn create_new(&mut self, value: bool) -> &mut Self {
        self.0.create_new(value);
        self
    }
    pub async fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let path = path.as_ref().to_path_buf();
        let options = self.0.clone();
        submitted(async move { options.open(path).await.map(File::owned) }).await
    }
}

pub async fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref().to_path_buf();
    submitted(async move { tokio::fs::create_dir_all(path).await }).await
}

pub async fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref().to_path_buf();
    submitted(async move { tokio::fs::remove_file(path).await }).await
}

pub async fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref().to_path_buf();
    submitted(async move { tokio::fs::remove_dir_all(path).await }).await
}

pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    let from = from.as_ref().to_path_buf();
    let to = to.as_ref().to_path_buf();
    submitted(async move { tokio::fs::rename(from, to).await }).await
}

pub async fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref().to_path_buf();
    let contents = contents.as_ref().to_vec();
    submitted(async move { tokio::fs::write(path, contents).await }).await
}
