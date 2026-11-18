# HTTP endpoints

## Transport and request conventions

Base URL: `https://cloud.culturedcode.com/version/1`

All paths below are relative to that URL.
The Rust client sends these headers on requests;
whether each is strictly required by the server is not established.

| Header | Rust client value |
| --- | --- |
| `Accept` | `application/json` |
| `Accept-Charset` | `UTF-8` |
| `User-Agent` | `ThingsMac/32209501` |
| `App-Id` | `com.culturedcode.ThingsMac` |
| `App-Instance-Id` | `THINGS_APP_INSTANCE_ID`, default `things3-cloud` |
| `Schema` | `301` |
| `things-client-info` | Base64-encoded JSON client metadata |
| `Content-Type` | `application/json; charset=UTF-8`, when sending JSON |
| `Content-Encoding` | `UTF-8`, when sending JSON; this is observed protocol usage, not compression |
| `Push-Priority` | `10` for commits; the pinned SDK uses `5` |

Decoded client metadata contains `dm`
(device model),
`lr` (region), `nf` and `nk` (undocumented booleans), `nn` (app name), `nv`
(app version),
`on` (OS name), `ov` (OS version), `pl` (language), and `ul` (locale).
See `CLIENT_INFO` in [`client.rs`](../src/client.rs)
for the exact current profile.

The Rust client treats any 2xx response as success,
parses nonempty bodies as JSON, and treats empty bodies as `{}`.
Errors currently include HTTP status, URL, and response body.
There is no documented rate limit, request-size limit, idempotency-key header,
or general transaction guarantee.

## Endpoint inventory

| Method | Path | Purpose | Evidence |
| --- | --- | --- | --- |
| GET | `/account/{email}` | Authenticate / account metadata / own history key | Implemented, observed |
| PUT | `/account/{email}` | Sign up, confirm, accept SLA, change password | SDK-only |
| DELETE | `/account/{email}` | Delete account | SDK-only HTTP route |
| GET | `/history/{history-key}` | History metadata | Observed, SDK |
| GET | `/history/{history-key}/items?start-index=N` | Read history pages | Implemented, observed |
| POST | `/history/{history-key}/commit?ancestor-index=N&_cnt=1` | Commit object changes | Implemented, observed |
| GET | `/account/{email}/own-history-keys` | List history keys | SDK-only; route returned 404 during recovery |
| POST | `/account/{email}/own-history-keys` | Create history | SDK-only; not verified working here |
| DELETE | `/account/{email}/own-history-keys/{history-key}` | Delete history | SDK-only |
| PUT | `/app-instance/{app-instance-id}` | Register APNS push destination | SDK-only |

There are no known dedicated task, project, checklist, search, Today,
or Logbook HTTP endpoints.
The client implements these by folding history
and posting entity mutations to `commit`.

## GET account: authentication

```http
GET /version/1/account/{percent-encoded-email}
Authorization: Password {percent-encoded-password}
```

This is the literal `Password` authorization scheme, not HTTP Basic or Bearer.
The Rust client percent-encodes both email and password.
The older SDK inserts them directly;
do not assume the SDK's escaping behavior is equivalent.

The Rust client requires this response field:

```json
{"history-key": "<history-key>"}
```

The SDK also models `email`, `status`, `maildrop-email`, `issues`,
and `SLA-version-accepted`.
Its known active status is `SYAccountStatusActive`.
Other statuses and the full `issues` schema are unknown.

Authentication stores the history key in the client;
it does not establish an OAuth token or a session cookie in this implementation.
Subsequent history requests use the key in the path without an Authorization
header.
Treat the history key as sensitive capability-like information.

## GET history: metadata

```http
GET /version/1/history/{history-key}
```

Observed during recovery.
The SDK expects 200 and models:

| Response field | Meaning |
| --- | --- |
| `latest-server-index` | Latest history head |
| `latest-schema-version` | Latest schema version |
| `latest-total-content-size` | Total history content-size marker |
| `is-empty` | Whether history is empty |

The Rust CLI normally obtains its head through the items endpoint instead.

## GET items: incremental history

```http
GET /version/1/history/{history-key}/items?start-index=0
```

An illustrative page:

