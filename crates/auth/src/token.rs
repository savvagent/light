//! Opaque bearer-token minting and hashing.
//!
//! Tokens are returned to the client in full but stored in the database only
//! as a SHA-256 digest, so a database leak does not yield usable sessions.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Mint a new opaque token (32 random bytes, base64url, no padding).
pub fn mint_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    B64.encode(bytes)
}

/// Crockford-style alphabet with the easily-confused glyphs removed.
const USER_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Mint a short, human-typed pairing code like `ABCD-EFGH` for the device
/// authorization grant. Low entropy by design: it is short-lived, single-use,
/// and must be typed by a human.
pub fn mint_user_code() -> String {
    const LEN: usize = 8;
    let mut out = Vec::with_capacity(LEN + 1);
    for i in 0..LEN {
        if i == 4 {
            out.push(b'-');
        }
        let mut b = [0u8; 1];
        rand::rngs::OsRng.fill_bytes(&mut b);
        out.push(USER_CODE_ALPHABET[(b[0] as usize) % USER_CODE_ALPHABET.len()]);
    }
    String::from_utf8(out).expect("alphabet is ASCII")
}

/// Hex-encoded SHA-256 of a token, used as the database key.
pub fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
