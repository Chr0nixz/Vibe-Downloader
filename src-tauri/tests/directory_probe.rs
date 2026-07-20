//! FUN-04: directory probe credentials and SOCKS5 proxy coverage.

mod common;

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use base64::Engine as _;
use tauri_app_lib::{
    db,
    download::{probe_ftp_directory_url, probe_sftp_directory_url, probe_webdav_directory_url},
    models::TaskProxyMode,
    proxy::{AppProxyMode, ResolvedProxyConfig},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener as TokioTcpListener;

fn b64(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

// --- WebDAV password directory ---------------------------------------------

#[derive(Clone)]
struct WebDavDirState {
    require_basic_auth: bool,
    expected_credentials: Option<String>,
    observed_authorization: Arc<Mutex<Option<String>>>,
}

fn start_webdav_dir_server(state: WebDavDirState) -> common::TestServer {
    common::TestServer::start(move |mut stream| {
        let mut buffer = [0u8; 8192];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let request = String::from_utf8_lossy(&buffer[..read]);
        let request_line = request.lines().next().unwrap_or_default();
        let authorization = request.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("authorization") {
                Some(value.trim().to_string())
            } else {
                None
            }
        });
        if let Some(auth) = &authorization {
            *state.observed_authorization.lock().expect("lock") = Some(auth.clone());
        }
        if state.require_basic_auth {
            let expected = state.expected_credentials.as_deref().expect("expected");
            let provided = authorization
                .as_deref()
                .and_then(|value| value.strip_prefix("Basic ").map(str::trim));
            if provided != Some(expected) {
                let _ = write!(
                    stream,
                    "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"vibe\"\r\nContent-Length: 0\r\n\r\n"
                );
                return;
            }
        }
        if request_line.starts_with("PROPFIND ") {
            let body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dir/</d:href>
    <d:propstat><d:prop><d:displayname>dir</d:displayname><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dir/readme.txt</d:href>
    <d:propstat><d:prop><d:displayname>readme.txt</d:displayname><d:resourcetype/><d:getcontentlength>12</d:getcontentlength><d:getcontenttype>text/plain</d:getcontenttype></d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
            let _ = write!(
                stream,
                "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            return;
        }
        let _ = write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"
        );
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun04_webdav_directory_probe_uses_draft_password() {
    let state = WebDavDirState {
        require_basic_auth: true,
        expected_credentials: Some(b64("alice:s3cret")),
        observed_authorization: Arc::new(Mutex::new(None)),
    };
    let observed = state.observed_authorization.clone();
    let server = start_webdav_dir_server(state);
    let url = format!("webdav://{}/dir/", server.authority());

    let without = probe_webdav_directory_url(&url, ResolvedProxyConfig::default(), None).await;
    assert!(without.is_err(), "missing credentials must fail auth");

    let credentials = db::TaskCredentials {
        username: "alice".into(),
        password: "s3cret".into(),
        private_key_data: None,
        private_key_passphrase: None,
    };
    let probe =
        probe_webdav_directory_url(&url, ResolvedProxyConfig::default(), Some(&credentials))
            .await
            .expect("directory probe with draft credentials");

    assert!(
        !probe.directory_url.contains("alice") && !probe.directory_url.contains("s3cret"),
        "directory_url must not leak credentials: {}",
        probe.directory_url
    );
    assert!(
        probe
            .entries
            .iter()
            .any(|entry| entry.name.contains("readme.txt")),
        "expected file entry, got {:?}",
        probe.entries
    );
    let auth = observed.lock().expect("lock").clone().expect("auth seen");
    assert!(auth.starts_with("Basic "));
}

// --- FTP password directory ------------------------------------------------

#[derive(Clone)]
struct FtpDirConfig {
    required_user: Option<String>,
    required_pass: Option<String>,
    files: Vec<String>,
}

struct FtpDirServer {
    addr: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
}

impl FtpDirServer {
    fn start(config: FtpDirConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let cfg = config.clone();
                        thread::spawn(move || handle_ftp_dir_session(stream, cfg));
                    }
                    Err(_) => break,
                }
            }
        });
        Self { addr, stop }
    }

    fn url(&self) -> String {
        format!("ftp://{}/pub/", self.addr)
    }
}

impl Drop for FtpDirServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
    }
}

