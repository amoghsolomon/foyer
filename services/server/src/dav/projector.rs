use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use super::client::{DavClient, DavCollection, DavResource, ResourceFetch, SyncChange, SyncPage};
use super::error::DavError;
use super::path::{CollectionKind, DavHref};
use super::protocol::{ETag, SyncToken};

#[derive(Clone, Debug, Eq, PartialEq, FromRow)]
pub struct CollectionCheckpoint {
    pub user_id: String,
    pub collection_href: String,
    pub collection_kind: String,
    pub collection_id: String,
    pub display_name: Option<String>,
    pub sync_token: Option<String>,
    pub collection_etag: Option<String>,
    pub last_error: Option<String>,
    pub last_projected_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CollectionCheckpoint {
    pub fn href(&self) -> Result<DavHref, DavError> {
        DavHref::parse(&self.collection_href)
    }

    pub fn kind(&self) -> Result<CollectionKind, DavError> {
        CollectionKind::parse(&self.collection_kind)
    }

    pub fn token(&self) -> Result<Option<SyncToken>, DavError> {
        self.sync_token.as_deref().map(SyncToken::parse).transpose()
    }
}

#[derive(Clone, Debug)]
pub struct SyncPlan {
    pub checkpoint: CollectionCheckpoint,
    pub page: SyncPage,
    pub resources: Vec<ResourceFetch>,
}

#[derive(Clone, Debug)]
pub struct OperationBinding {
    pub user_id: String,
    pub operation_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub request_body: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundDavWrite {
    pub href: String,
    pub etag: Option<String>,
    pub uid: Option<String>,
    pub created: bool,
}

#[derive(Clone, Debug)]
pub struct Projector {
    pool: PgPool,
}

impl Projector {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn load_checkpoint(
        &self,
        user_id: &str,
        href: &DavHref,
    ) -> Result<Option<CollectionCheckpoint>, DavError> {
        sqlx::query_as::<_, CollectionCheckpoint>(
            "SELECT user_id, collection_href, collection_kind, collection_id, display_name,
                    sync_token, collection_etag, last_error, last_projected_at, created_at, updated_at
             FROM dav_collection_checkpoints
             WHERE user_id = $1 AND collection_href = $2",
        )
        .bind(user_id)
        .bind(href.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)
    }

    pub async fn list_checkpoints(
        &self,
        user_id: &str,
    ) -> Result<Vec<CollectionCheckpoint>, DavError> {
        sqlx::query_as::<_, CollectionCheckpoint>(
            "SELECT user_id, collection_href, collection_kind, collection_id, display_name,
                    sync_token, collection_etag, last_error, last_projected_at, created_at, updated_at
             FROM dav_collection_checkpoints
             WHERE user_id = $1
             ORDER BY collection_kind, collection_id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)
    }

    pub async fn remember_collection(
        &self,
        user_id: &str,
        collection_id: &str,
        collection: &DavCollection,
    ) -> Result<CollectionCheckpoint, DavError> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO dav_collection_checkpoints
                (user_id, collection_href, collection_kind, collection_id, display_name,
                 sync_token, collection_etag, last_error, last_projected_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, NULL, $6, NULL, NULL, $7, $7)
             ON CONFLICT (user_id, collection_href) DO UPDATE SET
                collection_kind = EXCLUDED.collection_kind,
                collection_id = EXCLUDED.collection_id,
                display_name = EXCLUDED.display_name,
                collection_etag = EXCLUDED.collection_etag,
                updated_at = EXCLUDED.updated_at",
        )
        .bind(user_id)
        .bind(collection.href.as_str())
        .bind(collection.kind.as_str())
        .bind(collection_id)
        .bind(collection.display_name.as_deref())
        .bind(collection.etag.as_ref().map(ETag::as_str))
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        self.load_checkpoint(user_id, &collection.href)
            .await?
            .ok_or_else(|| DavError::Unavailable("checkpoint disappeared after upsert".into()))
    }

    pub async fn plan_sync(
        &self,
        client: &DavClient,
        user_id: &str,
        href: &DavHref,
    ) -> Result<SyncPlan, DavError> {
        let checkpoint = self.load_checkpoint(user_id, href).await?.ok_or_else(|| {
            DavError::NotFound("no DAV checkpoint exists for this collection".into())
        })?;
        let token = checkpoint.token()?;
        let page = client
            .sync_collection(user_id, href, token.as_ref())
            .await?;
        let kind = checkpoint.kind()?;
        let hrefs = page
            .upserts
            .iter()
            .map(|change| change.href.clone())
            .collect::<Vec<_>>();
        let resources = client.fetch_resources(user_id, kind, &hrefs).await?;
        Ok(SyncPlan {
            checkpoint,
            page,
            resources,
        })
    }

    pub async fn commit_checkpoint(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: &str,
        href: &DavHref,
        sync_token: Option<&SyncToken>,
        collection_etag: Option<&ETag>,
    ) -> Result<(), DavError> {
        let now = Utc::now();
        let result = sqlx::query(
            "UPDATE dav_collection_checkpoints
             SET sync_token = $3,
                 collection_etag = COALESCE($4, collection_etag),
                 last_error = NULL,
                 last_projected_at = $5,
                 updated_at = $5
             WHERE user_id = $1 AND collection_href = $2",
        )
        .bind(user_id)
        .bind(href.as_str())
        .bind(sync_token.map(SyncToken::as_str))
        .bind(collection_etag.map(ETag::as_str))
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(database_error)?;
        if result.rows_affected() == 0 {
            return Err(DavError::NotFound(
                "cannot advance a missing DAV checkpoint".into(),
            ));
        }
        Ok(())
    }

