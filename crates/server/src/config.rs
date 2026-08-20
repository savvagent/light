//! Server configuration read from the environment.

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Duration;
use light_factory_auth::{Config, secret::SecretCipher};

/// Build the [`SecretCipher`] from `LIGHT_SECRET_KEY`, a base64-encoded 32-byte
/// master key. Fail closed: refuse to start without a valid key, since a
/// rotating key would render stored TOTP seeds undecryptable.
pub fn secret_cipher_from_env() -> anyhow::Result<SecretCipher> {
    let encoded = std::env::var("LIGHT_SECRET_KEY").map_err(|_| {
        anyhow::anyhow!(
            "LIGHT_SECRET_KEY is required: a base64-encoded 32-byte key \
             (generate with `openssl rand -base64 32`)"
        )
    })?;
    let bytes = B64
        .decode(encoded.trim())
        .map_err(|_| anyhow::anyhow!("LIGHT_SECRET_KEY is not valid base64"))?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("LIGHT_SECRET_KEY must decode to exactly 32 bytes"))?;
    Ok(SecretCipher::new(&key))
}

/// Build the auth [`Config`] from the environment.
///
/// Defaults: 30-day sessions, 5-minute registration challenges, and 10-minute
/// device authorizations.
pub fn config_from_env() -> Config {
    Config {
        session_ttl: Duration::days(30),
        challenge_ttl: Duration::minutes(5),
        device_ttl: Duration::minutes(10),
    }
}

/// The origin of the web SPA, used to build device-authorization verification
/// URLs (`DEVICE_VERIFICATION_URI`; default the Vite dev server).
pub fn device_verification_uri_from_env() -> String {
    std::env::var("DEVICE_VERIFICATION_URI")
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|_| "http://localhost:5173".to_string())
}

/// The TCP address to bind, from `ADDR` (default `127.0.0.1:8080`).
pub fn addr_from_env() -> String {
    std::env::var("ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string())
}

/// `DATABASE_URL` (default the local dev cluster).
pub fn database_url_from_env() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://light@127.0.0.1:5432/light".to_string())
}

/// Comma-separated `CORS_ORIGINS` (default the Vite dev server). In production
/// set this to the Cloudflare Pages origin serving the SPA.
pub fn cors_origins_from_env() -> Vec<String> {
    std::env::var("CORS_ORIGINS")
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|_| vec!["http://localhost:5173".to_string()])
}