fn handle_ftp_dir_session(mut stream: TcpStream, config: FtpDirConfig) {
    let _ = writeln!(stream, "220 vibe-test FTP ready");
    let mut buf = [0u8; 512];
    let mut user = String::new();
    let mut data_listener: Option<TcpListener> = None;
    loop {
        let Ok(read) = stream.read(&mut buf) else {
            return;
        };
        if read == 0 {
            return;
        }
        let line = String::from_utf8_lossy(&buf[..read]);
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let upper = trimmed.to_ascii_uppercase();
        if upper.starts_with("USER ") {
            user = trimmed[5..].trim().to_string();
            let _ = writeln!(stream, "331 password required");
        } else if upper.starts_with("PASS ") {
            let pass = trimmed[5..].trim().to_string();
            let ok = match (&config.required_user, &config.required_pass) {
                (Some(expected_user), Some(expected_pass)) => {
                    &user == expected_user && pass == *expected_pass
                }
                _ => true,
            };
            if ok {
                let _ = writeln!(stream, "230 Login successful");
            } else {
                let _ = writeln!(stream, "530 Login incorrect");
            }
        } else if upper.starts_with("TYPE ") {
            let _ = writeln!(stream, "200 Type set");
        } else if upper.starts_with("CWD ") || upper == "CWD" {
            let _ = writeln!(stream, "250 Directory changed");
        } else if upper.starts_with("PWD") {
            let _ = writeln!(stream, "257 \"/pub\" is the current directory");
        } else if upper.starts_with("PASV") {
            let listener = TcpListener::bind("127.0.0.1:0").expect("data");
            let data_addr = listener.local_addr().expect("data addr");
            data_listener = Some(listener);
            let octets = data_addr.ip().to_string();
            let parts: Vec<&str> = octets.split('.').collect();
            let p1 = data_addr.port() / 256;
            let p2 = data_addr.port() % 256;
            let _ = writeln!(
                stream,
                "227 Entering Passive Mode ({},{},{},{},{p1},{p2})",
                parts[0], parts[1], parts[2], parts[3]
            );
        } else if upper.starts_with("MLSD") {
            let _ = writeln!(stream, "500 MLSD not implemented");
        } else if upper.starts_with("LIST") {
            let _ = writeln!(stream, "150 Opening data connection");
            if let Some(listener) = data_listener.take() {
                if let Ok((mut data, _)) = listener.accept() {
                    for name in &config.files {
                        let _ = writeln!(data, "-rw-r--r-- 1 vibe vibe 12 Jan 01 00:00 {name}");
                    }
                }
            }
            let _ = writeln!(stream, "226 Transfer complete");
        } else if upper.starts_with("QUIT") {
            let _ = writeln!(stream, "221 Bye");
            return;
        } else {
            let _ = writeln!(stream, "502 Command not implemented");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun04_ftp_directory_probe_uses_draft_password() {
    let server = FtpDirServer::start(FtpDirConfig {
        required_user: Some("ftpuser".into()),
        required_pass: Some("ftppass".into()),
        files: vec!["a.bin".into(), "b.bin".into()],
    });
    let url = server.url();
    let credentials = db::TaskCredentials {
        username: "ftpuser".into(),
        password: "ftppass".into(),
        private_key_data: None,
        private_key_passphrase: None,
    };
    let probe = probe_ftp_directory_url(&url, ResolvedProxyConfig::default(), Some(&credentials))
        .await
        .expect("ftp directory probe");
    assert!(
        !probe.directory_url.contains("ftpuser") && !probe.directory_url.contains("ftppass"),
        "directory_url must not leak credentials: {}",
        probe.directory_url
    );
    assert!(
        probe
            .entries
            .iter()
            .any(|entry| entry.name.contains("a.bin")),
        "expected listed file, got {:?}",
        probe.entries
    );
}

// --- SFTP private-key directory --------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fun04_sftp_directory_probe_uses_private_key_credentials() {
    common::install_test_secret_key();
    let mut files = HashMap::new();
    files.insert("/pub/note.txt".into(), b"hello".to_vec());
    let server = common::sftp_server::start_sftp_server(common::sftp_server::SftpServerConfig {
        files,
        ..Default::default()
    })
    .await;
    // Pre-seed TOFU so connect does not fail host-key verification.
    let pool = common::test_pool("fun04-sftp-dir").await;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO sftp_known_hosts (host, port, algorithm, fingerprint_sha256, first_seen_at, last_seen_at)
        VALUES (?, ?, 'ssh-ed25519', ?, ?, ?)
        "#,
    )
    .bind(server.addr.ip().to_string().to_ascii_lowercase())
    .bind(i64::from(server.addr.port()))
    .bind(&server.host_key_fingerprint)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    .expect("seed known host");

    let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
        .expect("client key");
    let key_pem = key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("pem")
        .to_string();

    let url = format!("sftp://{}:{}/pub/", server.addr.ip(), server.addr.port());
    let credentials = db::TaskCredentials {
        username: "sftpuser".into(),
        password: String::new(),
        private_key_data: Some(key_pem),
        private_key_passphrase: None,
    };

    let result = probe_sftp_directory_url(
        &pool,
        &url,
        ResolvedProxyConfig::default(),
        Some(&credentials),
    )
    .await
    .expect("sftp directory probe with private key must list entries");

    assert!(
        !result.directory_url.contains("sftpuser"),
        "directory_url must not leak username: {}",
        result.directory_url
    );
    assert!(
        result
            .entries
            .iter()
            .any(|entry| entry.name.contains("note.txt")),
        "expected listed file, got {:?}",
        result.entries
    );
    // Private-key auth leaves password empty; username@host is OK for create
    // handoff, but user:pass@ must never appear in candidates.
    assert!(
        result
            .entries
            .iter()
            .filter_map(|entry| entry.probable_file_url.as_deref())
            .all(|candidate| !candidate.contains(":@") && !candidate.contains("sftppass")),
        "probable URLs must not embed passwords: {:?}",
        result.entries
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fun04_sftp_directory_probe_uses_socks5_proxy() {
    common::install_test_secret_key();
    let mut files = HashMap::new();
    files.insert("/pub/proxied.bin".into(), b"via-proxy".to_vec());
    let server = common::sftp_server::start_sftp_server(common::sftp_server::SftpServerConfig {
        files,
        ..Default::default()
    })
    .await;
    let pool = common::test_pool("fun04-sftp-dir-proxy").await;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO sftp_known_hosts (host, port, algorithm, fingerprint_sha256, first_seen_at, last_seen_at)
        VALUES (?, ?, 'ssh-ed25519', ?, ?, ?)
        "#,
    )
    .bind(server.addr.ip().to_string().to_ascii_lowercase())
    .bind(i64::from(server.addr.port()))
    .bind(&server.host_key_fingerprint)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    .expect("seed known host");

    let origin_port = server.addr.port();
    let proxy_listener = TokioTcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy bind");
    let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
    let _proxy_task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = proxy_listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut greeting_len = [0_u8; 2];
                if stream.read_exact(&mut greeting_len).await.is_err() {
                    return;
                }
                if greeting_len[0] != 0x05 {
                    return;
                }
                let mut methods = vec![0_u8; greeting_len[1] as usize];
                let _ = stream.read_exact(&mut methods).await;
                let _ = stream.write_all(&[0x05, 0x00]).await;

                let mut header = [0_u8; 4];
                if stream.read_exact(&mut header).await.is_err() {
                    return;
                }
                let atyp = header[3];
                match atyp {
                    0x01 => {
                        let mut ip = [0_u8; 4];
                        let _ = stream.read_exact(&mut ip).await;
                    }
                    0x03 => {
                        let mut len = [0_u8; 1];
                        let _ = stream.read_exact(&mut len).await;
                        let mut domain = vec![0_u8; len[0] as usize];
                        let _ = stream.read_exact(&mut domain).await;
                    }
                    0x04 => {
                        let mut ip = [0_u8; 16];
                        let _ = stream.read_exact(&mut ip).await;
                    }
                    _ => return,
                }
                let mut port_bytes = [0_u8; 2];
                if stream.read_exact(&mut port_bytes).await.is_err() {
                    return;
                }
                let port = u16::from_be_bytes(port_bytes);
                assert_eq!(port, origin_port, "SOCKS5 CONNECT must target SFTP origin");
                let Ok(mut upstream) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await
                else {
                    let _ = stream
                        .write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                        .await;
                    return;
                };
                let _ = stream
                    .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
                    .await;
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
            });
        }
    });

    let global = ResolvedProxyConfig::default();
    let resolved = db::resolve_probe_proxy_config(
        &global,
        "sftp",
        Some(TaskProxyMode::Custom),
        Some(&format!("socks5://{proxy_addr}")),
        None,
        None,
        None,
    )
    .expect("resolve socks5 for sftp directory probe");

    let url = format!("sftp://{}:{}/pub/", server.addr.ip(), server.addr.port());
    let credentials = db::TaskCredentials {
        username: "sftpuser".into(),
        password: "sftppass".into(),
        private_key_data: None,
        private_key_passphrase: None,
    };
    let probe = probe_sftp_directory_url(&pool, &url, resolved, Some(&credentials))
        .await
        .expect("sftp directory probe via socks5");
    assert!(
        probe
            .entries
            .iter()
            .any(|entry| entry.name.contains("proxied.bin")),
        "expected listed file via SOCKS5, got {:?}",
        probe.entries
    );
}

