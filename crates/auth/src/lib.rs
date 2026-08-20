//! Authentication domain: TOTP, session tokens, and the store seam. This crate
//! performs no I/O and depends on no web framework, so the entire auth flow is
//! unit-testable without a database or network.

pub mod error;
pub mod secret;
pub mod service;
pub mod store;
pub mod token;
pub mod totp;

pub use error::{AuthError, StoreError};
pub use service::{
    AuthService, Config, DeviceAuthorization, DevicePoll, IssuedSession, RegistrationChallenge,
};
pub use store::{Challenge, DeviceGrant, NewUser, Session, Store, User};
