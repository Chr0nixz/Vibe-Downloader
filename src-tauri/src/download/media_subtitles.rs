//! B6: validate and merge bounded textual WebVTT before creating a media output.

use super::{
    error::engine_error, owned_fs as fs, read_local_file_limited, CONTROL_PLANE_MAX_BYTES,
};
use std::path::{Path, PathBuf};

pub(crate) fn mp4_language(language: &str) -> &str {
    match language.split(['-', '_']).next().unwrap_or("") {
        "en" => "eng",
        "zh" => "zho",
        "ja" => "jpn",
        "ko" => "kor",
        "es" => "spa",
        "ru" => "rus",
        "fr" => "fra",
        "de" => "deu",
        "it" => "ita",
        "pt" => "por",
        "ar" => "ara",
        "hi" => "hin",
        value if value.len() == 3 && value.bytes().all(|b| b.is_ascii_lowercase()) => value,
        _ => "und",
    }
}

pub(crate) async fn merge_webvtt(
    inputs: &[PathBuf],
    output: &Path,
    code: &str,
) -> Result<PathBuf, String> {
    let mut merged = String::from("WEBVTT\n\n");
    let mut base_map = None;
    let mut previous_start = 0_i64;
    let mut seen = std::collections::HashSet::new();
    for input in inputs {
        let bytes = read_local_file_limited(input, CONTROL_PLANE_MAX_BYTES)
            .await
            .map_err(|e| engine_error(code, format!("{e:?}"), false))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| engine_error(code, "Subtitle input is not UTF-8 WebVTT", false))?
            .trim_start_matches('\u{feff}')
            .replace("\r\n", "\n");
        if !text
            .lines()
            .next()
            .is_some_and(|line| line == "WEBVTT" || line.starts_with("WEBVTT "))
        {
            return Err(engine_error(
                code,
                "Only textual WebVTT subtitles are supported",
                false,
            ));
        }
        let mut shift = 0_i64;
        for line in text
            .lines()
            .filter(|line| line.starts_with("X-TIMESTAMP-MAP="))
        {
            let fields = line.trim_start_matches("X-TIMESTAMP-MAP=");
            let local = fields
                .split(',')
                .find_map(|field| field.trim().strip_prefix("LOCAL:").and_then(timestamp));
            let ticks = fields.split(',').find_map(|field| {
                field
                    .trim()
                    .strip_prefix("MPEGTS:")
                    .and_then(|v| v.parse::<i64>().ok())
            });
            let (Some(local), Some(ticks)) = (local, ticks) else {
                return Err(engine_error(code, "Invalid WebVTT timestamp map", false));
            };
            if !(0..(1_i64 << 33)).contains(&ticks) {
                return Err(engine_error(code, "Invalid WebVTT MPEGTS timestamp", false));
            }
            let base = *base_map.get_or_insert(ticks);
            let delta = (ticks - base + (1_i64 << 32)).rem_euclid(1_i64 << 33) - (1_i64 << 32);
            shift = delta / 90 - local;
        }
        for block in text.split("\n\n") {
            let lines: Vec<_> = block.lines().collect();
            let Some(index) = lines.iter().position(|line| line.contains("-->")) else {
                continue;
            };
            let (left, right) = lines[index].split_once("-->").unwrap();
            let mut end_and_settings = right.split_whitespace();
            let start = timestamp(left.trim())
                .ok_or_else(|| engine_error(code, "Invalid WebVTT cue start", false))?
                .saturating_add(shift);
            let end = end_and_settings
                .next()
                .and_then(timestamp)
                .ok_or_else(|| engine_error(code, "Invalid WebVTT cue end", false))?
                .saturating_add(shift);
            if end <= start || start < 0 {
                return Err(engine_error(code, "Invalid WebVTT cue interval", false));
            }
            let cue = format!(
                "{} --> {}\n{}\n\n",
                format_timestamp(start),
                format_timestamp(end),
                lines[index + 1..].join("\n")
            );
            if !seen.insert(cue.clone()) {
                continue;
            }
            // Without a valid map, never guess offsets for reset timestamps.
            if start < previous_start {
                return Err(engine_error(
                    code,
                    "Segmented WebVTT timestamps must be monotonic or have a valid timestamp map",
                    false,
                ));
            }
            previous_start = start;
            if merged.len().saturating_add(cue.len()) > CONTROL_PLANE_MAX_BYTES {
                return Err(engine_error(
                    code,
                    "Subtitle output exceeds the size limit",
                    false,
                ));
            }
            merged.push_str(&cue);
        }
    }
    fs::write(output, merged)
        .await
        .map_err(|e| engine_error(code, e.to_string(), false))?;
    Ok(output.to_path_buf())
}

fn timestamp(value: &str) -> Option<i64> {
    let fields: Vec<_> = value.split(':').collect();
    let (hours, minutes, seconds) = match fields.as_slice() {
        [m, s] => (0, m.parse::<i64>().ok()?, *s),
        [h, m, s] => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?, *s),
        _ => return None,
    };
    let (seconds, millis) = seconds.split_once('.')?;
    let seconds = seconds.parse::<i64>().ok()?;
    if hours < 0
        || !(0..60).contains(&minutes)
        || !(0..60).contains(&seconds)
        || millis.len() != 3
        || !millis.bytes().all(|value| value.is_ascii_digit())
    {
        return None;
    }
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes * 60_000)?
        .checked_add(seconds * 1000)?
        .checked_add(millis.parse::<i64>().ok()?)
}

fn format_timestamp(ms: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_are_checked() {
        assert_eq!(timestamp("00:01.234"), Some(1234));
        assert_eq!(timestamp("01:02:03.004"), Some(3_723_004));
        assert_eq!(timestamp("00:99.001"), None);
        assert_eq!(timestamp("00:00.1"), None);
    }
}