// --- SOCKS5 proxy directory (FTP control channel through relay) -------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fun04_ftp_directory_probe_uses_socks5_proxy() {
    let server = FtpDirServer::start(FtpDirConfig {
        required_user: None,
        required_pass: None,
        files: vec!["proxied.bin".into()],
    });
    let origin_addr = server.addr;
    let origin_port = origin_addr.port();

    let proxy_listener = TokioTcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy bind");
    let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
    let proxy_task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = proxy_listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut greeting_len = [0_u8; 2];
                if stream.read_exact(&mut greeting_len).await.is_err() {
                    return;
                }
                if greeting_len[0] != 0x05 {
                    return;
                }
                let mut methods = vec![0_u8; greeting_len[1] as usize];
                let _ = stream.read_exact(&mut methods).await;
                let _ = stream.write_all(&[0x05, 0x00]).await;

                let mut header = [0_u8; 4];
                if stream.read_exact(&mut header).await.is_err() {
                    return;
                }
                let atyp = header[3];
                match atyp {
                    0x01 => {
                        let mut ip = [0_u8; 4];
                        let _ = stream.read_exact(&mut ip).await;
                    }
                    0x03 => {
                        let mut len = [0_u8; 1];
                        let _ = stream.read_exact(&mut len).await;
                        let mut domain = vec![0_u8; len[0] as usize];
                        let _ = stream.read_exact(&mut domain).await;
                    }
                    0x04 => {
                        let mut ip = [0_u8; 16];
                        let _ = stream.read_exact(&mut ip).await;
                    }
                    _ => return,
                }
                let mut port_bytes = [0_u8; 2];
                if stream.read_exact(&mut port_bytes).await.is_err() {
                    return;
                }
                let port = u16::from_be_bytes(port_bytes);
                let Ok(mut upstream) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await
                else {
                    let _ = stream
                        .write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                        .await;
                    return;
                };
                let _ = stream
                    .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
                    .await;
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
            });
        }
    });

    let global = ResolvedProxyConfig::default();
    let resolved = db::resolve_probe_proxy_config(
        &global,
        "ftp",
        Some(TaskProxyMode::Custom),
        Some(&format!("socks5://{proxy_addr}")),
        None,
        None,
        None,
    )
    .expect("resolve socks5 for ftp directory probe");
    assert_eq!(resolved.mode, AppProxyMode::Custom);
    // normalize_proxy_url may append `/`; accept either form.
    assert!(
        resolved
            .url
            .as_deref()
            .is_some_and(|url| url.contains(&proxy_addr.to_string())),
        "resolved proxy url should target the relay: {:?}",
        resolved.url
    );

    let proxy_config = ResolvedProxyConfig {
        mode: AppProxyMode::Custom,
        url: Some(format!("socks5://{proxy_addr}")),
        no_proxy: None,
        username: None,
        password: None,
    };
    let url = format!("ftp://127.0.0.1:{origin_port}/pub/");
    let probe = probe_ftp_directory_url(&url, proxy_config, None)
        .await
        .expect("ftp directory via socks5");
    assert!(
        probe
            .entries
            .iter()
            .any(|entry| entry.name.contains("proxied.bin")),
        "expected proxied listing, got {:?}",
        probe.entries
    );
    proxy_task.abort();
}
