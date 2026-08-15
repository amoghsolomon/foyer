package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.amazity.foyer.contacts.ContactDraft
import com.amazity.foyer.contacts.addressTypes
import com.amazity.foyer.contacts.contactTypes
import com.amazity.foyer.contacts.toDraft
import com.amazity.foyer.contacts.typeLabel
import com.amazity.foyer.contacts.validateBookName
import com.amazity.foyer.contacts.validateContactDraft
import com.amazity.foyer.model.AddressBook
import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.ContactEmail
import com.amazity.foyer.model.ContactPhone
import com.amazity.foyer.model.ContactPostalAddress
import com.amazity.foyer.model.ContactsCatalog
import com.amazity.foyer.model.ContactsStatus
import com.amazity.foyer.model.ContactsSyncBanner
import com.amazity.foyer.model.StructuredContactName
import com.amazity.foyer.ui.components.ChevronGlyph
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.ErrorStatePanel
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.LoadingStatePanel
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.PlusGlyph
import com.amazity.foyer.ui.components.SearchGlyph
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerSurfaceRaised
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted

fun contactSectionKey(displayName: String): String {
    val first = displayName.trim().firstOrNull { it.isLetterOrDigit() } ?: '#'
    return if (first.isLetter()) first.uppercaseChar().toString() else "#"
}

fun groupedContacts(contacts: List<Contact>): List<Pair<String, List<Contact>>> =
    contacts.groupBy { contactSectionKey(it.displayName) }
        .toSortedMap(compareBy { if (it == "#") "zzz" else it })
        .map { it.key to it.value }

@Composable
fun ContactsPage(
    catalog: ContactsCatalog,
    selectedBookId: String?,
    searchQuery: String,
    onSearchQueryChange: (String) -> Unit,
    onSelectBook: (String?) -> Unit,
    onOpenContact: (String) -> Unit,
    onCreateContact: () -> Unit,
    onCreateAddressBook: (String) -> Unit = {},
    isLoading: Boolean = false,
    errorMessage: String? = null,
    onRetry: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    var namingBook by rememberSaveable { mutableStateOf(false) }
    val visible = catalog.search(searchQuery, selectedBookId)
    val groups = groupedContacts(visible)
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(top = 14.dp, bottom = 88.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SectionLabel("Contacts")
            Row {
                Box(
                    modifier = Modifier.clickable { namingBook = true }.padding(8.dp),
                ) {
                    Text("Book", style = MaterialTheme.typography.labelMedium, color = FoyerTextMuted)
                }
                Box(
                    modifier = Modifier.clickable(onClick = onCreateContact).padding(8.dp),
                ) { PlusGlyph() }
            }
        }
        Spacer(Modifier.height(8.dp))
        ContactsStatusBanner(catalog.status)
        ContactSearchField(query = searchQuery, onQueryChange = onSearchQueryChange)
        Spacer(Modifier.height(12.dp))
        AddressBookFilterRow(
            books = catalog.addressBooks,
            selectedId = selectedBookId,
            onSelect = onSelectBook,
        )
        Spacer(Modifier.height(16.dp))
        when {
            isLoading || catalog.status.loading -> {
                LoadingStatePanel("Loading your address books")
                return@Column
            }
            (errorMessage != null || catalog.status.lastError != null) &&
                catalog.addressBooks.isEmpty() && catalog.contacts.isEmpty() -> {
                ErrorStatePanel(errorMessage ?: catalog.status.lastError.orEmpty(), onRetry)
                return@Column
            }
            catalog.addressBooks.isEmpty() -> {
                ContentStatePanel(
                    "No address books yet",
                    "Create an address book to start collecting people.",
                    "New book",
                    { namingBook = true },
                )
                return@Column
            }
            visible.isEmpty() && searchQuery.isNotBlank() -> {
                ContentStatePanel("No matches", "Nothing in this book matches “$searchQuery”.")
                return@Column
            }
            visible.isEmpty() -> {
                ContentStatePanel(
                    "No contacts yet",
                    "Add someone to this address book.",
                    "New contact",
                    onCreateContact,
                )
                return@Column
            }
        }
        groups.forEach { (letter, contacts) ->
            SectionLabel(letter)
            Spacer(Modifier.height(4.dp))
            contacts.forEachIndexed { index, contact ->
                ContactRow(contact = contact, onClick = { onOpenContact(contact.id) })
                if (index != contacts.lastIndex) HairlineDivider()
            }
            Spacer(Modifier.height(18.dp))
        }
    }
    if (namingBook) {
        AddressBookNameDialog(
            title = "New address book",
            initial = "",
            confirmLabel = "Create",
            onDismiss = { namingBook = false },
            onConfirm = { name ->
                namingBook = false
                onCreateAddressBook(name)
            },
        )
    }
}

