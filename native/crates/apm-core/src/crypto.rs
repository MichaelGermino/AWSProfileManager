//! The 1.x master-password envelope, byte-compatible with `credentialStorage.ts`:
//! `"v1:" + base64(salt[16] | iv[12] | AES-256-GCM ciphertext | tag[16])`, key = PBKDF2-SHA256
//! (100 000 iterations, 32 bytes) of the password and salt.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;

pub const PREFIX: &str = "v1:";
pub const PBKDF2_ITERATIONS: u32 = 100_000;
const SALT_LEN: usize = 16;
const IV_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Plaintext stored under the verifier keychain entry; decrypting it proves a password is right.
pub const VERIFIER_PLAINTEXT: &str = "aws-profile-manager:master-password-verifier:v1";

fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut key);
    key
}

/// Encrypt with an explicit salt and IV (tests and vectors). Use [`encrypt`] otherwise.
pub fn encrypt_with(password: &str, salt: &[u8; SALT_LEN], iv: &[u8; IV_LEN], plaintext: &str) -> String {
    let key = derive_key(password, salt);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key");
    let sealed =
        cipher.encrypt(Nonce::from_slice(iv), Payload { msg: plaintext.as_bytes(), aad: &[] }).expect("AES-GCM encryption does not fail for in-memory buffers");
    let mut raw = Vec::with_capacity(SALT_LEN + IV_LEN + sealed.len());
    raw.extend_from_slice(salt);
    raw.extend_from_slice(iv);
    raw.extend_from_slice(&sealed); // ciphertext followed by the 16-byte tag, as 1.x lays it out
    format!("{PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(raw))
}

/// Encrypt with a fresh random salt and IV.
pub fn encrypt(password: &str, plaintext: &str) -> String {
    let mut salt = [0u8; SALT_LEN];
    let mut iv = [0u8; IV_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut iv);
    encrypt_with(password, &salt, &iv, plaintext)
}

pub fn is_v1(blob: &str) -> bool {
    blob.starts_with(PREFIX)
}

/// The plaintext, or `None` for a wrong password, a damaged blob or one that is not `v1:`.
pub fn decrypt(password: &str, blob: &str) -> Option<String> {
    let body = blob.strip_prefix(PREFIX)?;
    let raw = base64::engine::general_purpose::STANDARD.decode(body).ok()?;
    if raw.len() < SALT_LEN + IV_LEN + TAG_LEN {
        return None;
    }
    let (salt, rest) = raw.split_at(SALT_LEN);
    let (iv, sealed) = rest.split_at(IV_LEN);
    let key = derive_key(password, salt);
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;
    let plain = cipher.decrypt(Nonce::from_slice(iv), Payload { msg: sealed, aad: &[] }).ok()?;
    Some(String::from_utf8_lossy(&plain).into_owned())
}
