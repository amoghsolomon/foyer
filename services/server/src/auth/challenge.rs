use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, Utc};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

use super::jwk::{PublicJwk, parse_public_jwk, verify_p1363};
use super::rate::{CHALLENGE_TTL_SECS, MAX_OUTSTANDING_CHALLENGES};
use super::tokens::TokenSigner;

pub const CHALLENGE_DOMAIN: &[u8] = b"FOYER-AUTH-CHALLENGE-V1";
pub const NONCE_LEN: usize = 32;

#[derive(Debug, Deserialize)]
pub struct CreateChallengeRequest {
    #[serde(rename = "deviceKeyId")]
    pub device_key_id: String,
}

#[derive(Debug, Serialize)]
pub struct ChallengeBody {
    #[serde(rename = "challengeId")]
    pub challenge_id: String,
    #[serde(rename = "signingPayload")]
    pub signing_payload: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    #[serde(rename = "challengeId")]
    pub challenge_id: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct SessionIssuedBody {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "tokenType")]
    pub token_type: &'static str,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "deviceKeyId")]
    pub device_key_id: String,
}

#[derive(FromRow)]
struct LiveDevice {
    user_id: String,
    #[allow(dead_code)]
    public_jwk: serde_json::Value,
}

#[derive(FromRow)]
struct ChallengeRow {
    device_key_id: String,
    user_id: String,
    signing_payload: Vec<u8>,
    payload_sha256: Vec<u8>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    public_jwk: serde_json::Value,
    revoked_at: Option<DateTime<Utc>>,
}

pub fn build_signing_payload(
    device_key_id: &str,
    api_audience: &str,
    expires_at: DateTime<Utc>,
    nonce: &[u8],
) -> Vec<u8> {
    let expires = expires_at.to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut payload = Vec::with_capacity(
        CHALLENGE_DOMAIN.len()
            + device_key_id.len()
            + api_audience.len()
            + expires.len()
            + nonce.len()
            + 4,
    );
    payload.extend_from_slice(CHALLENGE_DOMAIN);
    payload.push(0);
    payload.extend_from_slice(device_key_id.as_bytes());
    payload.push(0);
    payload.extend_from_slice(api_audience.as_bytes());
    payload.push(0);
    payload.extend_from_slice(expires.as_bytes());
    payload.push(0);
    payload.extend_from_slice(nonce);
    payload
}

pub fn random_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

pub async fn issue_challenge(
    pool: &PgPool,
    signer: &TokenSigner,
    device_key_id: &str,
) -> ApiResult<ChallengeBody> {
    let expires_at = Utc::now() + chrono::Duration::seconds(CHALLENGE_TTL_SECS);
    let nonce = random_nonce();
    let payload = build_signing_payload(device_key_id, &signer.api_audience, expires_at, &nonce);
    let device = live_device(pool, device_key_id).await?;
    let Some(device) = device else {
        return Ok(dummy_challenge(device_key_id, signer, expires_at, &payload));
    };

    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ApiError::unavailable("failed to start an authentication transaction"))?;
    cleanup_expired(&mut tx).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(device_key_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::unavailable("failed to serialize authentication challenges"))?;
    let outstanding = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM auth_challenges
         WHERE device_key_id = $1 AND consumed_at IS NULL AND expires_at > now()",
    )
    .bind(device_key_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::unavailable("failed to count outstanding challenges"))?;
    if outstanding >= MAX_OUTSTANDING_CHALLENGES {
        return Err(ApiError::rate_limited());
    }

    let challenge_id = Uuid::new_v4().to_string();
    let digest = Sha256::digest(&payload).to_vec();
    sqlx::query(
        "INSERT INTO auth_challenges (
            challenge_id, device_key_id, user_id, signing_payload, payload_sha256,
            expires_at, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, now())",
    )
    .bind(&challenge_id)
    .bind(device_key_id)
    .bind(&device.user_id)
    .bind(&payload)
    .bind(&digest)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::unavailable("failed to persist the authentication challenge"))?;
    record_audit(
        &mut tx,
        "challenge_issued",
        Some(&device.user_id),
        Some(device_key_id),
        Some(&challenge_id),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::unavailable("failed to commit the authentication challenge"))?;

    Ok(ChallengeBody {
        challenge_id,
        signing_payload: URL_SAFE_NO_PAD.encode(payload),
        expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
    })
}

