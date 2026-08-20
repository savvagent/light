//! Encryption of secrets at rest (TOTP seeds) with AES-256-GCM.
//!
//! The server holds a single master key (32 bytes, from `LIGHT_SECRET_KEY`).
//! Sensitive values are encrypted before they touch the database, so a DB
//! leak alone does not expose usable TOTP seeds.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

use crate::error::AuthError;

/// 12-byte GCM nonce.
const NONCE_LEN: usize = 12;

/// AES-256-GCM cipher wrapping the master key.
pub struct SecretCipher {
    cipher: Aes256Gcm,
}

impl SecretCipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)),
        }
    }

    /// Encrypt `plaintext`, returning base64(nonce || ciphertext).
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String, AuthError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        use rand::RngCore;
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| AuthError::Internal(e.to_string()))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(B64.encode(out))
    }

    /// Decrypt base64(nonce || ciphertext).
    pub fn decrypt(&self, encoded: &str) -> Result<Vec<u8>, AuthError> {
        let raw = B64
            .decode(encoded)
            .map_err(|_| AuthError::Internal("corrupt secret encoding".into()))?;
        if raw.len() <= NONCE_LEN {
            return Err(AuthError::Internal("corrupt secret encoding".into()));
        }
        let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| AuthError::Internal("secret decryption failed".into()))
    }
}
