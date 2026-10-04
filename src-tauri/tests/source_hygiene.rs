//! SEC-03: no engine or command may build a reqwest client outside the shared
//! network factory.
//!
//! A per-call-site `Client::builder()` silently drops the shared policy stack:
//! proxy resolution (Off must mean `no_proxy()`), the SSRF connection-time
//! resolver filter, the SSRF redirect policy, and pooling/invalidation. The
//! factory (`download/net_factory.rs`) is the only place allowed to construct
//! clients; everything else must obtain one via
//! `NetworkClientFactory::client_for` / `HttpEngine::client_for_config`.

use std::path::{Path, PathBuf};

const FACTORY_FILE: &str = "net_factory.rs";
/// Directories scanned relative to the crate root. Deliberately conservative:
/// `download/` (all engines) and `commands/` (backend-side network callers).
const SCAN_DIRS: &[&str] = &["src/download", "src/commands", "src/bin"];

fn crate_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at src-tauri during integration tests.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn reqwest_clients_are_only_built_in_the_network_factory() {
    let root = crate_root();
    let mut files = Vec::new();
    for dir in SCAN_DIRS {
        collect_rs_files(&root.join(dir), &mut files);
    }
    assert!(
        files.len() > 50,
        "source scan found too few files; the scan roots are broken"
    );

    let mut violations = Vec::new();
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == FACTORY_FILE {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            if line.contains("Client::builder(") {
                violations.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "SEC-03 regression: reqwest clients must be built only in \
         download/net_factory.rs — route new callers through \
         NetworkClientFactory::client_for / HttpEngine::client_for_config:\n{}",
        violations.join("\n")
    );
}

#[test]
fn http_requests_cannot_bypass_origin_safe_redirects() {
    let root = crate_root();
    let mut files = Vec::new();
    for dir in ["src/download", "src/commands"] {
        collect_rs_files(&root.join(dir), &mut files);
    }
    let mut violations = Vec::new();
    for path in files {
        if path.ends_with("http/request.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("source");
        let compact: String = source.split_whitespace().collect();
        if compact.contains(".send()") || compact.contains("client.execute(") {
            violations.push(
                path.strip_prefix(&root)
                    .expect("relative path")
                    .display()
                    .to_string(),
            );
        }
    }
    assert!(
        violations.is_empty(),
        "FUN-39: HTTP sends must use the shared origin-safe redirect path: {violations:?}"
    );
}
