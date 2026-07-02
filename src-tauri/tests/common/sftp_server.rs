//! Shared SFTP test server infrastructure for integration tests.
//!
//! Extracted from `tests/sftp_concurrency_poc.rs` and enhanced with
//! configurable authentication and read-error injection so that the
//! `sftp_engine.rs` integration tests (and future Phase 8 cross-engine
//! coverage) can exercise failure paths without standing up a real SSH
//! daemon. The server runs on `127.0.0.1:0`, accepts any password by
//! default, and serves files from an in-memory `HashMap`.
//!
//! Tests that need host-key mismatch behavior should call
//! [`TestSftpServer::host_key_fingerprint`] after `start` to read the
//! generated ed25519 fingerprint, then pre-seed `sftp_known_hosts` with a
//! different value before invoking the engine.

// The entire module is shared test infrastructure included via
// `tests/common/mod.rs`. Individual test binaries opt into only the
// helpers they need (e.g. `hls_engine.rs` uses `TestServer` but not the
// SFTP server). Items unused by a given binary are not actually dead —
// they are exercised by other binaries — so suppress the per-binary
// dead-code lint, mirroring the `#[allow(dead_code)]` already applied to
// `TestServer` / `TestPaths` in `common/mod.rs`.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use russh::{
    client::{self, AuthResult, Config as ClientConfig, Handler},
    keys::{
        ssh_key::{HashAlg, PublicKey},
        Algorithm, PrivateKey,
    },
    server::{Auth, Msg, Server as _, Session},
    Channel, ChannelId,
};
use russh_sftp::{
    client::SftpSession as ClientSftpSession,
    protocol::{
        Attrs, Data, FileAttributes, FileMode, Handle, Name, Status, StatusCode, Version,
    },
    server::{self as sftp_server, Handler as SftpHandler},
};
use tokio::{net::TcpListener, sync::Mutex};

/// Configuration for the test SFTP server.
#[derive(Clone, Default)]
pub struct SftpServerConfig {
    /// Files to serve, keyed by absolute path (e.g. `/file.bin`).
    pub files: HashMap<String, Vec<u8>>,
    /// When `true`, the server rejects all password authentication
    /// attempts. Used to exercise `sftp_auth_failed` paths.
    pub reject_auth: bool,
    /// When `Some(i)`, the `i`-th read call returns
    /// `SSH_FXP_STATUS permission_denied` instead of data. Used to
    /// exercise concurrent read failure handling. Counting is per
    /// session and resets on each new connection.
    pub fail_on_read: Option<usize>,
    /// When `true`, the server's `read` handler never responds — the
    /// read future hangs indefinitely. Used to exercise the E-1 idle-read
    /// timeout path: `download_sftp_segment_inner` wraps `remote.read()`
    /// with `read_with_idle_timeout`, so a stalled server yields
    /// `sftp_read_timeout` after `READ_IDLE_TIMEOUT` (60s). Tests verify
    /// the stall at a short timeout rather than waiting the full 60s.
    pub stall_on_read: bool,
}

/// A running SFTP test server. Drop is a no-op; the server task ends when
/// the test process exits or the listener errors.
pub struct TestSftpServer {
    pub addr: SocketAddr,
    pub host_key_fingerprint: String,
}

/// Start a test SFTP server with the given configuration. The server
/// binds `127.0.0.1:0` and returns the assigned address plus the SHA-256
/// fingerprint of the generated ed25519 host key (without the `SHA256:`
/// prefix, matching the format stored in `sftp_known_hosts`).
pub async fn start_sftp_server(config: SftpServerConfig) -> TestSftpServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .expect("generate ed25519 host key");
    let fingerprint = host_key
        .fingerprint(HashAlg::Sha256)
        .to_string()
        .trim_start_matches("SHA256:")
        .to_string();

    let server_config = Arc::new(russh::server::Config {
        keys: vec![host_key],
        inactivity_timeout: Some(Duration::from_secs(60)),
        auth_rejection_time: Duration::from_secs(1),
        ..Default::default()
    });

    let mut server = TestSshServer {
        fs: InMemFs {
            files: Arc::new(config.files),
            read_counter: Arc::new(Mutex::new(0)),
            fail_on_read: config.fail_on_read,
            stall_on_read: config.stall_on_read,
        },
        reject_auth: config.reject_auth,
    };
    tokio::spawn(async move {
        let _ = server.run_on_socket(server_config, &listener).await;
    });

    TestSftpServer {
        addr,
        host_key_fingerprint: fingerprint,
    }
}

