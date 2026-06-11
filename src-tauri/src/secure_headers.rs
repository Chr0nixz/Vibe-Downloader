use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};

const SERVICE: &str = "Vibe Downloader";
const ACCOUNT: &str = "task-request-headers";

pub fn encrypt_headers(headers_json: &str) -> Result<(String, String), String> {
    let key = encryption_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, headers_json.as_bytes())
        .map_err(|_| "Could not encrypt browser request headers.".to_string())?;
    Ok((STANDARD.encode(ciphertext), STANDARD.encode(nonce)))
}

pub fn decrypt_headers(ciphertext: &str, nonce: &str) -> Result<String, String> {
    let key = encryption_key()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let ciphertext = STANDARD
        .decode(ciphertext)
        .map_err(|_| "Stored browser request headers are invalid.".to_string())?;
    let nonce = STANDARD
        .decode(nonce)
        .map_err(|_| "Stored browser request header nonce is invalid.".to_string())?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "Could not decrypt browser request headers.".to_string())?;
    String::from_utf8(plaintext)
        .map_err(|_| "Stored browser request headers are not valid UTF-8.".to_string())
}

fn encryption_key() -> Result<[u8; 32], String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("OS key store is unavailable: {e}"))?;
    match entry.get_password() {
        Ok(value) => decode_key(&value),
        Err(_) => {
            let key = ChaCha20Poly1305::generate_key(&mut OsRng);
            let encoded = STANDARD.encode(key);
            entry
                .set_password(&encoded)
                .map_err(|e| format!("Could not save header encryption key: {e}"))?;
            decode_key(&encoded)
        }
    }
}

fn decode_key(value: &str) -> Result<[u8; 32], String> {
    let raw = STANDARD
        .decode(value)
        .map_err(|_| "Header encryption key is invalid.".to_string())?;
    raw.try_into()
        .map_err(|_| "Header encryption key has invalid length.".to_string())
}