@Composable
fun ContactDetailScreen(
    catalog: ContactsCatalog,
    contactId: String,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
    onBack: () -> Unit,
) {
    val contact = catalog.contact(contactId) ?: return
    val book = catalog.addressBook(contact.addressBookId)
    var confirmingDelete by rememberSaveable { mutableStateOf(false) }
    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(title = contact.displayName, onBack = onBack)
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState()),
            ) {
                Spacer(Modifier.height(20.dp))
                ContactsStatusBanner(catalog.status)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    ContactAvatar(contact.initials())
                    Spacer(Modifier.width(14.dp))
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(contact.displayName, style = MaterialTheme.typography.titleLarge, color = FoyerText)
                        if (contact.jobTitle.isNotBlank() || contact.organization.isNotBlank()) {
                            Text(
                                listOf(contact.jobTitle, contact.organization).filter { it.isNotBlank() }.joinToString(" · "),
                                style = MaterialTheme.typography.bodyMedium,
                                color = FoyerTextMuted,
                            )
                        }
                        Text(
                            book?.displayName ?: "Unknown book",
                            style = MaterialTheme.typography.bodySmall,
                            color = FoyerTextDim,
                        )
                    }
                }
                Spacer(Modifier.height(18.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    Text(
                        "Edit",
                        style = MaterialTheme.typography.labelMedium,
                        color = FoyerText,
                        modifier = Modifier.clickable(onClick = onEdit).padding(vertical = 8.dp),
                    )
                    Text(
                        "Delete",
                        style = MaterialTheme.typography.labelMedium,
                        color = FoyerTextMuted,
                        modifier = Modifier.clickable { confirmingDelete = true }.padding(vertical = 8.dp),
                    )
                }
                Spacer(Modifier.height(10.dp))
                if (!contact.name.isBlank()) {
                    DetailBlock("Name", contact.name.formatted())
                }
                contact.emails.forEach { email ->
                    DetailBlock("${typeLabel(email.type)} email", email.value)
                }
                contact.phones.forEach { phone ->
                    DetailBlock("${typeLabel(phone.type)} phone", phone.value)
                }
                contact.addresses.forEach { address ->
                    DetailBlock("${typeLabel(address.type)} address", address.oneLine())
                }
                contact.birthday?.takeIf { it.isNotBlank() }?.let { DetailBlock("Birthday", it) }
                if (contact.notes.isNotBlank()) {
                    DetailBlock("Notes", contact.notes)
                }
                Spacer(Modifier.height(28.dp))
            }
        }
    }
    if (confirmingDelete) {
        ConfirmDeleteContactDialog(
            name = contact.displayName,
            onDismiss = { confirmingDelete = false },
            onConfirm = {
                confirmingDelete = false
                onDelete()
            },
        )
    }
}

