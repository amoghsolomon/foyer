//! Stable projection IDs retain existing client UUIDs and isolate foreign DAV UID collisions.
use sha2::{Digest, Sha256};
use sqlx::PgPool;

pub fn scoped_id(domain: &str, user_id: &str, identity: &str) -> String {
    let digest = Sha256::digest(format!("foyer:{domain}\0{user_id}\0{identity}").as_bytes());
    let mut bytes: [u8; 16] = digest[..16].try_into().expect("SHA-256 prefix");
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

pub async fn owned_id(
    pool: &PgPool,
    table: &str,
    user_id: &str,
    candidate: &str,
) -> Result<String, sqlx::Error> {
    // Call sites select a fixed server-owned table name, never request input.
    assert!(matches!(
        table,
        "tasks" | "task_lists" | "contacts" | "contacts_address_books" | "calendar_calendars"
    ));
    let owner =
        sqlx::query_scalar::<_, String>(&format!("SELECT user_id FROM {table} WHERE id = $1"))
            .bind(candidate)
            .fetch_optional(pool)
            .await?;
    Ok(if owner.as_deref().is_some_and(|owner| owner != user_id) {
        scoped_id(table, user_id, candidate)
    } else {
        candidate.to_owned()
    })
}

/// This transaction deliberately commits before any DAV request is sent.
pub async fn bind_intent(
    pool: &PgPool,
    user_id: &str,
    operation_id: &str,
    entity_type: &str,
    entity_id: &str,
    operation: &str,
    request_body: &serde_json::Value,
) -> Result<(), crate::error::ApiError> {
    use crate::error::ApiError;
    let stored = sqlx::query_as::<_, (String, String, String, String, serde_json::Value)>(
        "INSERT INTO dav_mutation_intents (operation_id,user_id,entity_type,entity_id,operation,request_body)
         VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (operation_id) DO UPDATE SET operation_id = EXCLUDED.operation_id
         RETURNING user_id,entity_type,entity_id,operation,request_body")
        .bind(operation_id).bind(user_id).bind(entity_type).bind(entity_id).bind(operation).bind(request_body)
        .fetch_one(pool).await.map_err(|_| ApiError::unavailable("Failed to bind DAV operation intent."))?;
    if stored
        != (
            user_id.to_string(),
            entity_type.to_string(),
            entity_id.to_string(),
            operation.to_string(),
            request_body.clone(),
        )
    {
        return Err(ApiError::conflict(
            "This operation id is already bound to a different request.",
        ));
    }
    Ok(())
}