    pub async fn record_collection_error(
        &self,
        user_id: &str,
        href: &DavHref,
        error: &DavError,
    ) -> Result<(), DavError> {
        sqlx::query(
            "UPDATE dav_collection_checkpoints
             SET last_error = $3, updated_at = $4
             WHERE user_id = $1 AND collection_href = $2",
        )
        .bind(user_id)
        .bind(href.as_str())
        .bind(error.public_message())
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    pub async fn reset_user_checkpoints(&self, user_id: &str) -> Result<u64, DavError> {
        let result = sqlx::query("DELETE FROM dav_collection_checkpoints WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(result.rows_affected())
    }

    pub async fn reset_all_checkpoints(&self) -> Result<u64, DavError> {
        let result = sqlx::query("DELETE FROM dav_collection_checkpoints")
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(result.rows_affected())
    }

    pub async fn with_operation<T, F>(
        &self,
        binding: OperationBinding,
        work: F,
    ) -> Result<T, DavError>
    where
        T: Serialize + DeserializeOwned + Send,
        F: for<'c> FnOnce(
            &'c mut Transaction<'_, Postgres>,
        )
            -> Pin<Box<dyn Future<Output = Result<T, DavError>> + Send + 'c>>,
    {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&binding.operation_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 1))")
            .bind(&binding.user_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        if let Some(stored) = load_binding(&mut tx, &binding.operation_id).await? {
            if stored.user_id != binding.user_id
                || stored.entity_type != binding.entity_type
                || stored.entity_id != binding.entity_id
                || stored.operation != binding.operation
                || stored.request_body != binding.request_body
            {
                return Err(DavError::OperationConflict);
            }
            tx.commit().await.map_err(database_error)?;
            if stored.result_status != 200 {
                return Err(DavError::Conflict(
                    "this DAV operation id already produced a non-success result".into(),
                ));
            }
            return serde_json::from_value(stored.result_body).map_err(|error| {
                DavError::Unavailable(format!("stored DAV operation payload is invalid: {error}"))
            });
        }
        let result = work(&mut tx).await?;
        let body = serde_json::to_value(&result).map_err(|error| {
            DavError::Unavailable(format!("failed to store DAV operation: {error}"))
        })?;
        let write = serde_json::from_value::<BoundDavWrite>(body.clone()).ok();
        sqlx::query(
            "INSERT INTO dav_operation_bindings
                (operation_id, user_id, entity_type, entity_id, operation, request_body,
                 collection_href, resource_href, etag, dav_uid, result_status, result_body, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 200, $11, $12)",
        )
        .bind(&binding.operation_id)
        .bind(&binding.user_id)
        .bind(&binding.entity_type)
        .bind(&binding.entity_id)
        .bind(&binding.operation)
        .bind(&binding.request_body)
        .bind(write.as_ref().and_then(|item| parent_href(&item.href)))
        .bind(write.as_ref().map(|item| item.href.as_str()))
        .bind(write.as_ref().and_then(|item| item.etag.as_deref()))
        .bind(write.as_ref().and_then(|item| item.uid.as_deref()))
        .bind(body)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
        tx.commit().await.map_err(database_error)?;
        Ok(result)
    }
}

impl BoundDavWrite {
    pub fn from_resource(resource: &DavResource, created: bool) -> Self {
        Self {
            href: resource.href.as_str().to_string(),
            etag: Some(resource.etag.as_str().to_string()),
            uid: resource.payload.uid(),
            created,
        }
    }
}

#[derive(Debug, FromRow)]
struct StoredBinding {
    user_id: String,
    entity_type: String,
    entity_id: String,
    operation: String,
    request_body: Value,
    result_status: i32,
    result_body: Value,
}

async fn load_binding(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: &str,
) -> Result<Option<StoredBinding>, DavError> {
    sqlx::query_as::<_, StoredBinding>(
        "SELECT user_id, entity_type, entity_id, operation, request_body, result_status, result_body
         FROM dav_operation_bindings
         WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

fn parent_href(href: &str) -> Option<String> {
    DavHref::parse(href)
        .ok()
        .and_then(|href| href.parent())
        .map(|parent| parent.as_str().to_string())
}

fn database_error(error: sqlx::Error) -> DavError {
    DavError::Unavailable(format!("database error: {error}"))
}

pub fn successful_resources(fetches: &[ResourceFetch]) -> Vec<&DavResource> {
    fetches
        .iter()
        .filter_map(|fetch| fetch.result.as_ref().ok())
        .collect()
}

pub fn failed_resources(fetches: &[ResourceFetch]) -> Vec<(&DavHref, &DavError)> {
    fetches
        .iter()
        .filter_map(|fetch| {
            fetch
                .result
                .as_ref()
                .err()
                .map(|error| (&fetch.href, error))
        })
        .collect()
}

pub fn upsert_hrefs(page: &SyncPage) -> Vec<&DavHref> {
    page.upserts
        .iter()
        .map(|change: &SyncChange| &change.href)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_write_round_trips() {
        let write = BoundDavWrite {
            href: "/alice/calendars/home/event.ics".into(),
            etag: Some("\"1\"".into()),
            uid: Some("event-1".into()),
            created: true,
        };
        let value = serde_json::to_value(&write).unwrap();
        let parsed: BoundDavWrite = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, write);
    }
}
