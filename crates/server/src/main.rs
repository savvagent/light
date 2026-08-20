//! light-factory server entry point.

use std::sync::Arc;

use light_factory_auth::{AuthService, Store};
use light_factory_persistence::PgStore;
use light_factory_server::build_router;
use light_factory_server::config;
use light_factory_server::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "light_factory_server=info,tower_http=info".into()),
        )
        .init();

    let pool = sqlx::PgPool::connect(&config::database_url_from_env()).await?;
    light_factory_persistence::run_migrations(&pool).await?;

    let cipher = config::secret_cipher_from_env()?;
    let auth_config = config::config_from_env();

    let store: Arc<dyn Store> = Arc::new(PgStore::new(pool));
    let auth = Arc::new(AuthService::new(store, cipher, auth_config));
    let state = AppState {
        auth,
        device_verification_uri: config::device_verification_uri_from_env(),
    };

    let addr = config::addr_from_env();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("light-factory listening on http://{addr}");

    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
