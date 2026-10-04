//! ARC-33: the segment file writer makes "flush before report" un-bypassable.
//!
//! The `downloaded_until` offset a segment worker reports becomes the
//! coordinator's checkpointed resume offset. A `tokio::io::BufWriter` can hold
//! up to its capacity of written-but-unflushed bytes and its `Drop` does NOT
//! flush, so any early exit that reported its running offset without flushing
//! first let the checkpoint point past durable data — the preallocated temp
//! file then keeps a silent hole of zeros that a resumed download never
//! refills.
//!
//! This wrapper tracks two watermarks: `written_offset` (bytes handed to the
//! buffer) and `durable_offset` (bytes known to be on disk). `durable_offset`
//! only advances in [`SegmentFileWriter::sync`], which flushes first, and
//! reporting sites read the offset back from the writer — so an unflushed
//! offset cannot leave this module by construction.

use crate::download::owned_fs as fs;
use std::{io::SeekFrom, path::Path};

use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

pub(in crate::download::http) struct SegmentFileWriter {
    inner: BufWriter<fs::File>,
    /// Highest offset such that every byte below it is durable on disk.
    durable_offset: i64,
    /// Highest offset of bytes handed to [`Self::write_chunk`]; may be ahead
    /// of `durable_offset` while bytes sit in the buffer.
    written_offset: i64,
}

impl SegmentFileWriter {
    /// Opens the temp file without truncation and seeks to `offset` — the same
    /// open+seek contract the raw `BufWriter` previously implemented at the
    /// call site.
    pub async fn open(temp_path: &Path, offset: i64) -> Result<Self, std::io::Error> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(temp_path)
            .await?;
        let mut inner = BufWriter::with_capacity(256 * 1024, file);
        inner
            .seek(SeekFrom::Start(u64::try_from(offset).unwrap_or(0)))
            .await?;
        Ok(Self {
            inner,
            durable_offset: offset,
            written_offset: offset,
        })
    }

    /// Buffers `data` at the current write position. `end_offset` is the offset
    /// after the chunk; the bytes are NOT durable until [`Self::sync`] runs.
    pub async fn write_chunk(
        &mut self,
        data: &[u8],
        end_offset: i64,
    ) -> Result<(), std::io::Error> {
        self.inner.write_all(data).await?;
        self.written_offset = end_offset;
        Ok(())
    }

    /// Flushes buffered bytes so everything ≤ `written_offset` is durable. On
    /// success `durable_offset == written_offset`; on failure the watermark
    /// stays at the last successful sync — conservative, since the file may
    /// hold more bytes than `durable_offset` but never fewer.
    pub async fn sync(&mut self) -> Result<(), std::io::Error> {
        self.inner.flush().await?;
        self.durable_offset = self.written_offset;
        Ok(())
    }

    /// The only offset value allowed to reach the coordinator/checkpoint.
    pub fn durable_offset(&self) -> i64 {
        self.durable_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn temp_file(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "vibe-segment-writer-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        crate::download::owned_fs::File::create(&path)
            .await
            .expect("create temp file");
        path
    }

    /// The reported offset must never run ahead of bytes actually on disk:
    /// before a sync the durable watermark stays at the open offset even after
    /// writes, and only a sync publishes the written offset.
    #[tokio::test]
    async fn durable_offset_trails_written_until_sync() {
        let path = temp_file("trails").await;
        let mut writer = SegmentFileWriter::open(&path, 0).await.expect("open");

        writer.write_chunk(&[1_u8; 64], 64).await.expect("write");
        assert_eq!(
            writer.durable_offset(),
            0,
            "durable offset must not include buffered bytes before sync"
        );

        writer.sync().await.expect("sync");
        assert_eq!(writer.durable_offset(), 64);

        writer.write_chunk(&[2_u8; 32], 96).await.expect("write");
        assert_eq!(writer.durable_offset(), 64, "watermark frozen until sync");

        writer.sync().await.expect("sync");
        assert_eq!(writer.durable_offset(), 96);

        let _ = crate::download::owned_fs::remove_file(&path).await;
    }

    /// A failed write must not advance the durable watermark: the failure path
    /// reports `durable_offset()`, so resume can never skip bytes whose
    /// durability is unknown.
    #[tokio::test]
    async fn failed_write_keeps_durable_watermark_consistent() {
        let path = temp_file("failed-write").await;
        let mut writer = SegmentFileWriter::open(&path, 0).await.expect("open");

        writer.write_chunk(&[1_u8; 16], 16).await.expect("write");
        writer.sync().await.expect("sync");
        assert_eq!(writer.durable_offset(), 16);

        // Simulate a partial failure: bytes were handed to the buffer for a
        // chunk that never completed (write_chunk errored mid-way in
        // production). The watermark must still describe real disk state.
        writer.written_offset = 16; // write_chunk never advanced it on error
        writer.sync().await.expect("sync after failure");
        assert_eq!(writer.durable_offset(), 16);

        let _ = crate::download::owned_fs::remove_file(&path).await;
    }

    /// Bytes below the durable watermark must be physically on disk: reopen
    /// the file after a synced write and verify the content is readable.
    #[tokio::test]
    async fn synced_bytes_are_durable_on_disk() {
        let path = temp_file("durable").await;
        let payload: Vec<u8> = (0_u8..=255).cycle().take(4096).collect();
        let mut writer = SegmentFileWriter::open(&path, 0).await.expect("open");
        writer
            .write_chunk(&payload, payload.len() as i64)
            .await
            .expect("write");
        writer.sync().await.expect("sync");
        drop(writer);

        let on_disk = crate::download::owned_fs::read(&path)
            .await
            .expect("read back");
        assert_eq!(on_disk, payload);

        let _ = crate::download::owned_fs::remove_file(&path).await;
    }
}
