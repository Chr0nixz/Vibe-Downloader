use crate::models::{SegmentStatus, TaskRecord, TaskSegmentRecord};

use super::{
    DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES, DEFAULT_SEGMENT_COUNT, MAX_SEGMENT_COUNT,
    MIN_SEGMENT_COUNT,
};

pub fn planned_segment_count(task: &TaskRecord) -> usize {
    planned_segment_count_with_plan(
        task,
        DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        DEFAULT_SEGMENT_COUNT,
    )
}

pub fn planned_segment_count_with_plan(
    task: &TaskRecord,
    multi_connection_threshold_bytes: i64,
    segment_count: i32,
) -> usize {
    if is_bt_protocol(&task.protocol) || is_dash_protocol(&task.protocol) {
        return 1;
    }
    if is_hls_protocol(&task.protocol) {
        return segment_count.clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT) as usize;
    }

    let segment_count = segment_count.clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT) as usize;
    let segment_count = if is_ftp_protocol(&task.protocol) {
        segment_count.min(4)
    } else if is_sftp_protocol(&task.protocol) {
        // Conservative cap matching SFTP_MAX_DYNAMIC_CONNECTIONS so the
        // initial plan never exceeds the per-task parallelism ceiling.
        segment_count.min(2)
    } else if is_metalink_protocol(&task.protocol) {
        // Cap parallel mirrors per file at METALINK_MAX_PARALLEL_MIRRORS.
        // SFTP/FTP use independent TCP/SSH connections per worker, but
        // Metalink workers target DIFFERENT HTTP mirrors, so each mirror
        // only sees one connection — the cap is about local resource use
        // (file handles, throughput accounting) rather than per-mirror
        // load. 3 mirrors aligns with industry conventions (aria2 default
        // is 3-5, lftp default is 3).
        segment_count.min(3)
    } else {
        segment_count
    };
    if task.supports_parallel
        && task.total_size > 0
        && task.total_size >= multi_connection_threshold_bytes.max(0)
    {
        segment_count
    } else {
        1
    }
}

pub fn planned_segments_for_task(task: &TaskRecord) -> Vec<TaskSegmentRecord> {
    planned_segments_for_task_with_plan(
        task,
        DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        DEFAULT_SEGMENT_COUNT,
    )
}

pub fn planned_segments_for_task_with_plan(
    task: &TaskRecord,
    multi_connection_threshold_bytes: i64,
    segment_count: i32,
) -> Vec<TaskSegmentRecord> {
    if is_bt_protocol(&task.protocol) {
        return vec![single_segment_for_task(task, "bt_piece")];
    }

    if is_hls_protocol(&task.protocol) {
        return vec![single_segment_for_task(task, "hls_segment")];
    }

    if is_metalink_protocol(&task.protocol) {
        // Metalink tasks above the multi-connection threshold use parallel
        // range segments (one per mirror); smaller tasks fall back to a
        // single full-file unit so the serial mirror failover path handles
        // them. Note that this plan is per-task; the engine downloads each
        // selected file in turn, so the per-file threshold check happens in
        // `download_metalink_file`'s parallel-vs-serial dispatch.
        let count = planned_segment_count_with_plan(
            task,
            multi_connection_threshold_bytes,
            segment_count,
        );
        if count > 1 && task.total_size > 0 {
            return parallel_range_segments(task, "metalink_range", count);
        }
        return vec![single_segment_for_task(task, "metalink_file")];
    }

    if is_dash_protocol(&task.protocol) {
        return vec![single_segment_for_task(task, "dash_media")];
    }

    if is_ftp_protocol(&task.protocol) {
        return vec![single_segment_for_task(task, "ftp_rest")];
    }

    // SFTP falls through to the HTTP-style multi-range generator below when
    // the task supports parallelism and meets the threshold; otherwise it
    // uses a single-segment plan, matching the FTP fallback behaviour.
    let unit_kind = if is_sftp_protocol(&task.protocol) {
        "sftp_range"
    } else {
        "http_range"
    };
    let single_unit_kind = if is_sftp_protocol(&task.protocol) {
        "sftp_file"
    } else {
        "http_range"
    };

    let count =
        planned_segment_count_with_plan(task, multi_connection_threshold_bytes, segment_count);
    let total_size = task.total_size.max(0);
    let completed = task.downloaded_bytes >= total_size && total_size > 0;

    if count == 1 || total_size == 0 {
        return vec![single_segment_for_task(task, single_unit_kind)];
    }

    parallel_range_segments_with_completion(task, unit_kind, count, completed)
}

/// Build `count` non-overlapping range segments covering [0, total_size)
/// with the given `unit_kind`. Each segment is marked `Pending`.
/// Used by SFTP and Metalink parallel paths.
fn parallel_range_segments(task: &TaskRecord, unit_kind: &str, count: usize) -> Vec<TaskSegmentRecord> {
    let total_size = task.total_size.max(0);
    let completed = task.downloaded_bytes >= total_size && total_size > 0;
    parallel_range_segments_with_completion(task, unit_kind, count, completed)
}

fn parallel_range_segments_with_completion(
    task: &TaskRecord,
    unit_kind: &str,
    count: usize,
    completed: bool,
) -> Vec<TaskSegmentRecord> {
    let total_size = task.total_size.max(0);
    let count_i64 = i64::try_from(count).unwrap_or(1);
    let base = total_size / count_i64;
    let remainder = total_size % count_i64;
    let mut start = 0_i64;

    (0..count)
        .map(|index| {
            let extra = if i64::try_from(index).unwrap_or(0) < remainder {
                1
            } else {
                0
            };
            let length = base + extra;
            let end = start + length - 1;
            let downloaded_until = if completed { end + 1 } else { start };
            let segment = TaskSegmentRecord {
                id: uuid::Uuid::new_v4().to_string(),
                task_id: task.id.clone(),
                file_id: None,
                unit_kind: unit_kind.to_string(),
                range_start: start,
                range_end: end,
                downloaded_until,
                status: if completed {
                    SegmentStatus::Completed
                } else {
                    SegmentStatus::Pending
                },
                speed_bps: 0,
                retry_count: 0,
                last_error: None,
            };
            start = end + 1;
            segment
        })
        .collect()
}

fn is_ftp_protocol(protocol: &str) -> bool {
    matches!(protocol, "ftp" | "ftps")
}

fn is_bt_protocol(protocol: &str) -> bool {
    matches!(protocol, "bt" | "magnet")
}

fn is_hls_protocol(protocol: &str) -> bool {
    protocol == "hls"
}

fn is_metalink_protocol(protocol: &str) -> bool {
    protocol == "metalink"
}

fn is_dash_protocol(protocol: &str) -> bool {
    protocol == "dash"
}

fn is_sftp_protocol(protocol: &str) -> bool {
    protocol == "sftp"
}

fn single_segment_for_task(task: &TaskRecord, unit_kind: &str) -> TaskSegmentRecord {
    let total_size = task.total_size.max(0);
    let completed = task.downloaded_bytes >= total_size && total_size > 0;
    TaskSegmentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        file_id: None,
        unit_kind: unit_kind.to_string(),
        range_start: 0,
        range_end: total_size.saturating_sub(1).max(0),
        downloaded_until: task.downloaded_bytes.max(0),
        speed_bps: 0,
        status: if completed {
            SegmentStatus::Completed
        } else {
            SegmentStatus::Pending
        },
        retry_count: 0,
        last_error: None,
    }
}