@Composable
fun ContactEditorScreen(
    catalog: ContactsCatalog,
    contact: Contact?,
    initialBookId: String?,
    onCancel: () -> Unit,
    onSave: (ContactDraft) -> Unit,
    saving: Boolean = false,
    saveError: String? = null,
) {
    var displayName by rememberSaveable(contact?.id) { mutableStateOf(contact?.displayName.orEmpty()) }
    var givenName by rememberSaveable(contact?.id) { mutableStateOf(contact?.name?.givenName.orEmpty()) }
    var familyName by rememberSaveable(contact?.id) { mutableStateOf(contact?.name?.familyName.orEmpty()) }
    var organization by rememberSaveable(contact?.id) { mutableStateOf(contact?.organization.orEmpty()) }
    var jobTitle by rememberSaveable(contact?.id) { mutableStateOf(contact?.jobTitle.orEmpty()) }
    var birthday by rememberSaveable(contact?.id) { mutableStateOf(contact?.birthday.orEmpty()) }
    var notes by rememberSaveable(contact?.id) { mutableStateOf(contact?.notes.orEmpty()) }
    var addressBookId by rememberSaveable(contact?.id) {
        mutableStateOf(contact?.addressBookId ?: initialBookId ?: catalog.addressBooks.firstOrNull()?.id.orEmpty())
    }
    var emails by remember(contact?.id) {
        mutableStateOf(contact?.emails?.ifEmpty { listOf(ContactEmail("")) } ?: listOf(ContactEmail("")))
    }
    var phones by remember(contact?.id) {
        mutableStateOf(contact?.phones?.ifEmpty { listOf(ContactPhone("")) } ?: listOf(ContactPhone("")))
    }
    var addresses by remember(contact?.id) {
        mutableStateOf(contact?.addresses?.ifEmpty { listOf(ContactPostalAddress()) } ?: listOf(ContactPostalAddress()))
    }
    var pickingBook by rememberSaveable { mutableStateOf(false) }
    val draft = ContactDraft(
        displayName = displayName,
        name = StructuredContactName(givenName = givenName, familyName = familyName),
        emails = emails,
        phones = phones,
        organization = organization,
        jobTitle = jobTitle,
        addresses = addresses,
        birthday = birthday,
        notes = notes,
        addressBookId = addressBookId,
    )
    val validation = validateContactDraft(draft)
    val canSave = !saving && validation == null
    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(
                title = if (contact == null) "New contact" else "Edit contact",
                onBack = onCancel,
            )
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(bottom = 28.dp),
            ) {
                Spacer(Modifier.height(16.dp))
                ContactsStatusBanner(catalog.status)
                EditorField("Display name", displayName, { displayName = it })
                EditorField("Given name", givenName, { givenName = it })
                EditorField("Family name", familyName, { familyName = it })
                EditorField("Organization", organization, { organization = it })
                EditorField("Job title", jobTitle, { jobTitle = it })
                EditorField("Birthday", birthday, { birthday = it }, placeholder = "YYYY-MM-DD")
                Spacer(Modifier.height(10.dp))
                val selectedBook = catalog.addressBook(addressBookId)
                SectionLabel("Address book")
                Spacer(Modifier.height(6.dp))
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { pickingBook = true }
                        .padding(vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        selectedBook?.displayName ?: "Select an address book",
                        style = MaterialTheme.typography.titleMedium,
                        color = FoyerText,
                        modifier = Modifier.weight(1f),
                    )
                    ChevronGlyph()
                }
                HairlineDivider()
                Spacer(Modifier.height(16.dp))
                SectionLabel("Emails")
                emails.forEachIndexed { index, email ->
                    TypedValueRow(
                        value = email.value,
                        type = email.type,
                        types = contactTypes,
                        onValue = { next ->
                            emails = emails.toMutableList().also { rows ->
                                rows[index] = email.copy(value = next)
                            }
                        },
                        onType = { next ->
                            emails = emails.toMutableList().also { rows ->
                                rows[index] = email.copy(type = next)
                            }
                        },
                    )
                }
                Text(
                    "Add email",
                    style = MaterialTheme.typography.labelMedium,
                    color = FoyerTextMuted,
                    modifier = Modifier.clickable { emails = emails + ContactEmail("") }.padding(vertical = 10.dp),
                )
                SectionLabel("Phones")
                phones.forEachIndexed { index, phone ->
                    TypedValueRow(
                        value = phone.value,
                        type = phone.type,
                        types = contactTypes,
                        onValue = { next ->
                            phones = phones.toMutableList().also { rows ->
                                rows[index] = phone.copy(value = next)
                            }
                        },
                        onType = { next ->
                            phones = phones.toMutableList().also { rows ->
                                rows[index] = phone.copy(type = next)
                            }
                        },
                    )
                }
                Text(
                    "Add phone",
                    style = MaterialTheme.typography.labelMedium,
                    color = FoyerTextMuted,
                    modifier = Modifier.clickable { phones = phones + ContactPhone("") }.padding(vertical = 10.dp),
                )
                SectionLabel("Addresses")
                addresses.forEachIndexed { index, address ->
                    EditorField("Street", address.street, { next ->
                        addresses = addresses.toMutableList().also { rows ->
                            rows[index] = address.copy(street = next)
                        }
                    })
                    EditorField("City", address.locality, { next ->
                        addresses = addresses.toMutableList().also { rows ->
                            rows[index] = address.copy(locality = next)
                        }
                    })
                    EditorField("Region", address.region, { next ->
                        addresses = addresses.toMutableList().also { rows ->
                            rows[index] = address.copy(region = next)
                        }
                    })
                    EditorField("Postal code", address.postalCode, { next ->
                        addresses = addresses.toMutableList().also { rows ->
                            rows[index] = address.copy(postalCode = next)
                        }
                    })
                    EditorField("Country", address.country, { next ->
                        addresses = addresses.toMutableList().also { rows ->
                            rows[index] = address.copy(country = next)
                        }
                    })
                    TypeChips(addressTypes, address.type) { type ->
                        addresses = addresses.toMutableList().also { it[index] = address.copy(type = type) }
                    }
                    Spacer(Modifier.height(8.dp))
                }
                Text(
                    "Add address",
                    style = MaterialTheme.typography.labelMedium,
                    color = FoyerTextMuted,
                    modifier = Modifier
                        .clickable { addresses = addresses + ContactPostalAddress() }
                        .padding(vertical = 10.dp),
                )
                EditorField("Notes", notes, { notes = it }, singleLine = false)
                if (saveError != null || validation != null) {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        saveError ?: validation.orEmpty(),
                        style = MaterialTheme.typography.bodySmall,
                        color = FoyerTextMuted,
                    )
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    if (saving) "Saving…" else "Save",
                    style = MaterialTheme.typography.labelMedium,
                    color = if (canSave) FoyerText else FoyerTextDim,
                    modifier = Modifier
                        .clickable(enabled = canSave) { onSave(draft) }
                        .padding(vertical = 10.dp),
                )
            }
        }
    }
    if (pickingBook) {
        AddressBookPickerDialog(
            books = catalog.addressBooks,
            selectedId = addressBookId,
            onDismiss = { pickingBook = false },
            onConfirm = { id ->
                addressBookId = id
                pickingBook = false
            },
        )
    }
}

