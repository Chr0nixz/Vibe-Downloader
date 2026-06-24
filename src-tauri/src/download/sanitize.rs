//! Filename sanitization shared across all download engines.
//!
//! Centralizes the "server-supplied name → safe local file name" conversion
//! so that probe results, UI labels, log lines, and the filesystem write path
//! all agree on the same safety rules. The rules are:
//!
//! * Path separators (`/`, `\`) and Windows reserved characters
//!   (`<`, `>`, `:`, `"`, `|`, `?`, `*`) are replaced with `_`.
//! * Control characters (U+0000–U+001F, U+007F) are replaced with `_`.
//! * Leading and trailing whitespace and dots are trimmed, which neutralizes
//!   `.` / `..` path components when they appear alone.
//! * Windows reserved device names (CON, PRN, AUX, NUL, COM1–9, LPT1–9) are
//!   prefixed with `_` to avoid file creation failures on Windows.
//! * File names exceeding 200 characters are truncated to prevent MAX_PATH
//!   issues and excessively long path names.
//! * An empty or all-reserved result falls back to `download-{unix_timestamp}`.
//!
//! This module does **not** perform conflict resolution (`foo (1).mp4`) or
//! extension inference from the Content-Type — those remain the responsibility
//! of `task_file_planning::unique_final_path` and
//! `http::probe::ensure_extension_from_content_type` respectively.

use std::time::{SystemTime, UNIX_EPOCH};

/// Characters that must be rewritten as `_` inside a single file-name component.
///
/// The set covers path separators plus every character Windows forbids in a
/// file name. `..` traversal is handled by the trim step in
/// [`sanitize_single_file_name`], not by this set.
const RESERVED_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Maximum file name length in characters. Names exceeding this are truncated
/// at the last character boundary that fits, preserving the extension.
const MAX_FILE_NAME_LEN: usize = 200;

/// Windows reserved device names (case-insensitive, with or without extension).
/// These names cannot be used as file names on Windows regardless of extension.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitize a server-supplied file name so it is safe to display, log, and
/// (after the caller's own conflict resolution) write to the local filesystem.
///
/// The input may be a bare name (`report.pdf`) or a path-like string
/// (`../../etc/passwd`); in both cases the function returns a single path
/// component with no separators and no traversal semantics.
///
/// Callers that need a multi-component relative path (Metalink's
/// `<file name="docs/readme.md">`, BitTorrent multi-file torrents) should
/// keep using their engine-specific path sanitizers — those split on `/`
/// first, reject `.` / `..` components explicitly, and apply this function
/// per component.
pub fn sanitize_single_file_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| {
            if RESERVED_CHARS.contains(&ch) || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '_') {
        return fallback_name();
    }

    // Neutralize Windows reserved device names (CON, PRN, NUL, COM1–9, …).
    // Windows rejects these names regardless of extension, so check the stem.
    let result = if let Some(dot_pos) = trimmed.rfind('.') {
        let stem = &trimmed[..dot_pos];
        if is_windows_reserved(stem) {
            format!("_{trimmed}")
        } else {
            trimmed.to_string()
        }
    } else if is_windows_reserved(trimmed) {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    };

    // Clamp to MAX_FILE_NAME_LEN, preserving the extension when possible.
    if result.chars().count() <= MAX_FILE_NAME_LEN {
        result
    } else {
        truncate_preserving_extension(&result, MAX_FILE_NAME_LEN)
    }
}

/// Returns `true` if `stem` matches a Windows reserved device name
/// (case-insensitive). These names are invalid as file names on Windows
/// both with and without an extension.
fn is_windows_reserved(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    WINDOWS_RESERVED_NAMES.iter().any(|&reserved| reserved == upper)
}

/// Truncate `name` to at most `max_chars` characters, trying to keep the file
/// extension intact. If the extension itself exceeds the budget the stem is
/// returned as-is (still truncated).
fn truncate_preserving_extension(name: &str, max_chars: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max_chars {
        return name.to_string();
    }

    // Try to find the last dot to preserve extension.
    if let Some(dot_pos) = name.rfind('.') {
        let ext: Vec<char> = name[dot_pos..].chars().collect();
        if ext.len() < max_chars {
            let stem_budget = max_chars - ext.len();
            let stem_chars: Vec<char> = name[..dot_pos].chars().collect();
            let truncated_stem: String = stem_chars.into_iter().take(stem_budget).collect();
            let ext_str: String = ext.into_iter().collect();
            return format!("{truncated_stem}{ext_str}");
        }
    }

    // No extension or extension exceeds budget — just truncate.
    chars.into_iter().take(max_chars).collect()
}

/// Build a timestamped fallback name for when sanitization leaves nothing
/// usable (empty input, all-reserved input, `..` only, etc.).
fn fallback_name() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("download-{timestamp}")
}

#[cfg(test)]
mod tests {
    use super::sanitize_single_file_name;

