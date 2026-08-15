//! Bounded CalDAV/CardDAV adapter for pinned Radicale 3.7.3.
//!
//! Radicale is the canonical store. PostgreSQL holds rebuildable collection
//! checkpoints and operation bindings only. First-party clients never receive
//! the service credential, and this module never opens Radicale's filesystem.

#![allow(dead_code)]

#[path = "dav/client.rs"]
mod client;
#[path = "dav/config.rs"]
mod config;
#[path = "dav/error.rs"]
mod error;
#[path = "dav/http.rs"]
mod http;
#[path = "dav/path.rs"]
mod path;
#[path = "dav/payload.rs"]
mod payload;
#[path = "dav/projector.rs"]
mod projector;
#[path = "dav/protocol.rs"]
mod protocol;
#[path = "dav/xml.rs"]
mod xml;

#[allow(unused_imports)]
pub use client::{
    DavClient, DavCollection, DavDiscovery, DavResource, DavWriteResult, NewAddressBook,
    NewCalendar, PutPrecondition, ResourceFetch, SyncChange, SyncPage,
};
#[allow(unused_imports)]
pub use config::{
    DavConfig, MAX_DISPLAY_NAME, MAX_MULTIGET, MAX_RESOURCE_BYTES, MAX_RESPONSE_BYTES,
    MAX_XML_BYTES, MAX_XML_DEPTH, MAX_XML_ELEMENTS, Secret, USER_AGENT,
};
pub use error::DavError;
#[allow(unused_imports)]
pub use path::{
    ADDRESSBOOKS_SEGMENT, CALENDARS_SEGMENT, CollectionKind, DavHref, HttpUrl, TASKS_SEGMENT,
    UserPaths, encode_path, percent_decode, resource_filename, validate_display_name,
    validate_segment,
};
#[allow(unused_imports)]
pub use payload::{ComponentKind, DavMediaType, DavPayload, PropertyUpdate};
#[allow(unused_imports)]
pub use projector::{
    BoundDavWrite, CollectionCheckpoint, OperationBinding, Projector, SyncPlan, failed_resources,
    successful_resources,
};
pub use protocol::{ETag, SyncToken};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_surface_does_not_expose_filesystem_types() {
        let paths = UserPaths::for_user("dev-user").unwrap();
        assert!(paths.principal.as_str().starts_with("/dev-user/"));
        assert!(!paths.principal.as_str().contains("data/collections"));
    }
}
