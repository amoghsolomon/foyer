use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::error::{ApiError, ApiResult};

pub async fn connect(database_url: &str) -> Result<PgPool, String> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .map_err(|error| format!("failed to connect to PostgreSQL: {error}"))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|error| format!("failed to run database migrations: {error}"))?;
    Ok(pool)
}

pub async fn ping(pool: &PgPool) -> ApiResult<()> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .map(|_| ())
        .map_err(|error| ApiError::unavailable(format!("PostgreSQL is not ready: {error}")))
}
