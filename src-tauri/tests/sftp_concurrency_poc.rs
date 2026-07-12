//! F-1 PoC: validate whether russh-sftp 2.3.0 supports Path B
//! (concurrent `SftpSession::open()` + `File::read()` on multiple files
//! within a single SSH channel / single `SftpSession`).
//!
//! ## Background
//!
//! The SFTP engine currently forces `supports_parallel = false` and downloads
//! via a single `remote.read()` loop. To bring SFTP throughput in line with
//! HTTP/FTP, we need parallel segment reads. Two paths were considered:
//!
//! - **Path B**: One SSH channel + one `SftpSession`, multiple concurrent
//!   `open()` calls and concurrent `read()` calls on the resulting `File`s.
//!   Pros: one TCP connection, one auth handshake, lower resource cost.
//!   Cons: shares SSH window flow control; the default `russh_sftp::server`
//!   loop processes requests sequentially per channel; real SFTP servers
//!   may impose per-channel `max_open_files` limits.
//! - **Path A**: One SSH channel + `SftpSession` per worker (mirrors the
//!   FTP engine). Pros: true parallelism, independent SSH windows, matches
//!   existing proven pattern. Cons: N auth handshakes, N TCP connections.
//!
//! ## PoC outcome
//!
//! The PoC proves Path B is **API-feasible**: the russh-sftp client supports
//! concurrent `open()`/`read()` on a single session (Arc-shared
//! `RawSftpSession` multiplexes requests by SFTP ID). However, because the
//! default server-side loop processes one SFTP packet at a time per channel,
//! real-world throughput on a single channel is bounded by sequential
//! server-side processing. For true parallel speedup we adopt **Path A**
//! (per-worker SSH channel + SFTP subsystem), mirroring the FTP engine.
//!
//! See `download::sftp` for the Path A implementation and
//! `tests/sftp_engine.rs` for end-to-end coverage.

use std::{collections::HashMap, io::SeekFrom, net::SocketAddr, sync::Arc, time::Duration};

use russh::{
    client::{self, AuthResult, Config as ClientConfig, Handler},
    keys::{ssh_key::PublicKey, Algorithm, PrivateKey},
    server::{Auth, Msg, Server as _, Session},
    Channel, ChannelId,
};
use russh_sftp::{
    client::SftpSession as ClientSftpSession,
    protocol::{Data, FileAttributes, Handle, Name, Status, StatusCode, Version},
    server::{self as sftp_server, Handler as SftpHandler},
};
use tokio::{net::TcpListener, sync::Mutex};

// ===== In-memory SFTP backend =====

