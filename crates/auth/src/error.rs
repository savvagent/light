//! Error types for the auth domain.

/// Errors produced by the auth service. Each maps to a stable machine-readable
/// `code` surfaced in the HTTP error envelope.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid email address")]
    InvalidEmail,
    #[error("an account with that email already exists")]
    EmailTaken,
    #[error("invalid email or code")]
    InvalidCredentials,
    #[error("invalid TOTP code")]
    InvalidTotpCode,
    #[error("invalid or expired challenge")]
    InvalidChallenge,
    #[error("invalid or expired session")]
    InvalidSession,
    #[error("invalid device code")]
    InvalidDeviceGrant,
    #[error("device authorization expired")]
    ExpiredDeviceToken,
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AuthError {
    /// Stable machine-readable code for the HTTP error envelope.
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::InvalidEmail => "invalid_email",
            AuthError::EmailTaken => "email_taken",
            AuthError::InvalidCredentials => "invalid_credentials",
            AuthError::InvalidTotpCode => "invalid_totp_code",
            AuthError::InvalidChallenge => "invalid_challenge",
            AuthError::InvalidSession => "invalid_session",
            AuthError::InvalidDeviceGrant => "invalid_grant",
            AuthError::ExpiredDeviceToken => "expired_token",
            AuthError::Store(_) => "storage_error",
            AuthError::Internal(_) => "internal_error",
        }
    }
}

/// Errors surfaced by [`Store`](crate::store::Store) implementations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("record already exists")]
    Duplicate,
    #[error("record not found")]
    NotFound,
    #[error("{0}")]
    Other(String),
}
