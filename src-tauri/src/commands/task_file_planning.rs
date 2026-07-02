use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{
    download::ProbeOutput,
    models::{ProbedFile, TaskFileRecord, TaskRecord, TaskStatus},
};

pub fn normalized_probe_files(probe: &ProbeOutput) -> Vec<ProbedFile> {
    if probe.files.is_empty() {
        return vec![ProbedFile {
            relative_path: probe.display_name.clone(),
            size: probe.total_size.to_string(),
            content_type: probe.content_type.clone(),
        }];
    }
    probe.files.clone()
}

pub fn task_file_records_from_probe(
    task: &TaskRecord,
    files: &[ProbedFile],
    save_dir: &Path,
    single_final_path: &Path,
    single_temp_path: &Path,
    single_file_name: &str,
    selected_relative_paths: Option<&HashSet<String>>,
) -> Result<Vec<TaskFileRecord>, String> {
    let single_file = files.len() == 1;
    let mut records = Vec::with_capacity(files.len());
    for file in files {
        let selection_key = sanitize_probe_relative_path(&file.relative_path);
        let (relative_path, file_name, final_path, temp_path) = if single_file {
            (
                single_file_name.to_string(),
                single_file_name.to_string(),
                single_final_path.to_path_buf(),
                single_temp_path.to_path_buf(),
            )
        } else {
            let relative_path = sanitize_relative_path(&file.relative_path);
            let parent = save_dir.join(relative_path.parent().unwrap_or_else(|| Path::new("")));
            std::fs::create_dir_all(&parent)
                .map_err(|e| format!("Could not create the download directory: {e}"))?;
            let requested_name = relative_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("download");
            let final_path = unique_final_path(&parent, requested_name);
            let relative_path = final_path
                .strip_prefix(save_dir)
                .unwrap_or(&final_path)
                .to_string_lossy()
                .replace('\\', "/");
            let file_name = final_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(requested_name)
                .to_string();
            let temp_path = PathBuf::from(format!("{}.vibe-downloading", final_path.display()));
            (relative_path, file_name, final_path, temp_path)
        };

        let selected = selected_relative_paths
            .map(|paths| paths.contains(&selection_key))
            .unwrap_or(true);

        records.push(TaskFileRecord {
            id: Uuid::new_v4().to_string(),
            task_id: task.id.clone(),
            relative_path,
            file_name,
            save_dir: task.save_dir.clone(),
            temp_path: Some(temp_path.to_string_lossy().to_string()),
            final_path: Some(final_path.to_string_lossy().to_string()),
            total_size: parse_probed_file_size(&file.size),
            downloaded_bytes: 0,
            selected,
            status: TaskStatus::Queued,
            content_type: file.content_type.clone(),
        });
    }
    Ok(records)
}

pub fn normalize_sha256(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = value.to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("SHA-256 must be 64 hexadecimal characters.".to_string());
    }
    Ok(Some(normalized))
}

/// F-5: Normalize an expected hash digest for any supported algorithm.
/// Validates hex length per algorithm: MD5=32, SHA-1=40, SHA-256=64, SHA-512=128.
/// Returns lowercase hex digest. Empty/whitespace input returns Ok(None).
pub fn normalize_expected_hash(
    value: Option<&str>,
    algorithm: crate::models::ChecksumAlgorithm,
) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = value.to_ascii_lowercase();
    let expected_len = match algorithm {
        crate::models::ChecksumAlgorithm::Md5 => 32,
        crate::models::ChecksumAlgorithm::Sha1 => 40,
        crate::models::ChecksumAlgorithm::Sha256 => 64,
        crate::models::ChecksumAlgorithm::Sha512 => 128,
    };
    let label = algorithm.as_str().to_ascii_uppercase();
    if normalized.len() != expected_len || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("{label} must be {expected_len} hexadecimal characters."));
    }
    Ok(Some(normalized))
}

pub fn unique_final_path(save_dir: &Path, requested_file_name: &str) -> PathBuf {
    let sanitized = sanitize_file_name(requested_file_name);
    let candidate = save_dir.join(&sanitized);
    if !candidate.exists()
        && !PathBuf::from(format!("{}.vibe-downloading", candidate.display())).exists()
    {
        return candidate;
    }

    let path = Path::new(&sanitized);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());

    for index in 1..10_000 {
        let name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem} ({index}).{extension}"),
            _ => format!("{stem} ({index})"),
        };
        let candidate = save_dir.join(name);
        if !candidate.exists()
            && !PathBuf::from(format!("{}.vibe-downloading", candidate.display())).exists()
        {
            return candidate;
        }
    }

    save_dir.join(format!("download-{}", chrono::Utc::now().timestamp()))
}

pub fn sanitize_probe_relative_path(value: &str) -> String {
    sanitize_relative_path(value)
        .to_string_lossy()
        .replace('\\', "/")
}

fn sanitize_relative_path(value: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for component in value
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|component| !component.is_empty() && *component != "." && *component != "..")
    {
        path.push(sanitize_file_name(component));
    }
    if path.as_os_str().is_empty() {
        path.push(format!("download-{}", chrono::Utc::now().timestamp()));
    }
    path
}

fn parse_probed_file_size(value: &str) -> i64 {
    value.parse::<i64>().unwrap_or(0)
}

fn sanitize_file_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        format!("download-{}", chrono::Utc::now().timestamp())
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChecksumAlgorithm;

    #[test]
    fn normalize_expected_hash_accepts_valid_digest_per_algorithm() {
        let md5 = "d41d8cd98f00b204e9800998ecf8427e";
        let sha1 = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        let sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let sha512 = "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";

        assert_eq!(
            normalize_expected_hash(Some(md5), ChecksumAlgorithm::Md5).unwrap(),
            Some(md5.to_string())
        );
        assert_eq!(
            normalize_expected_hash(Some(sha1), ChecksumAlgorithm::Sha1).unwrap(),
            Some(sha1.to_string())
        );
        assert_eq!(
            normalize_expected_hash(Some(sha256), ChecksumAlgorithm::Sha256).unwrap(),
            Some(sha256.to_string())
        );
        assert_eq!(
            normalize_expected_hash(Some(sha512), ChecksumAlgorithm::Sha512).unwrap(),
            Some(sha512.to_string())
        );
    }

    #[test]
    fn normalize_expected_hash_lowercases_and_trims() {
        let upper = "D41D8CD98F00B204E9800998ECF8427E";
        let expected = "d41d8cd98f00b204e9800998ecf8427e";
        assert_eq!(
            normalize_expected_hash(Some(&format!("  {upper}  ")), ChecksumAlgorithm::Md5).unwrap(),
            Some(expected.to_string())
        );
    }

    #[test]
    fn normalize_expected_hash_rejects_wrong_length() {
        // MD5 digest passed as SHA-256 → length mismatch.
        let md5 = "d41d8cd98f00b204e9800998ecf8427e";
        assert!(normalize_expected_hash(Some(md5), ChecksumAlgorithm::Sha256).is_err());
    }

    #[test]
    fn normalize_expected_hash_rejects_non_hex() {
        // Right length but contains non-hex chars.
        let bad = "z".repeat(64);
        assert!(normalize_expected_hash(Some(&bad), ChecksumAlgorithm::Sha256).is_err());
    }

    #[test]
    fn normalize_expected_hash_none_for_empty_input() {
        assert_eq!(
            normalize_expected_hash(None, ChecksumAlgorithm::Sha256).unwrap(),
            None
        );
        assert_eq!(
            normalize_expected_hash(Some("   "), ChecksumAlgorithm::Sha256).unwrap(),
            None
        );
    }
}