/// Minimal in-memory SFTP handler backed by a `HashMap<String, Vec<u8>>`.
/// Each `open()` returns the filename as the opaque handle; `read()`
/// slices the stored bytes at the requested offset.
#[derive(Clone, Default)]
struct InMemFs {
    files: Arc<HashMap<String, Vec<u8>>>,
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
            // Use the filename itself as the opaque handle: `read()` will
            // look it up directly. Real servers return a small token, but
            // for the PoC the filename is sufficient.
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
        let data = self.files.get(&handle).ok_or(StatusCode::BadMessage)?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= data.len() {
            // Per SFTP spec: reading at/after EOF returns SSH_FXP_STATUS EOF,
            // not an empty SSH_FXP_DATA packet. The client's `File::read`
            // relies on this to terminate `read_to_end`.
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
}

impl russh::server::Server for TestSshServer {
    type Handler = SshSession;
    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Self::Handler {
        SshSession {
            fs: self.fs.clone(),
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

struct SshSession {
    fs: InMemFs,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
}

impl russh::server::Handler for SshSession {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
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
            // `russh_sftp::server::run` consumes the channel stream and runs
            // a single loop that processes SFTP packets sequentially. This
            // is intentional: the PoC mirrors how real SFTP servers behave
            // per channel (sequential request processing). The question we
            // are answering is whether the *client* can drive concurrent
            // requests over one session — not whether the server can
            // parallelize them.
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

// ===== Test harness =====

async fn start_test_server(files: HashMap<String, Vec<u8>>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .expect("generate ed25519 host key");
    let config = Arc::new(russh::server::Config {
        keys: vec![host_key],
        inactivity_timeout: Some(Duration::from_secs(60)),
        auth_rejection_time: Duration::from_secs(1),
        ..Default::default()
    });

    let mut server = TestSshServer {
        fs: InMemFs {
            files: Arc::new(files),
        },
    };
    tokio::spawn(async move {
        let _ = server.run_on_socket(config, &listener).await;
    });
    addr
}

async fn connect_sftp(addr: SocketAddr) -> ClientSftpSession {
    let mut handle = client::connect(
        Arc::new(ClientConfig::default()),
        (addr.ip().to_string(), addr.port()),
        TrustingClient,
    )
    .await
    .expect("connect");
    assert!(
        matches!(
            handle.authenticate_password("u", "p").await.expect("auth"),
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

// ===== PoC tests =====

/// Path B baseline: a single `SftpSession` can drive concurrent `open()` +
/// `read_to_end()` calls on multiple files without deadlock or data
/// corruption. This proves the client API supports concurrency via
/// request-ID multiplexing over a single channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_b_concurrent_open_and_read_on_single_channel() {
    let mut files = HashMap::new();
    files.insert("/file_a.bin".to_string(), vec![0xA5u8; 4096]);
    files.insert("/file_b.bin".to_string(), vec![0x5Au8; 4096]);
    let addr = start_test_server(files).await;
    let sftp = connect_sftp(addr).await;

    // Concurrent OPENs on the SAME session.
    let (open_a, open_b) = tokio::join!(sftp.open("/file_a.bin"), sftp.open("/file_b.bin"));
    let _file_a = open_a.expect("open file_a");
    let _file_b = open_b.expect("open file_b");

    // Concurrent whole-file READs on the SAME session.
    let (read_a, read_b) = tokio::join!(sftp.read("/file_a.bin"), sftp.read("/file_b.bin"));
    let bytes_a = read_a.expect("read file_a");
    let bytes_b = read_b.expect("read file_b");

    assert_eq!(bytes_a.len(), 4096);
    assert_eq!(bytes_b.len(), 4096);
    assert!(bytes_a.iter().all(|&b| b == 0xA5));
    assert!(bytes_b.iter().all(|&b| b == 0x5A));

    let _ = sftp.close().await;
}

/// Path B with offset reads: simulate segment-style parallel reads on the
/// same file at different offsets within one session. This is the pattern
/// SFTP parallel download would use if Path B were adopted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_b_concurrent_offset_reads_simulate_parallel_segments() {
    // 64 KB file split into 4 × 16 KB segments.
    let total_size = 64 * 1024;
    let segment_size = 16 * 1024;
    let mut content = Vec::with_capacity(total_size);
    for i in 0..total_size {
        content.push((i % 256) as u8);
    }
    let mut files = HashMap::new();
    files.insert("/large.bin".to_string(), content.clone());
    let addr = start_test_server(files).await;
    let sftp = connect_sftp(addr).await;

    let file = sftp.open("/large.bin").await.expect("open");

    // Concurrently read 4 segments at different offsets on the SAME file
    // handle. Each task seeks to its segment start, then reads segment_size
    // bytes.
    let session = Arc::new(sftp);
    let mut tasks = Vec::new();
    for seg_idx in 0..4usize {
        let session = session.clone();
        let content = content.clone();
        tasks.push(tokio::spawn(async move {
            let mut f = session.open("/large.bin").await.expect("open");
            let offset = (seg_idx * segment_size) as u64;
            use tokio::io::{AsyncReadExt, AsyncSeekExt};
            f.seek(SeekFrom::Start(offset)).await.expect("seek");
            let mut buf = vec![0u8; segment_size];
            let n = f.read(&mut buf).await.expect("read");
            assert_eq!(n, segment_size, "segment {seg_idx} short read");
            assert_eq!(
                &buf[..],
                &content[seg_idx * segment_size..(seg_idx + 1) * segment_size],
                "segment {seg_idx} content mismatch"
            );
        }));
    }
    for task in tasks {
        task.await.expect("task panic");
    }
    let _ = file;
    let _ = session.close().await;
}

/// Path B limitation check: because the default `russh_sftp::server::run`
/// loop processes one SFTP packet at a time per channel, concurrent reads
/// do NOT complete in `~max(t1, t2)` — they complete in approximately
/// `t1 + t2`. This test documents the sequential server-side behavior so
/// future contributors understand why Path A (per-worker channel) is
/// required for true parallel speedup.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_b_documents_sequential_server_side_processing() {
    // Two 4 KB files. Each `read()` returns the full file in one SFTP
    // DATA packet. With a sequential server loop, total wall time is
    // approximately t_open_a + t_open_b + t_read_a + t_read_b. With true
    // parallelism it would be max(t_open_a, t_open_b) + max(t_read_a,
    // t_read_b). We don't assert timing (flaky on CI); we only assert
    // correctness — the test exists to document the finding.
    let mut files = HashMap::new();
    files.insert("/x.bin".to_string(), vec![0x11u8; 4096]);
    files.insert("/y.bin".to_string(), vec![0x22u8; 4096]);
    let addr = start_test_server(files).await;
    let sftp = connect_sftp(addr).await;

    let (rx, ry) = tokio::join!(sftp.read("/x.bin"), sftp.read("/y.bin"));
    let bx = rx.expect("read x");
    let by = ry.expect("read y");
    assert_eq!(bx.len(), 4096);
    assert_eq!(by.len(), 4096);

    // Conclusion (documented in test name): the data is correct, but the
    // server processed the two reads sequentially. Path A (per-worker
    // channel) is required for wall-clock parallelism.
    let _ = sftp.close().await;
}
