package com.amazity.foyer.model

/** Normalized contact DTOs shared by the replica, editor, and UI. */

data class ContactEmail(
    val value: String,
    val type: String = "other",
    val pref: Boolean = false,
)

data class ContactPhone(
    val value: String,
    val type: String = "other",
    val pref: Boolean = false,
)

data class ContactPostalAddress(
    val poBox: String = "",
    val extended: String = "",
    val street: String = "",
    val locality: String = "",
    val region: String = "",
    val postalCode: String = "",
    val country: String = "",
    val type: String = "other",
    val pref: Boolean = false,
) {
    fun isBlank(): Boolean =
        listOf(poBox, extended, street, locality, region, postalCode, country).all { it.isBlank() }

    fun oneLine(): String =
        listOf(street, locality, region, postalCode, country).filter { it.isNotBlank() }.joinToString(", ")
}

data class StructuredContactName(
    val familyName: String = "",
    val givenName: String = "",
    val additionalNames: String = "",
    val honorificPrefix: String = "",
    val honorificSuffix: String = "",
) {
    fun isBlank(): Boolean =
        listOf(familyName, givenName, additionalNames, honorificPrefix, honorificSuffix).all { it.isBlank() }

    fun formatted(): String =
        listOf(honorificPrefix, givenName, additionalNames, familyName, honorificSuffix)
            .filter { it.isNotBlank() }
            .joinToString(" ")
}

data class AddressBook(
    val id: String,
    val uid: String = id,
    val href: String = "",
    val etag: String? = null,
    val displayName: String,
    val description: String = "",
    val revision: Long = 1,
)

data class Contact(
    val id: String,
    val addressBookId: String,
    val uid: String = id,
    val href: String = "",
    val etag: String = "",
    val displayName: String,
    val name: StructuredContactName = StructuredContactName(),
    val emails: List<ContactEmail> = emptyList(),
    val phones: List<ContactPhone> = emptyList(),
    val organization: String = "",
    val jobTitle: String = "",
    val addresses: List<ContactPostalAddress> = emptyList(),
    val birthday: String? = null,
    val notes: String = "",
    val revision: Long = 1,
    val updatedAt: String = "",
) {
    fun subtitle(): String =
        organization.ifBlank { emails.firstOrNull()?.value ?: phones.firstOrNull()?.value.orEmpty() }

    fun initials(): String {
        val parts = displayName.split(" ").filter { it.isNotBlank() }
        return when {
            parts.size >= 2 -> "${parts.first().first()}${parts.last().first()}".uppercase()
            parts.isNotEmpty() -> parts.first().take(2).uppercase()
            else -> "?"
        }
    }
}

data class ContactsStatus(
    val loading: Boolean = true,
    val connected: Boolean = false,
    val offline: Boolean = false,
    val pendingUploads: Int = 0,
    val lastError: String? = null,
    val conflictCode: String? = null,
    val conflictMessage: String? = null,
    val developmentAuth: Boolean = false,
    val usingPowerSync: Boolean = false,
) {
    fun banner(): ContactsSyncBanner? = contactsSyncBanner(this)
}

sealed class ContactsSyncBanner {
    data class Offline(val pendingUploads: Int) : ContactsSyncBanner()
    data class Pending(val pendingUploads: Int) : ContactsSyncBanner()
    data class StaleEtag(val message: String) : ContactsSyncBanner()
    data class Error(val message: String) : ContactsSyncBanner()
}

fun contactsSyncBanner(status: ContactsStatus): ContactsSyncBanner? {
    val conflict = status.conflictMessage?.takeIf { it.isNotBlank() }
    if (conflict != null) {
        return if (
            status.conflictCode == "stale_etag" ||
            status.conflictCode == "stale_revision" ||
            conflict.contains("stale", ignoreCase = true)
        ) {
            ContactsSyncBanner.StaleEtag(conflict)
        } else {
            ContactsSyncBanner.Error(conflict)
        }
    }
    status.lastError?.takeIf { it.isNotBlank() }?.let { return ContactsSyncBanner.Error(it) }
    if (status.offline) return ContactsSyncBanner.Offline(status.pendingUploads)
    if (status.pendingUploads > 0) return ContactsSyncBanner.Pending(status.pendingUploads)
    return null
}

data class ContactsCatalog(
    val addressBooks: List<AddressBook>,
    val contacts: List<Contact>,
    val status: ContactsStatus = ContactsStatus(loading = false),
) {
    fun addressBook(id: String): AddressBook? = addressBooks.firstOrNull { it.id == id }

    fun contact(id: String): Contact? = contacts.firstOrNull { it.id == id }

    fun contactsIn(addressBookId: String?): List<Contact> {
        val scoped = if (addressBookId == null) contacts else contacts.filter { it.addressBookId == addressBookId }
        return scoped.sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER, Contact::displayName).thenBy(Contact::id))
    }

    fun search(query: String, addressBookId: String? = null): List<Contact> {
        val needle = query.trim().lowercase()
        return contactsIn(addressBookId).filter { contactMatches(it, needle) }
    }

    fun validateDelete(contact: Contact): String? =
        if (contact(contact.id) == null) "The contact was not found." else null

    fun validateMove(contact: Contact, addressBookId: String): String? {
        if (contact(contact.id) == null) return "The contact was not found."
        if (addressBook(addressBookId) == null) return "The destination address book was not found."
        return null
    }

    fun validateAddressBookDelete(book: AddressBook): String? =
        if (contactsIn(book.id).isEmpty()) {
            null
        } else {
            "Address book is not empty. Move or delete its contacts first."
        }
}

fun contactMatches(contact: Contact, query: String): Boolean {
    if (query.isBlank()) return true
    val haystacks = buildList {
        add(contact.displayName)
        add(contact.name.givenName)
        add(contact.name.familyName)
        add(contact.name.additionalNames)
        add(contact.organization)
        add(contact.jobTitle)
        add(contact.notes)
        addAll(contact.emails.map(ContactEmail::value))
        addAll(contact.phones.map(ContactPhone::value))
        addAll(contact.addresses.map { it.oneLine() })
    }
    return haystacks.any { it.lowercase().contains(query) }
}