/// Convenience wrapper: start a server that accepts any password and
/// serves the given files.
pub async fn start_sftp_server_with_files(
    files: HashMap<String, Vec<u8>>,
) -> TestSftpServer {
    start_sftp_server(SftpServerConfig {
        files,
        reject_auth: false,
        fail_on_read: None,
        stall_on_read: false,
    })
    .await
}

/// Connect a trusting SFTP client (accepts any host key) and authenticate
/// with the given username/password. Returns an open `SftpSession`.
pub async fn connect_sftp(addr: SocketAddr, user: &str, password: &str) -> ClientSftpSession {
    let mut handle = client::connect(
        Arc::new(ClientConfig::default()),
        (addr.ip().to_string(), addr.port()),
        TrustingClient,
    )
    .await
    .expect("connect");
    assert!(
        matches!(
            handle.authenticate_password(user, password).await.expect("auth"),
            AuthResult::Success
        ),
        "auth should succeed"
    );
    let channel = handle
        .channel_open_session()
        .await
        .expect("channel_open_session");
    channel
        .request_subsystem(true, "sftp")
        .await
        .expect("request_subsystem");
    ClientSftpSession::new(channel.into_stream())
        .await
        .expect("SftpSession::new")
}

// ===== In-memory SFTP backend =====

#[derive(Clone)]
struct InMemFs {
    files: Arc<HashMap<String, Vec<u8>>>,
    /// Per-session read counter; used by `fail_on_read` to inject a
    /// failure on the N-th read call.
    read_counter: Arc<Mutex<usize>>,
    fail_on_read: Option<usize>,
    /// When `true`, `read` never responds — simulates a stalled server.
    stall_on_read: bool,
}

impl SftpHandler for InMemFs {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        _version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        _pflags: russh_sftp::protocol::OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        if self.files.contains_key(&filename) {
            Ok(Handle {
                id,
                handle: filename,
            })
        } else {
            Err(StatusCode::NoSuchFile)
        }
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        if self.stall_on_read {
            // Simulate a stalled server: never respond. The E-1 idle-read
            // timeout in `download_sftp_segment_inner` wraps this read with
            // `read_with_idle_timeout`, so production code would emit
            // `sftp_read_timeout` after 60s. Tests verify the stall at a
            // shorter timeout via `tokio::time::timeout`.
            std::future::pending::<()>().await;
            return Err(StatusCode::OpUnsupported);
        }
        if let Some(fail_at) = self.fail_on_read {
            let mut counter = self.read_counter.lock().await;
            *counter += 1;
            if *counter == fail_at + 1 {
                return Err(StatusCode::PermissionDenied);
            }
        }
        let data = self.files.get(&handle).ok_or(StatusCode::BadMessage)?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= data.len() {
            return Err(StatusCode::Eof);
        }
        let end = (start + len as usize).min(data.len());
        Ok(Data {
            id,
            data: data[start..end].to_vec(),
        })
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn stat(
        &mut self,
        id: u32,
        path: String,
    ) -> Result<Attrs, Self::Error> {
        let data = self.files.get(&path).ok_or(StatusCode::NoSuchFile)?;
        // Construct permissions directly rather than via `FileAttributes::default()`,
        // whose `Default` impl pre-sets the DIR bit (Some(0o777 | DIR)) — that
        // would make `is_dir()` return true even after `set_regular(true)`.
        let attrs = FileAttributes {
            size: Some(data.len() as u64),
            permissions: Some(FileMode::REG.bits() | 0o644),
            mtime: Some(0),
            ..FileAttributes::empty()
        };
        Ok(Attrs { id, attrs })
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let normalized = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };
        Ok(Name {
            id,
            files: vec![russh_sftp::protocol::File::new(
                &normalized,
                FileAttributes::default(),
            )],
        })
    }
}

// ===== SSH server plumbing =====

struct TestSshServer {
    fs: InMemFs,
    reject_auth: bool,
}

impl russh::server::Server for TestSshServer {
    type Handler = SshSession;
    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Self::Handler {
        SshSession {
            fs: self.fs.clone(),
            reject_auth: self.reject_auth,
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

struct SshSession {
    fs: InMemFs,
    reject_auth: bool,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
}

impl russh::server::Handler for SshSession {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        if self.reject_auth {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        } else {
            Ok(Auth::Accept)
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.channels.lock().await.insert(channel.id(), channel);
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" {
            let channel = self.channels.lock().await.remove(&channel_id);
            let Some(channel) = channel else {
                return Ok(());
            };
            session.channel_success(channel_id)?;
            sftp_server::run(channel.into_stream(), self.fs.clone()).await;
        } else {
            session.channel_failure(channel_id)?;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.close(channel)?;
        Ok(())
    }
}

// ===== Client handler: trust any host key (TEST ONLY) =====

struct TrustingClient;

impl Handler for TrustingClient {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}
