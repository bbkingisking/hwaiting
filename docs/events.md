# Live events

`GET /api/events` is an optional, cookie-authenticated SSE subscription. Existing
reads and writes work without any subscribers. A browser can use:

```js
const events = new EventSource(`${apiBase}/api/events`, { withCredentials: true });
events.addEventListener('card-reviewed', event => {
  const change = JSON.parse(event.data);
  // Compare change.card_id and change.review_revision with local state.
});
// Close on logout/unmount.
events.close();
```

Every event has a JSON `type` matching its SSE event name, and `mutation_id`
(string or null). Clients may attach an optional UUID `X-Mutation-Id` header to
writes. It is echoed for correlation only: it is not authentication, an
idempotency key, or a device identity. Register it before sending the write,
because an event may arrive before the HTTP response. Invalid UUIDs are ignored.
Cookie/CSRF/CORS deployment settings apply as usual; CORS permits this header.

| Event | Additional fields | Meaning |
|---|---|---|
| `card-reviewed` | `card_id`, `review_revision` | A committed review advances the user's revision; queue/stats may change. |
| `queue-changed` | `card_id`, `suppressed` | A card was suppressed/restored for this user. |
| `settings-changed` | `queue_affected` | User settings or FSRS parameters changed; refetch settings and, when indicated, queue/stat information. |
| `card-updated` | `card_id` | Shared content changed; all connected accounts receive this event. |
| `state-reset` | — | An import may replace settings, history, flags, and review state. |
| `resync-required` | — | Fetch current state, including the validity of any displayed prompt. |

A review emits only `card-reviewed`, not a second `queue-changed`. A settings
change or import likewise emits one event. Failed mutations emit none. Events
contain no answer fields, and user-specific events go only to that user's
subscriptions. Shared-card edits contain only the card ID and correlation ID.

## Recovery and lifetime

Subscriptions are in-memory and bounded (64 notifications per subscriber's user
channel). The server subscribes before sending the initial `resync-required`, so
mutations during a client's initial read are not missed. If a subscriber lags,
the backlog is discarded and `resync-required` is sent. A reconnect always
resynchronises; there is no durable replay or `Last-Event-ID` contract. Events
are invalidations, not a transaction log, and concurrent mutations need not be
observed in commit order. Compare review revisions, and reread authoritative
state for broader changes. Coalesce refreshes and discard responses superseded
by a notification during the read.

The server sends comment heartbeats every 15 seconds, without database queries.
Disconnected subscriptions release their channel when the last receiver leaves.
Streams close at JWT expiry; clients should distinguish an authenticated HTTP
401 from a network outage. EventSource's error event does not expose HTTP status,
so the React client checks `/api/user/me` before treating a failure as logout.

This implementation assumes one backend process and mutations through its API.
Multiple backend processes or external database writers require shared event
distribution. A process crash disconnects streams and clients resynchronise;
notifications are not persisted in SQLite.

## React behaviour

One stream per mounted authenticated app. An unrelated review refreshes stats
and invalidates prefetch, but does not fetch or replace the active card. Queue
changes while empty fetch availability and a new deadline. The local timer
still checks when the known deadline arrives; SSE informs it of mutations that
could change that deadline. Returning with a live stream does not itself cause
a fresh subscription or read.

A stale displayed prompt keeps the user's text, blocks submission, and offers
Next. Own operations are recognised via their mutation IDs and retain their
normal HTTP-response UI flow; local successful mutations still refresh without
SSE. The existing `409 stale_review` guard remains authoritative.

`GET /api/cards/{card_id}/state` returns `exists`, `suppressed`, `review_revision`,
and `content_revision`, without revealing the answer. The next-card response
includes both revisions. Reconnects and imports revalidate displayed prompts;
content revisions detect edits missed while disconnected. This endpoint reports
prompt validity, not whether it would currently rank first or be selected under
new queue preferences.

## Checks

- `cargo test --manifest-path backend/Cargo.toml`: event scoping, cleanup, lag,
  expiry, committed review revisions, stale writes, settings/import/suppression,
  and shared edits.
- In `frontend`, `deno task test`: mutation correlation, refresh coalescing,
  stale-response rejection, retries, connection lifetime, existing deadline tests.
- Build backend and frontend, then in `frontend` run
  `python3 tests/sse-browser.py` (requires Firefox, geckodriver, Python selenium):
  two tabs, delayed review response, preserved text, missed-event recovery,
  unrelated-card request counts, empty-queue wakeup, and remote suppression.
  This uses an isolated temporary database and static directory.
