# Sync, conflicts, and recovery

## Full and incremental sync

1. Authenticate with GET account and identify the current history key.
2. Read `/history/{key}/items?start-index={cursor}`.
3. Fold each outer history record in order.
4. Advance the cursor by the number of outer records read.
5. Record `current-item-index` separately as the latest server head.
6. Continue until the page's end-size marker reaches the latest-size marker.

The read cursor and commit ancestor are related but distinct. A page can report
a head far beyond the records it contains. Using the head as the next page
offset can silently skip data.

The Rust append-log cache consists of:

| File | Purpose |
| --- | --- |
| `things.log` | One JSON history record per line |
| `cursor.json` | `next_start_index`, `history_key`, `head_index`, `updated_at` |
| `state_cache.json` | Materialized state, format version, consumed byte offset |

On macOS these live under
`~/Library/Application Support/things3/append-log/`.
Credentials are separate in `auth.json` in the parent directory. The official
Things app and other third-party clients have independent caches.

Account recreation can produce a different history key. Old append logs and
cursor offsets must not be reused as though they belonged to the new history.
The recovery procedure archived/reset the CLI cache and rebuilt from index 0.
That reset was distinct from changing credentials or the official app database.

## Commit head lifecycle and HTTP 409

Commit with the known current ancestor and retain the `server-head-index`
returned by the server. Never infer it by counting changed objects.

The low-level Rust `ThingsCloudClient::commit` advances its in-memory head but
does not itself append history to disk or update the disk cursor. The writer
initially reads its head from the disk cursor. Recreating a writer after a
successful commit without first syncing that cursor can therefore reintroduce
a stale ancestor, even when no other device has written anything.

The TUI now keeps its writer across actions, refreshes before ordinary idle
saves, and recreates the writer after the refresh. On an HTTP 409 it fetches
history and retries once with a refreshed writer. If that retry fails, queued
changes remain pending. `--no-sync` normally skips cloud reads, but conflict
recovery explicitly fetches history to obtain a usable head.

A 409 establishes a conflict response, not necessarily its complete cause.
Examine the response body when stale-head recovery does not resolve it. The
server's full conflict-resolution rules and `xx` metadata are not known.

## Queued TUI saves

The current implementation:

- Validates actions against its local state and applies them optimistically.
- Combines pending property updates by entity ID.
- Preserves a queued create when later updates modify that new entity.
- Cancels a queued create followed by deletion of the same new entity.
- Flushes after two seconds without keyboard activity, or before refresh/quit.
- Clears the queue only after an accepted commit.
- Keeps failed saves queued; failed quit-saves leave the interface open.

The queue is in memory, not a durable outbox. A forced process termination does
not preserve it. Detail edits queue automatically after a 300ms typing pause;
quick-create confirmation also queues changes. Leaving details submits any last edits;
the status bar separately reports pending versus saved state.

Freshly fetched state is overlaid with pending patches before submission. This
is not a complete three-way semantic merge. Concurrent edits to the same field,
remote deletions, relationship changes, and ambiguous network failures can
require reconciliation. Automatic retry is currently limited to HTTP 409;
there is no established server idempotency-key mechanism. A timeout after
submission does not prove the server rejected the write.

## Errors and observed limits

| Result | Evidence / interpretation |
| --- | --- |
| 2xx | Rust transport accepts it; commit response supplies the new head |
| 401 | SDK explicitly handles unauthorized account/history responses |
| 404 | `own-history-keys` route returned this during recovery; not evidence that task history is empty |
| 409 | Observed during queued TUI writes; stale ancestry is a known local cause |
| 413 | Observed for a large restore commit; splitting into dependency-ordered batches worked |
| Network error / timeout | Outcome may be unknown if submission reached the server |
| Invalid JSON | Rust transport reports parsing failure for nonempty non-JSON success bodies |

No exact payload ceiling, rate limit, retry delay, or maximum entity count per
commit was established. The recovery used 12 batches successfully; that is a
historical observation, not an API limit or recommended universal batch size.

## Compatibility findings

The service accepting a JSON commit does **not** prove the official Things
apps can interpret the resulting history. Relevant observations:

- Older tooling that only recognizes Task6 can miss Task7 data and display an
  incomplete, older-looking state.
- The recovery investigation identified a legacy Task3 record later updated
  as Task6 by third-party tooling. Excluding that malformed transition and
  restoring a normalized snapshot was part of recovery.
- The third-party SDK documents official-app failures involving malformed
  identifiers, creation timestamps, and invalid heading placement. Those
  reports inform compatibility checks but do not prove the exact cause of
  every crash encountered here.
- Modern writes need consistent Base58 entity IDs and normalized references.
- The local task creation planner emits `md: null`; updates set modification
  timestamps. The SDK reports compatibility issues with non-null task `md`
  on creates. This is not a blanket rule for every entity family.
- Headings use task type `tp=2` and must not be Inbox items.
- Notes deltas require UTF-8 byte offsets and checksum-aware reconstruction.
  Reading only the latest delta loses prior text.
- Clearing null notes on an unrelated task did not resolve the observed iOS
  setup crash. Null notes alone were not established as its cause.

External investigation:
[`client-side-bugs.md` at the pinned SDK revision](https://github.com/pkwagner/things-cloud-sdk/blob/ffa72e9d0479/docs/client-side-bugs.md).

## Snapshot restoration versus history replay

The verified recovery approach was:

1. Preserve raw server pages, a full journal, and local databases before edits.
2. Fold the complete history, resolving notes patches and entity relationships.
3. Exclude the identified malformed transition and normalize legacy IDs.
4. Validate references and compare the reconstructed state with exports.
5. Obtain the recreated account's history through GET account.
6. Preserve that account's existing settings record.
7. Upload current entities in dependency order, splitting large payloads.
8. Read back and compare the restored records.
9. Rebuild local caches from the new history.

This restores current task state, including completed/canceled records and
completion dates. It does not recreate the original edit-by-edit cloud journal.
That original history remains in the private backup. Readback equality verifies
server records, not official-app compatibility; an official-app sync remains
a separate check.

No working in-place cloud history reset, history rollback, or supported
server-side database repair endpoint was established.
