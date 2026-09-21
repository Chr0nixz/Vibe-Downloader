use crate::models::AppErrorPayload;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "Vibe Downloader";
const ACCOUNT: &str = "task-secrets";

/// Version byte prepended to all new ciphertexts.
/// 0x01 = ChaCha20-Poly1305 with AAD binding.
/// Legacy ciphertexts (no version prefix) are still decryptable for backward compat.
const CIPHERTEXT_VERSION: u8 = 0x01;

pub fn encrypt_headers(headers_json: &str) -> Result<(String, String), String> {
    encrypt_secret(headers_json, "browser request headers", &[])
}

pub fn decrypt_headers(ciphertext: &str, nonce: &str) -> Result<Zeroizing<String>, String> {
    decrypt_secret(ciphertext, nonce, "browser request headers", &[])
}

pub fn encrypt_secret(value: &str, label: &str, aad: &[u8]) -> Result<(String, String), String> {
    let key = encryption_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_slice()));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let payload = Payload {
        msg: value.as_bytes(),
        aad,
    };
    // AAD binds the ciphertext to its context (label + caller-provided bytes) so a credential
    // encrypted for one purpose can't be decrypted for another. Version byte (0x01) enables
    // future algorithm changes; legacy ciphertexts without the prefix are decrypted without
    // AAD for backward compat.
    let raw_ct = cipher
        .encrypt(&nonce, payload)
        .map_err(|_| format!("Could not encrypt {label}."))?;
    // Prepend version byte: [version | ciphertext+tag]
    let mut versioned = Vec::with_capacity(1 + raw_ct.len());
    versioned.push(CIPHERTEXT_VERSION);
    versioned.extend_from_slice(&raw_ct);
    Ok((STANDARD.encode(versioned), STANDARD.encode(nonce)))
}

pub fn decrypt_secret(
    ciphertext: &str,
    nonce: &str,
    label: &str,
    aad: &[u8],
) -> Result<Zeroizing<String>, String> {
    validate_secret_structure(ciphertext, nonce)
        .map_err(|field| format!("Stored {label} {field} is invalid."))?;
    let key = encryption_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_slice()));
    let raw = Zeroizing::new(
        STANDARD
            .decode(ciphertext)
            .map_err(|_| format!("Stored {label} are invalid."))?,
    );
    let nonce_bytes = Zeroizing::new(
        STANDARD
            .decode(nonce)
            .map_err(|_| format!("Stored {label} nonce is invalid."))?,
    );
    // A malformed backup can contain valid Base64 with an arbitrary nonce
    // length. `Nonce::from_slice` asserts its fixed 12-byte shape, so validate
    // before constructing it and keep the corruption on the Result path.
    let nonce_array: [u8; 12] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| format!("Stored {label} nonce has invalid length."))?;
    let nonce_ref = Nonce::from_slice(&nonce_array);

    // Dispatch on version byte.
    let raw_plaintext = if raw.first() == Some(&CIPHERTEXT_VERSION) {
        // v1: [0x01 | ciphertext+tag], decrypt with AAD.
        let payload = Payload {
            msg: &raw[1..],
            aad,
        };
        cipher.decrypt(nonce_ref, payload)
    } else {
        // Legacy: no version prefix, no AAD. Decrypt as-is for backward compat.
        cipher.decrypt(nonce_ref, raw.as_ref())
    }
    .map_err(|_| format!("Could not decrypt {label}."))?;

    // SEC-05: the decrypted plaintext (credential JSON, header cookies) must
    // not survive scope exit in process memory. The UTF-8 error path also
    // owns the plaintext bytes, so wipe them before returning the error.
    match String::from_utf8(raw_plaintext) {
        Ok(value) => Ok(Zeroizing::new(value)),
        Err(error) => {
            let mut bytes = error.into_bytes();
            bytes.zeroize();
            Err(format!("Stored {label} are not valid UTF-8."))
        }
    }
}

/// Validate portable storage structure without consulting this machine's key.
/// A backup made elsewhere may be well formed but not locally decryptable.
pub fn validate_secret_structure(ciphertext: &str, nonce: &str) -> Result<(), &'static str> {
    let nonce = Zeroizing::new(STANDARD.decode(nonce).map_err(|_| "nonce")?);
    if nonce.len() != 12 {
        return Err("nonce");
    }
    let raw = Zeroizing::new(STANDARD.decode(ciphertext).map_err(|_| "ciphertext")?);
    let prefix = usize::from(raw.first() == Some(&CIPHERTEXT_VERSION));
    if raw.len() < prefix + 16 {
        return Err("ciphertext");
    }
    Ok(())
}

pub fn ensure_secret_encryption_available() -> Result<(), String> {
    encryption_key().map(|_| ())
}

