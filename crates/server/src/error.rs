//! Uniform error envelope: every non-2xx response carries [`ErrorBody`].

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use light_factory_auth::AuthError;
use light_factory_protocol::auth::{ErrorBody, ErrorDetail};

/// An error that maps to a single HTTP status and the shared [`ErrorBody`]
/// envelope.
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    /// Build a 400 from a failed JSON extraction.
    pub fn invalid_json(rejection: &JsonRejection) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_json".to_string(),
            message: rejection.to_string(),
        }
    }

    /// Build an error with an explicit status, code, and message.
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        let status = status_for(&e);
        Self {
            status,
            code: e.code().to_string(),
            message: e.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code,
                message: self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

fn status_for(e: &AuthError) -> StatusCode {
    use AuthError::*;
    match e {
        InvalidEmail => StatusCode::BAD_REQUEST,
        EmailTaken => StatusCode::CONFLICT,
        InvalidCredentials | InvalidTotpCode | InvalidChallenge | InvalidSession => {
            StatusCode::UNAUTHORIZED
        }
        InvalidDeviceGrant | ExpiredDeviceToken => StatusCode::BAD_REQUEST,
        Store(_) | Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