```json
{
  "items": [
    {"A7h5eCi24RvAWKC3Hv3muf": {"t": 1, "e": "Task7", "p": {"tt": "Example"}}}
  ],
  "current-item-index": 42,
  "end-total-content-size": 1024,
  "latest-total-content-size": 2048
}
```

`items` is an array of outer history records.
Each record is an object keyed by entity IDs
and can contain multiple entity changes.
`current-item-index` reports the overall head;
it is **not** the next page offset.
Advance `start-index` by the number of outer `items` consumed.
Do not count entities inside each record,
or jump directly to `current-item-index`.

Continue until `end-total-content-size >= latest-total-content-size`.
The SDK also models `start-total-content-size`
(page start-size marker) and `schema` (page schema version).
These are not required by the Rust reader.
The incremental Rust reader also stops on an empty page.
The size fields are completion markers, not interchangeable with record indices.
Page-size controls and a snapshot-consistency guarantee during concurrent writes
are unknown.

## POST commit: create, patch, or delete entities

```http
POST /version/1/history/{history-key}/commit?ancestor-index={head}&_cnt=1
Schema: 301
Push-Priority: 10
Content-Type: application/json; charset=UTF-8
```

The body is one ID-keyed object, **not** an array or an `items` wrapper:

```json
{
  "A7h5eCi24RvAWKC3Hv3muf": {
    "t": 1,
    "e": "Task7",
    "p": {"ss": 3, "sp": 1800000000.0, "md": 1800000000.0}
  },
  "KGvAPpMrzHAKMdgMiERP1V": {
    "t": 1,
    "e": "Task7",
    "p": {"dd": null, "md": 1800000000.0}
  }
}
```

```json
{"server-head-index": 43}
```

Use the returned head for subsequent writes.
`_cnt=1` is used by both clients;
its general semantics and other supported values are unknown.
Do not infer the new head from the number of changed entities.

A stale ancestor can produce HTTP 409.
Refresh history, retain/reconcile the pending patches,
and retry with the new head.
See [conflicts and queued saves](sync-and-recovery.md).
Large recovery commits produced HTTP 413; no exact size limit was established.

## PUT account: SDK-only variants

All variants use `/account/{email}`.
The pinned SDK expects:

| Operation | JSON body | Explicit authorization in SDK | Expected status |
| --- | --- | --- | --- |
| Sign up | `{"password":"<new-password>"}` | None | 201 |
| Confirm | `{"confirmation-code":"<code>"}` | `Password <password>` | 200 |
| Accept SLA | `{"SLA-version-accepted":"<SLA-URL>"}` | `Password <password>` | 200 |
| Change password | `{"password":"<new-password>"}` | None in this revision | 200 |

The SDK's SLA URL is
`https://cloud.culturedcode.com/sla/v1.5-rich.html?language=en`.
Its password-change implementation does not explicitly send authorization,
and the shared transport does not add it.
This is an implementation observation,
not evidence that anonymous password changes work.
The current server contract for these routes is unverified.

## DELETE account: SDK-only

`DELETE /account/{email}` with `Authorization: Password <password>`;
SDK expects 202.
This deletes the account, not just a task or local cache.
The user recreated an account during recovery,
but this HTTP route was not exercised by our code.

## History management: SDK-only / unavailable in recovery

- GET `/account/{email}/own-history-keys`:
  SDK expects 200 and a JSON array of history-key strings.
- POST the same path, with no body:
  SDK expects 200 and `{"new-history-key":"<history-key>"}`.
- DELETE `/account/{email}/own-history-keys/{history-key}`: SDK expects 202.
  Its comment says unknown keys also receive 202;
  that behavior is unverified here.

These SDK methods do not explicitly attach password authorization.
The `own-history-keys` route returned HTTP 404 in our recovery investigation,
so it was not a working history-reset mechanism.
Use the verified GET-account `history-key` for ordinary sync.
There is no verified in-place history-reset API.

## PUT app-instance: SDK-only push registration

```http
PUT /version/1/app-instance/{app-instance-id}
```

```json
{
  "history-key": "<history-key>",
  "apns-token": "<device-token>",
  "app-id": "com.culturedcode.ThingsMac",
  "dev": false
}
```

SDK expects 200.
The instance ID belongs in the path, not the JSON body.
No explicit password authorization is added in that revision.
Token validation, registration lifetime, unregistration,
and push payload shape are unknown.
The Rust CLI does not implement this endpoint.
