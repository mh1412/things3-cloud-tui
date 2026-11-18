# Things Cloud API reference

Last reviewed: 2026-11-18.

This directory records the reverse-engineered Things Cloud protocol known to
this project. It is not an official Cultured Code specification or a claim
that every server endpoint has been discovered.

- [HTTP endpoints](endpoints.md): authentication, accounts, histories, commits,
  and push registration.
- [Wire format](wire-format.md): operations, identifiers, entity versions,
  property dictionaries, notes, and recurrence.
- [Sync and recovery](sync-and-recovery.md): pagination, caches, idle saves,
  conflicts, errors, and compatibility findings.

## Evidence levels

| Label | Meaning |
| --- | --- |
| Implemented | Used by this Rust client; source links identify the implementation. |
| Observed | Exercised during the local November 2026 recovery investigation. |
| SDK-only | Found in the pinned third-party SDK; not verified against the current service here. |
| Unknown | Shape, semantics, limits, or behavior have not been established. |

Examples use synthetic object IDs and placeholder account/history identifiers.
No account passwords, actual history keys, or user task contents are included.

## Sources

The local implementation is the primary reference:

- [`src/client.rs`](../src/client.rs): HTTP transport and core endpoints.
- [`src/log_cache.rs`](../src/log_cache.rs): incremental sync and disk cursor.
- [`src/cloud_writer.rs`](../src/cloud_writer.rs): commit writer lifecycle.
- [`src/wire/`](../src/wire/): typed wire payloads and enums.
- [`src/store/state.rs`](../src/store/state.rs): history folding.
- [`src/ids/things_id.rs`](../src/ids/things_id.rs): identifier normalization.
- [`src/commands/`](../src/commands/): mutation planning.
- [`src/commands/tui/worker.rs`](../src/commands/tui/worker.rs): queued saves.

Additional endpoints come from
[`pkwagner/things-cloud-sdk` at `ffa72e9d0479`](https://github.com/pkwagner/things-cloud-sdk/tree/ffa72e9d0479),
specifically `account.go`, `verify.go`, `histories.go`, `items.go`,
`app_instance.go`, and `client.go`. References to SDK behavior in these docs
mean that revision, not an assertion about the latest SDK.

Local recovery evidence is retained outside this repository in the sibling
backup and restore directories. This reference summarizes the findings without
copying those private artifacts.
