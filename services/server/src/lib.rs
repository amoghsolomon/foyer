use std::future::Future;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::Serialize;
use sqlx::PgPool;

use crate::auth::{AuthLimiter, Principal, TokenSigner, session_body, sync_credentials};
use crate::dav::{DavClient, DavConfig, Projector};
use crate::error::{ApiError, ApiResult};

pub mod auth;
pub mod bookmarks;
pub mod calendar;
pub mod config;
pub mod contacts;
pub mod dav;
pub mod db;
pub mod error;
pub mod notes;
pub mod tasks;

pub use config::Config;

pub const SERVICE_NAME: &str = "foyer-server";

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub pool: PgPool,
    pub signer: TokenSigner,
    pub auth_limiter: AuthLimiter,
    pub dav: Option<DavClient>,
    pub projector: Projector,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Health {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

impl Health {
    fn ok() -> Self {
        Self {
            status: "ok",
            service: SERVICE_NAME,
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn unavailable() -> Self {
        Self {
            status: "unavailable",
            service: SERVICE_NAME,
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl AppState {
    pub fn dav_client(&self) -> ApiResult<&DavClient> {
        self.dav.as_ref().ok_or_else(|| {
            ApiError::unavailable("DAV is not configured on this Foyer Server process.")
        })
    }
}

pub async fn app_state(config: Config) -> Result<AppState, String> {
    let signer = TokenSigner::from_config(&config)?;
    let pool = db::connect(&config.database_url).await?;
    ensure_known_users(&pool, &config).await?;
    let dav = match &config.dav {
        Some(settings) => {
            let dav_config = DavConfig::new(
                settings.base_url.clone(),
                settings.username.clone(),
                settings.password.clone(),
            )
            .map_err(|error| format!("invalid DAV configuration: {error}"))?;
            Some(DavClient::new(dav_config).map_err(|error| format!("DAV client: {error}"))?)
        }
        None => None,
    };
    let projector = Projector::new(pool.clone());
    let state = AppState {
        config,
        pool,
        signer,
        auth_limiter: AuthLimiter::new(),
        dav,
        projector,
    };
    if state.dav.is_some() {
        let background = state.clone();
        spawn_background(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if let Err(error) = reconcile_known_users(&background).await {
                    tracing::warn!(error = %error, "personal-data projector pass failed");
                }
            }
        });
    }
    Ok(state)
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .route("/v1/session", get(session))
        .route("/v1/sync/credentials", get(sync_credentials_handler))
        .route("/v1/auth/challenges", post(auth::create_challenge))
        .route("/v1/auth/sessions", post(auth::create_session))
        .route("/v1/auth/jwks", get(auth::jwks))
        .route("/v1/dev/jwks", get(auth::development_jwks))
        .route(
            "/v1/folders",
            get(notes::list_folders).post(notes::create_folder),
        )
        .route("/v1/folders/{folderId}", get(notes::get_folder))
        .route("/v1/folders/{folderId}/rename", post(notes::rename_folder))
        .route("/v1/folders/{folderId}/move", post(notes::move_folder))
        .route("/v1/folders/{folderId}/delete", post(notes::delete_folder))
        .route("/v1/notes", get(notes::list_notes).post(notes::create_note))
        .route("/v1/notes/{noteId}", get(notes::get_note))
        .route("/v1/notes/{noteId}/update", post(notes::update_note))
        .route("/v1/notes/{noteId}/move", post(notes::move_note))
        .route("/v1/notes/{noteId}/delete", post(notes::delete_note))
        .merge(tasks::routes())
        .merge(contacts::routes())
        .merge(calendar::routes())
        .merge(bookmarks::routes())
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}

async fn ensure_known_users(pool: &PgPool, config: &Config) -> Result<(), String> {
    for user in &config.dev_users {
        sqlx::query(
            "INSERT INTO foyer_users (id, created_at, updated_at)
             VALUES ($1, now(), now())
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&user.user_id)
        .execute(pool)
        .await
        .map_err(|error| format!("failed to record the development user: {error}"))?;
    }
    Ok(())
}

async fn reconcile_known_users(state: &AppState) -> Result<(), String> {
    let mut users = state
        .config
        .dev_users
        .iter()
        .map(|user| user.user_id.clone())
        .collect::<Vec<_>>();
    let persisted = sqlx::query_scalar::<_, String>("SELECT id FROM foyer_users")
        .fetch_all(&state.pool)
        .await
        .map_err(|error| error.to_string())?;
    for user_id in persisted {
        if !users.iter().any(|existing| existing == &user_id) {
            users.push(user_id);
        }
    }
    let projected = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT user_id FROM dav_collection_checkpoints
         UNION
         SELECT DISTINCT user_id FROM task_lists
         UNION
         SELECT DISTINCT user_id FROM contacts_address_books
         UNION
         SELECT DISTINCT user_id FROM calendar_calendars",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| error.to_string())?;
    for user_id in projected {
        if !users.iter().any(|existing| existing == &user_id) {
            users.push(user_id);
        }
    }
    for user_id in users {
        if let Err(error) = tasks::reconcile_user(state, &user_id).await {
            tracing::warn!(user_id, error = %error, "task projection failed");
        }
        if let Err(error) = contacts::reconcile_user(state, &user_id).await {
            tracing::warn!(user_id, error = %error, "contact projection failed");
        }
        if let Err(error) = calendar::reconcile_user(state, &user_id).await {
            tracing::warn!(user_id, error = %error, "calendar projection failed");
        }
    }
    Ok(())
}

async fn liveness() -> Json<Health> {
    Json(Health::ok())
}

async fn readiness(
    State(state): State<AppState>,
) -> Result<Json<Health>, (StatusCode, Json<Health>)> {
    db::ping(&state.pool)
        .await
        .map(|_| Json(Health::ok()))
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, Json(Health::unavailable())))
}

async fn session(State(state): State<AppState>, principal: Principal) -> Json<auth::SessionBody> {
    Json(session_body(&state.config, &principal))
}

async fn sync_credentials_handler(
    State(state): State<AppState>,
    principal: Principal,
) -> error::ApiResult<Json<auth::SyncCredentialsBody>> {
    sync_credentials(&state.config, &state.signer, &principal).map(Json)
}

pub fn spawn_background<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(future);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample_config() -> Config {
        Config::test_development("postgres://foyer:foyer@127.0.0.1:5432/foyer")
    }

    #[test]
    fn default_configuration_binds_only_to_loopback() {
        assert_eq!(sample_config().bind, "127.0.0.1:3583".parse().unwrap());
    }

    #[tokio::test]
    async fn liveness_payload_is_stable() {
        let Json(payload) = liveness().await;
        assert_eq!(payload.status, "ok");
        assert_eq!(payload.service, SERVICE_NAME);
        assert_eq!(payload.version, env!("CARGO_PKG_VERSION"));
    }
}
