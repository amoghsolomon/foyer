# Legacy Cloudflare HTTP API

> **Historical only.** The JavaScript Cloudflare/Flue service that implemented this API was removed
> when Foyer adopted a self-hosted Rust service. This document is retained to identify Android
> migration requirements; it does not define the new server contract.

The Android app sends JSON to the Hono application at the configured server
base URL. Requests use `Authorization: Bearer <signed-better-auth-token>` in
both local and production environments.

## Session, account administration, and model integration

- `GET /api/session` returns the authenticated principal.
- `POST /api/auth/sign-in/email` accepts `email`, `password`, and `rememberMe`.
  Public signup is disabled. Android persists the signed `set-auth-token`
  response header in encrypted local storage.
- `POST /api/admin/accounts` provisions an allowlisted email/password account or
  resets its password. `DELETE /api/admin/accounts/:email` disables it. Both
  require `X-Foyer-Admin-Token` and are backend-administration endpoints.
- `GET /api/admin/metrics/summary` requires the same administrator token and
  returns last-seven-day aggregate run counts, status, latency, token usage, and
  tool-call counts grouped by run kind. It never returns prompts or replies.
- `GET /api/integrations/grok` returns the app-wide SuperGrok connection state
  and whether the current email may manage it.
- `POST /api/integrations/grok/device/start` begins `pi-grok-cli` device-code
  authorization and returns `flowId`, `userCode`, `verificationUri`, polling
  interval, and expiry.
- `POST /api/integrations/grok/device/poll` accepts `{ "flowId": "..." }` and
  performs one token-endpoint poll. It returns `pending` with a retry interval or
  `connected`; Android repeats this call after the advertised delay.
- `DELETE /api/integrations/grok` removes the encrypted shared model credential.

The Grok credential consumes the owner's subscription quota for all allowlisted
Foyer users. Access tokens, refresh tokens, and pending device codes are
AES-GCM encrypted before being written to D1.

## Per-user connections

- `GET /api/connections` lists the authenticated user's configured connections
  without returning tokens.
- `PUT /api/connections/:provider`, where `provider` is a lowercase generic
  provider slug, stores a credential result from a trusted client. The JSON body is
  `{ "accessToken": "...", "refreshToken": "...", "expiresAt": "<ISO-8601>", "scopes": ["..."] }`.
  `refreshToken`, `expiresAt`, and `scopes` are optional; omitting a refresh
  token preserves an existing one.
- `DELETE /api/connections/:provider` removes the authenticated user's
  connection.

Access and refresh tokens are AES-GCM encrypted in D1. The service does not
perform provider-specific authorization or token refresh; expired access tokens
are marked as requiring reconnection. MCP attachment is driven only by stored
connections that match a configured `MCP_*_URL` binding.

## Agent and memory

- `POST /api/chat` accepts `{ "message": "..." }`, creates a durable Activity
  thread, and returns its queued Activity. This is the compatibility entry point
  used by older Android builds.
- Interactive Android assistant turns call
  `POST /api/flue/agents/foyer/:userId?wait=result` and render `result.text`.
  When the agent requests one device action, the text ends with a single
  `<foyer-client-action>{...}</foyer-client-action>` block. Android strips the
  block, validates its allowlisted type and arguments, applies its own
  confirmation policy, and never executes arbitrary intents or code.
- URL opening, web search, and navigation are not client actions. The assistant
  includes ordinary HTTPS or geo/maps links in reply text; Android renders them
  as tappable rich previews and never opens them automatically.
- If a turn consumes output from `browse_page`, any `mcp__*` tool, or notification
  context, only the local `recall`, `memory_search`, `activity_search`,
  `activity_get`, `agenda_list`, and `note_search` tools remain available. A
  second browse, MCP call, delegation, sandbox shell command, write, or
  destructive action is rejected before execution and records a
  `taint_rejected` audit row. The server also strips every client-action block
  before serving or storing the application-visible assistant reply.