@Composable
fun AddressBookPickerDialog(
    books: List<AddressBook>,
    selectedId: String?,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Address book", color = FoyerText) },
        text = {
            Column {
                books.forEach { book ->
                    val selected = book.id == selectedId
                    Text(
                        book.displayName,
                        style = MaterialTheme.typography.bodyLarge,
                        color = if (selected) FoyerText else FoyerTextMuted,
                        fontWeight = if (selected) FontWeight.Medium else FontWeight.Normal,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onConfirm(book.id) }
                            .padding(vertical = 10.dp),
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
        containerColor = FoyerBlack,
    )
}

@Composable
fun ConfirmDeleteContactDialog(
    name: String,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Delete contact?", color = FoyerText) },
        text = {
            Text("“$name” will be removed from this address book.", color = FoyerTextMuted)
        },
        confirmButton = {
            TextButton(onClick = onConfirm) { Text("Delete") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
        containerColor = FoyerBlack,
    )
}

@Composable
fun ContactsStatusBanner(status: ContactsStatus, modifier: Modifier = Modifier) {
    val banner = status.banner() ?: return
    val (title, message) = when (banner) {
        is ContactsSyncBanner.Offline -> "Offline" to if (banner.pendingUploads == 0) {
            "Reading the local replica. New changes will upload when Foyer Server is reachable."
        } else {
            "${banner.pendingUploads} change(s) are queued and will upload when you are back online."
        }
        is ContactsSyncBanner.Pending -> "Pending sync" to
            "${banner.pendingUploads} change(s) are waiting to upload to Foyer Server."
        is ContactsSyncBanner.StaleEtag -> "Stale contact" to banner.message
        is ContactsSyncBanner.Error -> "Couldn’t sync" to banner.message
    }
    Surface(
        modifier = modifier.fillMaxWidth().padding(bottom = 12.dp),
        shape = RoundedCornerShape(14.dp),
        color = FoyerBlack,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Column(Modifier.padding(horizontal = 14.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(title, style = MaterialTheme.typography.labelMedium, color = FoyerText)
            Text(message, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
        }
    }
}

@Composable
private fun ContactSearchField(query: String, onQueryChange: (String) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, FoyerLine, RoundedCornerShape(14.dp))
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SearchGlyph()
        Spacer(Modifier.width(10.dp))
        BasicTextField(
            value = query,
            onValueChange = onQueryChange,
            singleLine = true,
            textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
            cursorBrush = SolidColor(FoyerText),
            modifier = Modifier.weight(1f),
            decorationBox = { inner ->
                if (query.isEmpty()) {
                    Text("Search name, email, phone", style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
                }
                inner()
            },
        )
    }
}

@Composable
private fun AddressBookFilterRow(
    books: List<AddressBook>,
    selectedId: String?,
    onSelect: (String?) -> Unit,
) {
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        FilterChip("All", selectedId == null) { onSelect(null) }
        books.forEach { book ->
            FilterChip(book.displayName, selectedId == book.id) { onSelect(book.id) }
        }
    }
}

@Composable
private fun FilterChip(label: String, selected: Boolean, onClick: () -> Unit) {
    Surface(
        modifier = Modifier.clickable(onClick = onClick),
        shape = RoundedCornerShape(20.dp),
        color = if (selected) FoyerSurfaceRaised else FoyerBlack,
        border = BorderStroke(1.dp, if (selected) FoyerTextDim else FoyerLine),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = if (selected) FoyerText else FoyerTextMuted,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
        )
    }
}

@Composable
private fun ContactRow(contact: Contact, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ContactAvatar(contact.initials())
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(contact.displayName, style = MaterialTheme.typography.titleMedium, color = FoyerText)
            if (contact.subtitle().isNotBlank()) {
                Text(contact.subtitle(), style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
            }
        }
        ChevronGlyph()
    }
}

@Composable
private fun ContactAvatar(initials: String) {
    Box(
        modifier = Modifier
            .size(36.dp)
            .background(FoyerSurfaceRaised, CircleShape)
            .border(1.dp, FoyerLine, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        Text(initials, style = MaterialTheme.typography.labelMedium, color = FoyerText)
    }
}

@Composable
private fun DetailBlock(label: String, value: String) {
    Spacer(Modifier.height(14.dp))
    SectionLabel(label)
    Spacer(Modifier.height(4.dp))
    Text(value, style = MaterialTheme.typography.bodyLarge, color = FoyerText)
    Spacer(Modifier.height(10.dp))
    HairlineDivider()
}

@Composable
private fun EditorField(
    label: String,
    value: String,
    onValue: (String) -> Unit,
    placeholder: String = "",
    singleLine: Boolean = true,
) {
    Spacer(Modifier.height(12.dp))
    SectionLabel(label)
    Spacer(Modifier.height(4.dp))
    BasicTextField(
        value = value,
        onValueChange = onValue,
        singleLine = singleLine,
        textStyle = MaterialTheme.typography.bodyLarge.copy(color = FoyerText),
        cursorBrush = SolidColor(FoyerText),
        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
        decorationBox = { inner ->
            if (value.isEmpty() && placeholder.isNotEmpty()) {
                Text(placeholder, style = MaterialTheme.typography.bodyLarge, color = FoyerTextDim)
            }
            inner()
        },
    )
    HairlineDivider()
}

@Composable
private fun TypedValueRow(
    value: String,
    type: String,
    types: List<String>,
    onValue: (String) -> Unit,
    onType: (String) -> Unit,
) {
    EditorField("Value", value, onValue)
    TypeChips(types, type, onType)
}

@Composable
private fun TypeChips(types: List<String>, selected: String, onSelect: (String) -> Unit) {
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()).padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        types.forEach { type ->
            FilterChip(typeLabel(type), selected == type) { onSelect(type) }
        }
    }
}

@Composable
private fun AddressBookNameDialog(
    title: String,
    initial: String,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var value by rememberSaveable { mutableStateOf(initial) }
    val error = validateBookName(value)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title, color = FoyerText) },
        text = {
            Column {
                BasicTextField(
                    value = value,
                    onValueChange = { value = it },
                    singleLine = true,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(color = FoyerText),
                    cursorBrush = SolidColor(FoyerText),
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                )
                if (error != null && value.isNotBlank()) {
                    Text(error, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
                }
            }
        },
        confirmButton = {
            TextButton(enabled = error == null, onClick = { onConfirm(value.trim()) }) {
                Text(confirmLabel)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
        containerColor = FoyerBlack,
    )
}

@Suppress("unused")
fun editorSeed(contact: Contact?, bookId: String?): ContactDraft =
    contact?.toDraft() ?: ContactDraft(addressBookId = bookId.orEmpty())
