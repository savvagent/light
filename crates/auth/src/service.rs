//! The auth orchestrator: passwordless register/login via TOTP, sessions.

use std::sync::Arc;

use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::{
    error::{AuthError, StoreError},
    secret::SecretCipher,
    store::{Challenge, DeviceGrant, NewUser, Session, Store, User},
    token, totp,
};

/// Server-side auth configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Session lifetime for bearer tokens.
    pub session_ttl: Duration,
    /// Lifetime for the short-lived registration challenge (between the email
    /// step and TOTP confirmation).
    pub challenge_ttl: Duration,
    /// Lifetime for a pending device-authorization grant (RFC 8628).
    pub device_ttl: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            session_ttl: Duration::days(30),
            challenge_ttl: Duration::minutes(5),
            device_ttl: Duration::minutes(10),
        }
    }
}

/// Outcome of starting registration: the TOTP provisioning values plus a
/// single-use setup token that authorizes the confirmation step.
#[derive(Debug)]
pub struct RegistrationChallenge {
    pub setup_token: String,
    pub expires_at: i64,
    pub secret_base32: String,
    pub otpauth_url: String,
}

/// A freshly issued bearer session plus the user it belongs to.
#[derive(Debug)]
pub struct IssuedSession {
    pub token: String,
    pub expires_at: i64,
    pub user: User,
}

/// Outcome of starting a device-authorization flow: the codes the client shows
/// and polls with.
#[derive(Debug)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    /// Seconds until the codes expire.
    pub expires_in: i64,
}

/// Outcome of polling the token endpoint for a pending device grant.
#[derive(Debug)]
pub enum DevicePoll {
    /// Not yet approved; the client should keep polling.
    Pending,
    /// The browser user approved the device; a session is ready.
    Approved(IssuedSession),
}

/// The auth service. Cheap to clone: wraps `Arc`d state.
#[derive(Clone)]
pub struct AuthService {
    store: Arc<dyn Store>,
    cipher: Arc<SecretCipher>,
    config: Config,
}

impl AuthService {
    pub fn new(store: Arc<dyn Store>, cipher: SecretCipher, config: Config) -> Self {
        Self {
            store,
            cipher: Arc::new(cipher),
            config,
        }
    }

    /// Normalize an email for storage and lookup (lowercase + trim).
    fn normalize_email(email: &str) -> String {
        email.trim().to_lowercase()
    }

    fn validate_email(email: &str) -> Result<(), AuthError> {
        let e = Self::normalize_email(email);
        if e.is_empty() || !e.contains('@') || e.len() > 320 {
            return Err(AuthError::InvalidEmail);
        }
        let (local, domain) = e.split_once('@').expect("checked contains '@'");
        if local.is_empty() || domain.is_empty() || !domain.contains('.') {
            return Err(AuthError::InvalidEmail);
        }
        Ok(())
    }

    /// Begin registration: create (or reset) a pending account and return the
    /// TOTP provisioning values plus a single-use setup token. Re-registering
    /// an email that has not yet confirmed TOTP regenerates the secret.
    pub async fn register(
        &self,
        email: &str,
        display_name: Option<&str>,
    ) -> Result<RegistrationChallenge, AuthError> {
        Self::validate_email(email)?;

        let email = Self::normalize_email(email);
        let display_name: String = display_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| email.split('@').next().unwrap_or("user").to_string());

        let (raw, secret_base32, url) = totp::generate(&email);
        let enc = self.cipher.encrypt(&raw)?;

        let user_id = match self.store.get_user_by_email(&email).await? {
            Some(user) if user.totp_enabled => return Err(AuthError::EmailTaken),
            Some(user) => {
                self.store.set_totp_secret(user.id, &enc).await?;
                user.id
            }
            None => {
                let id = Uuid::new_v4();
                let user = NewUser {
                    id,
                    email,
                    display_name,
                    totp_secret_enc: enc,
                };
                match self.store.create_user(&user).await {
                    Ok(()) => {}
                    Err(StoreError::Duplicate) => return Err(AuthError::EmailTaken),
                    Err(e) => return Err(e.into()),
                }
                id
            }
        };

        let expires_at = Utc::now() + self.config.challenge_ttl;
        let setup_token = token::mint_token();
        let challenge = Challenge {
            token_hash: token::hash_token(&setup_token),
            user_id,
            created_at: Utc::now(),
            expires_at,
        };
        self.store.create_challenge(&challenge).await?;

