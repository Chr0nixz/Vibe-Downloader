pub mod http;
pub mod sftp_server;

use std::{
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// A minimal TCP test server for integration tests.
///
/// Each accepted connection is handed to the provided `handler` closure in a
/// dedicated thread. The handler must be `Clone` (closures capturing `Arc<_>`
/// satisfy this) so it can be cloned per connection.
///
/// The server runs on a random port (`127.0.0.1:0`) and is stopped on drop.
#[allow(dead_code)]
pub struct TestServer {
    pub base_url: String,
    stop: Arc<AtomicBool>,
}

#[allow(dead_code)]
impl TestServer {
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(TcpStream) + Send + Sync + Clone + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready_tx, ready_rx) = mpsc::channel();

        thread::spawn(move || {
            let _ = ready_tx.send(());
            while !thread_stop.load(Ordering::SeqCst) {
                // Blocking accept: the thread always waits inside accept(),
                // eliminating the polling gap that caused flaky connection
                // resets under high parallelism. Drop wakes the listener via
                // a dummy connect so the loop can check thread_stop and exit.
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = handler.clone();
                        thread::spawn(move || handler(stream));
                    }
                    Err(_) => break,
                }
            }
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("test server ready");

        Self {
            base_url: format!("http://{addr}"),
            stop,
        }
    }

    /// Returns the `host:port` portion of the server's URL, suitable for
    /// constructing non-HTTP URLs (e.g. `ftp://host:port/...`,
    /// `webdav://host:port/...`).
    #[allow(dead_code)]
    pub fn authority(&self) -> &str {
        self.base_url.trim_start_matches("http://")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
    }
}

/// Temporary file paths for download tests.
#[allow(dead_code)]
pub struct TestPaths {
    pub temp: PathBuf,
    pub final_path: PathBuf,
}

#[allow(dead_code)]
impl TestPaths {
    pub fn new(label: &str) -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("vibe-downloader-{label}-{id}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self {
            temp: dir.join("file.bin.vibe-downloading"),
            final_path: dir.join("file.bin"),
        }
    }
}
