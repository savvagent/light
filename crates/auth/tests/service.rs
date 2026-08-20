//! Tests for the auth service against an in-memory store (no DB, no network).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use light_factory_auth::{
    AuthError, AuthService, Challenge, Config, DeviceGrant, NewUser, Session, Store, StoreError,
    User, token,
};

/// A simple thread-safe in-memory store for tests.
#[derive(Default)]
struct MemStore {
    users: Mutex<HashMap<Uuid, User>>,
    by_email: Mutex<HashMap<String, Uuid>>,
    sessions: Mutex<HashMap<String, Session>>,
    challenges: Mutex<HashMap<String, Challenge>>,
    device_grants: Mutex<HashMap<String, DeviceGrant>>,
    device_by_user_code: Mutex<HashMap<String, String>>,
}

#[async_trait]
impl Store for MemStore {
    async fn create_user(&self, user: &NewUser) -> Result<(), StoreError> {
        let mut by_email = self.by_email.lock().unwrap();
        if by_email.contains_key(&user.email) {
            return Err(StoreError::Duplicate);
        }
        by_email.insert(user.email.clone(), user.id);
        let mut users = self.users.lock().unwrap();
        users.insert(
            user.id,
            User {
                id: user.id,
                email: user.email.clone(),
                display_name: user.display_name.clone(),
                totp_secret_enc: user.totp_secret_enc.clone(),
                totp_enabled: false,
                created_at: Utc::now(),
            },
        );
        Ok(())
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError> {
        let by_email = self.by_email.lock().unwrap();
        let id = by_email.get(email).copied();
        Ok(match id {
            Some(id) => self.users.lock().unwrap().get(&id).cloned(),
            None => None,
        })
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>, StoreError> {
        Ok(self.users.lock().unwrap().get(&id).cloned())
    }

    async fn set_totp_secret(&self, user_id: Uuid, secret_enc: &str) -> Result<(), StoreError> {
        self.users
            .lock()
            .unwrap()
            .get_mut(&user_id)
            .ok_or(StoreError::NotFound)?
            .totp_secret_enc = secret_enc.to_string();
        Ok(())
    }

    async fn enable_totp(&self, user_id: Uuid) -> Result<(), StoreError> {
        self.users
            .lock()
            .unwrap()
            .get_mut(&user_id)
            .ok_or(StoreError::NotFound)?
            .totp_enabled = true;
        Ok(())
    }

    async fn create_session(&self, session: &Session) -> Result<(), StoreError> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.token_hash.clone(), session.clone());
        Ok(())
    }

    async fn get_session(&self, token_hash: &str) -> Result<Option<Session>, StoreError> {
        Ok(self.sessions.lock().unwrap().get(token_hash).cloned())
    }

    async fn delete_session(&self, token_hash: &str) -> Result<(), StoreError> {
        self.sessions.lock().unwrap().remove(token_hash);
        Ok(())
    }

    async fn create_challenge(&self, challenge: &Challenge) -> Result<(), StoreError> {
        self.challenges
            .lock()
            .unwrap()
            .insert(challenge.token_hash.clone(), challenge.clone());
        Ok(())
    }

    async fn consume_challenge(&self, token_hash: &str) -> Result<Option<Challenge>, StoreError> {
        Ok(self.challenges.lock().unwrap().remove(token_hash))
    }

    async fn create_device_grant(&self, grant: &DeviceGrant) -> Result<(), StoreError> {
        let mut map = self.device_grants.lock().unwrap();
        self.device_by_user_code
            .lock()
            .unwrap()
            .insert(grant.user_code.clone(), grant.device_code_hash.clone());
        map.insert(grant.device_code_hash.clone(), grant.clone());
        Ok(())
    }

    async fn get_device_grant(
        &self,
        device_code_hash: &str,
    ) -> Result<Option<DeviceGrant>, StoreError> {
        Ok(self.device_grants.lock().unwrap().get(device_code_hash).cloned())
    }

    async fn get_device_grant_by_user_code(
        &self,
        user_code: &str,
    ) -> Result<Option<DeviceGrant>, StoreError> {
        let hash = self.device_by_user_code.lock().unwrap().get(user_code).cloned();
        Ok(match hash {
            Some(h) => self.device_grants.lock().unwrap().get(&h).cloned(),
            None => None,
        })
    }

