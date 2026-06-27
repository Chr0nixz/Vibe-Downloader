use tauri_app_lib::clipboard::extract_download_urls;

#[test]
fn extracts_single_url_and_trims_trailing_punctuation() {
    assert_eq!(
        extract_download_urls("Download: https://example.com/file.zip)."),
        vec!["https://example.com/file.zip".to_string()]
    );
}

#[test]
fn extracts_multiple_urls_and_deduplicates_normalized_values() {
    assert_eq!(
        extract_download_urls(
            "https://example.com/file.zip\nhttps://example.com/file.zip ftp://mirror.example.org/a.iso",
        ),
        vec![
            "https://example.com/file.zip".to_string(),
            "ftp://mirror.example.org/a.iso".to_string(),
        ]
    );
}

#[test]
fn accepts_supported_download_protocols() {
    let urls = extract_download_urls(
        "https://example.com/a.torrent ftps://files.example.com/b.bin sftp://files.example.com/c.bin magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10 file:///C:/Downloads/sample.torrent file:///C:/Downloads/list.meta4",
    );

    assert_eq!(
        urls,
        vec![
            "https://example.com/a.torrent".to_string(),
            "ftps://files.example.com/b.bin".to_string(),
            "sftp://files.example.com/c.bin".to_string(),
            "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10".to_string(),
            "file:///C:/Downloads/sample.torrent".to_string(),
            "file:///C:/Downloads/list.meta4".to_string(),
        ]
    );
}

#[test]
fn rejects_unsupported_or_sensitive_urls() {
    let urls = extract_download_urls(
        "ssh://example.com/file https://user:pass@example.com/secret.zip sftp://user:pass@example.com/secret.bin file:///C:/Downloads/readme.txt",
    );

    assert!(urls.is_empty());
}

#[test]
fn ignores_oversized_clipboard_text() {
    let text = format!("{} https://example.com/file.zip", "x".repeat(64 * 1024));
    assert!(extract_download_urls(&text).is_empty());
}

#[test]
fn short_circuits_plain_text_without_url_scheme() {
    // E-9: 纯文本无 "://" 且无 "magnet:" 应直接返回，不触发 to_ascii_lowercase 分配
    let plain_text = "x".repeat(60 * 1024);
    assert!(extract_download_urls(&plain_text).is_empty());
}

#[test]
fn short_circuit_still_extracts_magnet_lowercase() {
    // magnet: 无 "://"，短路逻辑须单独检查 "magnet:"
    let urls = extract_download_urls("magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10");
    assert_eq!(
        urls,
        vec!["magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10".to_string()]
    );
}

#[test]
fn short_circuit_falls_back_for_uppercase_magnet() {
    // 大写 MAGNET: 不命中 case-sensitive "magnet:" 快路径，
    // 但后续 to_ascii_lowercase + 前缀匹配应兜底提取
    let urls = extract_download_urls("MAGNET:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10");
    assert_eq!(
        urls,
        vec!["magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10".to_string()]
    );
}

#[test]
fn short_circuit_passes_through_non_download_scheme() {
    // 含 "://" 但非下载协议（如 foo://），短路放行后由 normalize_download_url 过滤
    let urls = extract_download_urls("foo://bar baz qux");
    assert!(urls.is_empty());
}
