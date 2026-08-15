package com.amazity.foyer.contacts

import com.amazity.foyer.model.AddressBook
import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.ContactEmail
import com.amazity.foyer.model.ContactPhone
import com.amazity.foyer.model.ContactsCatalog
import com.amazity.foyer.model.ContactsStatus
import com.amazity.foyer.model.ContactsSyncBanner
import com.amazity.foyer.model.StructuredContactName
import com.amazity.foyer.model.contactsSyncBanner
import com.amazity.foyer.ui.screens.contactSectionKey
import com.amazity.foyer.ui.screens.groupedContacts
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ContactsCatalogTest {
    @Test
    fun `search matches email phone and organization`() {
        val catalog = sampleCatalog()
        assertEquals(listOf("ada"), catalog.search("ADA@").map(Contact::id))
        assertEquals(listOf("ada"), catalog.search("555").map(Contact::id))
        assertEquals(listOf("grace"), catalog.search("navy").map(Contact::id))
        assertTrue(catalog.search("nobody").isEmpty())
    }

    @Test
    fun `address book filter scopes the list`() {
        val catalog = sampleCatalog()
        assertEquals(listOf("ada"), catalog.contactsIn("personal").map(Contact::id))
        assertEquals(listOf("ada", "grace"), catalog.contactsIn(null).map(Contact::id))
    }

    @Test
    fun `move and delete validation follow server rules`() {
        val catalog = sampleCatalog()
        val ada = catalog.contact("ada")!!
        assertNull(catalog.validateMove(ada, "work"))
        assertEquals(
            "The destination address book was not found.",
            catalog.validateMove(ada, "missing"),
        )
        assertEquals(
            "Address book is not empty. Move or delete its contacts first.",
            catalog.validateAddressBookDelete(catalog.addressBook("personal")!!),
        )
        assertNull(catalog.validateAddressBookDelete(catalog.addressBook("empty")!!))
    }

    @Test
    fun `list grouping uses first letter and sorts`() {
        val grouped = groupedContacts(sampleCatalog().contactsIn(null))
        assertEquals(listOf("A", "G"), grouped.map { it.first })
        assertEquals("A", contactSectionKey("ada lovelace"))
        assertEquals("#", contactSectionKey("123 Support"))
    }

    @Test
    fun `sync banner prefers stale etag over offline`() {
        assertEquals(
            ContactsSyncBanner.StaleEtag("This contact changed on another device."),
            contactsSyncBanner(
                ContactsStatus(
                    loading = false,
                    offline = true,
                    pendingUploads = 2,
                    conflictCode = "stale_etag",
                    conflictMessage = "This contact changed on another device.",
                    lastError = "download failed",
                ),
            ),
        )
        assertTrue(
            contactsSyncBanner(ContactsStatus(loading = false, offline = true, pendingUploads = 3))
                is ContactsSyncBanner.Offline,
        )
        assertNull(contactsSyncBanner(ContactsStatus(loading = false, connected = true)))
    }

    private fun sampleCatalog() = ContactsCatalog(
        addressBooks = listOf(
            AddressBook("personal", displayName = "Personal"),
            AddressBook("work", displayName = "Work"),
            AddressBook("empty", displayName = "Empty"),
        ),
        contacts = listOf(
            Contact(
                id = "ada",
                addressBookId = "personal",
                displayName = "Ada Lovelace",
                name = StructuredContactName(givenName = "Ada", familyName = "Lovelace"),
                emails = listOf(ContactEmail("ada@example.com", "work")),
                phones = listOf(ContactPhone("+1-555-0100", "cell")),
                organization = "Analytical",
            ),
            Contact(
                id = "grace",
                addressBookId = "work",
                displayName = "Grace Hopper",
                organization = "Navy",
            ),
        ),
    )
}
