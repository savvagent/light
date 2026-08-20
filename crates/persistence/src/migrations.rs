//! Database migrations, applied at startup.

use sqlx::PgPool;

/// Apply all pending migrations.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