/// SEC-08: the outcome of reading the OS keyring, classified so that only a
/// genuine "no entry yet" may rotate the key. A transient keyring failure
/// (service unavailable, ACL denial, IPC timeout) reported as an arbitrary
/// Err used to take the same branch and OVERWRITE the existing key — making
/// every historical ciphertext permanently undecryptable.
enum KeyringRead {
    Exists(Zeroizing<String>),
    NoEntry,
    Unavailable(String),
}

fn classify_keyring_read(entry: &keyring::Entry) -> KeyringRead {
    match entry.get_password() {
        Ok(value) => KeyringRead::Exists(Zeroizing::new(value)),
        Err(keyring::Error::NoEntry) => KeyringRead::NoEntry,
        Err(error) => KeyringRead::Unavailable(error.to_string()),
    }
}

/// ENG-03: in-process injection point for the integration suites so they
/// never call `std::env::set_var` (a data race with concurrent `getenv` in
/// other test threads). Compile-gated like the env fallback below.
#[cfg(any(test, debug_assertions))]
static TEST_SECRET_KEY_OVERRIDE: std::sync::OnceLock<Zeroizing<[u8; 32]>> =
    std::sync::OnceLock::new();

/// Install the fixed credential-encryption key used by the integration
/// suites. The env-var fallback (`VIBE_DOWNLOADER_TEST_SECRET_KEY`) remains
/// for the lib's own unit-test helper, which still sets it directly.
#[cfg(any(test, debug_assertions))]
#[doc(hidden)]
pub fn install_test_secret_key(key_b64: &str) {
    let key = decode_key(key_b64).expect("integration tests must pass a valid base64 key");
    let _ = TEST_SECRET_KEY_OVERRIDE.set(key);
}

fn encryption_key() -> Result<Zeroizing<[u8; 32]>, String> {
    // First-use auto-generates a 256-bit key and stores it in the OS keyring. Key loss is
    // unrecoverable — all encrypted credentials become undecryptable (no rotation/escrow).
    // The env-var override is gated on `debug_assertions` (not just `test`) so integration
    // tests in debug builds can use a fixed key; release builds always use the keyring.
    #[cfg(any(test, debug_assertions))]
    {
        if let Some(key) = TEST_SECRET_KEY_OVERRIDE.get() {
            return Ok(key.clone());
        }
        if let Ok(value) = std::env::var("VIBE_DOWNLOADER_TEST_SECRET_KEY") {
            return decode_key(&value);
        }
    }

    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("OS key store is unavailable: {e}"))?;
    match classify_keyring_read(&entry) {
        KeyringRead::Exists(value) => decode_key(&value),
        KeyringRead::NoEntry => {
            let key: [u8; 32] = ChaCha20Poly1305::generate_key(&mut OsRng).into();
            let key = Zeroizing::new(key);
            // The base64 copy handed to the keyring is key material too.
            let encoded = Zeroizing::new(STANDARD.encode(key.as_slice()));
            entry
                .set_password(&encoded)
                .map_err(|e| format!("Could not save secret encryption key: {e}"))?;
            decode_key(&encoded)
        }
        KeyringRead::Unavailable(detail) => Err(AppErrorPayload::new(
            "secrets_unavailable",
            format!(
                "The OS key store reported an error and the secret encryption key was not read. Existing credentials stay intact; retry after restoring keyring access. ({detail})"
            ),
            true,
            vec!["retry"],
        )
        .command_error()),
    }
}

fn decode_key(value: &str) -> Result<Zeroizing<[u8; 32]>, String> {
    // SEC-05: both the decoded bytes and the derived array are wiped on drop;
    // the intermediate heap buffer must not outlive the scope unzeroed.
    let raw = Zeroizing::new(
        STANDARD
            .decode(value)
            .map_err(|_| "Secret encryption key is invalid.".to_string())?,
    );
    let mut key = [0_u8; 32];
    if raw.len() != key.len() {
        return Err("Secret encryption key has invalid length.".to_string());
    }
    key.copy_from_slice(&raw);
    Ok(Zeroizing::new(key))
}

#[cfg(test)]
mod malformed_input_tests {
    use super::*;

    #[test]
    fn malformed_nonce_lengths_return_errors_without_panicking() {
        let key = STANDARD.encode([7_u8; 32]);
        install_test_secret_key(&key);
        let ciphertext = STANDARD.encode(vec![CIPHERTEXT_VERSION]);
        for length in [0, 1, 11, 13] {
            let nonce = STANDARD.encode(vec![0_u8; length]);
            let result = std::panic::catch_unwind(|| {
                decrypt_secret(&ciphertext, &nonce, "test secret", b"task")
            });
            assert!(result.is_ok(), "nonce length {length} panicked");
            assert!(
                result.unwrap().is_err(),
                "nonce length {length} was accepted"
            );
        }
    }
}
