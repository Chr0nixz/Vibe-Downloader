use crate::{
    download::{ProbeOutput, ProbeResult},
    models::{TaskRecord, TaskSegmentRecord},
};

pub fn local_resume_error(
    recorded_progress: i64,
    temp_exists: bool,
    temp_size: i64,
    total_size: i64,
    supports_parallel: bool,
) -> Option<&'static str> {
    if temp_size > total_size && total_size > 0 {
        return Some("Temporary file is larger than the remote file.");
    }
    if recorded_progress > 0 && !temp_exists {
        return Some("Temporary file is missing. Restart this download.");
    }
    if recorded_progress > temp_size {
        return Some("Temporary file is smaller than the recorded progress.");
    }
    if temp_size > 0 && !supports_parallel {
        return Some("Resume unavailable. Restart this download from the beginning.");
    }
    None
}

pub fn segment_resume_error(
    segments: &[TaskSegmentRecord],
    task_downloaded_bytes: i64,
    temp_exists: bool,
    temp_size: i64,
    total_size: i64,
    supports_parallel: bool,
) -> Option<&'static str> {
    if segments.is_empty() {
        return Some("Task has no segment records. Restart this download.");
    }
    if temp_size > total_size && total_size > 0 {
        return Some("Temporary file is larger than the remote file.");
    }

    let mut expected_start = 0_i64;
    let mut highest_recorded_offset = 0_i64;
    let mut downloaded_bytes = 0_i64;

    for segment in segments {
        if segment.range_start != expected_start || segment.range_end < segment.range_start {
            return Some("Segment records are inconsistent. Restart this download.");
        }
        if segment.downloaded_until < segment.range_start
            || segment.downloaded_until > segment.range_end.saturating_add(1)
        {
            return Some("Segment progress is outside its byte range. Restart this download.");
        }

        let clamped_until = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        if clamped_until > segment.range_start {
            highest_recorded_offset = highest_recorded_offset.max(clamped_until);
        }
        downloaded_bytes += clamped_until.saturating_sub(segment.range_start);
        expected_start = segment.range_end.saturating_add(1);
    }

    if total_size > 0 && expected_start != total_size {
        return Some("Segment records do not match the remote file size. Restart this download.");
    }

    let recorded_progress = task_downloaded_bytes
        .max(downloaded_bytes)
        .max(highest_recorded_offset);
    if recorded_progress > 0 && !temp_exists {
        return Some("Temporary file is missing. Restart this download.");
    }
    if highest_recorded_offset > temp_size {
        return Some("Temporary file is smaller than the recorded progress.");
    }
    if temp_size > 0 && !supports_parallel {
        return Some("Resume unavailable. Restart this download from the beginning.");
    }
    None
}

pub trait ResumeProbe {
    fn total_size(&self) -> i64;
    fn supports_resume(&self) -> bool;
    fn etag(&self) -> Option<&String>;
    fn last_modified(&self) -> Option<&String>;
}

impl ResumeProbe for ProbeOutput {
    fn total_size(&self) -> i64 {
        self.total_size
    }

    fn supports_resume(&self) -> bool {
        self.capabilities.supports_resume
    }

    fn etag(&self) -> Option<&String> {
        self.etag.as_ref()
    }

    fn last_modified(&self) -> Option<&String> {
        self.last_modified.as_ref()
    }
}

impl ResumeProbe for ProbeResult {
    fn total_size(&self) -> i64 {
        self.total_size
    }

    fn supports_resume(&self) -> bool {
        self.supports_resume
    }

    fn etag(&self) -> Option<&String> {
        self.etag.as_ref()
    }

    fn last_modified(&self) -> Option<&String> {
        self.last_modified.as_ref()
    }
}

pub fn resume_mismatch_message<P: ResumeProbe>(task: &TaskRecord, probe: &P) -> Option<String> {
    if task.total_size != probe.total_size() {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    if !probe.supports_resume() {
        return Some("Server no longer supports resume. Restart this download.".to_string());
    }
    if strong_etag(task.etag.as_deref())
        && strong_etag(probe.etag().map(String::as_str))
        && task.etag.as_ref() != probe.etag()
    {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    if task.last_modified.is_some()
        && probe.last_modified().is_some()
        && task.last_modified.as_ref() != probe.last_modified()
    {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    None
}

pub fn resume_decision_message<P: ResumeProbe>(task: &TaskRecord, probe: &P) -> Option<String> {
    if weak_etag(task.etag.as_deref()) || weak_etag(probe.etag().map(String::as_str)) {
        return Some(
            "Resume allowed with weak ETag metadata. Verify the file if the source is unstable."
                .to_string(),
        );
    }
    if task.etag.is_none() && task.last_modified.is_none() {
        return Some("Resume allowed without remote validators. Range metadata matched, but integrity depends on the server.".to_string());
    }
    Some("Resume metadata matched. Continuing from the temporary file.".to_string())
}

fn weak_etag(value: Option<&str>) -> bool {
    value
        .map(str::trim_start)
        .is_some_and(|value| value.starts_with("W/") || value.starts_with("w/"))
}

fn strong_etag(value: Option<&str>) -> bool {
    value.is_some_and(|value| !weak_etag(Some(value)))
}
