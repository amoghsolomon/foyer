use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type, Value};

use crate::error::KeyStoreError;
use crate::keystore::DeviceSigningKey;

const KEY_LABEL: &str = "Foyer Shell device signing key";
const CONTENT_TYPE: &str = "application/octet-stream";

#[zbus::proxy(
    interface = "org.freedesktop.Secret.Service",
    default_service = "org.freedesktop.secrets",
    default_path = "/org/freedesktop/secrets"
)]
trait Service {
    fn open_session(
        &self,
        algorithm: &str,
        input: &Value<'_>,
    ) -> zbus::Result<(OwnedValue, OwnedObjectPath)>;
    fn unlock(
        &self,
        objects: &[OwnedObjectPath],
    ) -> zbus::Result<(Vec<OwnedObjectPath>, OwnedObjectPath)>;
    fn read_alias(&self, name: &str) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Secret.Collection",
    default_service = "org.freedesktop.secrets"
)]
trait Collection {
    fn search_items(&self, attributes: HashMap<&str, &str>) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn create_item(
        &self,
        properties: HashMap<&str, Value<'_>>,
        secret: DbusSecret,
        replace: bool,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;
    #[zbus(property)]
    fn locked(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Secret.Item",
    default_service = "org.freedesktop.secrets"
)]
trait Item {
    fn get_secret(&self, session: &OwnedObjectPath) -> zbus::Result<DbusSecret>;
    #[zbus(property)]
    fn locked(&self) -> zbus::Result<bool>;
}

#[derive(Clone, Debug, Type, Serialize, Deserialize)]
struct DbusSecret {
    session: OwnedObjectPath,
    parameters: Vec<u8>,
    value: Vec<u8>,
    content_type: String,
}

pub fn load_or_create() -> Result<DeviceSigningKey, KeyStoreError> {
    let connection = Connection::session().map_err(|_| KeyStoreError::Unavailable)?;
    let service = ServiceProxyBlocking::new(&connection).map_err(|_| KeyStoreError::Unavailable)?;
    let (_output, session) = service
        .open_session("plain", &Value::from(""))
        .map_err(|_| KeyStoreError::Unavailable)?;
    let collection_path = service
        .read_alias("default")
        .map_err(|_| KeyStoreError::Unavailable)?;
    if collection_path.as_str() == "/" {
        return Err(KeyStoreError::Unavailable);
    }
    let collection = CollectionProxyBlocking::builder(&connection)
        .path(&collection_path)
        .map_err(|_| KeyStoreError::Unavailable)?
        .build()
        .map_err(|_| KeyStoreError::Unavailable)?;
    unlock_if_needed(&service, &collection, collection_path.clone())?;

    let attributes = attributes();
    let items = collection
        .search_items(attributes.clone())
        .map_err(|_| KeyStoreError::Unavailable)?;
    if let Some(item_path) = items.first() {
        let item = ItemProxyBlocking::builder(&connection)
            .path(item_path)
            .map_err(|_| KeyStoreError::Unavailable)?
            .build()
            .map_err(|_| KeyStoreError::Unavailable)?;
        if item.locked().unwrap_or(true) {
            return Err(KeyStoreError::Locked);
        }
        let secret = item
            .get_secret(&session)
            .map_err(|_| KeyStoreError::Locked)?;
        return DeviceSigningKey::from_secret_bytes(&secret.value);
    }

    let key = DeviceSigningKey::generate()?;
    let secret = DbusSecret {
        session,
        parameters: Vec::new(),
        value: key.secret_bytes().as_ref().to_vec(),
        content_type: CONTENT_TYPE.into(),
    };
    let mut properties = HashMap::new();
    properties.insert("org.freedesktop.Secret.Item.Label", Value::from(KEY_LABEL));
    properties.insert(
        "org.freedesktop.Secret.Item.Attributes",
        Value::from(attributes),
    );
    collection
        .create_item(properties, secret, true)
        .map_err(|_| {
            KeyStoreError::Failed(
                "Secret Service refused to store the Foyer Shell device signing key.".into(),
            )
        })?;
    Ok(key)
}

fn attributes() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("application", "foyer-shell"),
        ("xdg:schema", "org.foyer.DeviceSigningKey"),
        ("purpose", "device-signing-v1"),
    ])
}

fn unlock_if_needed(
    service: &ServiceProxyBlocking<'_>,
    collection: &CollectionProxyBlocking<'_>,
    collection_path: OwnedObjectPath,
) -> Result<(), KeyStoreError> {
    if !collection.locked().unwrap_or(true) {
        return Ok(());
    }
    let (unlocked, prompt) = service
        .unlock(&[collection_path])
        .map_err(|_| KeyStoreError::Locked)?;
    if prompt.as_str() != "/" || unlocked.is_empty() {
        return Err(KeyStoreError::Locked);
    }
    Ok(())
}