    async fn approve_device_grant(
        &self,
        user_code: &str,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        let hash = self.device_by_user_code.lock().unwrap().get(user_code).cloned();
        let Some(hash) = hash else {
            return Ok(false);
        };
        let mut map = self.device_grants.lock().unwrap();
        let Some(grant) = map.get_mut(&hash) else {
            return Ok(false);
        };
        if grant.user_id.is_some() {
            return Ok(false);
        }
        grant.user_id = Some(user_id);
        Ok(true)
    }

    async fn consume_device_grant(
        &self,
        device_code_hash: &str,
    ) -> Result<Option<DeviceGrant>, StoreError> {
        let grant = self.device_grants.lock().unwrap().remove(device_code_hash);
        if let Some(g) = &grant {
            self.device_by_user_code
                .lock()
                .unwrap()
                .remove(&g.user_code);
        }
        Ok(grant)
    }
}

fn service() -> AuthService {
    let key = [7u8; 32];
    AuthService::new(
        std::sync::Arc::new(MemStore::default()),
        light_factory_auth::secret::SecretCipher::new(&key),
        Config::default(),
    )
}

fn totp_current_code(secret_base32: &str) -> String {
    use totp_rs::{Algorithm, TOTP};
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(secret_base32.to_string())
            .to_bytes()
            .unwrap(),
        Some("light-factory".to_string()),
        "a@b.com".to_string(),
    )
    .unwrap();
    totp.generate_current().unwrap()
}

#[tokio::test]
async fn register_confirm_issues_session_and_normalizes_email() {
    let svc = service();
    let reg = svc.register("  Rob@Example.COM ", None).await.unwrap();

    assert!(reg.otpauth_url.starts_with("otpauth://"));
    assert!(!reg.secret_base32.is_empty());
    assert!(reg.expires_at > 0);

    let issued = svc
        .register_confirm(&reg.setup_token, &totp_current_code(&reg.secret_base32))
        .await
        .unwrap();
    assert_eq!(issued.user.email, "rob@example.com");
    assert_eq!(issued.user.display_name, "rob");
    assert!(issued.user.totp_enabled);

    let authed = svc.authenticate(&issued.token).await.unwrap();
    assert_eq!(authed.id, issued.user.id);
}

#[tokio::test]
async fn register_rejects_duplicate_email() {
    let svc = service();
    let reg = svc.register("a@b.com", None).await.unwrap();
    svc.register_confirm(&reg.setup_token, &totp_current_code(&reg.secret_base32))
        .await
        .unwrap();

    let err = svc.register("a@b.com", None).await.unwrap_err();
    assert!(matches!(err, AuthError::EmailTaken));
}

#[tokio::test]
async fn register_confirm_rejects_wrong_code() {
    let svc = service();
    let reg = svc.register("a@b.com", None).await.unwrap();
    let err = svc
        .register_confirm(&reg.setup_token, "000000")
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::InvalidTotpCode));

    // The challenge is single-use: the failed attempt consumed it.
    let err = svc
        .register_confirm(&reg.setup_token, &totp_current_code(&reg.secret_base32))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::InvalidChallenge));
}