- Rejected outbound/write actions create a user-and-Activity-scoped pending
  action containing the exact rejected input. The assistant relays its short
  description and ID. On the next user message, the internal
  `confirm_pending_action` tool may consume and execute it only when that message
  clearly confirms the specific action; it accepts no replacement arguments.
  Consumption is one-time, and unconsumed records become unavailable after 15
  minutes. The fresh user turn is otherwise untainted, so newly requested
  actions use their normal tools.
- `GET /api/flue/agents/foyer/:userId` streams the authenticated user's durable
  agent events. Cross-user agent IDs return `404`.
- `GET /api/memories?limit=&cursor=` lists the authenticated user's non-deleted
  memories newest first. Each item includes `kind`, `importance`, `created_at`,
  and the stored content. The response contains an opaque `nextCursor` or `null`.
- `POST /api/memories` creates a memory from `content`, optional `kind`, and
  optional `importance`.
- `DELETE /api/memories/:id` removes a memory owned by the user, de-indexes its
  Vectorize record, and appends its sync change record.
- `GET /api/profile` returns `{ "profile": null }` until consolidation has run;
  otherwise `profile` contains the current consolidated `text` and `updatedAt`.

## User settings

- `GET /api/settings` returns `{ "settings": null }` until onboarding has saved
  settings, otherwise it returns the authenticated user's timezone and timestamps.
- `PUT /api/settings` accepts a partial object containing
  `{ "timezone": "<IANA time-zone>" }` and/or
  `{ "notificationWhitelist": ["com.example.app"] }`. It validates IANA zones
  with the runtime `Intl` implementation and Android package names with the
  server's package-name grammar. Creating settings for the first time still
  requires `timezone`; omitted fields are preserved on later updates.

Heartbeat quiet hours, heartbeat daily caps, and briefing generation use the
stored setting first. A timezone inferred from an existing activity/briefing
record is only a compatibility fallback; the service never substitutes UTC when
no timezone is known.

## Device notifications

- `POST /api/notifications/batch` accepts an authenticated JSON array of 1–100
  notifications. Each item is
  `{ "id", "appPackage", "title"?, "body", "postedAt", "redacted" }`.
  `id` is client-supplied and idempotent per user; duplicates are ignored and
  reported in the response counts.
- Every `appPackage` in a batch must be present in the authenticated user's
  `notificationWhitelist`. A single non-whitelisted package rejects the entire
  batch with `403`; an empty whitelist accepts none.
- Bodies are stored as supplied for no more than seven days. `redacted` is a
  required client assertion; redacted bodies are excluded from agent context.
  Notification text is not indexed into recall or Vectorize.

Heartbeat and hourly briefing generation may include a capped digest of the
last 12 hours. This digest is untrusted input: it cannot authorize tools or
client actions, and any action block on a consuming turn is suppressed.

## Home briefing

- `GET /api/home-briefing?timezone=<iana-zone>` returns the latest cached home
  briefing and whether a refresh is in progress. Calling it records recent app
  activity and queues generation when the briefing is missing, expired, dirty,
  or belongs to a previous local date.
- `dailyMessage` is generated once per local day and remains stable across the
  hourly insight refreshes. It intentionally contains no weather.
- `insight` is either `null` or a concise message attached to one validated
  canonical `activity`, `calendar`, or `task` ID. The generator cannot create a
  new agenda item or reminder merely to populate the card.

The hourly cron refreshes briefings only for users who opened Foyer during the
previous seven days. Activity and agenda/task mutations mark a briefing stale;
Android keeps showing its cached value while the next generation is queued.

## Activities

Each Activity owns one persistent Flue agent instance and begins as a normal
conversation. A conversation becomes a job only when the agent has enough
information to atomically save a versioned job definition and its first
schedule. Android cannot promote a conversation by attaching a schedule. A job
thread keeps the same transcript for discussion, while scheduled and manual job
runs execute the saved definition rather than the thread title or latest chat
message. Different Activities never share implicit conversation history.

