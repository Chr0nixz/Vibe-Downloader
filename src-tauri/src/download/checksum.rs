use std::path::Path;

use sha2::Digest;
use tokio::io::AsyncReadExt;

use crate::models::ChecksumAlgorithm;

const HASH_READ_BUFFER_SIZE: usize = 1024 * 1024;

/// Compute a lowercase-hexadecimal digest of `path` using the given algorithm.
///
/// Shared by task hash verification (`commands::tasks::actions`) and Metalink
/// checksum verification (`download::metalink`).
pub(crate) async fn hash_file(
    path: &Path,
    algorithm: ChecksumAlgorithm,
) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Could not open file for checksum verification: {e}"))?;
    let mut buffer = vec![0_u8; HASH_READ_BUFFER_SIZE];

    macro_rules! compute_hash {
        ($hasher:expr) => {{
            let mut hasher = $hasher;
            loop {
                let read = file
                    .read(&mut buffer)
                    .await
                    .map_err(|e| format!("Could not read file for checksum verification: {e}"))?;
                if read == 0 {
                    break;
                }
                Digest::update(&mut hasher, &buffer[..read]);
            }
            format!("{:x}", hasher.finalize())
        }};
    }

    match algorithm {
        ChecksumAlgorithm::Sha256 => Ok(compute_hash!(sha2::Sha256::new())),
        ChecksumAlgorithm::Sha512 => Ok(compute_hash!(sha2::Sha512::new())),
        ChecksumAlgorithm::Sha1 => Ok(compute_hash!(sha1::Sha1::new())),
        ChecksumAlgorithm::Md5 => Ok(compute_hash!(md5::Md5::new())),
    }
}
