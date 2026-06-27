//! URL 协议分类工具：按 URL scheme 与路径后缀判定归属引擎。
//!
//! 这是 `is_torrent_url` / `is_metalink_url` / `is_hls_url` / `is_dash_url`
//! 的唯一来源，供 `EngineRegistry::engine_for_uri` 与任务创建路径共享。

pub(crate) fn is_torrent_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".torrent"))
}

pub(crate) fn is_metalink_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.ends_with(".meta4") || name.ends_with(".metalink")
            })
}

pub(crate) fn is_hls_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".m3u8"))
}

pub(crate) fn is_dash_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".mpd"))
}
