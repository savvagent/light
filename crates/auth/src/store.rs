//! The storage seam implemented by the persistence crate.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::StoreError;

/// A user account. The TOTP seed is stored only in its encrypted form.
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    /// Base64(AES-GCM) TOTP seed. Always present: registration is TOTP-first.
    pub totp_secret_enc: String,
    pub totp_enabled: bool,
    pub created_at: DateTime<Utc>,
}

/// Input for creating a user.
#[derive(Debug, Clone)]
pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub totp_secret_enc: String,
}

/// An issued session, keyed by the SHA-256 digest of the bearer token.
#[derive(Debug, Clone)]
pub struct Session {
    pub token_hash: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// A short-lived, single-use challenge bridging a multi-step flow (currently
/// registration -> TOTP confirmation).
#[derive(Debug, Clone)]
pub struct Challenge {
    pub token_hash: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// A pending device-authorization request (RFC 8628). The `device_code` is a
/// high-entropy secret stored only as its hash; the `user_code` is short and
/// human-typed. `user_id` is `Some` once the browser user approves.
#[derive(Debug, Clone)]
pub struct DeviceGrant {
    pub device_code_hash: String,
    pub user_code: String,
    pub user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Storage operations required by the auth service.
#[async_trait]
pub trait Store: Send + Sync {
    /// Create a user. Returns [`StoreError::Duplicate`] if the email exists.
    async fn create_user(&self, user: &NewUser) -> Result<(), StoreError>;

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError>;

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>, StoreError>;

    /// Overwrite the encrypted TOTP seed (used when restarting registration).
    async fn set_totp_secret(&self, user_id: Uuid, secret_enc: &str) -> Result<(), StoreError>;

    /// Flip `totp_enabled = true` (completes registration).
    async fn enable_totp(&self, user_id: Uuid) -> Result<(), StoreError>;

    async fn create_session(&self, session: &Session) -> Result<(), StoreError>;

    async fn get_session(&self, token_hash: &str) -> Result<Option<Session>, StoreError>;

    async fn delete_session(&self, token_hash: &str) -> Result<(), StoreError>;

    async fn create_challenge(&self, challenge: &Challenge) -> Result<(), StoreError>;

    /// Atomically consume a challenge (delete it) and return it, or `None` if
    /// it does not exist or has expired. Single-use by construction.
    async fn consume_challenge(&self, token_hash: &str) -> Result<Option<Challenge>, StoreError>;

    async fn create_device_grant(&self, grant: &DeviceGrant) -> Result<(), StoreError>;

    async fn get_device_grant(
        &self,
        device_code_hash: &str,
    ) -> Result<Option<DeviceGrant>, StoreError>;

    async fn get_device_grant_by_user_code(
        &self,
        user_code: &str,
    ) -> Result<Option<DeviceGrant>, StoreError>;

    /// Set `user_id` on the grant with this `user_code`, returning whether a
    /// matching (still-pending) grant was updated.
    async fn approve_device_grant(
        &self,
        user_code: &str,
        user_id: Uuid,
    ) -> Result<bool, StoreError>;

    /// Atomically consume a device grant (delete it) and return it, or `None`
    /// if it does not exist. Single-use by construction.
    async fn consume_device_grant(
        &self,
        device_code_hash: &str,
    ) -> Result<Option<DeviceGrant>, StoreError>;
}
