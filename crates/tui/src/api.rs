//! HTTP client for the auth endpoints, mirroring `web/src/lib/api.js`.

use light_factory_protocol::auth::{
    AuthResponse, DeviceAuthResponse, ErrorBody, ErrorDetail, LoginRequest,
    RegisterConfirmRequest, RegisterRequest, RegisterResponse, UserView,
};

/// A uniform client error surfaced directly in the UI.
#[derive(Debug)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Thin async client around a `reqwest` client bound to a base URL.
#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    pub fn new(base: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.to_string(),
        }
    }

    async fn request<B, T>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
        token: Option<&str>,
    ) -> Result<T, ApiError>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let mut req = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(tok) = token {
            req = req.bearer_auth(tok);
        }
        if let Some(b) = body {
            req = req.json(b);
        }

        let res = req.send().await.map_err(|e| ApiError {
            code: "network".into(),
            message: format!("could not reach the server: {e}"),
        })?;
        let status = res.status();
        let data: serde_json::Value = res.json().await.unwrap_or(serde_json::Value::Null);

        if !status.is_success() {
            let body: ErrorBody = serde_json::from_value(data).unwrap_or(ErrorBody {
                error: ErrorDetail {
                    code: "unknown".into(),
                    message: status.to_string(),
                },
            });
            return Err(ApiError {
                code: body.error.code,
                message: body.error.message,
            });
        }

        serde_json::from_value(data).map_err(|e| ApiError {
            code: "decode".into(),
            message: format!("unexpected response: {e}"),
        })
    }

    pub async fn register(
        &self,
        email: &str,
        display_name: Option<&str>,
    ) -> Result<RegisterResponse, ApiError> {
        let body = RegisterRequest {
            email: email.to_string(),
            display_name: display_name.map(str::to_string),
        };
        self.request(reqwest::Method::POST, "/auth/register", Some(&body), None)
            .await
    }

    pub async fn register_confirm(
        &self,
        setup_token: &str,
        code: &str,
    ) -> Result<AuthResponse, ApiError> {
        let body = RegisterConfirmRequest {
            setup_token: setup_token.to_string(),
            code: code.to_string(),
        };
        self.request(
            reqwest::Method::POST,
            "/auth/register/confirm",
            Some(&body),
            None,
        )
        .await
    }

    pub async fn login(&self, email: &str, code: &str) -> Result<AuthResponse, ApiError> {
        let body = LoginRequest {
            email: email.to_string(),
            code: code.to_string(),
        };
        self.request(reqwest::Method::POST, "/auth/login", Some(&body), None)
            .await
    }

    pub async fn me(&self, token: &str) -> Result<UserView, ApiError> {
        self.request(reqwest::Method::GET, "/auth/me", None::<&()>, Some(token))
            .await
    }

    pub async fn logout(&self, token: &str) -> Result<(), ApiError> {
        self.request::<_, ()>(
            reqwest::Method::POST,
            "/auth/logout",
            None::<&()>,
            Some(token),
        )
        .await
    }

    /// Start a device-authorization grant (RFC 8628).
    pub async fn device(&self) -> Result<DeviceAuthResponse, ApiError> {
        self.request(reqwest::Method::POST, "/auth/device", None::<&()>, None)
            .await
    }

    /// Poll the token endpoint for a pending device grant. `authorization_pending`
    /// surfaces as an [`ApiError`] with code `authorization_pending`.
    pub async fn device_token(&self, device_code: &str) -> Result<AuthResponse, ApiError> {
        #[derive(serde::Serialize)]
        struct Body<'a> {
            device_code: &'a str,
        }
        self.request(
            reqwest::Method::POST,
            "/auth/device/token",
            Some(&Body { device_code }),
            None,
        )
        .await
    }
}
