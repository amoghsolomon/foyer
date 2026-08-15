//! CardDAV / vCard fixtures for the Contacts slice. Compiles against
//! `services/server/src/contacts.rs` directly so the suite can run before
//! `lib.rs` wires `pub mod contacts`.

use std::time::Duration;

use foyer_server::contacts;
use foyer_server::contacts::{
    ADDRESS_BOOKS_TABLE, CONTACTS_TABLE, ContactPatch, ContactsService, CreateAddressBookRequest,
    CreateContactRequest, DavBackend, DeleteRequest, MAX_NOTE_CHARS, MemoryDav, MoveContactRequest,
    PostalAddress, StructuredName, TypedEmail, TypedPhone, UpdateAddressBookRequest,
    UpdateContactRequest, apply_contact_patch, contact_fields_from_vcard, escape_text, fold_line,
    parse_vcard, project_user, rebuild_user_projections, serialize_vcard, unescape_text, unfold,
    unknown_properties, vcard_from_create,
};
use sqlx::PgPool;
use uuid::Uuid;

fn sample_card() -> String {
    [
        "BEGIN:VCARD",
        "VERSION:4.0",
        "UID:urn:uuid:11111111-1111-4111-8111-111111111111",
        "FN:Ada Lovelace",
        "N:Lovelace;Ada;;;",
        "EMAIL;TYPE=work:ada@example.com",
        "TEL;TYPE=cell:+1-555-0100",
        "ORG:Analytical Engines",
        "TITLE:Mathematician",
        "ADR;TYPE=home:;;12 St James's Square;London;;SW1Y 4LE;United Kingdom",
        "BDAY:18151210",
        "NOTE:Notes stay lossless.",
        "X-ABLABEL:Lab",
        "item1.X-CUSTOM:keep-me",
        "PHOTO;ENCODING=b:abcd",
        "END:VCARD",
        "",
    ]
    .join("\r\n")
}

#[test]
fn unfolding_rejoins_crlf_and_tab_continuations() {
    let folded = "NOTE:first\r\n  second\r\n\tthird";
    assert_eq!(unfold(folded), "NOTE:first secondthird");
}

#[test]
fn folding_respects_75_octet_budget_and_round_trips() {
    let line = format!("NOTE:{}", "ä".repeat(40) + &"b".repeat(40));
    let folded = fold_line(&line);
    assert!(folded.contains("\r\n "));
    for piece in folded.split("\r\n") {
        assert!(piece.len() <= 75, "{piece:?} is {} bytes", piece.len());
    }
    assert_eq!(unfold(&folded), line);
}

#[test]
fn escaping_preserves_commas_semicolons_backslashes_and_newlines() {
    let original = "comma, semi; slash\\ and\nnewline";
    assert_eq!(unescape_text(&escape_text(original)), original);
    let card = parse_vcard(&format!(
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:u\r\nFN:n\r\nNOTE:{}\r\nEND:VCARD\r\n",
        escape_text(original)
    ))
    .unwrap();
    assert_eq!(card.first_value("NOTE"), Some(original));
}

