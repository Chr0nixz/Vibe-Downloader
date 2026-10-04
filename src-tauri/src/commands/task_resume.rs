use crate::{
    download::{ProbeOutput, ProbeResult},
    models::{AppErrorPayload, TaskRecord, TaskSegmentRecord},
};

fn resume_error(code: &str, message: &str) -> String {
    // Resume blockers require a full restart; keep the human message copyable.
    AppErrorPayload::new(code, message, false, vec!["restart", "check_url"]).command_error()
}

pub fn local_resume_error(
    recorded_progress: i64,
    temp_exists: bool,
    temp_size: i64,
    total_size: i64,
    supports_parallel: bool,
) -> Option<String> {
    if temp_size > total_size && total_size > 0 {
        return Some(resume_error(
            "temp_file_larger_than_remote",
            "Temporary file is larger than the remote file.",
        ));
    }
    if recorded_progress > 0 && !temp_exists {
        return Some(resume_error(
            "temp_file_missing",
            "Temporary file is missing. Restart this download.",
        ));
    }
    if recorded_progress > temp_size {
        return Some(resume_error(
            "temp_file_smaller_than_progress",
            "Temporary file is smaller than the recorded progress.",
        ));
    }
    if temp_size > 0 && !supports_parallel {
        return Some(resume_error(
            "resume_unavailable",
            "Resume unavailable. Restart this download from the beginning.",
        ));
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
) -> Option<String> {
    if segments.is_empty() {
        return Some(resume_error(
            "segment_records_missing",
            "Task has no segment records. Restart this download.",
        ));
    }
    if temp_size > total_size && total_size > 0 {
        return Some(resume_error(
            "temp_file_larger_than_remote",
            "Temporary file is larger than the remote file.",
        ));
    }

    let mut expected_start = 0_i64;
    let mut highest_recorded_offset = 0_i64;
    let mut downloaded_bytes = 0_i64;

    for segment in segments {
        if segment.range_start != expected_start || segment.range_end < segment.range_start {
            return Some(resume_error(
                "segment_records_inconsistent",
                "Segment records are inconsistent. Restart this download.",
            ));
        }
        if segment.downloaded_until < segment.range_start
            || segment.downloaded_until > segment.range_end.saturating_add(1)
        {
            return Some(resume_error(
                "segment_progress_invalid",
                "Segment progress is outside its byte range. Restart this download.",
            ));
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
        return Some(resume_error(
            "segment_records_size_mismatch",
            "Segment records do not match the remote file size. Restart this download.",
        ));
    }

    let recorded_progress = task_downloaded_bytes
        .max(downloaded_bytes)
        .max(highest_recorded_offset);
    if recorded_progress > 0 && !temp_exists {
        return Some(resume_error(
            "temp_file_missing",
            "Temporary file is missing. Restart this download.",
        ));
    }
    if highest_recorded_offset > temp_size {
        return Some(resume_error(
            "temp_file_smaller_than_progress",
            "Temporary file is smaller than the recorded progress.",
        ));
    }
    if temp_size > 0 && !supports_parallel {
        return Some(resume_error(
            "resume_unavailable",
            "Resume unavailable. Restart this download from the beginning.",
        ));
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
    let both_unknown = task.total_size == 0 && probe.total_size() == 0;
    if task.total_size != probe.total_size() && !both_unknown {
        return Some(resume_error(
            "remote_changed",
            "Remote file changed. Restart download to avoid corruption.",
        ));
    }
    if !probe.supports_resume() {
        return Some(resume_error(
            "resume_unavailable",
            "Server no longer supports resume. Restart this download.",
        ));
    }
    if both_unknown && task.etag.is_none() && task.last_modified.is_none() {
        return Some(resume_error(
            "resume_unavailable",
            "Unknown-size resources require a stable ETag or Last-Modified validator before resuming.",
        ));
    }
    if strong_etag(task.etag.as_deref())
        && strong_etag(probe.etag().map(String::as_str))
        && task.etag.as_ref() != probe.etag()
    {
        return Some(resume_error(
            "remote_changed",
            "Remote file changed. Restart download to avoid corruption.",
        ));
    }
    if task.last_modified.is_some()
        && probe.last_modified().is_some()
        && task.last_modified.as_ref() != probe.last_modified()
    {
        return Some(resume_error(
            "remote_changed",
            "Remote file changed. Restart download to avoid corruption.",
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppErrorPayload;

    fn code_of(error: &str) -> String {
        serde_json::from_str::<AppErrorPayload>(error)
            .expect("resume errors must be AppErrorPayload JSON")
            .code
    }

    #[test]
    fn resume_errors_dispatch_on_code_not_message_text() {
        // ARC-16: rewriting the human message must not change the stable code.
        let missing = resume_error(
            "temp_file_missing",
            "Temporary file vanished — please restart (localized).",
        );
        assert_eq!(code_of(&missing), "temp_file_missing");
        assert!(missing.contains("Temporary file vanished"));
    }

    #[test]
    fn local_resume_emits_stable_codes() {
        assert_eq!(
            code_of(&local_resume_error(10, false, 0, 100, true).unwrap()),
            "temp_file_missing"
        );
        assert_eq!(
            code_of(&local_resume_error(50, true, 40, 100, true).unwrap()),
            "temp_file_smaller_than_progress"
        );
        assert_eq!(
            code_of(&local_resume_error(0, true, 120, 100, true).unwrap()),
            "temp_file_larger_than_remote"
        );
        assert_eq!(
            code_of(&local_resume_error(0, true, 10, 100, false).unwrap()),
            "resume_unavailable"
        );
    }
}
