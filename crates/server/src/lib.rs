//! HTTP server for light-factory: auth endpoints plus an authenticated
//! WebSocket. Built on axum.

pub mod auth_extract;
pub mod config;
pub mod error;
pub mod routes;
pub mod state;
pub mod ws;

use axum::Router;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::routing::{get, post};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::state::AppState;

/// CORS restricted to the configured origins (Cloudflare Pages in prod, the
/// Vite dev server locally).
fn cors_layer() -> CorsLayer {
    let origins: Vec<HeaderValue> = config::cors_origins_from_env()
        .into_iter()
        .map(|o| HeaderValue::from_str(&o).expect("CORS origin must be a valid header value"))
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
}

/// Build the axum router with all routes wired to `state`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/auth/register", post(routes::register))
        .route("/auth/register/confirm", post(routes::register_confirm))
        .route("/auth/login", post(routes::login))
        .route("/auth/me", get(routes::me))
        .route("/auth/logout", post(routes::logout))
        .route("/auth/device", post(routes::device_auth))
        .route("/auth/device/token", post(routes::device_token))
        .route("/auth/device/approve", post(routes::device_approve))
        .route("/ws", get(ws::ws_handler))
        .layer(cors_layer())
        .with_state(state)
}
