package com.amazity.foyer.contacts

import com.amazity.foyer.model.ContactEmail
import com.amazity.foyer.model.ContactPhone
import com.amazity.foyer.model.ContactPostalAddress
import com.amazity.foyer.model.StructuredContactName
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ContactValidationTest {
    @Test
    fun `partial draft derives display name and drops empty multivalue rows`() {
        val draft = ContactDraft(
            displayName = "  ",
            name = StructuredContactName(givenName = "Ada", familyName = "Lovelace"),
            emails = listOf(ContactEmail(" ada@example.com "), ContactEmail("")),
            phones = listOf(ContactPhone("")),
            addresses = listOf(ContactPostalAddress(), ContactPostalAddress(street = "12 Square")),
            addressBookId = "book",
        ).normalized()
        assertEquals("Ada Lovelace", draft.displayName)
        assertEquals(1, draft.emails.size)
        assertEquals("ada@example.com", draft.emails.single().value)
        assertTrue(draft.phones.isEmpty())
        assertEquals(1, draft.addresses.size)
        assertNull(validateContactDraft(draft))
    }

    @Test
    fun `unknown extra emails survive JSON round trip`() {
        val encoded = emailsJson(
            listOf(
                ContactEmail("ada@example.com", "work", pref = true),
                ContactEmail("ada@home.example", "home"),
            ),
        )
        val parsed = parseEmails(encoded)
        assertEquals(2, parsed.size)
        assertEquals("work", parsed[0].type)
        assertTrue(parsed[0].pref)
        assertEquals("ada@home.example", parsed[1].value)
    }

    @Test
    fun `notes stay lossless including trailing spaces and newlines`() {
        val notes = "Keep  trailing  spaces \nand a newline"
        val draft = ContactDraft(
            displayName = "Ada",
            notes = notes,
            addressBookId = "book",
        )
        assertEquals(notes, draft.normalized().notes)
        assertNull(validateContactDraft(draft))
    }

    @Test
    fun `strict bounds reject oversize and malformed fields`() {
        assertEquals(
            "Display name is too long.",
            validateContactDraft(
                ContactDraft(displayName = "x".repeat(MAX_DISPLAY_NAME + 1), addressBookId = "book"),
            ),
        )
        assertEquals(
            "Enter a valid email address.",
            validateContactDraft(
                ContactDraft(
                    displayName = "Ada",
                    emails = listOf(ContactEmail("not-an-email")),
                    addressBookId = "book",
                ),
            ),
        )
        assertEquals(
            "Birthday must be YYYY-MM-DD.",
            validateContactDraft(
                ContactDraft(displayName = "Ada", birthday = "13/01/1990", addressBookId = "book"),
            ),
        )
        assertEquals(
            "Notes cannot contain NUL bytes.",
            validateContactDraft(
                ContactDraft(displayName = "Ada", notes = "ok\u0000no", addressBookId = "book"),
            ),
        )
        assertEquals("Address book name is required.", validateBookName("   "))
        assertEquals(
            "Address book name is too long.",
            validateBookName("b".repeat(MAX_BOOK_NAME + 1)),
        )
    }

    @Test
    fun `address JSON preserves structured fields`() {
        val encoded = addressesJson(
            listOf(
                ContactPostalAddress(
                    street = "12 Square",
                    locality = "London",
                    postalCode = "SW1Y",
                    country = "UK",
                    type = "home",
                ),
            ),
        )
        val parsed = parseAddresses(encoded).single()
        assertEquals("12 Square", parsed.street)
        assertEquals("London", parsed.locality)
        assertEquals("home", parsed.type)
        assertEquals("12 Square, London, SW1Y, UK", parsed.oneLine())
    }
}
