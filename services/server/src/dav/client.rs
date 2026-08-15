use super::config::DavConfig;
use super::error::DavError;
use super::http::{HttpRequest, HttpResponse, exchange};
use super::path::{CollectionKind, DavHref, UserPaths, validate_display_name, validate_segment};
use super::payload::{DavMediaType, DavPayload};
use super::protocol::{
    ETag, MultiStatus, PropResponse, ResourceType, SyncToken, XML_CONTENT_TYPE,
    addressbook_multiget_xml, calendar_multiget_xml, classify_response, extract_dav_error,
    mkcalendar_xml, mkcol_addressbook_xml, mkcol_xml, parse_multistatus, propfind_collection,
    propfind_discovery, proppatch_xml, sync_collection_xml,
};

#[derive(Clone, Debug)]
pub struct DavClient {
    config: DavConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavDiscovery {
    pub user_id: String,
    pub principal: DavHref,
    pub calendar_home: DavHref,
    pub addressbook_home: DavHref,
    pub collections: Vec<DavCollection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavCollection {
    pub href: DavHref,
    pub kind: CollectionKind,
    pub display_name: Option<String>,
    pub etag: Option<ETag>,
    pub sync_token: Option<SyncToken>,
    pub supported_components: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewCalendar {
    pub collection_id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAddressBook {
    pub collection_id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PutPrecondition {
    IfNoneMatchStar,
    IfMatch(ETag),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavResource {
    pub href: DavHref,
    pub etag: ETag,
    pub payload: DavPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceFetch {
    pub href: DavHref,
    pub result: Result<DavResource, DavError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavWriteResult {
    pub href: DavHref,
    pub etag: Option<ETag>,
    pub created: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncPage {
    pub upserts: Vec<SyncChange>,
    pub deletions: Vec<DavHref>,
    pub sync_token: Option<SyncToken>,
    pub token_reset: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncChange {
    pub href: DavHref,
    pub etag: Option<ETag>,
}

impl DavClient {
    pub fn new(config: DavConfig) -> Result<Self, DavError> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &DavConfig {
        &self.config
    }

    pub fn user_paths(&self, user_id: &str) -> Result<UserPaths, DavError> {
        UserPaths::for_user(user_id)
    }

    pub async fn discover(&self, user_id: &str) -> Result<DavDiscovery, DavError> {
        let paths = self.user_paths(user_id)?;
        let well_known = DavHref::parse("/.well-known/caldav")?;
        let _ = self
            .request(
                HttpRequest::new("PROPFIND", well_known)
                    .header("Depth", "0")
                    .body(propfind_discovery(), XML_CONTENT_TYPE),
            )
            .await;
        let probe = match self
            .propfind(&paths.principal, 0, propfind_discovery())
            .await
        {
            Ok(status) => Ok(status),
            Err(DavError::NotFound(_)) => {
                self.propfind(&DavHref::root(), 0, propfind_discovery())
                    .await
            }
            Err(error) => Err(error),
        };
        let (discovered_principal, discovered_calendar_home, discovered_addressbook_home) =
            match probe {
                Ok(status) => {
                    let first = status.responses.first();
                    (
                        first
                            .and_then(|row| row.current_user_principal.clone())
                            .unwrap_or_else(|| paths.principal.clone()),
                        first
                            .and_then(|row| row.calendar_home_set.clone())
                            .unwrap_or_else(|| paths.calendar_home.clone()),
                        first
                            .and_then(|row| row.addressbook_home_set.clone())
                            .unwrap_or_else(|| paths.addressbook_home.clone()),
                    )
                }
                Err(DavError::NotFound(_)) | Err(DavError::Protocol { status: 404, .. }) => (
                    paths.principal.clone(),
                    paths.calendar_home.clone(),
                    paths.addressbook_home.clone(),
                ),
                Err(error) => return Err(error),
            };
        // The Foyer server authenticates to Radicale with a service account while
        // each application user lives below a separate logical path. Radicale's
        // current-user-principal therefore points at the service account (for
        // example `/foyer/`), not at `/dev-user/`. Only accept discovery values
        // that remain inside the logical user's subtree; otherwise use the paths
        // Foyer owns and validates itself.
        let principal = if paths.ensure_owned(&discovered_principal).is_ok() {
            discovered_principal
        } else {
            paths.principal.clone()
        };
        let calendar_home = if paths.ensure_owned(&discovered_calendar_home).is_ok() {
            discovered_calendar_home
        } else {
            paths.calendar_home.clone()
        };
        let addressbook_home = if paths.ensure_owned(&discovered_addressbook_home).is_ok() {
            discovered_addressbook_home
        } else {
            paths.addressbook_home.clone()
        };
        let mut collections = Vec::new();
        for home in [&calendar_home, &addressbook_home, &paths.task_home] {
            match self.propfind(home, 1, propfind_collection()).await {
                Ok(status) => collections.extend(collections_from_status(status, home)),
                Err(DavError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(DavDiscovery {
            user_id: paths.user_id,
            principal,
            calendar_home,
            addressbook_home,
            collections,
        })
    }

    pub async fn create_calendar(
        &self,
        user_id: &str,
        spec: &NewCalendar,
    ) -> Result<DavCollection, DavError> {
        self.create_caldav(user_id, CollectionKind::Calendar, spec, &["VEVENT"])
            .await
    }

    pub async fn create_task_list(
        &self,
        user_id: &str,
        spec: &NewCalendar,
    ) -> Result<DavCollection, DavError> {
        self.create_caldav(user_id, CollectionKind::TaskList, spec, &["VTODO"])
            .await
    }

    pub async fn create_address_book(
        &self,
        user_id: &str,
        spec: &NewAddressBook,
    ) -> Result<DavCollection, DavError> {
        let paths = self.user_paths(user_id)?;
        let display_name = validate_display_name(&spec.display_name)?;
        let collection_id = validate_segment(&spec.collection_id)?;
        self.ensure_collection(&paths.principal).await?;
        self.ensure_collection(&paths.addressbook_home).await?;
        let href = paths.collection(CollectionKind::AddressBook, &collection_id)?;
        let response = self
            .request(
                HttpRequest::new("MKCOL", href.clone())
                    .body(mkcol_addressbook_xml(&display_name), XML_CONTENT_TYPE),
            )
            .await?;
        self.created_or_existing(
            response,
            href,
            CollectionKind::AddressBook,
            Some(display_name),
        )
        .await
    }

    pub async fn sync_collection(
        &self,
        user_id: &str,
        href: &DavHref,
        token: Option<&SyncToken>,
    ) -> Result<SyncPage, DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        match self.sync_collection_once(href, token).await {
            Ok(page) => Ok(page),
            Err(DavError::InvalidSyncToken) => {
                let mut page = self.sync_collection_once(href, None).await?;
                page.token_reset = true;
                Ok(page)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn fetch_resources(
        &self,
        user_id: &str,
        kind: CollectionKind,
        hrefs: &[DavHref],
    ) -> Result<Vec<ResourceFetch>, DavError> {
        let paths = self.user_paths(user_id)?;
        for href in hrefs {
            paths.ensure_owned(href)?;
        }
        if hrefs.is_empty() {
            return Ok(Vec::new());
        }
        if hrefs.len() > self.config.max_multiget {
            return self.fetch_resources_bounded(kind, hrefs).await;
        }
        match self.multiget(kind, hrefs).await {
            Ok(fetches) => Ok(fetches),
            Err(DavError::Protocol { status: 404, .. })
            | Err(DavError::NotFound(_))
            | Err(DavError::Protocol { status: 405, .. }) => self.get_each(hrefs, kind).await,
            Err(error) => Err(error),
        }
    }

    pub async fn get_resource(
        &self,
        user_id: &str,
        href: &DavHref,
        kind: CollectionKind,
    ) -> Result<DavResource, DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        self.get_one(href, kind).await
    }

    pub async fn put_resource(
        &self,
        user_id: &str,
        href: &DavHref,
        payload: &DavPayload,
        precondition: PutPrecondition,
    ) -> Result<DavWriteResult, DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        if payload.raw().len() > self.config.max_resource_bytes {
            return Err(DavError::InvalidRequest(format!(
                "DAV payload exceeds {} bytes",
                self.config.max_resource_bytes
            )));
        }
        let mut request = HttpRequest::new("PUT", href.clone()).body(
            payload.raw().as_bytes().to_vec(),
            payload.media_type().as_str(),
        );
        request = match &precondition {
            PutPrecondition::IfNoneMatchStar => request.header("If-None-Match", "*"),
            PutPrecondition::IfMatch(etag) => request.header("If-Match", etag.header_value()),
        };
        let response = self.request(request).await?;
        if matches!(response.status, 200 | 201 | 204) {
            return Ok(DavWriteResult {
                href: href.clone(),
                etag: response_etag(&response),
                created: response.status == 201,
            });
        }
        if response.status == 412 && matches!(precondition, PutPrecondition::IfNoneMatchStar) {
            return self.recover_create(href, payload).await;
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    pub async fn delete_resource(
        &self,
        user_id: &str,
        href: &DavHref,
        expected: &ETag,
    ) -> Result<(), DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        let response = self
            .request(
                HttpRequest::new("DELETE", href.clone())
                    .header("If-Match", expected.header_value()),
            )
            .await?;
        if matches!(response.status, 200 | 204) {
            return Ok(());
        }
        if response.status == 404 {
            return Err(DavError::NotFound("DAV resource not found.".into()));
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    pub async fn delete_collection(
        &self,
        user_id: &str,
        href: &DavHref,
        expected: Option<&ETag>,
    ) -> Result<(), DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        let mut request = HttpRequest::new("DELETE", href.clone()).header("Depth", "infinity");
        if let Some(etag) = expected {
            request = request.header("If-Match", etag.header_value());
        }
        let response = self.request(request).await?;
        if matches!(response.status, 200 | 204 | 404) {
            return Ok(());
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    pub async fn move_resource(
        &self,
        user_id: &str,
        href: &DavHref,
        destination: &DavHref,
        expected: &ETag,
    ) -> Result<DavWriteResult, DavError> {
        let paths = self.user_paths(user_id)?;
        paths.ensure_owned(href)?;
        paths.ensure_owned(destination)?;
        let origin = self.config.origin()?;
        let response = self
            .request(
                HttpRequest::new("MOVE", href.clone())
                    .header("Destination", origin.join_href(destination))
                    .header("Overwrite", "F")
                    .header("If-Match", expected.header_value()),
            )
            .await?;
        if matches!(response.status, 200 | 201 | 204) {
            return Ok(DavWriteResult {
                href: destination.clone(),
                etag: response_etag(&response),
                created: response.status == 201,
            });
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    pub async fn set_display_name(
        &self,
        user_id: &str,
        href: &DavHref,
        display_name: &str,
        calendar_order: Option<i32>,
    ) -> Result<(), DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        let display_name = validate_display_name(display_name)?;
        let response = self
            .request(HttpRequest::new("PROPPATCH", href.clone()).body(
                proppatch_xml(&display_name, calendar_order),
                XML_CONTENT_TYPE,
            ))
            .await?;
        if matches!(response.status, 200 | 204 | 207) {
            return Ok(());
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    pub async fn list_collection(
        &self,
        user_id: &str,
        href: &DavHref,
    ) -> Result<Vec<DavCollection>, DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        let status = self.propfind(href, 1, propfind_collection()).await?;
        Ok(collections_from_status(status, href))
    }

    pub async fn load_collection(
        &self,
        user_id: &str,
        href: &DavHref,
        kind: CollectionKind,
    ) -> Result<DavCollection, DavError> {
        self.user_paths(user_id)?.ensure_owned(href)?;
        self.load_collection_inner(href, kind).await
    }

    async fn create_caldav(
        &self,
        user_id: &str,
        kind: CollectionKind,
        spec: &NewCalendar,
        components: &[&str],
    ) -> Result<DavCollection, DavError> {
        let paths = self.user_paths(user_id)?;
        let display_name = validate_display_name(&spec.display_name)?;
        let collection_id = validate_segment(&spec.collection_id)?;
        self.ensure_collection(&paths.principal).await?;
        self.ensure_collection(paths.home(kind)).await?;
        let href = paths.collection(kind, &collection_id)?;
        let response = self
            .request(
                HttpRequest::new("MKCALENDAR", href.clone())
                    .body(mkcalendar_xml(&display_name, components), XML_CONTENT_TYPE),
            )
            .await?;
        self.created_or_existing(response, href, kind, Some(display_name))
            .await
    }

    async fn created_or_existing(
        &self,
        response: HttpResponse,
        href: DavHref,
        kind: CollectionKind,
        display_name: Option<String>,
    ) -> Result<DavCollection, DavError> {
        if matches!(response.status, 201 | 200 | 204) {
            return Ok(DavCollection {
                href,
                kind,
                display_name,
                etag: response_etag(&response),
                sync_token: None,
                supported_components: default_components(kind),
            });
        }
        if matches!(response.status, 405 | 409)
            && let Ok(existing) = self.load_collection_inner(&href, kind).await
        {
            return Ok(existing);
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    async fn ensure_collection(&self, href: &DavHref) -> Result<(), DavError> {
        match self.propfind(href, 0, propfind_collection()).await {
            Ok(_) => Ok(()),
            Err(DavError::NotFound(_)) => {
                let response = self
                    .request(
                        HttpRequest::new("MKCOL", href.clone()).body(mkcol_xml(), XML_CONTENT_TYPE),
                    )
                    .await?;
                if matches!(response.status, 201 | 200 | 204 | 405 | 409) {
                    Ok(())
                } else {
                    Err(classify_response(
                        response.status,
                        &response.body,
                        &self.config,
                    ))
                }
            }
            Err(error) => Err(error),
        }
    }

    async fn load_collection_inner(
        &self,
        href: &DavHref,
        kind: CollectionKind,
    ) -> Result<DavCollection, DavError> {
        let status = self.propfind(href, 0, propfind_collection()).await?;
        status
            .responses
            .into_iter()
            .find_map(|row| row_to_collection(&row, kind))
            .ok_or_else(|| DavError::NotFound("DAV collection not found.".into()))
    }

    async fn sync_collection_once(
        &self,
        href: &DavHref,
        token: Option<&SyncToken>,
    ) -> Result<SyncPage, DavError> {
        let response = self
            .request(
                HttpRequest::new("REPORT", href.clone())
                    .header("Depth", "1")
                    .body(sync_collection_xml(token), XML_CONTENT_TYPE),
            )
            .await?;
        if response.status == 207 {
            let parsed = parse_multistatus(&response.body, &self.config, &self.config.origin()?)?;
            return Ok(sync_page_from_multistatus(parsed, href));
        }
        if let Some(name) = extract_dav_error(&response.body, &self.config)
            && name.eq_ignore_ascii_case("valid-sync-token")
        {
            return Err(DavError::InvalidSyncToken);
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    async fn fetch_resources_bounded(
        &self,
        kind: CollectionKind,
        hrefs: &[DavHref],
    ) -> Result<Vec<ResourceFetch>, DavError> {
        let mut out = Vec::new();
        for chunk in hrefs.chunks(self.config.max_multiget) {
            out.extend(self.fetch_resources_chunk(kind, chunk).await?);
        }
        Ok(out)
    }

    async fn fetch_resources_chunk(
        &self,
        kind: CollectionKind,
        hrefs: &[DavHref],
    ) -> Result<Vec<ResourceFetch>, DavError> {
        match self.multiget(kind, hrefs).await {
            Ok(fetches) => Ok(fetches),
            Err(DavError::Protocol { status: 405, .. }) | Err(DavError::NotFound(_)) => {
                self.get_each(hrefs, kind).await
            }
            Err(error) => Err(error),
        }
    }

    async fn multiget(
        &self,
        kind: CollectionKind,
        hrefs: &[DavHref],
    ) -> Result<Vec<ResourceFetch>, DavError> {
        let Some(first) = hrefs.first() else {
            return Ok(Vec::new());
        };
        let collection = first.parent().unwrap_or_else(DavHref::root);
        let body = match kind {
            CollectionKind::AddressBook => addressbook_multiget_xml(hrefs),
            CollectionKind::Calendar | CollectionKind::TaskList => calendar_multiget_xml(hrefs),
        };
        let response = self
            .request(
                HttpRequest::new("REPORT", collection)
                    .header("Depth", "1")
                    .body(body, XML_CONTENT_TYPE),
            )
            .await?;
        if response.status != 207 {
            return Err(classify_response(
                response.status,
                &response.body,
                &self.config,
            ));
        }
        let parsed = parse_multistatus(&response.body, &self.config, &self.config.origin()?)?;
        Ok(parsed
            .responses
            .into_iter()
            .map(|row| resource_fetch(row, kind))
            .collect())
    }

    async fn get_each(
        &self,
        hrefs: &[DavHref],
        kind: CollectionKind,
    ) -> Result<Vec<ResourceFetch>, DavError> {
        let mut out = Vec::with_capacity(hrefs.len());
        for href in hrefs {
            out.push(ResourceFetch {
                href: href.clone(),
                result: self.get_one(href, kind).await,
            });
        }
        Ok(out)
    }

    async fn get_one(&self, href: &DavHref, kind: CollectionKind) -> Result<DavResource, DavError> {
        let response = self.request(HttpRequest::new("GET", href.clone())).await?;
        if response.status != 200 {
            return Err(classify_response(
                response.status,
                &response.body,
                &self.config,
            ));
        }
        if response.body.len() > self.config.max_resource_bytes {
            return Err(DavError::ResponseTooLarge {
                limit: self.config.max_resource_bytes,
            });
        }
        let etag = response_etag(&response)
            .ok_or_else(|| DavError::MalformedRemote("GET response is missing an ETag".into()))?;
        let media = DavMediaType::from_content_type(response.header("content-type"), Some(kind));
        let payload = DavPayload::from_raw(media, response.text()?.to_string())?;
        Ok(DavResource {
            href: href.clone(),
            etag,
            payload,
        })
    }

    async fn recover_create(
        &self,
        href: &DavHref,
        payload: &DavPayload,
    ) -> Result<DavWriteResult, DavError> {
        let kind = match payload.media_type() {
            DavMediaType::VCard => CollectionKind::AddressBook,
            DavMediaType::ICalendar => CollectionKind::Calendar,
        };
        let existing = self.get_one(href, kind).await?;
        if existing.payload.semantically_eq(payload) {
            return Ok(DavWriteResult {
                href: href.clone(),
                etag: Some(existing.etag),
                created: false,
            });
        }
        Err(DavError::Conflict(
            "a different DAV resource already exists at this href".into(),
        ))
    }

    async fn propfind(
        &self,
        href: &DavHref,
        depth: u8,
        body: String,
    ) -> Result<MultiStatus, DavError> {
        if depth > 1 {
            return Err(DavError::InvalidRequest(
                "PROPFIND depth is limited to 0 or 1".into(),
            ));
        }
        let response = self
            .request(
                HttpRequest::new("PROPFIND", href.clone())
                    .header("Depth", depth.to_string())
                    .body(body, XML_CONTENT_TYPE),
            )
            .await?;
        if response.status == 207 {
            return parse_multistatus(&response.body, &self.config, &self.config.origin()?);
        }
        Err(classify_response(
            response.status,
            &response.body,
            &self.config,
        ))
    }

    async fn request(&self, request: HttpRequest) -> Result<HttpResponse, DavError> {
        let method = request.method.clone();
        let href = request.href.clone();
        let response = exchange(&self.config, request).await?;
        tracing::debug!(
            method = %method,
            href = %href,
            status = response.status,
            "DAV request completed"
        );
        Ok(response)
    }
}

fn response_etag(response: &HttpResponse) -> Option<ETag> {
    response
        .header("etag")
        .and_then(|value| ETag::parse(value).ok())
}

fn default_components(kind: CollectionKind) -> Vec<String> {
    match kind {
        CollectionKind::Calendar => vec!["VEVENT".into()],
        CollectionKind::TaskList => vec!["VTODO".into()],
        CollectionKind::AddressBook => Vec::new(),
    }
}

fn collections_from_status(status: MultiStatus, home: &DavHref) -> Vec<DavCollection> {
    status
        .responses
        .into_iter()
        .filter(|row| row.href.as_str() != home.as_str())
        .filter_map(|row| {
            let kind = row
                .resource_type
                .inferred_kind(&row.supported_components, &row.href)?;
            row_to_collection(&row, kind)
        })
        .collect()
}

fn row_to_collection(row: &PropResponse, kind: CollectionKind) -> Option<DavCollection> {
    if matches!(row.resource_type, ResourceType::Resource) {
        return None;
    }
    Some(DavCollection {
        href: row.href.as_collection(),
        kind,
        display_name: row.display_name.clone(),
        etag: row.etag.clone(),
        sync_token: row.sync_token.clone(),
        supported_components: row.supported_components.clone(),
    })
}

fn sync_page_from_multistatus(parsed: MultiStatus, collection: &DavHref) -> SyncPage {
    let mut upserts = Vec::new();
    let mut deletions = Vec::new();
    for row in parsed.responses {
        if row.href.as_str() == collection.as_str() || row.href.is_collection() {
            continue;
        }
        if row.status == 404 {
            deletions.push(row.href);
            continue;
        }
        if (200..300).contains(&row.status) {
            upserts.push(SyncChange {
                href: row.href,
                etag: row.etag,
            });
        }
    }
    SyncPage {
        upserts,
        deletions,
        sync_token: parsed.sync_token,
        token_reset: false,
    }
}

fn resource_fetch(row: PropResponse, kind: CollectionKind) -> ResourceFetch {
    let href = row.href.clone();
    if !(200..300).contains(&row.status) {
        return ResourceFetch {
            href,
            result: Err(classify_http_row(&row)),
        };
    }
    let Some(etag) = row.etag.clone() else {
        return ResourceFetch {
            href,
            result: Err(DavError::MalformedRemote(
                "multiget response is missing an ETag".into(),
            )),
        };
    };
    let raw = row.calendar_data.or(row.address_data).unwrap_or_default();
    if raw.is_empty() {
        return ResourceFetch {
            href,
            result: Err(DavError::MalformedRemote(
                "multiget response is missing calendar-data or address-data".into(),
            )),
        };
    }
    let media = DavMediaType::from_content_type(row.content_type.as_deref(), Some(kind));
    match DavPayload::from_raw(media, raw) {
        Ok(payload) => ResourceFetch {
            href,
            result: Ok(DavResource {
                href: row.href,
                etag,
                payload,
            }),
        },
        Err(error) => ResourceFetch {
            href,
            result: Err(error),
        },
    }
}

fn classify_http_row(row: &PropResponse) -> DavError {
    super::error::classify_http(row.status, row.error_name.as_deref(), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_calendar_paths_are_user_scoped() {
        let paths = UserPaths::for_user("dev-user").unwrap();
        let href = paths.collection(CollectionKind::Calendar, "home").unwrap();
        assert_eq!(href.as_str(), "/dev-user/calendars/home/");
    }
}
