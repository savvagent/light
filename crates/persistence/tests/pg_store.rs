//! Integration tests against a real PostgreSQL instance.
//!
//! These read `DATABASE_URL` (default: the local dev cluster) and are skipped
//! when Postgres is unreachable so `cargo test` stays green on machines
//! without a database.

use light_factory_auth::store::{Challenge, DeviceGrant, NewUser, Session, Store};
use light_factory_persistence::{PgStore, run_migrations};
use sqlx::PgPool;
use uuid::Uuid;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://light@127.0.0.1:5432/light".to_string())
}

async fn connect() -> Option<PgPool> {
    let pool = PgPool::connect(&database_url()).await.ok()?;
    run_migrations(&pool).await.ok()?;
    Some(pool)
}

#[tokio::test]
async fn store_round_trip() {
    let Some(pool) = connect().await else {
        eprintln!("skipping: Postgres unavailable");
        return;
    };
    let store = PgStore::new(pool);

    let id = Uuid::new_v4();
    let user = NewUser {
        id,
        email: format!("{id}@example.com"),
        display_name: "test".into(),
        totp_secret_enc: "enc-seed".into(),
    };
    store.create_user(&user).await.unwrap();

    // Fetch by email and id.
    let fetched = store.get_user_by_email(&user.email).await.unwrap().unwrap();
    assert_eq!(fetched.id, id);
    assert!(!fetched.totp_enabled);
    assert_eq!(fetched.totp_secret_enc, "enc-seed");

    let by_id = store.get_user_by_id(id).await.unwrap().unwrap();
    assert_eq!(by_id.email, user.email);

    // Duplicate email -> StoreError::Duplicate.
    assert!(matches!(
        store.create_user(&user).await,
        Err(light_factory_auth::StoreError::Duplicate)
    ));

    // TOTP secret overwrite + enable.
    store.set_totp_secret(id, "enc-seed-2").await.unwrap();
    store.enable_totp(id).await.unwrap();
    let fetched = store.get_user_by_id(id).await.unwrap().unwrap();
    assert!(fetched.totp_enabled);
    assert_eq!(fetched.totp_secret_enc, "enc-seed-2");

    // Sessions.
    let session = Session {
        token_hash: "hash-1".into(),
        user_id: id,
        created_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::days(1),
    };
    store.create_session(&session).await.unwrap();
    let got = store.get_session("hash-1").await.unwrap().unwrap();
    assert_eq!(got.user_id, id);
    store.delete_session("hash-1").await.unwrap();
    assert!(store.get_session("hash-1").await.unwrap().is_none());

    // Challenge consume (single-use).
    let challenge = Challenge {
        token_hash: "chal-1".into(),
        user_id: id,
        created_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
    };
    store.create_challenge(&challenge).await.unwrap();
    let consumed = store.consume_challenge("chal-1").await.unwrap();
    assert!(consumed.is_some());
    assert!(store.consume_challenge("chal-1").await.unwrap().is_none());
}

#[tokio::test]
async fn device_grant_round_trip() {
    let Some(pool) = connect().await else {
        eprintln!("skipping: Postgres unavailable");
        return;
    };
    let store = PgStore::new(pool);

    let user_id = Uuid::new_v4();
    let user = NewUser {
        id: user_id,
        email: format!("{user_id}@example.com"),
        display_name: "test".into(),
        totp_secret_enc: "enc-seed".into(),
    };
    store.create_user(&user).await.unwrap();

    let device_code_hash = "device-hash-1".to_string();
    let user_code = "ABCD-EFGH".to_string();
    let grant = DeviceGrant {
        device_code_hash: device_code_hash.clone(),
        user_code: user_code.clone(),
        user_id: None,
        created_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
    };
    store.create_device_grant(&grant).await.unwrap();

    // Fetch by device code and by user code.
    let fetched = store.get_device_grant(&device_code_hash).await.unwrap().unwrap();
    assert_eq!(fetched.user_code, user_code);
    assert!(fetched.user_id.is_none());
    let by_code = store
        .get_device_grant_by_user_code(&user_code)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_code.device_code_hash, device_code_hash);

    // Approve sets the user_id.
    assert!(store.approve_device_grant(&user_code, user_id).await.unwrap());
    let fetched = store.get_device_grant(&device_code_hash).await.unwrap().unwrap();
    assert_eq!(fetched.user_id, Some(user_id));

    // Approving again returns false (no pending grant left to update).
    assert!(!store.approve_device_grant(&user_code, user_id).await.unwrap());

    // Consume is single-use.
    let consumed = store
        .consume_device_grant(&device_code_hash)
        .await
        .unwrap();
    assert!(consumed.is_some());
    assert!(store
        .consume_device_grant(&device_code_hash)
        .await
        .unwrap()
        .is_none());
}
