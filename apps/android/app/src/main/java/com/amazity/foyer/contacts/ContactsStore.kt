package com.amazity.foyer.contacts

import android.content.Context
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.model.AddressBook
import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.ContactsCatalog
import com.amazity.foyer.model.ContactsStatus
import com.amazity.foyer.model.StructuredContactName
import com.powersync.PowerSyncDatabase
import com.powersync.db.getLongOptional
import com.powersync.db.getString
import com.powersync.db.getStringOptional
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Immutable snapshot / command boundary. UI reads [catalog] and never touches
 * PowerSync or Foyer Server from a composition callback.
 */
class ContactsStore(
    @Suppress("UNUSED_PARAMETER") context: Context,
    private val databaseProvider: suspend () -> PowerSyncDatabase,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _catalog = MutableStateFlow(ContactsCatalog(emptyList(), emptyList()))
    private var watchJob: Job? = null
    private var statusJob: Job? = null
    private var startRequested = false
    @Volatile private var lastConflict: Pair<String, String>? = null
    @Volatile private var attachedDatabase: PowerSyncDatabase? = null

    val catalog: StateFlow<ContactsCatalog> = _catalog

    @Suppress("UNUSED_PARAMETER")
    constructor(context: Context, api: ContactsApi, databaseProvider: suspend () -> PowerSyncDatabase) : this(
        context,
        databaseProvider,
    )

    @Synchronized
    fun start() {
        if (startRequested) return
        startRequested = true
        scope.launch { initialize() }
    }

    fun stop() {
        watchJob?.cancel()
        statusJob?.cancel()
        startRequested = false
        attachedDatabase = null
    }

    fun reportConflict(code: String, message: String) {
        lastConflict = code to message
        attachedDatabase?.let { publishStatus(it) }
    }

    fun markUnavailable(message: String) {
        _catalog.update {
            it.copy(
                status = it.status.copy(
                    loading = false,
                    connected = false,
                    offline = true,
                    lastError = message,
                    usingPowerSync = false,
                ),
            )
        }
    }

    suspend fun ensureDefaultAddressBook(): AddressBook {
        val existing = _catalog.value.addressBooks.firstOrNull { it.displayName.equals("Contacts", true) }
            ?: _catalog.value.addressBooks.firstOrNull()
        return existing ?: createAddressBook("Contacts")
    }

    suspend fun createAddressBook(displayName: String): AddressBook {
        validateBookName(displayName)?.let { error(it) }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        requireDatabase().execute(
            """INSERT INTO $ADDRESS_BOOKS_TABLE
                (id, user_id, uid, href, etag, display_name, description, revision,
                 created_at, updated_at, client_operation, operation_id, expected_etag,
                 expected_revision, deleted_local)
                VALUES (?, '', ?, '', NULL, ?, '', 1, ?, ?, 'create', ?, NULL, NULL, 0)""",
            listOf(id, id, displayName.trim(), now, now, UUID.randomUUID().toString()),
        )
        return AddressBook(id = id, uid = id, displayName = displayName.trim())
    }

    suspend fun renameAddressBook(book: AddressBook, displayName: String): AddressBook {
        validateBookName(displayName)?.let { error(it) }
        val nextRevision = book.revision + 1
        requireDatabase().execute(
            """UPDATE $ADDRESS_BOOKS_TABLE
                SET display_name = ?, revision = ?, updated_at = ?, client_operation = 'update',
                    operation_id = ?, expected_etag = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                displayName.trim(),
                nextRevision,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                book.etag,
                book.revision,
                book.id,
            ),
        )
        return book.copy(displayName = displayName.trim(), revision = nextRevision)
    }

    suspend fun deleteAddressBook(book: AddressBook) {
        _catalog.value.validateAddressBookDelete(book)?.let { error(it) }
        requireDatabase().execute(
            """UPDATE $ADDRESS_BOOKS_TABLE
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_etag = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                book.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                book.etag,
                book.revision,
                book.id,
            ),
        )
    }

    suspend fun createContact(draft: ContactDraft): Contact {
        val clean = draft.normalized()
        validateContactDraft(clean)?.let { error(it) }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        val operationId = UUID.randomUUID().toString()
        val payload = contactWritePayload(operationId, clean)
        requireDatabase().execute(
            """INSERT INTO $CONTACTS_TABLE
                (id, user_id, address_book_id, uid, href, etag, display_name, given_name, family_name,
                 additional_names, honorific_prefix, honorific_suffix, organization, job_title,
                 birthday, notes, emails, phones, addresses, revision, created_at, updated_at,
                 client_operation, operation_id, expected_etag, expected_revision, deleted_local, client_payload)
                VALUES (?, '', ?, ?, '', '', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?,
                        'create', ?, NULL, NULL, 0, ?)""",
            listOf(
                id,
                clean.addressBookId,
                "urn:uuid:$id",
                clean.displayName,
                clean.name.givenName,
                clean.name.familyName,
                clean.name.additionalNames,
                clean.name.honorificPrefix,
                clean.name.honorificSuffix,
                clean.organization,
                clean.jobTitle,
                clean.birthday,
                clean.notes,
                emailsJson(clean.emails),
                phonesJson(clean.phones),
                addressesJson(clean.addresses),
                now,
                now,
                operationId,
                payload,
            ),
        )
        return Contact(
            id = id,
            addressBookId = clean.addressBookId,
            uid = "urn:uuid:$id",
            displayName = clean.displayName,
            name = clean.name,
            emails = clean.emails,
            phones = clean.phones,
            organization = clean.organization,
            jobTitle = clean.jobTitle,
            addresses = clean.addresses,
            birthday = clean.birthday,
            notes = clean.notes,
            updatedAt = now,
        )
    }

    suspend fun updateContact(contact: Contact, draft: ContactDraft): Contact {
        val clean = draft.normalized()
        validateContactDraft(clean)?.let { error(it) }
        val db = requireDatabase()
        val now = Instant.now().toString()
        val operationId = UUID.randomUUID().toString()
        val payload = contactWritePayload(operationId, clean)
        var revision = contact.revision + 1
        db.execute(
            """UPDATE $CONTACTS_TABLE
                SET display_name = ?, given_name = ?, family_name = ?, additional_names = ?,
                    honorific_prefix = ?, honorific_suffix = ?, organization = ?, job_title = ?,
                    birthday = ?, notes = ?, emails = ?, phones = ?, addresses = ?,
                    revision = ?, updated_at = ?, client_operation = 'update', operation_id = ?,
                    expected_etag = ?, expected_revision = ?, client_payload = ?
                WHERE id = ?""",
            listOf(
                clean.displayName,
                clean.name.givenName,
                clean.name.familyName,
                clean.name.additionalNames,
                clean.name.honorificPrefix,
                clean.name.honorificSuffix,
                clean.organization,
                clean.jobTitle,
                clean.birthday,
                clean.notes,
                emailsJson(clean.emails),
                phonesJson(clean.phones),
                addressesJson(clean.addresses),
                revision,
                now,
                operationId,
                contact.etag,
                contact.revision,
                payload,
                contact.id,
            ),
        )
        if (clean.addressBookId != contact.addressBookId) {
            db.execute(
                """UPDATE $CONTACTS_TABLE
                    SET address_book_id = ?, revision = ?, updated_at = ?, client_operation = 'move',
                        operation_id = ?, expected_etag = ?, expected_revision = ?
                    WHERE id = ?""",
                listOf(
                    clean.addressBookId,
                    revision + 1,
                    now,
                    UUID.randomUUID().toString(),
                    contact.etag,
                    revision,
                    contact.id,
                ),
            )
            revision += 1
        }
        return contact.copy(
            addressBookId = clean.addressBookId,
            displayName = clean.displayName,
            name = clean.name,
            emails = clean.emails,
            phones = clean.phones,
            organization = clean.organization,
            jobTitle = clean.jobTitle,
            addresses = clean.addresses,
            birthday = clean.birthday,
            notes = clean.notes,
            revision = revision,
            updatedAt = now,
        )
    }

    suspend fun deleteContact(contact: Contact) {
        requireDatabase().execute(
            """UPDATE $CONTACTS_TABLE
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_etag = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                contact.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                contact.etag,
                contact.revision,
                contact.id,
            ),
        )
    }

    private suspend fun initialize() {
        try {
            val db = requireDatabase()
            watchReplica(db)
            watchStatus(db)
        } catch (error: Throwable) {
            markUnavailable("PowerSync replica unavailable: ${error.message}")
        }
    }

    private fun watchReplica(db: PowerSyncDatabase) {
        val books = db.watch(
            """SELECT id, uid, href, etag, display_name, description, revision
                FROM $ADDRESS_BOOKS_TABLE WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            AddressBook(
                id = cursor.getString("id"),
                uid = cursor.getStringOptional("uid").orEmpty(),
                href = cursor.getStringOptional("href").orEmpty(),
                etag = cursor.getStringOptional("etag"),
                displayName = cursor.getString("display_name"),
                description = cursor.getStringOptional("description").orEmpty(),
                revision = cursor.getLongOptional("revision") ?: 1L,
            )
        }
        val contacts = db.watch(
            """SELECT id, address_book_id, uid, href, etag, display_name, given_name, family_name,
                      additional_names, honorific_prefix, honorific_suffix, organization, job_title,
                      birthday, notes, emails, phones, addresses, revision, updated_at
                FROM $CONTACTS_TABLE WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            Contact(
                id = cursor.getString("id"),
                addressBookId = cursor.getString("address_book_id"),
                uid = cursor.getStringOptional("uid").orEmpty(),
                href = cursor.getStringOptional("href").orEmpty(),
                etag = cursor.getStringOptional("etag").orEmpty(),
                displayName = cursor.getString("display_name"),
                name = StructuredContactName(
                    familyName = cursor.getStringOptional("family_name").orEmpty(),
                    givenName = cursor.getStringOptional("given_name").orEmpty(),
                    additionalNames = cursor.getStringOptional("additional_names").orEmpty(),
                    honorificPrefix = cursor.getStringOptional("honorific_prefix").orEmpty(),
                    honorificSuffix = cursor.getStringOptional("honorific_suffix").orEmpty(),
                ),
                emails = parseEmails(cursor.getStringOptional("emails")),
                phones = parsePhones(cursor.getStringOptional("phones")),
                organization = cursor.getStringOptional("organization").orEmpty(),
                jobTitle = cursor.getStringOptional("job_title").orEmpty(),
                addresses = parseAddresses(cursor.getStringOptional("addresses")),
                birthday = cursor.getStringOptional("birthday"),
                notes = cursor.getStringOptional("notes").orEmpty(),
                revision = cursor.getLongOptional("revision") ?: 1L,
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
            )
        }
        val pending = db.watch(
            """SELECT
                (SELECT COUNT(*) FROM $ADDRESS_BOOKS_TABLE WHERE operation_id IS NOT NULL) +
                (SELECT COUNT(*) FROM $CONTACTS_TABLE WHERE operation_id IS NOT NULL) AS count""",
        ) { cursor -> cursor.getLongOptional("count")?.toInt() ?: 0 }
        watchJob = scope.launch {
            combine(books, contacts, pending) { bookRows, contactRows, pendingRows ->
                Triple(bookRows, contactRows, pendingRows.firstOrNull() ?: 0)
            }.collect { (bookRows, contactRows, pendingCount) ->
                val status = db.currentStatus
                _catalog.value = ContactsCatalog(
                    addressBooks = bookRows.sortedWith(
                        compareBy(String.CASE_INSENSITIVE_ORDER, AddressBook::displayName)
                            .thenBy(AddressBook::id),
                    ),
                    contacts = contactRows,
                    status = ContactsStatus(
                        loading = false,
                        connected = status.connected,
                        offline = !status.connected,
                        pendingUploads = pendingCount,
                        lastError = status.anyError?.toString(),
                        conflictCode = lastConflict?.first,
                        conflictMessage = lastConflict?.second,
                        developmentAuth = BuildConfig.FOYER_DEVELOPMENT_AUTH,
                        usingPowerSync = true,
                    ),
                )
            }
        }
    }

    private fun watchStatus(db: PowerSyncDatabase) {
        statusJob = scope.launch {
            db.currentStatus.asFlow().collect { publishStatus(db) }
        }
    }

    private fun publishStatus(db: PowerSyncDatabase) {
        val sync = db.currentStatus
        _catalog.update { current ->
            current.copy(
                status = current.status.copy(
                    loading = false,
                    connected = sync.connected,
                    offline = !sync.connected,
                    lastError = sync.anyError?.toString(),
                    conflictCode = lastConflict?.first,
                    conflictMessage = lastConflict?.second,
                    developmentAuth = BuildConfig.FOYER_DEVELOPMENT_AUTH,
                    usingPowerSync = true,
                ),
            )
        }
    }

    private suspend fun requireDatabase(): PowerSyncDatabase {
        attachedDatabase?.let { return it }
        val db = databaseProvider()
        attachedDatabase = db
        return db
    }

    companion object {
        fun attach(
            context: Context,
            api: ContactsApi,
            databaseProvider: suspend () -> PowerSyncDatabase,
        ): ContactsStore = ContactsStore(context, api, databaseProvider)
    }
}

private fun contactWritePayload(operationId: String, draft: ContactDraft): String =
    JSONObject()
        .put("operationId", operationId)
        .put("displayName", draft.displayName)
        .put(
            "name",
            JSONObject()
                .put("familyName", draft.name.familyName)
                .put("givenName", draft.name.givenName)
                .put("additionalNames", draft.name.additionalNames)
                .put("honorificPrefix", draft.name.honorificPrefix)
                .put("honorificSuffix", draft.name.honorificSuffix),
        )
        .put("emails", org.json.JSONArray(draft.emails.map {
            JSONObject().put("value", it.value).put("type", it.type).put("pref", it.pref)
        }))
        .put("phones", org.json.JSONArray(draft.phones.map {
            JSONObject().put("value", it.value).put("type", it.type).put("pref", it.pref)
        }))
        .put("organization", draft.organization)
        .put("jobTitle", draft.jobTitle)
        .put("addresses", org.json.JSONArray(draft.addresses.map {
            JSONObject()
                .put("poBox", it.poBox)
                .put("extended", it.extended)
                .put("street", it.street)
                .put("locality", it.locality)
                .put("region", it.region)
                .put("postalCode", it.postalCode)
                .put("country", it.country)
                .put("type", it.type)
                .put("pref", it.pref)
        }))
        .put("birthday", draft.birthday ?: JSONObject.NULL)
        .put("notes", draft.notes)
        .toString()