    #[test]
    fn preserves_plain_name() {
        assert_eq!(sanitize_single_file_name("report.pdf"), "report.pdf");
    }

    #[test]
    fn preserves_unicode() {
        assert_eq!(sanitize_single_file_name("报告.pdf"), "报告.pdf");
    }

    #[test]
    fn blocks_path_traversal_via_slash() {
        // `/` is replaced with `_`, and leading dots are trimmed.
        assert_eq!(
            sanitize_single_file_name("../../etc/passwd"),
            "_.._etc_passwd",
        );
    }

    #[test]
    fn blocks_path_traversal_via_backslash() {
        assert_eq!(
            sanitize_single_file_name("..\\..\\windows\\system32"),
            "_.._windows_system32",
        );
    }

    #[test]
    fn strips_dotdot_component() {
        // A bare `..` trims to empty, then falls back to a timestamped name.
        let out = sanitize_single_file_name("..");
        assert!(out.starts_with("download-"), "got: {out}");
    }

    #[test]
    fn strips_single_dot() {
        let out = sanitize_single_file_name(".");
        assert!(out.starts_with("download-"), "got: {out}");
    }

    #[test]
    fn replaces_windows_reserved_characters() {
        assert_eq!(
            sanitize_single_file_name("<file>:name?.txt*"),
            "_file__name_.txt_",
        );
    }

    #[test]
    fn replaces_control_characters() {
        assert_eq!(
            sanitize_single_file_name("bad\x00name\x1F.txt"),
            "bad_name_.txt"
        );
    }

    #[test]
    fn trims_whitespace_and_dots() {
        assert_eq!(sanitize_single_file_name("  .name.  "), "name");
    }

    #[test]
    fn empty_input_falls_back() {
        let out = sanitize_single_file_name("");
        assert!(out.starts_with("download-"), "got: {out}");
    }

    #[test]
    fn all_reserved_falls_back() {
        let out = sanitize_single_file_name(":::***");
        assert!(out.starts_with("download-"), "got: {out}");
    }

    #[test]
    fn trailing_slash_is_neutralized() {
        assert_eq!(sanitize_single_file_name("folder/"), "folder_");
    }

    // ---- Windows reserved device name tests ----

    #[test]
    fn reserved_device_name_without_extension() {
        assert_eq!(sanitize_single_file_name("CON"), "_CON");
        assert_eq!(sanitize_single_file_name("NUL"), "_NUL");
        assert_eq!(sanitize_single_file_name("AUX"), "_AUX");
    }

    #[test]
    fn reserved_device_name_case_insensitive() {
        assert_eq!(sanitize_single_file_name("con"), "_con");
        assert_eq!(sanitize_single_file_name("Con"), "_Con");
        assert_eq!(sanitize_single_file_name("cOn"), "_cOn");
    }

    #[test]
    fn reserved_device_name_with_extension() {
        assert_eq!(sanitize_single_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_single_file_name("NUL.pdf"), "_NUL.pdf");
        assert_eq!(sanitize_single_file_name("com1.log"), "_com1.log");
    }

    #[test]
    fn reserved_com_and_lpt_variants() {
        for i in 1..=9 {
            assert_eq!(
                sanitize_single_file_name(&format!("COM{i}")),
                format!("_COM{i}"),
            );
            assert_eq!(
                sanitize_single_file_name(&format!("LPT{i}.txt")),
                format!("_LPT{i}.txt"),
            );
        }
    }

    #[test]
    fn non_reserved_similar_names_pass_through() {
        assert_eq!(sanitize_single_file_name("CONWAY"), "CONWAY");
        assert_eq!(sanitize_single_file_name("NULL"), "NULL");
        assert_eq!(sanitize_single_file_name("AUXILIARY"), "AUXILIARY");
        assert_eq!(sanitize_single_file_name("COM10"), "COM10");
    }

    // ---- Length clamping tests ----

    #[test]
    fn short_names_unchanged() {
        let name = "a".repeat(200);
        assert_eq!(sanitize_single_file_name(&name), name);
    }

    #[test]
    fn long_name_truncated_preserving_extension() {
        let stem = "a".repeat(300);
        let input = format!("{stem}.mp4");
        let result = sanitize_single_file_name(&input);
        assert_eq!(result.chars().count(), 200);
        assert!(result.ends_with(".mp4"));
    }

    #[test]
    fn long_name_without_extension_truncated() {
        let input = "b".repeat(500);
        let result = sanitize_single_file_name(&input);
        assert_eq!(result.chars().count(), 200);
    }

    #[test]
    fn long_name_unicode_truncated_at_char_boundary() {
        // Each 文 is 3 bytes but 1 char; truncation should be by char count.
        let input = "文".repeat(250);
        let result = sanitize_single_file_name(&input);
        assert_eq!(result.chars().count(), 200);
    }
}
