//! Shared application state passed to every handler.

use std::sync::Arc;

use light_factory_auth::AuthService;

/// The router's shared state. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthService>,
    /// Origin of the web SPA used to build device-authorization URLs.
    pub device_verification_uri: String,
}
