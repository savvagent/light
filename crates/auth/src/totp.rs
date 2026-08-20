//! TOTP secret generation and verification (RFC 6238, SHA-1, 6 digits, 30s).

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use rand::RngCore;
use totp_rs::{Algorithm, TOTP};

use crate::error::AuthError;

/// The TOTP issuer label surfaced in authenticator apps.
pub const ISSUER: &str = "light-factory";

/// Number of bytes in a TOTP seed (160 bits, RFC 6238 recommended minimum).
const SECRET_LEN: usize = 20;

/// Generate a new random TOTP seed.
///
/// Returns base64 of the raw secret bytes (what we encrypt at rest), the
/// base32 secret for manual entry, and the `otpauth://` provisioning URL for
/// a QR code.
pub fn generate(account_name: &str) -> (Vec<u8>, String, String) {
    let mut raw = vec![0u8; SECRET_LEN];
    rand::rngs::OsRng.fill_bytes(&mut raw);

    let totp = build(&raw, account_name);
    let secret_base32 = totp.get_secret_base32();
    let url = totp.get_url();

    (raw, secret_base32, url)
}

/// Verify a one-time code against a stored seed, with a ±1 step skew.
pub fn verify(raw_secret: &[u8], account_name: &str, code: &str) -> Result<bool, AuthError> {
    let totp = build(raw_secret, account_name);
    totp.check_current(code)
        .map_err(|e| AuthError::Internal(e.to_string()))
}

/// Parse a base64-encoded seed back into raw bytes.
pub fn decode_seed(encoded: &str) -> Result<Vec<u8>, AuthError> {
    B64.decode(encoded)
        .map_err(|_| AuthError::Internal("corrupt TOTP seed".into()))
}

fn build(secret: &[u8], account_name: &str) -> TOTP {
    // SHA-1 / 6 digits / 30s is the maximally compatible choice: every major
    // authenticator app supports it.
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret.to_vec(),
        Some(ISSUER.to_string()),
        account_name.to_string(),
    )
    .expect("TOTP parameters are static and valid")
}
