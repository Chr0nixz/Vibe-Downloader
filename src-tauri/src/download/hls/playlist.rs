//! HLS manifest / playlist parsing (ARC-17).
//!
//! Keeps master/media m3u8 parsing and attribute helpers separate from the
//! download engine so parser behavior can be tested without I/O.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use hls_m3u8::{MasterPlaylist as ParsedMasterPlaylist, MediaPlaylist as ParsedMediaPlaylist};

use crate::download::error::engine_error;
use crate::models::{HlsMediaTrack, HlsVariant};

/// ARC-11: Clamp EXT-X-TARGETDURATION so a malicious/misconfigured playlist
/// cannot pin a scheduler slot for hours between polls.
pub(crate) const HLS_MAX_TARGET_DURATION_SECS: i64 = 60;

pub(crate) fn clamp_hls_target_duration(value: i64) -> i64 {
    value.clamp(1, HLS_MAX_TARGET_DURATION_SECS)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlaylistKind {
    Vod,
    Event,
    Live,
}

impl PlaylistKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Vod => "vod",
            Self::Event => "event",
            Self::Live => "live",
        }
    }

    pub(crate) fn is_live_like(&self) -> bool {
        !matches!(self, Self::Vod)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MasterVariant {
    pub(crate) uri: String,
    pub(crate) bandwidth: i64,
    pub(crate) resolution: Option<(i64, i64)>,
    pub(crate) codecs: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct MediaPlaylist {
    pub(crate) kind: PlaylistKind,
    pub(crate) target_duration: i64,
    pub(crate) media_sequence: i64,
    pub(crate) end_list: bool,
    pub(crate) segments: Vec<HlsSegment>,
}

#[derive(Debug, Clone)]
pub(crate) struct HlsSegment {
    pub(crate) media_sequence: i64,
    pub(crate) discontinuity_sequence: i64,
    pub(crate) uri: String,
    pub(crate) duration_ms: i64,
    pub(crate) byte_range: Option<ByteRange>,
    pub(crate) init_map: Option<HlsInitMap>,
    pub(crate) key: Option<HlsKey>,
}

#[derive(Debug, Clone)]
pub(crate) struct HlsInitMap {
    pub(crate) uri: String,
    pub(crate) byte_range: Option<ByteRange>,
}

#[derive(Debug, Clone)]
pub(crate) struct ByteRange {
    pub(crate) start: Option<i64>,
    pub(crate) length: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct HlsKey {
    pub(crate) method: String,
    pub(crate) uri: Option<String>,
    pub(crate) iv: Option<String>,
}

pub(crate) fn is_master_playlist(body: &str) -> bool {
    body.lines()
        .any(|line| line.trim_start().starts_with("#EXT-X-STREAM-INF"))
}

pub(crate) fn validate_playlist_syntax(body: &str) -> Result<(), String> {
    let parsed = if is_master_playlist(body) {
        ParsedMasterPlaylist::try_from(body).map(|_| ())
    } else {
        ParsedMediaPlaylist::try_from(body).map(|_| ())
    };
    parsed.map_err(|error| {
        engine_error(
            "hls_invalid_playlist",
            format!("HLS playlist could not be parsed: {error}"),
            false,
        )
    })
}

pub(crate) fn choose_master_variant(body: &str) -> Result<MasterVariant, String> {
    let variants = parse_master_variants(body);
    variants
        .into_iter()
        .max_by_key(|variant| {
            (
                variant.bandwidth,
                variant
                    .resolution
                    .map(|(width, height)| width.saturating_mul(height))
                    .unwrap_or(0),
            )
        })
        .ok_or_else(|| {
            engine_error(
                "hls_invalid_playlist",
                "HLS master playlist does not contain a playable variant.",
                false,
            )
        })
}

pub(crate) fn hls_variants_from_master(body: &str, selected_uri: &str) -> Vec<HlsVariant> {
    parse_master_variants(body)
        .into_iter()
        .map(|variant| HlsVariant {
            selected: variant.uri == selected_uri,
            uri: variant.uri,
            bandwidth: variant.bandwidth.to_string(),
            resolution: variant
                .resolution
                .map(|(width, height)| format!("{width}x{height}")),
            codecs: variant.codecs,
        })
        .collect()
}

pub(crate) fn parse_master_variants(body: &str) -> Vec<MasterVariant> {
    let mut variants = Vec::new();
    let mut pending_attrs: Option<HashMap<String, String>> = None;
    for raw in body.lines() {
        let line = raw.trim();
        if line.starts_with("#EXT-X-STREAM-INF:") {
            pending_attrs = Some(parse_attributes(
                line.trim_start_matches("#EXT-X-STREAM-INF:"),
            ));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(attrs) = pending_attrs.take() {
            let bandwidth = attrs
                .get("BANDWIDTH")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0);
            let resolution = attrs.get("RESOLUTION").and_then(|value| {
                let (w, h) = value.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            });
            variants.push(MasterVariant {
                uri: line.to_string(),
                bandwidth,
                resolution,
                codecs: attrs.get("CODECS").cloned(),
            });
        }
    }
    variants
}

pub(crate) fn parse_media_playlist(body: &str) -> Result<MediaPlaylist, String> {
    if !body.lines().any(|line| line.trim() == "#EXTM3U") {
        return Err(engine_error(
            "hls_invalid_playlist",
            "HLS playlist is missing #EXTM3U.",
            false,
        ));
    }
    let mut playlist_type = None;
    let mut target_duration = 6_i64;
    let mut media_sequence = 0_i64;
    let mut discontinuity_sequence = 0_i64;
    let mut next_duration = None;
    let mut current_key: Option<HlsKey> = None;
    let mut current_init_map: Option<HlsInitMap> = None;
    let mut current_byte_range: Option<ByteRange> = None;
    let mut next_byte_range_start: Option<i64> = None;
    let mut segments = Vec::new();
    let mut end_list = false;
    let mut segment_index = 0_i64;

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            // ARC-11: clamp at parse time so probe UI and sleep share one bound.
            target_duration =
                clamp_hls_target_duration(value.trim().parse::<i64>().unwrap_or(target_duration));
        } else if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = value.trim().parse::<i64>().unwrap_or(0).max(0);
        } else if let Some(value) = line.strip_prefix("#EXT-X-PLAYLIST-TYPE:") {
            playlist_type = Some(value.trim().to_ascii_uppercase());
        } else if let Some(value) = line.strip_prefix("#EXT-X-KEY:") {
            current_key = parse_key(value)?;
        } else if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            current_byte_range = parse_byte_range(value);
        } else if let Some(value) = line.strip_prefix("#EXTINF:") {
            next_duration = parse_extinf_duration(value);
        } else if line == "#EXT-X-DISCONTINUITY" {
            discontinuity_sequence += 1;
            next_byte_range_start = None;
        } else if line == "#EXT-X-ENDLIST" {
            end_list = true;
        } else if let Some(value) = line.strip_prefix("#EXT-X-MAP:") {
            current_init_map = Some(parse_init_map(value)?);
        } else if !line.starts_with('#') {
            let sequence = media_sequence + segment_index;
            let byte_range = current_byte_range.take().map(|range| {
                let start = range.start.or(next_byte_range_start).unwrap_or(0);
                next_byte_range_start = Some(start.saturating_add(range.length));
                ByteRange {
                    start: Some(start),
                    length: range.length,
                }
            });
            if byte_range.is_none() {
                next_byte_range_start = None;
            }
            segments.push(HlsSegment {
                media_sequence: sequence,
                discontinuity_sequence,
                uri: line.to_string(),
                duration_ms: next_duration.take().unwrap_or(0),
                byte_range,
                init_map: current_init_map.clone(),
                key: current_key.clone(),
            });
            segment_index += 1;
        }
    }

    let kind = match playlist_type.as_deref() {
        Some("VOD") => PlaylistKind::Vod,
        Some("EVENT") => PlaylistKind::Event,
        _ if end_list => PlaylistKind::Vod,
        _ => PlaylistKind::Live,
    };
    Ok(MediaPlaylist {
        kind,
        target_duration,
        media_sequence,
        end_list,
        segments,
    })
}