- `GET /api/activities?q=` lists up to 100 non-archived Activities. The optional
  query searches titles, summaries, and latest results.
- `GET /api/activities/:id` returns an Activity with its ordered messages and
  optional schedule.
- `POST /api/activities` accepts `message` and optional `title`. It creates a
  conversation and immediately queues an interactive agent turn. Supplying a
  schedule is rejected; promotion belongs to the agent tool path.
- `POST /api/activities/:id/messages` appends a user message and queues the next
  conversational turn in the same persistent Activity agent instance. In a job
  thread this discusses or refines the job; it does not itself execute the job.
- `PATCH /api/activities/:id` renames a thread.
- `POST /api/activities/:id/archive` archives a thread, disables its schedule,
  and cancels queued runs.
- `DELETE /api/activities/:id` permanently removes the application-owned
  transcript, definitions, schedule, and runs after aborting unsettled agent work.
- `PUT /api/activities/:id/schedule` creates or replaces a schedule from
  `runAt`, `frequency` (`once`, `daily`, or `weekly`), optional `interval`, and
  `timezone`. It is accepted only for a job with a saved definition.
- `DELETE /api/activities/:id/schedule` disables the active schedule.
- `POST /api/activities/:id/run` executes the current saved job definition once
  without changing its schedule.
- `POST /api/activities/:id/runs/:runId/retry` retries a failed conversational
  turn or job execution from its persisted prompt snapshot.

Every definition change creates a new immutable version. Each job run records
the definition ID and prompt snapshot it used. The minute cron claims due
schedules idempotently, appends results to the Activity transcript, and advances
daily or weekly schedules before execution so overlapping cron deliveries do
not claim the same occurrence twice.

## Notes vault

D1 is the canonical notes store. Android keeps a disposable Room read cache;
note writes are online-only direct requests and never enter the device mutation
outbox.

- `GET /api/notes?q=` returns folders, notes, backlinks, and recent-note IDs.
- `GET /api/notes/:id` returns one note.
- `POST /api/notes` accepts `title`, Markdown `body`, `folderId`, and optional
  `tags`.
- `PATCH /api/notes/:id` accepts the edited fields plus the last-read `version`.
  A stale version returns `409 stale_note` without overwriting server data.
- `DELETE /api/notes/:id?version=N` deletes only the expected version.
- `POST /api/note-folders` creates a folder from `name`.

Successful writes are also recorded in the ordered changes feed. WorkManager
refreshes the complete notes catalog into Room so server- or agent-authored
notes appear in the app. Losing the Android database never loses a note.

## Offline synchronization

Android writes calendar and task changes to a Room outbox first. WorkManager
submits up to 100 mutations at a time:

```json
{
  "mutations": [
    {
      "mutationId": "device-generated-idempotency-key",
      "deviceId": "android-installation-id",
      "entityType": "task",
      "operation": "upsert",
      "payload": {
        "title": "Example",
        "notes": "Optional",
        "dueAt": "2026-07-20",
        "completed": false,
        "expectedVersion": 1
      }
    }
  ]
}
```

- `POST /api/sync/mutations` accepts task or calendar `upsert`, `delete`, and
  task `complete` mutations. `mutationId` makes retries idempotent.
- `GET /api/sync/changes?cursor=N` returns ordered changes and `nextCursor` for
  Room read-model updates.
- `GET /api/agenda?from=<iso>&to=<iso>&includeCompleted=false` returns canonical
  agenda occurrences and date-only tasks. Completed tasks are hidden by
  default.

The server is authoritative. Successful mutations immediately return the
canonical entity and version; stale `expectedVersion` values return `409`.
Calendar recurrence edits and deletes apply to the whole series.

## Files

`POST /api/files` accepts a raw body up to 20 MiB, with optional `x-filename`
and `content-type` headers. The object is private in R2 and its metadata is
stored in D1.
