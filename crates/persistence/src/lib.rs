//! PostgreSQL persistence for the auth domain, via `sqlx`.

use async_trait::async_trait;
use light_factory_auth::StoreError;
use light_factory_auth::store::{Challenge, DeviceGrant, NewUser, Session, Store, User};
use sqlx::PgPool;
use uuid::Uuid;

mod migrations;

pub use migrations::run_migrations;

/// A [`Store`] backed by a PostgreSQL pool.
#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Translate sqlx errors into the auth domain's [`StoreError`].
fn map_err(e: sqlx::Error) -> StoreError {
    use sqlx::Error as E;
    match e {
        E::Database(db) if db.is_unique_violation() => StoreError::Duplicate,
        E::RowNotFound => StoreError::NotFound,
        other => StoreError::Other(other.to_string()),
    }
}

#[async_trait]
impl Store for PgStore {
    async fn create_user(&self, user: &NewUser) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO users (id, email, display_name, totp_secret_enc)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(user.id)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&user.totp_secret_enc)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, display_name, totp_secret_enc, totp_enabled,
                    created_at
             FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(row.map(Into::into))
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>, StoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, display_name, totp_secret_enc, totp_enabled,
                    created_at
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(row.map(Into::into))
    }

    async fn set_totp_secret(&self, user_id: Uuid, secret_enc: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET totp_secret_enc = $2 WHERE id = $1")
            .bind(user_id)
            .bind(secret_enc)
            .execute(&self.pool)
            .await
            .map_err(map_err)?;
        Ok(())
    }

    async fn enable_totp(&self, user_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET totp_enabled = TRUE WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(map_err)?;
        Ok(())
    }

    async fn create_session(&self, session: &Session) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&session.token_hash)
        .bind(session.user_id)
        .bind(session.created_at)
        .bind(session.expires_at)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn get_session(&self, token_hash: &str) -> Result<Option<Session>, StoreError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT token_hash, user_id, created_at, expires_at FROM sessions WHERE token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(row.map(Into::into))
    }

    async fn delete_session(&self, token_hash: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await
            .map_err(map_err)?;
        Ok(())
    }

    async fn create_challenge(&self, challenge: &Challenge) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO challenges (token_hash, user_id, created_at, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&challenge.token_hash)
        .bind(challenge.user_id)
        .bind(challenge.created_at)
        .bind(challenge.expires_at)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn consume_challenge(&self, token_hash: &str) -> Result<Option<Challenge>, StoreError> {
        let row = sqlx::query_as::<_, ChallengeRow>(
            "DELETE FROM challenges
             WHERE token_hash = $1 AND expires_at > now()
             RETURNING token_hash, user_id, created_at, expires_at",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(row.map(Into::into))
    }

    async fn create_device_grant(&self, grant: &DeviceGrant) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO device_grants (device_code_hash, user_code, user_id, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&grant.device_code_hash)
        .bind(&grant.user_code)
        .bind(grant.user_id)
        .bind(grant.created_at)
        .bind(grant.expires_at)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn get_device_grant(
        &self,
        device_code_hash: &str,
    ) -> Result<Option<DeviceGrant>, StoreError> {
        let row = sqlx::query_as::<_, DeviceGrantRow>(
            "SELECT device_code_hash, user_code, user_id, created_at, expires_at
             FROM device_grants WHERE device_code_hash = $1",
        )
        .bind(device_code_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(row.map(Into::into))
    }

    async fn get_device_grant_by_user_code(
        &self,
        user_code: &str,
    ) -> Result<Option<DeviceGrant>, StoreError> {
        let row = sqlx::query_as::<_, DeviceGrantRow>(
            "SELECT device_code_hash, user_code, user_id, created_at, expires_at
             FROM device_grants WHERE user_code = $1",
        )
        .bind(user_code)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(row.map(Into::into))
    }

    async fn approve_device_grant(
        &self,
        user_code: &str,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE device_grants SET user_id = $2
             WHERE user_code = $1 AND user_id IS NULL AND expires_at > now()",
        )
        .bind(user_code)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    async fn consume_device_grant(
        &self,
        device_code_hash: &str,
    ) -> Result<Option<DeviceGrant>, StoreError> {
        let row = sqlx::query_as::<_, DeviceGrantRow>(
            "DELETE FROM device_grants
             WHERE device_code_hash = $1
             RETURNING device_code_hash, user_code, user_id, created_at, expires_at",
        )
        .bind(device_code_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_err)?;
        Ok(row.map(Into::into))
    }
}

// Row projections (decouple wire/DB types from domain types).

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    totp_secret_enc: String,
    totp_enabled: bool,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        User {
            id: r.id,
            email: r.email,
            display_name: r.display_name,
            totp_secret_enc: r.totp_secret_enc,
            totp_enabled: r.totp_enabled,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    token_hash: String,
    user_id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<SessionRow> for Session {
    fn from(r: SessionRow) -> Self {
        Session {
            token_hash: r.token_hash,
            user_id: r.user_id,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ChallengeRow {
    token_hash: String,
    user_id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<ChallengeRow> for Challenge {
    fn from(r: ChallengeRow) -> Self {
        Challenge {
            token_hash: r.token_hash,
            user_id: r.user_id,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct DeviceGrantRow {
    device_code_hash: String,
    user_code: String,
    user_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<DeviceGrantRow> for DeviceGrant {
    fn from(r: DeviceGrantRow) -> Self {
        DeviceGrant {
            device_code_hash: r.device_code_hash,
            user_code: r.user_code,
            user_id: r.user_id,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}