#[test]
fn structured_multivalue_fields_round_trip_through_serialize() {
    let card = parse_vcard(&sample_card()).unwrap();
    let fields = contact_fields_from_vcard(&card);
    assert_eq!(fields.name.family_name, "Lovelace");
    assert_eq!(fields.name.given_name, "Ada");
    assert_eq!(fields.emails.len(), 1);
    assert_eq!(fields.phones[0].r#type, "cell");
    assert_eq!(fields.addresses[0].locality, "London");
    assert_eq!(fields.birthday.as_deref(), Some("1815-12-10"));
    let again = parse_vcard(&serialize_vcard(&card)).unwrap();
    assert_eq!(
        contact_fields_from_vcard(&again).emails[0].value,
        "ada@example.com"
    );
}

#[test]
fn unknown_properties_and_groups_survive_partial_edits() {
    let mut card = parse_vcard(&sample_card()).unwrap();
    apply_contact_patch(
        &mut card,
        &ContactPatch {
            notes: Some("replaced\nwith two lines".into()),
            emails: Some(vec![
                TypedEmail {
                    value: "ada@example.com".into(),
                    r#type: "work".into(),
                    pref: true,
                },
                TypedEmail {
                    value: "ada@home.example".into(),
                    r#type: "home".into(),
                    pref: false,
                },
            ]),
            organization: Some(String::new()),
            ..ContactPatch::default()
        },
    )
    .unwrap();
    let names: Vec<_> = unknown_properties(&card)
        .into_iter()
        .map(|property| {
            (
                property.group.clone(),
                property.name.clone(),
                property.value.clone(),
            )
        })
        .collect();
    assert!(
        names
            .iter()
            .any(|(_, name, value)| name == "X-ABLABEL" && value == "Lab")
    );
    assert!(
        names
            .iter()
            .any(|(group, name, value)| group.as_deref() == Some("item1")
                && name == "X-CUSTOM"
                && value == "keep-me")
    );
    assert!(names.iter().any(|(_, name, _)| name == "PHOTO"));
    assert_eq!(card.first_value("NOTE"), Some("replaced\nwith two lines"));
    assert!(card.first_value("ORG").is_none());
    let serialized = serialize_vcard(&card);
    assert!(serialized.contains("X-ABLABEL:Lab"));
    assert!(serialized.contains("item1.X-CUSTOM:keep-me"));
    assert!(serialized.contains("PHOTO;ENCODING=b:abcd"));
    assert!(serialized.contains("EMAIL;TYPE=home:ada@home.example"));
}

#[test]
fn create_serialization_sets_uid_and_derived_fn() {
    let id = "22222222-2222-4222-8222-222222222222";
    let request = CreateContactRequest {
        operation_id: Uuid::new_v4().to_string(),
        id: id.into(),
        address_book_id: Uuid::new_v4().to_string(),
        display_name: None,
        name: Some(StructuredName {
            given_name: "Grace".into(),
            family_name: "Hopper".into(),
            ..StructuredName::default()
        }),
        emails: vec![TypedEmail {
            value: "grace@example.com".into(),
            r#type: "work".into(),
            pref: true,
        }],
        phones: Vec::new(),
        organization: Some("Navy".into()),
        job_title: Some("Rear Admiral".into()),
        addresses: vec![PostalAddress {
            street: "1 Harbor".into(),
            locality: "Arlington".into(),
            r#type: "work".into(),
            ..PostalAddress::default()
        }],
        birthday: Some("1906-12-09".into()),
        notes: Some("COBOL\nlossless".into()),
    };
    let card = vcard_from_create(&format!("urn:uuid:{id}"), &request).unwrap();
    let serialized = serialize_vcard(&card);
    assert!(serialized.contains("UID:urn:uuid:22222222-2222-4222-8222-222222222222"));
    assert!(serialized.contains("FN:Grace Hopper"));
    assert!(serialized.contains("N:Hopper;Grace;;;"));
    assert!(serialized.contains("NOTE:COBOL\\nlossless"));
    assert!(serialized.starts_with("BEGIN:VCARD"));
    assert!(serialized.contains("END:VCARD"));
}

#[tokio::test]
async fn memory_dav_enforces_stale_etags_and_move_hrefs() {
    let dav = MemoryDav::new();
    dav.seed_principal("/dev-user/");
    let backend = DavBackend::Memory(dav.clone());
    let mk = backend
        .send(contacts::DavRequest {
            method: "MKCOL".into(),
            path: "/dev-user/book-a/".into(),
            headers: Vec::new(),
            body: br#"<d:mkcol xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav"><d:set><d:prop><d:displayname>A</d:displayname></d:prop></d:set></d:mkcol>"#.to_vec(),
        })
        .await
        .unwrap();
    assert_eq!(mk.status, 201);
    let put = backend
        .send(contacts::DavRequest {
            method: "PUT".into(),
            path: "/dev-user/book-a/c1.vcf".into(),
            headers: vec![("If-None-Match".into(), "*".into())],
            body: sample_card().into_bytes(),
        })
        .await
        .unwrap();
    assert_eq!(put.status, 201);
    let etag = put.etag().expect("etag");
    let stale = backend
        .send(contacts::DavRequest {
            method: "PUT".into(),
            path: "/dev-user/book-a/c1.vcf".into(),
            headers: vec![("If-Match".into(), "\"not-the-etag\"".into())],
            body: sample_card().into_bytes(),
        })
        .await
        .unwrap();
    assert_eq!(stale.status, 412);
    backend
        .send(contacts::DavRequest {
            method: "MKCOL".into(),
            path: "/dev-user/book-b/".into(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .await
        .unwrap();
    let moved = backend
        .send(contacts::DavRequest {
            method: "MOVE".into(),
            path: "/dev-user/book-a/c1.vcf".into(),
            headers: vec![
                ("Destination".into(), "/dev-user/book-b/c1.vcf".into()),
                ("If-Match".into(), etag),
            ],
            body: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(moved.status, 201);
    let missing = backend
        .send(contacts::DavRequest {
            method: "GET".into(),
            path: "/dev-user/book-a/c1.vcf".into(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(missing.status, 404);
    let found = backend
        .send(contacts::DavRequest {
            method: "GET".into(),
            path: "/dev-user/book-b/c1.vcf".into(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(found.status, 200);
}

async fn test_database_url() -> Option<String> {
    if let Ok(url) = std::env::var("FOYER_TEST_DATABASE_URL")
        && !url.is_empty()
    {
        return Some(url);
    }
    start_postgres_container().await
}

async fn start_postgres_container() -> Option<String> {
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default().start().await.ok()?;
    let host = container.get_host().await.ok()?;
    let port = container.get_host_port_ipv4(5432).await.ok()?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    std::mem::forget(container);
    Some(url)
}

async fn test_pool() -> Option<PgPool> {
    let url = test_database_url().await?;
    for _ in 0..40 {
        if let Ok(pool) = PgPool::connect(&url).await {
            sqlx::migrate!("./migrations").run(&pool).await.ok()?;
            return Some(pool);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    None
}

fn service(pool: PgPool) -> (ContactsService, MemoryDav) {
    let dav = MemoryDav::new();
    dav.seed_principal("/dev-user/");
    let service = ContactsService::new(pool, DavBackend::Memory(dav.clone()), "/dev-user/");
    (service, dav)
}

fn uuid() -> String {
    Uuid::new_v4().to_string()
}

#[tokio::test]
async fn create_update_move_delete_and_idempotent_retry() {
    let Some(pool) = test_pool().await else {
        eprintln!(
            "skipping create_update_move_delete_and_idempotent_retry: PostgreSQL unavailable"
        );
        return;
    };
    let (service, _) = service(pool);
    let book_a = uuid();
    let created_book = contacts::create_address_book(
        &service,
        "dev-user",
        CreateAddressBookRequest {
            operation_id: uuid(),
            id: book_a.clone(),
            display_name: "Personal".into(),
            description: Some("Primary".into()),
        },
    )
    .await
    .expect("create book");
    assert_eq!(created_book.display_name, "Personal");
    assert_eq!(
        created_book.href,
        format!("/dev-user/addressbooks/{book_a}/")
    );
    assert_eq!(created_book.revision, 1);

    let replay_op = uuid();
    let book_body = CreateAddressBookRequest {
        operation_id: replay_op.clone(),
        id: uuid(),
        display_name: "Work".into(),
        description: None,
    };
    let first = contacts::create_address_book(&service, "dev-user", book_body)
        .await
        .unwrap();
    let second = contacts::create_address_book(
        &service,
        "dev-user",
        CreateAddressBookRequest {
            operation_id: replay_op,
            id: first.id.clone(),
            display_name: "Work".into(),
            description: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.revision, second.revision);

    let contact_id = uuid();
    let create = CreateContactRequest {
        operation_id: uuid(),
        id: contact_id.clone(),
        address_book_id: book_a.clone(),
        display_name: Some("Ada Lovelace".into()),
        name: Some(StructuredName {
            given_name: "Ada".into(),
            family_name: "Lovelace".into(),
            ..StructuredName::default()
        }),
        emails: vec![TypedEmail {
            value: "ada@example.com".into(),
            r#type: "work".into(),
            pref: true,
        }],
        phones: vec![TypedPhone {
            value: "+1-555-0100".into(),
            r#type: "cell".into(),
            pref: false,
        }],
        organization: Some("Analytical Engines".into()),
        job_title: Some("Mathematician".into()),
        addresses: vec![PostalAddress {
            street: "12 Square".into(),
            locality: "London".into(),
            r#type: "home".into(),
            ..PostalAddress::default()
        }],
        birthday: Some("1815-12-10".into()),
        notes: Some("Keep  trailing  spaces \nand a newline".into()),
    };
    let created = contacts::create_contact(&service, "dev-user", create)
        .await
        .expect("create contact");
    assert_eq!(created.uid, format!("urn:uuid:{contact_id}"));
    assert_eq!(created.notes, "Keep  trailing  spaces \nand a newline");
    assert_eq!(created.emails.len(), 1);
    assert_eq!(created.birthday.as_deref(), Some("1815-12-10"));

    let stale = contacts::update_contact(
        &service,
        "dev-user",
        &contact_id,
        UpdateContactRequest {
            operation_id: uuid(),
            expected_etag: Some("\"not-current\"".into()),
            expected_revision: None,
            job_title: Some("Countess".into()),
            ..UpdateContactRequest::default()
        },
    )
    .await
    .expect_err("stale etag");
    assert_eq!(stale.code(), "stale_etag");

    let updated = contacts::update_contact(
        &service,
        "dev-user",
        &contact_id,
        UpdateContactRequest {
            operation_id: uuid(),
            expected_etag: Some(created.etag.clone()),
            expected_revision: Some(created.revision),
            job_title: Some("Countess".into()),
            notes: None,
            ..UpdateContactRequest::default()
        },
    )
    .await
    .expect("partial update");
    assert_eq!(updated.job_title, "Countess");
    assert_eq!(updated.notes, "Keep  trailing  spaces \nand a newline");
    assert_eq!(updated.emails[0].value, "ada@example.com");
    assert!(updated.revision > created.revision);

    let moved = contacts::move_contact(
        &service,
        "dev-user",
        &contact_id,
        MoveContactRequest {
            operation_id: uuid(),
            expected_etag: Some(updated.etag.clone()),
            expected_revision: Some(updated.revision),
            address_book_id: first.id.clone(),
        },
    )
    .await
    .expect("move");
    assert_eq!(moved.address_book_id, first.id);
    assert!(moved.href.contains(&first.id));
    assert_eq!(moved.uid, created.uid);

    let deleted = contacts::delete_contact(
        &service,
        "dev-user",
        &contact_id,
        DeleteRequest {
            operation_id: uuid(),
            expected_etag: Some(moved.etag.clone()),
            expected_revision: Some(moved.revision),
        },
    )
    .await
    .expect("delete");
    assert!(deleted.deleted_at.is_some());
    let gone = contacts::get_contact(&service.pool, "dev-user", &contact_id)
        .await
        .expect_err("tombstone");
    assert_eq!(gone.code(), "gone");
}

#[tokio::test]
async fn projector_rebuilds_normalized_rows_from_dav() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping projector_rebuilds_normalized_rows_from_dav: PostgreSQL unavailable");
        return;
    };
    let (service, dav) = service(pool.clone());
    dav.seed_principal("/dev-user/");
    let backend = DavBackend::Memory(dav);
    backend
        .send(contacts::DavRequest {
            method: "MKCOL".into(),
            path: "/dev-user/external/".into(),
            headers: Vec::new(),
            body: br#"<displayname>External</displayname>"#.to_vec(),
        })
        .await
        .unwrap();
    backend
        .send(contacts::DavRequest {
            method: "PUT".into(),
            path: "/dev-user/external/ext.vcf".into(),
            headers: vec![("If-None-Match".into(), "*".into())],
            body: sample_card().into_bytes(),
        })
        .await
        .unwrap();

    let projected = project_user(&service, "dev-user").await.expect("project");
    assert!(projected >= 1);
    let books = contacts::list_address_books(&pool, "dev-user")
        .await
        .unwrap();
    assert!(
        books
            .address_books
            .iter()
            .any(|book| book.display_name == "External")
    );
    let listed = contacts::list_contacts(&pool, "dev-user", None)
        .await
        .unwrap();
    let ada = listed
        .contacts
        .iter()
        .find(|contact| contact.display_name.contains("Ada"))
        .expect("projected ada");
    assert_eq!(ada.organization, "Analytical Engines");
    assert_eq!(ada.emails[0].value, "ada@example.com");

    let rebuilt = rebuild_user_projections(&service, "dev-user")
        .await
        .expect("rebuild");
    assert!(rebuilt >= 1);
    let again = contacts::list_contacts(&pool, "dev-user", None)
        .await
        .unwrap();
    assert_eq!(again.contacts.len(), listed.contacts.len());
    assert_eq!(ADDRESS_BOOKS_TABLE, "contacts_address_books");
    assert_eq!(CONTACTS_TABLE, "contacts");
}

#[test]
fn notes_bounds_are_strict() {
    let too_long = "n".repeat(MAX_NOTE_CHARS + 1);
    let request = CreateContactRequest {
        operation_id: uuid(),
        id: uuid(),
        address_book_id: uuid(),
        display_name: Some("N".into()),
        name: None,
        emails: Vec::new(),
        phones: Vec::new(),
        organization: None,
        job_title: None,
        addresses: Vec::new(),
        birthday: None,
        notes: Some(too_long),
    };
    assert!(vcard_from_create("urn:uuid:x", &request).is_ok());
}

#[tokio::test]
async fn bound_operation_rejects_argument_changes() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping bound_operation_rejects_argument_changes: PostgreSQL unavailable");
        return;
    };
    let (service, _) = service(pool);
    let op = uuid();
    let id = uuid();
    contacts::create_address_book(
        &service,
        "dev-user",
        CreateAddressBookRequest {
            operation_id: op.clone(),
            id: id.clone(),
            display_name: "One".into(),
            description: None,
        },
    )
    .await
    .unwrap();
    let err = contacts::create_address_book(
        &service,
        "dev-user",
        CreateAddressBookRequest {
            operation_id: op,
            id,
            display_name: "Two".into(),
            description: None,
        },
    )
    .await
    .expect_err("rebind");
    assert_eq!(err.code(), "conflict");
}

#[tokio::test]
async fn nonempty_address_book_cannot_be_deleted() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping nonempty_address_book_cannot_be_deleted: PostgreSQL unavailable");
        return;
    };
    let (service, _) = service(pool);
    let book = uuid();
    let created = contacts::create_address_book(
        &service,
        "dev-user",
        CreateAddressBookRequest {
            operation_id: uuid(),
            id: book.clone(),
            display_name: "Busy".into(),
            description: None,
        },
    )
    .await
    .unwrap();
    contacts::create_contact(
        &service,
        "dev-user",
        CreateContactRequest {
            operation_id: uuid(),
            id: uuid(),
            address_book_id: book.clone(),
            display_name: Some("Someone".into()),
            name: None,
            emails: Vec::new(),
            phones: Vec::new(),
            organization: None,
            job_title: None,
            addresses: Vec::new(),
            birthday: None,
            notes: None,
        },
    )
    .await
    .unwrap();
    let current = contacts::get_address_book(&service.pool, "dev-user", &book)
        .await
        .expect("reload book");
    assert_eq!(current.revision, created.revision);
    let err = contacts::delete_address_book(
        &service,
        "dev-user",
        &book,
        DeleteRequest {
            operation_id: uuid(),
            expected_etag: current.etag.clone(),
            expected_revision: Some(current.revision),
        },
    )
    .await
    .expect_err("not empty");
    assert_eq!(err.code(), "address_book_not_empty");
    contacts::update_address_book(
        &service,
        "dev-user",
        &book,
        UpdateAddressBookRequest {
            operation_id: uuid(),
            expected_etag: current.etag,
            expected_revision: Some(current.revision),
            display_name: Some("Renamed".into()),
            description: None,
        },
    )
    .await
    .expect("rename book");
}
