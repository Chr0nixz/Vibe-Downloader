use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};

const SERVICE: &str = "Vibe Downloader";
const ACCOUNT: &str = "task-secrets";

pub fn encrypt_headers(headers_json: &str) -> Result<(String, String), String> {
    encrypt_secret(headers_json, "browser request headers")
}

pub fn decrypt_headers(ciphertext: &str, nonce: &str) -> Result<String, String> {
    decrypt_secret(ciphertext, nonce, "browser request headers")
}

pub fn encrypt_secret(value: &str, label: &str) -> Result<(String, String), String> {
    let key = encryption_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, value.as_bytes())
        .map_err(|_| format!("Could not encrypt {label}."))?;
    Ok((STANDARD.encode(ciphertext), STANDARD.encode(nonce)))
}

pub fn decrypt_secret(ciphertext: &str, nonce: &str, label: &str) -> Result<String, String> {
    let key = encryption_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let ciphertext = STANDARD
        .decode(ciphertext)
        .map_err(|_| format!("Stored {label} are invalid."))?;
    let nonce = STANDARD
        .decode(nonce)
        .map_err(|_| format!("Stored {label} nonce is invalid."))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| format!("Could not decrypt {label}."))?;
    String::from_utf8(plaintext).map_err(|_| format!("Stored {label} are not valid UTF-8."))
}

pub fn ensure_secret_encryption_available() -> Result<(), String> {
    encryption_key().map(|_| ())
}

fn encryption_key() -> Result<[u8; 32], String> {
    #[cfg(any(test, debug_assertions))]
    if let Ok(value) = std::env::var("VIBE_DOWNLOADER_TEST_SECRET_KEY") {
        return decode_key(&value);
    }

    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("OS key store is unavailable: {e}"))?;
    match entry.get_password() {
        Ok(value) => decode_key(&value),
        Err(_) => {
            let key = ChaCha20Poly1305::generate_key(&mut OsRng);
            let encoded = STANDARD.encode(key);
            entry
                .set_password(&encoded)
                .map_err(|e| format!("Could not save secret encryption key: {e}"))?;
            decode_key(&encoded)
        }
    }
}

fn decode_key(value: &str) -> Result<[u8; 32], String> {
    let raw = STANDARD
        .decode(value)
        .map_err(|_| "Secret encryption key is invalid.".to_string())?;
    raw.try_into()
        .map_err(|_| "Secret encryption key has invalid length.".to_string())
}
