//! URL protocol classification: determines the owning engine by URL scheme and path suffix.
//!
//! This is the single source of truth for `is_torrent_url` / `is_metalink_url` / `is_hls_url` / `is_dash_url`,
//! shared by `EngineRegistry::engine_for_uri` and the task creation path.

pub fn is_torrent_url(url: &reqwest::Url) -> bool {
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

pub fn is_hls_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".m3u8"))
}

pub fn is_dash_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".mpd"))
}