#[tokio::test]
async fn register_is_idempotent_for_pending_account() {
    let svc = service();
    let first = svc.register("a@b.com", None).await.unwrap();

    // Re-registering before confirmation regenerates a fresh secret + token.
    let second = svc.register("a@b.com", None).await.unwrap();
    assert_ne!(first.secret_base32, second.secret_base32);
    assert_ne!(first.setup_token, second.setup_token);

    // The latest setup token confirms cleanly.
    svc.register_confirm(
        &second.setup_token,
        &totp_current_code(&second.secret_base32),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn login_requires_confirmation_and_correct_code() {
    let svc = service();
    let reg = svc.register("a@b.com", None).await.unwrap();

    // Unknown email -> invalid credentials (no enumeration).
    assert!(matches!(
        svc.login("nobody@b.com", "123456").await,
        Err(AuthError::InvalidCredentials)
    ));

    // Unconfirmed account cannot log in.
    assert!(matches!(
        svc.login("a@b.com", &totp_current_code(&reg.secret_base32))
            .await,
        Err(AuthError::InvalidCredentials)
    ));

    svc.register_confirm(&reg.setup_token, &totp_current_code(&reg.secret_base32))
        .await
        .unwrap();

    // Wrong code is rejected.
    assert!(matches!(
        svc.login("a@b.com", "000000").await,
        Err(AuthError::InvalidTotpCode)
    ));

    // Correct code issues a session.
    let issued = svc
        .login("a@b.com", &totp_current_code(&reg.secret_base32))
        .await
        .unwrap();
    assert!(svc.authenticate(&issued.token).await.is_ok());
}

#[tokio::test]
async fn logout_revokes_session() {
    let svc = service();
    let reg = svc.register("a@b.com", None).await.unwrap();
    let issued = svc
        .register_confirm(&reg.setup_token, &totp_current_code(&reg.secret_base32))
        .await
        .unwrap();
    svc.logout(&issued.token).await.unwrap();
    assert!(matches!(
        svc.authenticate(&issued.token).await,
        Err(AuthError::InvalidSession)
    ));
}

#[tokio::test]
async fn token_hash_is_stable_and_distinct() {
    let a = token::mint_token();
    let b = token::mint_token();
    assert_ne!(a, b);
    assert_eq!(token::hash_token(&a), token::hash_token(&a));
    assert_ne!(token::hash_token(&a), token::hash_token(&b));
}

#[tokio::test]
async fn secret_cipher_round_trips() {
    let cipher = light_factory_auth::secret::SecretCipher::new(&[9u8; 32]);
    let enc = cipher.encrypt(b"hello world").unwrap();
    assert_ne!(enc.as_bytes(), b"hello world");
    assert_eq!(cipher.decrypt(&enc).unwrap(), b"hello world");
}

#[tokio::test]
async fn device_auth_flow_pends_then_approves_and_issues_session() {
    let svc = service();

    // A registered, TOTP-confirmed user to approve the device.
    let reg = svc.register("dev@example.com", None).await.unwrap();
    let confirmed = svc
        .register_confirm(&reg.setup_token, &totp_current_code(&reg.secret_base32))
        .await
        .unwrap();

    let auth = svc.start_device_auth().await.unwrap();
    assert_eq!(auth.user_code.len(), 9); // "ABCD-EFGH"
    assert!(auth.expires_in > 0);

    // Not yet approved -> Pending.
    assert!(matches!(
        svc.poll_device_token(&auth.device_code).await.unwrap(),
        light_factory_auth::DevicePoll::Pending
    ));

    // Approve, then poll -> Approved with a usable session.
    svc.approve_device(&auth.user_code, confirmed.user.id)
        .await
        .unwrap();
    let session = match svc.poll_device_token(&auth.device_code).await.unwrap() {
        light_factory_auth::DevicePoll::Approved(s) => s,
        light_factory_auth::DevicePoll::Pending => panic!("expected approval"),
    };
    assert_eq!(session.user.id, confirmed.user.id);
    assert!(svc.authenticate(&session.token).await.is_ok());

    // The device_code is single-use: a second poll fails.
    assert!(matches!(
        svc.poll_device_token(&auth.device_code).await,
        Err(AuthError::InvalidDeviceGrant)
    ));
}

#[tokio::test]
async fn device_auth_rejects_unknown_codes_and_approve() {
    let svc = service();

    // Unknown device_code -> invalid grant.
    assert!(matches!(
        svc.poll_device_token("nope").await,
        Err(AuthError::InvalidDeviceGrant)
    ));

    // Approving an unknown user_code -> invalid grant.
    let reg = svc.register("approver@example.com", None).await.unwrap();
    let confirmed = svc
        .register_confirm(&reg.setup_token, &totp_current_code(&reg.secret_base32))
        .await
        .unwrap();
    assert!(matches!(
        svc.approve_device("ZZZZ-ZZZZ", confirmed.user.id).await,
        Err(AuthError::InvalidDeviceGrant)
    ));
}