pub(crate) fn reject_unsupported_media_playlist(body: &str) -> Result<(), String> {
    for line in body.lines().map(str::trim) {
        if line.starts_with("#EXT-X-KEY:") {
            let attrs = parse_attributes(line.trim_start_matches("#EXT-X-KEY:"));
            let method = attrs.get("METHOD").map(String::as_str).unwrap_or("NONE");
            if !matches!(method, "NONE" | "AES-128") {
                return Err(engine_error(
                    "hls_unsupported_encryption",
                    format!("Unsupported HLS encryption method: {method}"),
                    false,
                ));
            }
            if attrs
                .get("KEYFORMAT")
                .is_some_and(|value| value != "identity" && value != "\"identity\"")
            {
                return Err(engine_error(
                    "hls_unsupported_encryption",
                    "Only identity HLS AES-128 keys are supported.",
                    false,
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_key(value: &str) -> Result<Option<HlsKey>, String> {
    let attrs = parse_attributes(value);
    let method = attrs
        .get("METHOD")
        .cloned()
        .unwrap_or_else(|| "NONE".to_string());
    if method == "NONE" {
        return Ok(None);
    }
    if method != "AES-128" {
        return Err(engine_error(
            "hls_unsupported_encryption",
            format!("Unsupported HLS encryption method: {method}"),
            false,
        ));
    }
    Ok(Some(HlsKey {
        method,
        uri: attrs.get("URI").cloned(),
        iv: attrs.get("IV").cloned(),
    }))
}

pub(crate) fn parse_init_map(value: &str) -> Result<HlsInitMap, String> {
    let attrs = parse_attributes(value);
    let uri = attrs.get("URI").cloned().ok_or_else(|| {
        engine_error(
            "hls_invalid_playlist",
            "HLS EXT-X-MAP is missing a URI.",
            false,
        )
    })?;
    Ok(HlsInitMap {
        uri,
        byte_range: attrs
            .get("BYTERANGE")
            .and_then(|value| parse_byte_range(value)),
    })
}

pub(crate) fn parse_ext_x_media(body: &str, base_url: &str) -> Vec<HlsMediaTrack> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("#EXT-X-MEDIA:")?;
            let attrs = parse_attributes(rest);
            let kind = attrs.get("TYPE").cloned()?;
            // Skip CLOSED-CAPTIONS — we don't handle CEA-608/708 embedded in video.
            if kind.eq_ignore_ascii_case("CLOSED-CAPTIONS") {
                return None;
            }
            // FUN-10: resolve relative rendition URIs against the master so
            // create/download never see opaque relative paths.
            let uri = attrs
                .get("URI")
                .map(|value| resolve_url(base_url, value).unwrap_or_else(|_| value.clone()));
            Some(HlsMediaTrack {
                kind,
                group_id: attrs.get("GROUP-ID").cloned().unwrap_or_default(),
                name: attrs.get("NAME").cloned().unwrap_or_default(),
                language: attrs.get("LANGUAGE").cloned(),
                default: attrs
                    .get("DEFAULT")
                    .is_some_and(|v| v.eq_ignore_ascii_case("YES")),
                auto_select: attrs
                    .get("AUTOSELECT")
                    .is_some_and(|v| v.eq_ignore_ascii_case("YES")),
                uri,
            })
        })
        .collect()
}

pub(crate) fn parse_attributes(value: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let mut key = String::new();
    let mut current = String::new();
    let mut in_key = true;
    let mut in_quote = false;
    for ch in value.chars() {
        match ch {
            '=' if in_key => {
                key = current.trim().to_ascii_uppercase();
                current.clear();
                in_key = false;
            }
            '"' => {
                in_quote = !in_quote;
            }
            ',' if !in_key && !in_quote => {
                attrs.insert(key.clone(), current.trim().trim_matches('"').to_string());
                key.clear();
                current.clear();
                in_key = true;
            }
            ch => current.push(ch),
        }
    }
    if !key.is_empty() {
        attrs.insert(key, current.trim().trim_matches('"').to_string());
    }
    attrs
}

pub(crate) fn parse_extinf_duration(value: &str) -> Option<i64> {
    let duration = value.split(',').next()?.trim().parse::<f64>().ok()?;
    Some((duration.max(0.0) * 1000.0).round() as i64)
}

pub(crate) fn parse_byte_range(value: &str) -> Option<ByteRange> {
    let (length, start) = value
        .trim()
        .split_once('@')
        .map_or((value.trim(), None), |(l, s)| (l.trim(), Some(s.trim())));
    Some(ByteRange {
        length: length.parse::<i64>().ok()?.max(0),
        start: start
            .and_then(|value| value.parse::<i64>().ok())
            .map(|value| value.max(0)),
    })
}

pub(crate) fn init_map_local_name(uri: &str, byte_range: Option<&ByteRange>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut hasher);
    if let Some(range) = byte_range {
        range.start.hash(&mut hasher);
        range.length.hash(&mut hasher);
    }
    format!("init-{:016x}.mp4", hasher.finish())
}

pub(crate) fn resolve_url(base: &str, value: &str) -> Result<String, String> {
    let base = reqwest::Url::parse(base).map_err(|_| "HLS base URL is invalid.".to_string())?;
    base.join(value)
        .map(|url| url.to_string())
        .map_err(|_| "HLS playlist contains an invalid relative URL.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_highest_bandwidth_variant() {
        let variant = choose_master_variant(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100,RESOLUTION=640x360\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=200,RESOLUTION=1280x720\nhi.m3u8\n",
        )
        .expect("variant");
        assert_eq!(variant.uri, "hi.m3u8");
        assert_eq!(variant.resolution, Some((1280, 720)));
    }

    #[test]
    fn parses_live_media_sequence_and_segments() {
        let media = parse_media_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:42\n#EXTINF:5.5,\nseg42.ts\n#EXTINF:5,\nseg43.ts\n",
        )
        .expect("media");
        assert_eq!(media.kind, PlaylistKind::Live);
        assert_eq!(media.media_sequence, 42);
        assert_eq!(media.segments[0].media_sequence, 42);
        assert_eq!(media.segments[0].duration_ms, 5500);
    }

    #[test]
    fn clamps_oversized_target_duration() {
        // ARC-11: huge TARGETDURATION must not survive parse.
        let media = parse_media_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:999999\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1,\nseg0.ts\n",
        )
        .expect("media");
        assert_eq!(media.target_duration, HLS_MAX_TARGET_DURATION_SECS);
        assert_eq!(clamp_hls_target_duration(0), 1);
        assert_eq!(clamp_hls_target_duration(-5), 1);
        assert_eq!(clamp_hls_target_duration(30), 30);
    }

    #[test]
    fn resolves_relative_hls_byte_ranges() {
        let media = parse_media_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-BYTERANGE:100@50\n#EXTINF:5,\nfile.ts\n#EXT-X-BYTERANGE:75\n#EXTINF:5,\nfile.ts\n",
        )
        .expect("media");
        assert_eq!(
            media.segments[0]
                .byte_range
                .as_ref()
                .map(|range| range.start),
            Some(Some(50))
        );
        assert_eq!(
            media.segments[0]
                .byte_range
                .as_ref()
                .map(|range| range.length),
            Some(100)
        );
        assert_eq!(
            media.segments[1]
                .byte_range
                .as_ref()
                .map(|range| range.start),
            Some(Some(150))
        );
        assert_eq!(
            media.segments[1]
                .byte_range
                .as_ref()
                .map(|range| range.length),
            Some(75)
        );
    }

    #[test]
    fn tracks_hls_discontinuity_sequences() {
        let media = parse_media_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:5,\nseg0.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:5,\nseg1.ts\n",
        )
        .expect("media");
        assert_eq!(media.segments[0].discontinuity_sequence, 0);
        assert_eq!(media.segments[1].discontinuity_sequence, 1);
    }

    #[test]
    fn rejects_sample_aes() {
        let error = reject_unsupported_media_playlist(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"key\"\n",
        )
        .unwrap_err();
        assert!(error.contains("hls_unsupported_encryption"));
    }

    // E-2: Safety-limit constants must stay pinned at their documented values.
    // Accidentally raising or removing these caps would reintroduce the
    // unbounded-memory-growth risk the caps are meant to prevent.

    #[test]
    fn parse_ext_x_media_extracts_audio_and_subtitle_tracks() {
        let master = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud1\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,URI=\"en.m3u8\"\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud1\",NAME=\"Spanish\",LANGUAGE=\"es\",DEFAULT=NO,AUTOSELECT=YES,URI=\"es.m3u8\"\n\
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"sub1\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,URI=\"en-subs.m3u8\"\n\
#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID=\"cc1\",NAME=\"CC\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES\n\
#EXT-X-STREAM-INF:BANDWIDTH=1000000,AUDIO=\"aud1\",SUBTITLES=\"sub1\"\n\
video.m3u8\n";
        let tracks = parse_ext_x_media(master, "https://cdn.example/master.m3u8");
        assert_eq!(
            tracks.len(),
            3,
            "should parse 3 tracks (2 audio + 1 subtitle, skip CLOSED-CAPTIONS)"
        );
        let audio: Vec<_> = tracks.iter().filter(|t| t.kind == "AUDIO").collect();
        let subs: Vec<_> = tracks.iter().filter(|t| t.kind == "SUBTITLES").collect();
        assert_eq!(audio.len(), 2);
        assert_eq!(subs.len(), 1);
        // English audio (default) — FUN-10 resolves relative URIs against master.
        assert_eq!(audio[0].group_id, "aud1");
        assert_eq!(audio[0].name, "English");
        assert_eq!(audio[0].language.as_deref(), Some("en"));
        assert!(audio[0].default);
        assert!(audio[0].auto_select);
        assert_eq!(audio[0].uri.as_deref(), Some("https://cdn.example/en.m3u8"));
        // Spanish audio (not default)
        assert_eq!(audio[1].name, "Spanish");
        assert!(!audio[1].default);
        assert_eq!(audio[1].uri.as_deref(), Some("https://cdn.example/es.m3u8"));
        // Subtitles
        assert_eq!(subs[0].kind, "SUBTITLES");
        assert_eq!(
            subs[0].uri.as_deref(),
            Some("https://cdn.example/en-subs.m3u8")
        );
        assert!(subs[0].default);
    }

    #[test]
    fn parse_ext_x_media_handles_embedded_tracks_with_null_uri() {
        let master = "#EXTM3U\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud1\",NAME=\"Audio\",DEFAULT=YES,AUTOSELECT=YES\n\
#EXT-X-STREAM-INF:BANDWIDTH=500000,AUDIO=\"aud1\"\n\
video.m3u8\n";
        let tracks = parse_ext_x_media(master, "https://cdn.example/master.m3u8");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].kind, "AUDIO");
        assert!(
            tracks[0].uri.is_none(),
            "embedded track should have null URI"
        );
    }

    #[test]
    fn parse_ext_x_media_returns_empty_for_media_playlist() {
        let media = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:6\n\
#EXTINF:6.0,\n\
seg1.ts\n\
#EXT-X-ENDLIST\n";
        let tracks = parse_ext_x_media(media, "https://cdn.example/media.m3u8");
        assert!(tracks.is_empty());
    }
}