        Ok(RegistrationChallenge {
            setup_token,
            expires_at: expires_at.timestamp(),
            secret_base32,
            otpauth_url: url,
        })
    }

    /// Complete registration: verify the TOTP code, enable the account, and
    /// issue a session.
    pub async fn register_confirm(
        &self,
        setup_token: &str,
        code: &str,
    ) -> Result<IssuedSession, AuthError> {
        let challenge = self
            .store
            .consume_challenge(&token::hash_token(setup_token))
            .await?
            .ok_or(AuthError::InvalidChallenge)?;

        let user = self
            .store
            .get_user_by_id(challenge.user_id)
            .await?
            .ok_or(AuthError::InvalidChallenge)?;

        if !self.verify_totp(&user, code)? {
            return Err(AuthError::InvalidTotpCode);
        }

        self.store.enable_totp(user.id).await?;
        let user = self
            .store
            .get_user_by_id(user.id)
            .await?
            .ok_or_else(|| AuthError::Internal("user not found after enable".into()))?;

        self.issue_session(user).await
    }

    /// Passwordless login: verify the email + TOTP code and issue a session.
    pub async fn login(&self, email: &str, code: &str) -> Result<IssuedSession, AuthError> {
        let email = Self::normalize_email(email);
        let user = self
            .store
            .get_user_by_email(&email)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;

        if !user.totp_enabled {
            return Err(AuthError::InvalidCredentials);
        }

        if !self.verify_totp(&user, code)? {
            return Err(AuthError::InvalidTotpCode);
        }

        self.issue_session(user).await
    }

    /// Resolve a bearer token to its user, enforcing expiry.
    pub async fn authenticate(&self, token: &str) -> Result<User, AuthError> {
        let session = self
            .store
            .get_session(&token::hash_token(token))
            .await?
            .ok_or(AuthError::InvalidSession)?;

        if session.expires_at <= Utc::now() {
            return Err(AuthError::InvalidSession);
        }

        self.store
            .get_user_by_id(session.user_id)
            .await?
            .ok_or(AuthError::InvalidSession)
    }

    /// Revoke a session (logout).
    pub async fn logout(&self, token: &str) -> Result<(), AuthError> {
        self.store.delete_session(&token::hash_token(token)).await?;
        Ok(())
    }

    /// Begin a device-authorization grant: mint a `device_code` + `user_code`,
    /// store the pending grant, and return the codes the client surfaces.
    pub async fn start_device_auth(&self) -> Result<DeviceAuthorization, AuthError> {
        let device_code = token::mint_token();
        let user_code = token::mint_user_code();
        let grant = DeviceGrant {
            device_code_hash: token::hash_token(&device_code),
            user_code: user_code.clone(),
            user_id: None,
            created_at: Utc::now(),
            expires_at: Utc::now() + self.config.device_ttl,
        };
        self.store.create_device_grant(&grant).await?;
        Ok(DeviceAuthorization {
            device_code,
            user_code,
            expires_in: self.config.device_ttl.num_seconds(),
        })
    }

    /// The browser user approves a pending device by `user_code`.
    pub async fn approve_device(&self, user_code: &str, user_id: Uuid) -> Result<(), AuthError> {
        let approved = self.store.approve_device_grant(user_code, user_id).await?;
        if !approved {
            return Err(AuthError::InvalidDeviceGrant);
        }
        Ok(())
    }

    /// Poll for a pending device grant. Returns [`DevicePoll::Pending`] until
    /// the browser user approves; on approval, atomically consumes the grant
    /// and issues a session.
    pub async fn poll_device_token(&self, device_code: &str) -> Result<DevicePoll, AuthError> {
        let hash = token::hash_token(device_code);
        let grant = self
            .store
            .get_device_grant(&hash)
            .await?
            .ok_or(AuthError::InvalidDeviceGrant)?;

        if grant.expires_at <= Utc::now() {
            return Err(AuthError::ExpiredDeviceToken);
        }

        let Some(user_id) = grant.user_id else {
            return Ok(DevicePoll::Pending);
        };

        // Approved: consume the grant atomically so the device_code is
        // single-use even under a concurrent poll race.
        let consumed = self.store.consume_device_grant(&hash).await?;
        let Some(_) = consumed else {
            return Err(AuthError::ExpiredDeviceToken);
        };

        let user = self
            .store
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| AuthError::Internal("device user not found".into()))?;

        let session = self.issue_session(user).await?;
        Ok(DevicePoll::Approved(session))
    }

    async fn issue_session(&self, user: User) -> Result<IssuedSession, AuthError> {
        let expires_at = Utc::now() + self.config.session_ttl;
        let raw = token::mint_token();
        let session = Session {
            token_hash: token::hash_token(&raw),
            user_id: user.id,
            created_at: Utc::now(),
            expires_at,
        };
        self.store.create_session(&session).await?;
        Ok(IssuedSession {
            token: raw,
            expires_at: expires_at.timestamp(),
            user,
        })
    }

    fn verify_totp(&self, user: &User, code: &str) -> Result<bool, AuthError> {
        let raw = self.cipher.decrypt(&user.totp_secret_enc)?;
        totp::verify(&raw, &user.email, code)
    }
}
