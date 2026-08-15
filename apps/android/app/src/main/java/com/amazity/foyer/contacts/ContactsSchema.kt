package com.amazity.foyer.contacts

import com.powersync.db.schema.Column
import com.powersync.db.schema.Schema
import com.powersync.db.schema.Table

const val CLIENT_OPERATION = "client_operation"
const val OPERATION_ID = "operation_id"
const val EXPECTED_ETAG = "expected_etag"
const val EXPECTED_REVISION = "expected_revision"
const val DELETED_LOCAL = "deleted_local"
const val CLIENT_PAYLOAD = "client_payload"

const val ADDRESS_BOOKS_TABLE = "contacts_address_books"
const val CONTACTS_TABLE = "contacts"

private fun clientColumns() = listOf(
    Column.text(CLIENT_OPERATION),
    Column.text(OPERATION_ID),
    Column.text(EXPECTED_ETAG),
    Column.integer(EXPECTED_REVISION),
    Column.integer(DELETED_LOCAL),
    Column.text(CLIENT_PAYLOAD),
)

fun addressBookTable(): Table = Table(
    name = ADDRESS_BOOKS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("uid"),
        Column.text("href"),
        Column.text("etag"),
        Column.text("display_name"),
        Column.text("description"),
        Column.text("sync_token"),
        Column.text("ctag"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun contactTable(): Table = Table(
    name = CONTACTS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("address_book_id"),
        Column.text("uid"),
        Column.text("href"),
        Column.text("etag"),
        Column.text("display_name"),
        Column.text("given_name"),
        Column.text("family_name"),
        Column.text("additional_names"),
        Column.text("honorific_prefix"),
        Column.text("honorific_suffix"),
        Column.text("organization"),
        Column.text("job_title"),
        Column.text("birthday"),
        Column.text("notes"),
        Column.text("emails"),
        Column.text("phones"),
        Column.text("addresses"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun contactsTables(): List<Table> = listOf(addressBookTable(), contactTable())

/**
 * Shared replica schema. Android and Foyer Shell must keep these tables and
 * projected columns aligned so one PowerSync replica can serve both clients.
 */
val contactsSchema: Schema = Schema(*contactsTables().toTypedArray())
