//! HTTP handlers.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use light_factory_auth::store::User;
use light_factory_auth::DevicePoll;
use light_factory_protocol::auth::{
    AuthResponse, DeviceApproveRequest, DeviceAuthResponse, DeviceTokenRequest, LoginRequest,
    RegisterConfirmRequest, RegisterRequest, RegisterResponse, UserView,
};

use crate::auth_extract::{AuthenticatedUser, JsonBody};
use crate::error::ApiError;
use crate::state::AppState;

/// Project a domain [`User`] into the client-facing [`UserView`] (never leaks a
/// TOTP seed).
pub fn to_user_view(user: &User) -> UserView {
    UserView {
        id: user.id.to_string(),
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        created_at: user.created_at.timestamp(),
    }
}

fn to_auth_response(token: String, expires_at: i64, user: &User) -> AuthResponse {
    AuthResponse {
        token,
        expires_at,
        user: to_user_view(user),
    }
}

pub async fn register(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<RegisterRequest>,
) -> Result<Json<RegisterResponse>, ApiError> {
    let challenge = state
        .auth
        .register(&req.email, req.display_name.as_deref())
        .await?;
    Ok(Json(RegisterResponse {
        setup_token: challenge.setup_token,
        expires_at: challenge.expires_at,
        secret: challenge.secret_base32,
        otpauth_url: challenge.otpauth_url,
    }))
}

pub async fn register_confirm(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<RegisterConfirmRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let session = state
        .auth
        .register_confirm(&req.setup_token, &req.code)
        .await?;
    Ok(Json(to_auth_response(
        session.token,
        session.expires_at,
        &session.user,
    )))
}

pub async fn login(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let session = state.auth.login(&req.email, &req.code).await?;
    Ok(Json(to_auth_response(
        session.token,
        session.expires_at,
        &session.user,
    )))
}

pub async fn me(AuthenticatedUser { user, .. }: AuthenticatedUser) -> Json<UserView> {
    Json(to_user_view(&user))
}

pub async fn logout(
    State(state): State<AppState>,
    AuthenticatedUser { token, .. }: AuthenticatedUser,
) -> Result<StatusCode, ApiError> {
    state.auth.logout(&token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// RFC 8628 device authorization: mint the codes and return the verification
/// URLs the client opens in a browser.
pub async fn device_auth(
    State(state): State<AppState>,
) -> Result<Json<DeviceAuthResponse>, ApiError> {
    let auth = state.auth.start_device_auth().await?;
    let verification_uri = format!("{}/#/device", state.device_verification_uri);
    let verification_uri_complete = format!(
        "{}/#/device?user_code={}",
        state.device_verification_uri, auth.user_code
    );
    Ok(Json(DeviceAuthResponse {
        device_code: auth.device_code,
        user_code: auth.user_code,
        verification_uri,
        verification_uri_complete,
        expires_in: auth.expires_in,
        interval: 2,
    }))
}

/// RFC 8628 token endpoint: poll a pending device grant with the `device_code`.
pub async fn device_token(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<DeviceTokenRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    match state.auth.poll_device_token(&req.device_code).await? {
        DevicePoll::Pending => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "authorization_pending",
            "the user has not yet authorized the device",
        )),
        DevicePoll::Approved(session) => Ok(Json(to_auth_response(
            session.token,
            session.expires_at,
            &session.user,
        ))),
    }
}

/// Approve a pending device grant, called by the authenticated web user.
pub async fn device_approve(
    State(state): State<AppState>,
    AuthenticatedUser { user, .. }: AuthenticatedUser,
    JsonBody(req): JsonBody<DeviceApproveRequest>,
) -> Result<StatusCode, ApiError> {
    state.auth.approve_device(&req.user_code, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
