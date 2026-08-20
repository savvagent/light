//! Extractors for bearer authentication and JSON bodies.

use axum::Json;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use light_factory_auth::store::User;
use serde::de::DeserializeOwned;

use crate::error::ApiError;
use crate::state::AppState;

/// An authenticated request: the resolved [`User`] plus the raw bearer token
/// (needed by logout to revoke the session).
pub struct AuthenticatedUser {
    pub user: User,
    pub token: String,
}

impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)
            .ok_or_else(|| ApiError::from(light_factory_auth::AuthError::InvalidSession))?;
        let user = state.auth.authenticate(&token).await?;
        Ok(AuthenticatedUser { user, token })
    }
}

/// Extract a `Bearer <token>` from the `Authorization` header.
pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A JSON body whose extraction failures use the shared [`ErrorBody`] envelope.
pub struct JsonBody<T>(pub T);

impl<T> FromRequest<AppState> for JsonBody<T>
where
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(JsonBody(value)),
            Err(rejection) => Err(ApiError::invalid_json(&rejection)),
        }
    }
}