pub async fn redeem_session(
    pool: &PgPool,
    signer: &TokenSigner,
    challenge_id: &str,
    signature: &str,
) -> ApiResult<SessionIssuedBody> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ApiError::unavailable("failed to start an authentication transaction"))?;
    let row = sqlx::query_as::<_, ChallengeRow>(
        "SELECT c.device_key_id, c.user_id, c.signing_payload, c.payload_sha256,
                c.expires_at, c.consumed_at,
                d.public_jwk, d.revoked_at
         FROM auth_challenges c
         JOIN device_keys d ON d.device_key_id = c.device_key_id
         WHERE c.challenge_id = $1
         FOR UPDATE OF c, d",
    )
    .bind(challenge_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::unavailable("failed to load the authentication challenge"))?;

    let Some(row) = row else {
        record_audit(&mut tx, "session_rejected", None, None, Some(challenge_id)).await?;
        tx.commit()
            .await
            .map_err(|_| ApiError::unavailable("failed to record an authentication rejection"))?;
        return Err(ApiError::authentication_failed());
    };

    let now = Utc::now();
    let jwk = parse_stored_jwk(&row.public_jwk);
    let payload_hash_matches =
        Sha256::digest(&row.signing_payload).as_slice() == row.payload_sha256;
    let accepted = row.consumed_at.is_none()
        && row.revoked_at.is_none()
        && row.expires_at > now
        && payload_hash_matches
        && jwk
            .as_ref()
            .is_some_and(|jwk| verify_p1363(jwk, &row.signing_payload, signature).is_ok());

    if !accepted {
        record_audit(
            &mut tx,
            "session_rejected",
            Some(&row.user_id),
            Some(&row.device_key_id),
            Some(challenge_id),
        )
        .await?;
        tx.commit()
            .await
            .map_err(|_| ApiError::unavailable("failed to record an authentication rejection"))?;
        return Err(ApiError::authentication_failed());
    }

    let consumed = sqlx::query(
        "UPDATE auth_challenges
         SET consumed_at = now()
         WHERE challenge_id = $1 AND consumed_at IS NULL AND expires_at > now()",
    )
    .bind(challenge_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::unavailable("failed to consume the authentication challenge"))?;
    if consumed.rows_affected() != 1 {
        record_audit(
            &mut tx,
            "session_rejected",
            Some(&row.user_id),
            Some(&row.device_key_id),
            Some(challenge_id),
        )
        .await?;
        tx.commit()
            .await
            .map_err(|_| ApiError::unavailable("failed to record an authentication rejection"))?;
        return Err(ApiError::authentication_failed());
    }

    sqlx::query(
        "UPDATE device_keys
         SET last_seen_at = now(), updated_at = now()
         WHERE device_key_id = $1 AND revoked_at IS NULL",
    )
    .bind(&row.device_key_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::unavailable("failed to update device last-seen"))?;

    let issued = signer.issue_access(&row.user_id, Some(&row.device_key_id))?;
    record_audit(
        &mut tx,
        "session_issued",
        Some(&row.user_id),
        Some(&row.device_key_id),
        Some(challenge_id),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|_| ApiError::unavailable("failed to commit the authenticated session"))?;

    Ok(SessionIssuedBody {
        access_token: issued.access_token,
        token_type: "Bearer",
        expires_at: issued.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        user_id: row.user_id,
        device_key_id: row.device_key_id,
    })
}

fn dummy_challenge(
    device_key_id: &str,
    signer: &TokenSigner,
    expires_at: DateTime<Utc>,
    payload: &[u8],
) -> ChallengeBody {
    let _ = (device_key_id, signer);
    ChallengeBody {
        challenge_id: Uuid::new_v4().to_string(),
        signing_payload: URL_SAFE_NO_PAD.encode(payload),
        expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
    }
}

async fn live_device(pool: &PgPool, device_key_id: &str) -> ApiResult<Option<LiveDevice>> {
    sqlx::query_as::<_, LiveDevice>(
        "SELECT user_id, public_jwk FROM device_keys
         WHERE device_key_id = $1 AND revoked_at IS NULL",
    )
    .bind(device_key_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::unavailable("failed to load the enrolled device"))
}

async fn cleanup_expired(tx: &mut Transaction<'_, Postgres>) -> ApiResult<()> {
    sqlx::query("DELETE FROM auth_challenges WHERE expires_at < now() - interval '10 minutes'")
        .execute(&mut **tx)
        .await
        .map_err(|_| ApiError::unavailable("failed to expire authentication challenges"))?;
    Ok(())
}

async fn record_audit(
    tx: &mut Transaction<'_, Postgres>,
    event_type: &str,
    user_id: Option<&str>,
    device_key_id: Option<&str>,
    challenge_id: Option<&str>,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO auth_audit_events (event_type, user_id, device_key_id, challenge_id, created_at)
         VALUES ($1, $2, $3, $4, now())",
    )
    .bind(event_type)
    .bind(user_id)
    .bind(device_key_id)
    .bind(challenge_id)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApiError::unavailable("failed to record an authentication audit event"))?;
    Ok(())
}

fn parse_stored_jwk(value: &serde_json::Value) -> Option<PublicJwk> {
    parse_public_jwk(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn signing_payload_is_domain_separated_and_bound() {
        let expires = Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap();
        let nonce = [0x11_u8; 32];
        let payload = build_signing_payload(
            "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s",
            "foyer-api",
            expires,
            &nonce,
        );
        assert!(payload.starts_with(CHALLENGE_DOMAIN));
        assert!(payload.windows(32).any(|window| window == nonce));
        assert!(payload.windows(9).any(|window| window == b"foyer-api"));
        assert!(
            payload
                .windows(20)
                .any(|window| window == b"2026-01-01T00:01:00Z")
        );
        assert!(payload.len() >= CHALLENGE_DOMAIN.len() + 32 + 4);
    }
}
