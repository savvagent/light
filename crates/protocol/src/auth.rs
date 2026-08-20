//! Authentication wire types.

use serde::{Deserialize, Serialize};

/// Request body for `POST /auth/register`. Passwordless: registration begins
/// with an email address and completes by confirming a TOTP code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub display_name: Option<String>,
}

/// Response for `POST /auth/register`. The client renders `otpauth_url` as a QR
/// code (`secret` is the manual-entry fallback) and confirms via
/// [`RegisterConfirmRequest`] using the single-use `setup_token`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    /// Short-lived single-use token authorizing the confirmation step.
    pub setup_token: String,
    /// Unix epoch seconds at which the setup token stops being accepted.
    pub expires_at: i64,
    /// Base32 secret for manual entry into an authenticator app.
    pub secret: String,
    /// `otpauth://` provisioning URL rendered as a QR code.
    pub otpauth_url: String,
}

/// Request body for `POST /auth/register/confirm`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterConfirmRequest {
    pub setup_token: String,
    pub code: String,
}

/// Request body for `POST /auth/login`. Passwordless: the TOTP code from the
/// user's authenticator app is the credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub code: String,
}

/// A user as seen by clients. Never carries a TOTP seed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserView {
    pub id: String,
    pub email: String,
    pub display_name: String,
    /// Unix epoch seconds.
    pub created_at: i64,
}

/// Success response for register-confirm and login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    /// The opaque session token. Presented as `Authorization: Bearer <token>`.
    pub token: String,
    /// Unix epoch seconds at which the token stops being accepted.
    pub expires_at: i64,
    pub user: UserView,
}

/// Response for `POST /auth/device` (OAuth 2.0 Device Authorization Grant,
/// RFC 8628). The client shows `user_code` and `verification_uri` to the user,
/// opens the browser, and polls [`DeviceTokenRequest`] until a token is issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthResponse {
    /// Long-lived, high-entropy secret presented when polling for a token.
    pub device_code: String,
    /// Short human-typed code shown to the user and entered in the browser.
    pub user_code: String,
    /// URL to open in the browser to authorize the device.
    pub verification_uri: String,
    /// URL with the `user_code` pre-filled for one-click authorization.
    pub verification_uri_complete: String,
    /// Seconds until the `device_code` and `user_code` expire.
    pub expires_in: i64,
    /// Minimum seconds between token polls.
    pub interval: u64,
}

/// Request body for `POST /auth/device/token`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTokenRequest {
    pub device_code: String,
}

/// Request body for `POST /auth/device/approve`, sent by the authenticated web
/// user to authorize a pending device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceApproveRequest {
    pub user_code: String,
}

/// Uniform error envelope returned by every non-2xx response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    /// Machine-readable code (e.g. `invalid_credentials`, `email_taken`).
    pub code: String,
    /// Human-readable message safe to surface directly in the UI.
    pub message: String,
}
