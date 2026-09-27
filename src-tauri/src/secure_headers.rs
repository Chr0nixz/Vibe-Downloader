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

    // SEC-14: version dispatch cannot rely on the first byte alone. A legacy
    // ciphertext whose plaintext happens to start with 0x01 is
    // indistinguishable from a v1 envelope by that check (~1/256 chance per
    // credential). Try v1 first; if authentication fails AND the byte is 0x01,
    // retry as legacy without stripping the prefix. A genuine v1 ciphertext
    // still authenticates on the first pass; a real legacy ciphertext only
    // passes the second. No AAD downgrade occurs — the second attempt still
    // uses AEAD on the original bytes.
    let raw_plaintext = if raw.first() == Some(&CIPHERTEXT_VERSION) {
        let v1_payload = Payload {
            msg: &raw[1..],
            aad,
        };
        match cipher.decrypt(nonce_ref, v1_payload) {
            Ok(plaintext) => Ok(plaintext),
            Err(_) => {
                // First-byte collision: retry as legacy before giving up.
                cipher.decrypt(nonce_ref, raw.as_ref())
            }
        }
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

    // TEST_SECRET_KEY_OVERRIDE is a OnceLock shared by all tests in the
    // process — whichever test installs first wins, so every test in this
    // module must encrypt fixtures with the SAME key or decryption silently
    // uses a different one.
    const TEST_KEY: [u8; 32] = [7_u8; 32];

    fn test_cipher() -> ChaCha20Poly1305 {
        install_test_secret_key(&STANDARD.encode(TEST_KEY));
        ChaCha20Poly1305::new(Key::from_slice(&TEST_KEY))
    }

    #[test]
    fn malformed_nonce_lengths_return_errors_without_panicking() {
        let _cipher = test_cipher();
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

    /// SEC-14: a legacy ciphertext whose first byte happens to equal 0x01
    /// would be misrouted to the v1 branch and fail. Try successive nonces
    /// until we produce such a ciphertext (~1/256 chance per try).
    #[test]
    fn legacy_ciphertext_with_leading_version_byte_still_decrypts() {
        let cipher = test_cipher();

        let legacy_plaintext = b"legacy-secret";
        let mut nonce = None;
        let mut legacy_ct = Vec::new();
        for counter in 0u64..10_000 {
            let mut nonce_bytes = [0_u8; 12];
            nonce_bytes[..8].copy_from_slice(&counter.to_le_bytes());
            let candidate_nonce = Nonce::from_slice(&nonce_bytes);
            let ct = cipher
                .encrypt(candidate_nonce, legacy_plaintext.as_ref())
                .expect("encrypt legacy");
            if ct.first() == Some(&0x01) {
                nonce = Some(*candidate_nonce);
                legacy_ct = ct;
                break;
            }
        }
        let nonce = nonce.expect("failed to produce a leading-0x01 ciphertext within 10k tries");

        let ciphertext_b64 = STANDARD.encode(&legacy_ct);
        let nonce_b64 = STANDARD.encode(nonce.as_slice());
        let decrypted = decrypt_secret(&ciphertext_b64, &nonce_b64, "legacy credential", b"")
            .expect("legacy ciphertext with 0x01 leading byte must decrypt");
        assert_eq!(decrypted.as_str(), "legacy-secret");
    }

    /// A real v1 ciphertext must still take the AAD-bound branch and fail on
    /// AAD mismatch.
    #[test]
    fn v1_ciphertext_with_wrong_aad_still_fails() {
        let cipher = test_cipher();
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

        let mut versioned = vec![CIPHERTEXT_VERSION];
        let ct = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: b"payload",
                    aad: b"correct",
                },
            )
            .expect("encrypt v1");
        versioned.extend_from_slice(&ct);
        let ciphertext_b64 = STANDARD.encode(&versioned);
        let nonce_b64 = STANDARD.encode(nonce.as_slice());

        let err = decrypt_secret(&ciphertext_b64, &nonce_b64, "test", b"wrong-aad")
            .expect_err("v1 with wrong AAD must fail");
        assert!(err.contains("Could not decrypt"));
    }
}
