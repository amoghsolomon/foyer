use async_trait::async_trait;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use rand_core::OsRng;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::error::KeyStoreError;
use crate::jwk::{EnrollmentMaterial, PublicJwk};

#[async_trait]
pub trait DeviceKeyStore: Send + Sync {
    async fn load_or_create(&self) -> Result<DeviceSigningKey, KeyStoreError>;
}

pub struct DeviceSigningKey {
    signing_key: SigningKey,
    material: EnrollmentMaterial,
}

impl DeviceSigningKey {
    pub fn generate() -> Result<Self, KeyStoreError> {
        Self::from_signing_key(SigningKey::random(&mut OsRng))
    }

    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self, KeyStoreError> {
        if bytes.len() != 32 {
            return Err(KeyStoreError::Corrupt);
        }
        let signing_key = SigningKey::from_slice(bytes).map_err(|_| KeyStoreError::Corrupt)?;
        Self::from_signing_key(signing_key)
    }

    fn from_signing_key(signing_key: SigningKey) -> Result<Self, KeyStoreError> {
        let jwk = PublicJwk::from_verifying_key(signing_key.verifying_key())
            .map_err(|_| KeyStoreError::Corrupt)?;
        Ok(Self {
            material: EnrollmentMaterial::new(jwk),
            signing_key,
        })
    }

    pub fn material(&self) -> &EnrollmentMaterial {
        &self.material
    }

    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        let bytes = self.signing_key.to_bytes();
        let mut secret = Zeroizing::new([0_u8; 32]);
        secret.copy_from_slice(bytes.as_slice());
        secret
    }

    pub fn sign_sha256(&self, payload: &[u8]) -> [u8; 64] {
        let signature: Signature = self.signing_key.sign(payload);
        let bytes = signature.to_bytes();
        let mut encoded = [0_u8; 64];
        encoded.copy_from_slice(&bytes);
        encoded
    }
}

impl std::fmt::Debug for DeviceSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSigningKey")
            .field("device_key_id", &self.material.device_key_id)
            .finish()
    }
}

#[derive(Default)]
pub struct MemoryKeyStore {
    key: Mutex<Option<DeviceSigningKey>>,
}

impl MemoryKeyStore {
    pub fn new() -> Self {
        Self {
            key: Mutex::new(None),
        }
    }

    pub fn with_key(key: DeviceSigningKey) -> Self {
        Self {
            key: Mutex::new(Some(key)),
        }
    }
}

#[async_trait]
impl DeviceKeyStore for MemoryKeyStore {
    async fn load_or_create(&self) -> Result<DeviceSigningKey, KeyStoreError> {
        let mut slot = self.key.lock().await;
        if slot.is_none() {
            *slot = Some(DeviceSigningKey::generate()?);
        }
        let existing = slot.as_ref().expect("memory key was just inserted");
        DeviceSigningKey::from_secret_bytes(existing.secret_bytes().as_ref())
    }
}

pub struct UnavailableKeyStore;

#[async_trait]
impl DeviceKeyStore for UnavailableKeyStore {
    async fn load_or_create(&self) -> Result<DeviceSigningKey, KeyStoreError> {
        Err(KeyStoreError::Unavailable)
    }
}

pub struct SecretServiceKeyStore;

#[async_trait]
impl DeviceKeyStore for SecretServiceKeyStore {
    async fn load_or_create(&self) -> Result<DeviceSigningKey, KeyStoreError> {
        tokio::task::spawn_blocking(crate::secret_service::load_or_create)
            .await
            .map_err(|_| KeyStoreError::Unavailable)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_reuses_the_same_public_key() {
        let store = MemoryKeyStore::new();
        let first = store.load_or_create().await.expect("generate");
        let second = store.load_or_create().await.expect("reload");
        assert_eq!(
            first.material().device_key_id,
            second.material().device_key_id
        );
        assert_eq!(first.material().jwk, second.material().jwk);
    }

    #[tokio::test]
    async fn unavailable_store_fails_visibly() {
        let error = UnavailableKeyStore
            .load_or_create()
            .await
            .expect_err("missing secret service");
        assert!(matches!(error, KeyStoreError::Unavailable));
        assert!(!error.to_string().to_ascii_lowercase().contains("private"));
    }

    #[test]
    fn corrupt_secret_is_rejected() {
        assert!(matches!(
            DeviceSigningKey::from_secret_bytes(&[1, 2, 3]),
            Err(KeyStoreError::Corrupt)
        ));
    }
}
