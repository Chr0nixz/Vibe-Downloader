mod common;

use std::{
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
};

use common::TestServer;
use tauri_app_lib::{
    download::{DashEngine, DownloadEngine, ProbeRequest},
    proxy::ResolvedProxyConfig,
};

const TEMPLATE_MPD: &str = r#"
<MPD type="static" mediaPresentationDuration="PT60S" xmlns="urn:mpeg:dash:schema:mpd:2011">
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <Representation id="v0" bandwidth="500000" codecs="avc1.42c01e">
        <SegmentTemplate media="seg-$Number$.m4s" initialization="init.m4s" startNumber="1" duration="2" timescale="1" />
      </Representation>
      <Representation id="v1" bandwidth="1000000" codecs="avc1.42c01e">
        <SegmentTemplate media="seg-$Number$.m4s" initialization="init.m4s" startNumber="1" duration="2" timescale="1" />
      </Representation>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" contentType="audio">
      <Representation id="a0" bandwidth="128000" codecs="mp4a.40.2">
        <SegmentTemplate media="aseg-$Number$.m4s" initialization="ainit.m4s" startNumber="1" duration="2" timescale="1" />
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>
"#;

const DYNAMIC_MPD: &str = r#"
<MPD type="dynamic" xmlns="urn:mpeg:dash:schema:mpd:2011">
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <Representation id="v0" bandwidth="500000" />
    </AdaptationSet>
  </Period>
</MPD>
"#;

const TIMELINE_MPD: &str = r#"
<MPD type="static" mediaPresentationDuration="PT60S" xmlns="urn:mpeg:dash:schema:mpd:2011">
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <Representation id="v0" bandwidth="500000">
        <SegmentTemplate media="seg-$Number$.m4s">
          <SegmentTimeline>
            <S t="0" d="2" />
          </SegmentTimeline>
        </SegmentTemplate>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>
"#;

const LIST_MPD: &str = r#"
<MPD type="static" mediaPresentationDuration="PT10S" xmlns="urn:mpeg:dash:schema:mpd:2011">
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <Representation id="v0" bandwidth="500000">
        <SegmentList initialization="init.m4s">
          <SegmentURL media="seg1.m4s" />
          <SegmentURL media="seg2.m4s" />
          <SegmentURL media="seg3.m4s" />
        </SegmentList>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>
"#;

const BASE_MPD: &str = r#"
<MPD type="static" mediaPresentationDuration="PT10S" xmlns="urn:mpeg:dash:schema:mpd:2011">
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <Representation id="v0" bandwidth="500000">
        <BaseURL>video.mp4</BaseURL>
        <SegmentBase>
          <Initialization range="0-1023" />
        </SegmentBase>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>
"#;

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn start_test_server() -> TestServer {
    TestServer::start(handle_connection)
}

fn handle_connection(mut stream: TcpStream) {
    let mut buffer = [0_u8; 4096];
    let Ok(read) = stream.read(&mut buffer) else {
        return;
    };
    if read == 0 {
        return;
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/");

    let (status, content_type, body) = match path {
        "/manifest.mpd" => (200, "application/dash+xml", TEMPLATE_MPD.as_bytes()),
        "/live.mpd" => (200, "application/dash+xml", DYNAMIC_MPD.as_bytes()),
        "/timeline.mpd" => (200, "application/dash+xml", TIMELINE_MPD.as_bytes()),
        "/list.mpd" => (200, "application/dash+xml", LIST_MPD.as_bytes()),
        "/base.mpd" => (200, "application/dash+xml", BASE_MPD.as_bytes()),
        _ => (404, "text/plain", b"not found" as &[u8]),
    };

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
}

fn new_engine() -> DashEngine {
    DashEngine::new(ResolvedProxyConfig::shared_default())
}

fn new_probe_request(uri: String) -> ProbeRequest {
    ProbeRequest {
        uri,
        source: None,
        request_headers: Vec::new(),
        pool: None,
        task_id: None,
        credentials: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_succeeds_on_vod_mpd_with_template() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!("{}/manifest.mpd", server.base_url)))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(output.capabilities.supports_resume);
    assert_eq!(
        output.content_type.as_deref(),
        Some("application/dash+xml")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_dynamic_mpd() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let error = engine
        .probe(new_probe_request(format!("{}/live.mpd", server.base_url)))
        .await
        .expect_err("dynamic MPD should be rejected");

    let message = error.to_string();
    assert!(
        message.contains("dash_live_unsupported"),
        "expected dash_live_unsupported in error, got: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_segment_timeline() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let error = engine
        .probe(new_probe_request(format!(
            "{}/timeline.mpd",
            server.base_url
        )))
        .await
        .expect_err("SegmentTimeline MPD should be rejected");

    let message = error.to_string();
    assert!(
        message.contains("dash_segment_timeline_unsupported"),
        "expected dash_segment_timeline_unsupported in error, got: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_handles_segment_list_manifest() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!("{}/list.mpd", server.base_url)))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(!output.display_name.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_handles_segment_base_manifest() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let server = start_test_server();
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(format!("{}/base.mpd", server.base_url)))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(output.capabilities.supports_resume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_file_scheme_reads_local_mpd() {
    if !ffmpeg_available() {
        eprintln!("skipping DASH probe test: ffmpeg not in PATH");
        return;
    }
    let engine = new_engine();

    let dir = std::env::temp_dir().join(format!(
        "vibe-dash-file-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let mpd_path: PathBuf = dir.join("local.mpd");
    std::fs::write(&mpd_path, TEMPLATE_MPD).expect("write mpd");

    let uri = format!("file:///{}", mpd_path.to_string_lossy().replace('\\', "/"));
    let uri = uri.replace("//", "/");

    let output = engine
        .probe(new_probe_request(uri))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "dash");
    assert!(output.capabilities.supports_resume);
}
