use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{FromRow, PgPool};

use super::jwk::{PublicJwk, parse_public_jwk_bytes};

const MAX_USER_ID: usize = 128;
const MAX_LABEL: usize = 80;

#[derive(Debug, Eq, PartialEq)]
pub enum DeviceAddStatus {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug)]
pub struct AddedDevice {
    pub user_id: String,
    pub device_key_id: String,
    pub label: String,
    pub status: DeviceAddStatus,
}

#[derive(Debug, FromRow)]
pub struct DeviceRecord {
    pub device_key_id: String,
    pub user_id: String,
    pub label: String,
    pub public_jwk: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DeviceRevokeStatus {
    Revoked,
    AlreadyRevoked,
}

#[derive(Debug)]
pub struct RevokedDevice {
    pub device_key_id: String,
    pub user_id: String,
    pub label: String,
    pub status: DeviceRevokeStatus,
}

pub fn validate_user_id(user_id: &str) -> Result<String, String> {
    let user_id = user_id.trim();
    if user_id.is_empty() || user_id.len() > MAX_USER_ID {
        return Err(format!(
            "user id must be 1..{MAX_USER_ID} characters after trimming"
        ));
    }
    if !user_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':'))
    {
        return Err(
            "user id may contain only ASCII letters, digits, '.', '_', '-', and ':'".into(),
        );
    }
    Ok(user_id.to_string())
}

pub fn validate_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() || label.len() > MAX_LABEL {
        return Err(format!(
            "device label must be 1..{MAX_LABEL} characters after trimming"
        ));
    }
    Ok(label.to_string())
}

pub fn public_jwk_from_bytes(bytes: &[u8]) -> Result<PublicJwk, String> {
    parse_public_jwk_bytes(bytes)
}

pub async fn add_device(
    pool: &PgPool,
    user_id: &str,
    label: &str,
    jwk: &PublicJwk,
) -> Result<AddedDevice, String> {
    let user_id = validate_user_id(user_id)?;
    let label = validate_label(label)?;
    let device_key_id = jwk.device_key_id();
    let public_jwk = jwk.to_value();

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("failed to start a device enrollment transaction: {error}"))?;
    sqlx::query(
        "INSERT INTO foyer_users (id, created_at, updated_at)
         VALUES ($1, now(), now())
         ON CONFLICT (id) DO UPDATE SET updated_at = foyer_users.updated_at",
    )
    .bind(&user_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("failed to ensure the Foyer user exists: {error}"))?;

    let existing = sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_key_id, user_id, label, public_jwk, created_at, updated_at,
                last_seen_at, revoked_at
         FROM device_keys
         WHERE device_key_id = $1
         FOR UPDATE",
    )
    .bind(&device_key_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("failed to look up the device key: {error}"))?;

    let status = if let Some(existing) = existing {
        if existing.user_id != user_id {
            return Err(format!(
                "device {device_key_id} is already enrolled for a different user"
            ));
        }
        if existing.public_jwk != public_jwk {
            return Err(format!(
                "device {device_key_id} is already enrolled with a different public JWK"
            ));
        }
        if existing.label == label && existing.revoked_at.is_none() {
            DeviceAddStatus::Unchanged
        } else {
            sqlx::query(
                "UPDATE device_keys
                 SET label = $2, revoked_at = NULL, updated_at = now()
                 WHERE device_key_id = $1",
            )
            .bind(&device_key_id)
            .bind(&label)
            .execute(&mut *tx)
            .await
            .map_err(|error| format!("failed to update the enrolled device: {error}"))?;
            DeviceAddStatus::Updated
        }
    } else {
        sqlx::query(
            "INSERT INTO device_keys (
                device_key_id, user_id, label, public_jwk, created_at, updated_at
             ) VALUES ($1, $2, $3, $4, now(), now())",
        )
        .bind(&device_key_id)
        .bind(&user_id)
        .bind(&label)
        .bind(&public_jwk)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("failed to enroll the device: {error}"))?;
        DeviceAddStatus::Created
    };

    if status != DeviceAddStatus::Unchanged {
        sqlx::query(
            "INSERT INTO auth_audit_events (event_type, user_id, device_key_id, created_at)
             VALUES ('device_enrolled', $1, $2, now())",
        )
        .bind(&user_id)
        .bind(&device_key_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("failed to record device enrollment: {error}"))?;
    }

    tx.commit()
        .await
        .map_err(|error| format!("failed to commit device enrollment: {error}"))?;

    Ok(AddedDevice {
        user_id,
        device_key_id,
        label,
        status,
    })
}

pub async fn list_devices(
    pool: &PgPool,
    user_id: Option<&str>,
) -> Result<Vec<DeviceRecord>, String> {
    let user_id = match user_id {
        Some(user_id) => Some(validate_user_id(user_id)?),
        None => None,
    };
    let rows = if let Some(user_id) = user_id {
        sqlx::query_as::<_, DeviceRecord>(
            "SELECT device_key_id, user_id, label, public_jwk, created_at, updated_at,
                    last_seen_at, revoked_at
             FROM device_keys
             WHERE user_id = $1
             ORDER BY created_at ASC, device_key_id ASC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, DeviceRecord>(
            "SELECT device_key_id, user_id, label, public_jwk, created_at, updated_at,
                    last_seen_at, revoked_at
             FROM device_keys
             ORDER BY user_id ASC, created_at ASC, device_key_id ASC",
        )
        .fetch_all(pool)
        .await
    };
    rows.map_err(|error| format!("failed to list devices: {error}"))
}

pub async fn revoke_device(pool: &PgPool, device_key_id: &str) -> Result<RevokedDevice, String> {
    let device_key_id = device_key_id.trim();
    if device_key_id.is_empty() {
        return Err("device key id is required".into());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("failed to start a device revocation transaction: {error}"))?;
    let existing = sqlx::query_as::<_, DeviceRecord>(
        "SELECT device_key_id, user_id, label, public_jwk, created_at, updated_at,
                last_seen_at, revoked_at
         FROM device_keys
         WHERE device_key_id = $1
         FOR UPDATE",
    )
    .bind(device_key_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| format!("failed to look up the device key: {error}"))?;
    let Some(existing) = existing else {
        return Err(format!("device {device_key_id} is not enrolled"));
    };
    let status = if existing.revoked_at.is_some() {
        DeviceRevokeStatus::AlreadyRevoked
    } else {
        sqlx::query(
            "UPDATE device_keys
             SET revoked_at = now(), updated_at = now()
             WHERE device_key_id = $1 AND revoked_at IS NULL",
        )
        .bind(device_key_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("failed to revoke the device: {error}"))?;
        sqlx::query(
            "INSERT INTO auth_audit_events (event_type, user_id, device_key_id, created_at)
             VALUES ('device_revoked', $1, $2, now())",
        )
        .bind(&existing.user_id)
        .bind(device_key_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("failed to record device revocation: {error}"))?;
        DeviceRevokeStatus::Revoked
    };
    tx.commit()
        .await
        .map_err(|error| format!("failed to commit device revocation: {error}"))?;
    Ok(RevokedDevice {
        device_key_id: existing.device_key_id,
        user_id: existing.user_id,
        label: existing.label,
        status,
    })
}
