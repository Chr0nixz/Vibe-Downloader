use reqwest::{
    header::{
        ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_LOCATION, CONTENT_RANGE,
        CONTENT_TYPE, ETAG, LAST_MODIFIED,
    },
    Response, StatusCode,
};

use super::ProbeResult;

pub(super) fn probe_from_response(
    original_url: &str,
    response: &Response,
    range_probe: bool,
) -> Result<ProbeResult, String> {
    let final_url = response.url().to_string();
    let headers = response.headers();
    let content_range_size = headers
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range_total);
    let content_length = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let total_size = content_range_size.or(content_length).unwrap_or(0);

    let supports_parallel = total_size > 0
        && (content_range_size.is_some()
            || (range_probe && response.status() == StatusCode::PARTIAL_CONTENT)
            || headers
                .get(ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.eq_ignore_ascii_case("bytes")));

    let source_key = response
        .url()
        .host_str()
        .map(str::to_string)
        .or_else(|| {
            reqwest::Url::parse(original_url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
        })
        .unwrap_or_else(|| "unknown".to_string());

    Ok(ProbeResult {
        file_name: file_name_from_response(response),
        final_url,
        total_size,
        supports_resume: supports_parallel,
        supports_parallel,
        supports_multi_file: false,
        source_key,
        etag: header_to_string(response, ETAG),
        last_modified: header_to_string(response, LAST_MODIFIED),
        content_type: header_to_string(response, CONTENT_TYPE),
    })
}

fn file_name_from_response(response: &Response) -> String {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());

    let name = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .or_else(|| {
            response
                .headers()
                .get(CONTENT_LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(file_name_from_path_like)
        })
        .or_else(|| {
            response
                .url()
                .query_pairs()
                .find(|(key, _)| key.eq_ignore_ascii_case("response-content-disposition"))
                .and_then(|(_, value)| parse_content_disposition_filename(&value))
        })
        .or_else(|| {
            response
                .url()
                .path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| format!("download-{}", chrono::Utc::now().timestamp()));

    ensure_extension_from_content_type(name, content_type)
}

fn parse_content_disposition_filename(value: &str) -> Option<String> {
    let mut plain = None;
    let mut encoded = None;

    for part in value.split(';') {
        let trimmed = part.trim();
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        if key.eq_ignore_ascii_case("filename*") {
            encoded = decode_rfc5987_filename(value);
        } else if key.eq_ignore_ascii_case("filename") {
            plain = Some(value.to_string());
        }
    }

    encoded.or(plain).filter(|name| !name.trim().is_empty())
}

fn decode_rfc5987_filename(value: &str) -> Option<String> {
    let encoded = value
        .split_once("''")
        .map(|(_, value)| value)
        .unwrap_or(value);
    let decoded = percent_decode_lossy(encoded);
    (!decoded.trim().is_empty()).then_some(decoded)
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).to_string()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn file_name_from_path_like(value: &str) -> Option<String> {
    reqwest::Url::parse(value)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
        })
        .or_else(|| {
            value
                .rsplit(['/', '\\'])
                .next()
                .map(str::to_string)
                .filter(|name| !name.trim().is_empty())
        })
}

fn ensure_extension_from_content_type(name: String, content_type: Option<&str>) -> String {
    if std::path::Path::new(&name).extension().is_some() {
        return name;
    }

    let Some(extension) = content_type.and_then(extension_from_content_type) else {
        return name;
    };
    format!("{name}.{extension}")
}

fn extension_from_content_type(value: &str) -> Option<&'static str> {
    let mime = value.split(';').next()?.trim().to_ascii_lowercase();
    match mime.as_str() {
        "application/zip" => Some("zip"),
        "application/pdf" => Some("pdf"),
        "application/json" => Some("json"),
        "application/octet-stream" => Some("bin"),
        "application/x-7z-compressed" => Some("7z"),
        "application/x-rar-compressed" | "application/vnd.rar" => Some("rar"),
        "application/x-tar" => Some("tar"),
        "application/gzip" => Some("gz"),
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "text/plain" => Some("txt"),
        "text/html" => Some("html"),
        "video/mp4" => Some("mp4"),
        "audio/mpeg" => Some("mp3"),
        _ => None,
    }
}

fn parse_content_range_total(value: &str) -> Option<i64> {
    value.rsplit_once('/')?.1.parse::<i64>().ok()
}

fn header_to_string(response: &Response, name: reqwest::header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
