//! Shared HTTP test server helpers extracted from `http_engine.rs`.
//!
//! These low-level TCP helpers let multiple engine integration test binaries
//! (HTTP, WebDAV, Metalink, HLS) stand up a fake HTTP server with consistent
//! response formatting — Content-Length, Content-Range, Content-Disposition,
//! and optional slow-drip bodies — without each binary re-implementing the
//! same `write_response` / `respond_file` plumbing.
//!
//! FTP and SFTP have their own dedicated fake servers (`ftp_engine.rs` ships
//! a minimal PASV+RETR server inline; SFTP uses `common::sftp_server`).

#![allow(dead_code)]

use std::{
    io::{Read, Write},
    net::TcpStream,
    thread,
    time::Duration,
};

/// A parsed `Range: bytes=start-end` header value. `end` is `None` for
/// open-ended ranges like `bytes=10-`.
#[derive(Clone, Copy)]
pub struct ByteRange {
    pub start: usize,
    pub end: Option<usize>,
}

/// Parse a single HTTP request line, returning the (method, path) tuple.
/// Reads at most `buffer.len()` bytes from the stream; on EOF or read error
/// returns `None` and the caller should close the connection.
pub fn read_request_line(stream: &mut TcpStream, buffer: &mut [u8]) -> Option<(String, String)> {
    let read = stream.read(buffer).ok()?;
    if read == 0 {
        return None;
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or("/").to_string();
    Some((method, path))
}

/// Returns the full request text (request line + headers) as a string.
/// Used by handlers that need to inspect arbitrary headers like
/// `If-Range`, `Authorization`, `Depth`, etc.
pub fn read_request_text(stream: &mut TcpStream, buffer: &mut [u8]) -> Option<String> {
    let read = stream.read(buffer).ok()?;
    if read == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&buffer[..read]).into_owned())
}

/// Parse a `Range: bytes=start-end` header from a request line iterator.
pub fn parse_range(line: &str) -> Option<ByteRange> {
    let (name, value) = line.split_once(':')?;
    if !name.eq_ignore_ascii_case("range") {
        return None;
    }
    let (start, end) = value.trim().strip_prefix("bytes=")?.split_once('-')?;
    Some(ByteRange {
        start: start.parse::<usize>().ok()?,
        end: if end.is_empty() {
            None
        } else {
            Some(end.parse::<usize>().ok()?)
        },
    })
}

/// Find and parse a specific header from the request. Comparison is
/// case-insensitive on the header name.
pub fn parse_header(lines: impl Iterator<Item = String>, expected_name: &str) -> Option<String> {
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case(expected_name) {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Write a complete HTTP response with the given status, headers, and body.
/// `Connection: close` is always appended; the body is written in one shot
/// unless `slow` is true, in which case the body is dripped 1KB at a time
/// with a 10ms delay between chunks (used by speed-limit regression tests).
pub fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
    slow: bool,
) {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        207 => "Multi-Status",
        301 => "Moved Permanently",
        302 => "Found",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        412 => "Precondition Failed",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut response = format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n");
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    if slow {
        for chunk in body.chunks(1024) {
            let _ = stream.write_all(chunk);
            let _ = stream.flush();
            thread::sleep(Duration::from_millis(10));
        }
    } else {
        let _ = stream.write_all(body);
        let _ = stream.flush();
    }
}

/// Respond with a file body, honoring an optional `Range` header. Emits a
/// 206 with `Content-Range` when `byte_range.is_some() && supports_parallel`,
/// otherwise a 200 with the full body.
///
/// `Content-Disposition: attachment; filename="<file_name>"` is always
/// attached so the engine's file-name derivation picks it up.
pub fn respond_file(
    stream: &mut TcpStream,
    method: &str,
    payload: &[u8],
    byte_range: Option<ByteRange>,
    supports_parallel: bool,
    file_name: &str,
    slow: bool,
) {
    let start = byte_range
        .map(|range| range.start)
        .unwrap_or(0)
        .min(payload.len());
    let end = byte_range
        .and_then(|range| range.end)
        .unwrap_or_else(|| payload.len().saturating_sub(1))
        .min(payload.len().saturating_sub(1));
    let body = if method == "HEAD" || start > end {
        &[][..]
    } else {
        &payload[start..=end]
    };
    let status = if byte_range.is_some() && supports_parallel {
        206
    } else {
        200
    };
    let content_length = if method == "HEAD" {
        payload.len().to_string()
    } else {
        body.len().to_string()
    };
    let content_range = format!("bytes {start}-{end}/{}", payload.len());
    let disposition = format!("attachment; filename=\"{file_name}\"");
    let mut headers = vec![
        ("Content-Length", content_length.as_str()),
        ("Content-Type", "application/octet-stream"),
        ("Content-Disposition", disposition.as_str()),
    ];
    if supports_parallel {
        headers.push(("Accept-Ranges", "bytes"));
    }
    if status == 206 {
        headers.push(("Content-Range", content_range.as_str()));
    }
    write_response(stream, status, &headers, body, slow);
}

/// Variant of [`respond_file`] that omits `Content-Disposition`. Used to
/// exercise URL-derived file name fallbacks.
pub fn respond_file_without_disposition(
    stream: &mut TcpStream,
    method: &str,
    payload: &[u8],
    byte_range: Option<ByteRange>,
    supports_parallel: bool,
    slow: bool,
) {
    let start = byte_range
        .map(|range| range.start)
        .unwrap_or(0)
        .min(payload.len());
    let end = byte_range
        .and_then(|range| range.end)
        .unwrap_or_else(|| payload.len().saturating_sub(1))
        .min(payload.len().saturating_sub(1));
    let body = if method == "HEAD" || start > end {
        &[][..]
    } else {
        &payload[start..=end]
    };
    let status = if byte_range.is_some() && supports_parallel {
        206
    } else {
        200
    };
    let content_length = if method == "HEAD" {
        payload.len().to_string()
    } else {
        body.len().to_string()
    };
    let content_range = format!("bytes {start}-{end}/{}", payload.len());
    let mut headers = vec![
        ("Content-Length", content_length.as_str()),
        ("Content-Type", "application/octet-stream"),
    ];
    if supports_parallel {
        headers.push(("Accept-Ranges", "bytes"));
    }
    if status == 206 {
        headers.push(("Content-Range", content_range.as_str()));
    }
    write_response(stream, status, &headers, body, slow);
}

/// Respond without `Content-Length`. Used to exercise the unknown-size
/// single-stream download path. The body is written only for non-HEAD
/// requests.
pub fn write_unknown_size_response(
    stream: &mut TcpStream,
    method: &str,
    body: &[u8],
    headers: &[(&str, &str)],
) {
    let mut response = "HTTP/1.1 200 OK\r\nConnection: close\r\n".to_string();
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

/// Convenience helper to derive a `ByteRange` from a raw request buffer.
/// Reads the request, finds the `Range:` header, parses it. Returns `None`
/// if the request has no Range header or the value is malformed.
pub fn extract_byte_range(request: &str) -> Option<ByteRange> {
    request.lines().find_map(parse_range)
}

/// Convenience helper to fetch the value of an arbitrary request header
/// (case-insensitive name match).
pub fn extract_header<'a>(request: &'a str, expected_name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(expected_name).then(|| value.trim())
    })
}
