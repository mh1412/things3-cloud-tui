# Wire format and entities

Source of truth: [`src/wire/`](../src/wire/).
These are the fields and behaviors modeled by this repository,
not a complete server schema.

## Envelope and operations

```json
{
  "A7h5eCi24RvAWKC3Hv3muf": {
    "t": 1,
    "e": "Task7",
    "p": {"tt": "Updated title", "md": 1800000000.0}
  }
}
```

| Field | Meaning |
| --- | --- |
| Object key | Entity identifier |
| `t` | Operation: `0` create/full snapshot, `1` partial update, `2` delete |
| `e` | Versioned entity name |
| `p` | Properties for the operation |

History folding applies records in order.
Create replaces state for that ID; update merges specified fields;
delete removes the object.
Missing patch fields mean unchanged.
Explicit `null` clears supported nullable fields,
and `[]` clears a relationship list.
Do not serialize absent patch fields as null.
Unknown operation values
and entity names are represented explicitly in the Rust types;
that does not make unknown entities safe to mutate.

## IDs

Modern entity IDs are Base58 representations of 16 bytes using:

```text
123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz
```

The parser accepts modern IDs, legacy hyphenated UUIDs,
and historical `ACTIONGROUP-<UUID>` heading IDs.
Legacy UUID canonicalization is:

1. Remove the heading discriminator if present.
2. Normalize the UUID to uppercase hyphenated text.
3. SHA-1 hash that text and take the first 16 bytes.
4. Base58 encode those bytes.

This is **not** simply Base58 of the original UUID bytes.
New IDs are generated through `ThingsId::random()`.
Normalize both object keys and relationships consistently.
Historical default-tag identifiers required additional handling in the recovery
script; the general `ThingsId` parser does not accept arbitrary legacy tag
strings.

History keys and app-instance IDs are separate identifier classes.
Do not convert them using entity-ID canonicalization.

## Entity versions

| Family | Names recognized locally | Write/read notes |
| --- | --- | --- |
| Tasks, projects, headings | `Task3`, `Task4`, `Task6`, `Task7` | Current mutations write `Task7`; normal mutation paths only upgrade `Task6`/`Task7` |
| Checklists | `ChecklistItem`, `ChecklistItem2`, `ChecklistItem3` | Current writes use `ChecklistItem3` |
| Areas | `Area2`, `Area3` | Current writes use `Area3` |
| Tags | `Tag3`, `Tag4` | Current writes use `Tag4` |
| Settings | `Settings3`, `Settings4`, `Settings5` | No editable settings schema documented |
| Tombstones | `Tombstone`, `Tombstone2` | Deletion metadata |
| Commands | `Command` | One-shot commands, semantics incomplete |

Do not reclassify a legacy entity by changing only `e`.
An unsupported entity may be readable
while still being unsafe to update through modern schemas.

## Task properties

| Key | Type / values | Meaning |
| --- | --- | --- |
| `tt` | string | Title |
| `nt` | string / structured object / null on reads | Notes; see below |
| `tp` | integer: `0` todo, `1` project, `2` heading | Task kind |
| `ss` | integer: `0` incomplete, `2` canceled, `3` completed | Status |
| `st` | integer: `0` inbox, `1` anytime, `2` someday | Scheduling location, not status |
| `sp` | number or null | Completion/cancellation timestamp |
| `sr` | integer timestamp or null | Scheduled/start day |
| `tir` | integer timestamp or null | Today-index reference day |
| `dd` | timestamp or null | Deadline; creates model integer seconds, patches permit fractional numbers |
| `dds` | opaque JSON or null | Suppressed-deadline metadata; exact semantics unknown |
| `pr` | ID array, usually zero/one | Parent project |
| `ar` | ID array, usually zero/one | Area |
| `agr` | ID array, usually zero/one | Heading/action group |
| `tg` | ID array | Tags |
| `ix` | integer | Structural/container order |
| `ti` | integer | Today order |
| `do` | integer, commonly zero | Due-date offset |
| `rr` | recurrence object or null | Legacy recurrence rule |
| `rmd` | opaque JSON or null | Reminder metadata |
| `rp` | opaque JSON or null | Task7 repeater payload |
| `rt` | ID array | Instance-to-template references |
| `icsd` | integer timestamp or null | Instance-creation start day |
| `acrd` | integer timestamp or null | After-completion reference date |
| `icc` | integer | Instance-creation count |
| `icp` | boolean | Instance creation paused |
| `ato` | integer or null | Alarm offset in seconds from day start |
| `lai` | number or null | Last alarm interaction timestamp |
| `sb` | integer, `0` normal / `1` evening | Evening-section bit |
| `lt` | boolean | Leaves tombstone on deletion |
| `tr` | boolean | In Trash |
| `dl` | array of opaque values | Deadline-list metadata |
| `xx` | opaque JSON or null | Conflict-override metadata; algorithm unknown |
| `cd` | number or null | Creation timestamp |
| `md` | number or null | Last user modification timestamp |

Timestamps are Unix seconds, not milliseconds.
Day fields and timezone conversion deserve special care:
`parse_day` currently builds local midnight using the current fixed local
offset, while `today_utc` supplies the CLI's Today reference.
Do not assume UTC midnight and local midnight are interchangeable,
or that this implementation completely specifies Things' DST behavior.

Read support is broader than write support.
`TaskPatch` exposes `tt`, `nt`, `st`, `sr`, `tir`, `pr`, `ar`, `agr`, `tg`,
`sb`, `tp`, `ss`, `sp`, `dd`, `ix`, `ti`, `rr`, `rp`, `rt`, `icsd`, `acrd`,
`icc`, `icp`, `lt`, `tr`, `cd`, and `md`.
Other read properties are not generally writable through current commands.

### Common mutation recipes

All recipes are sparse `Task7` updates unless stated otherwise.
Set `md` to the edit timestamp.
Full creation payloads should come from [`new.rs`](../src/commands/new.rs),
which initializes defaults and ordering.

| Action | Main property changes |
| --- | --- |
| Rename | `tt: "..."` |
| Complete | `ss: 3`, `sp: now` |
| Cancel | `ss: 2`, `sp: now` |
| Reopen | `ss: 0`, `sp: null` |
| Trash | `tr: true`; distinct from a `t=2` delete |
| Anytime | `st: 1`, `sr: null`, `tir: null`, `sb: 0` |
| Today | `st: 1`, `sr: today`, `tir: today`, `sb: 0` |
| Evening | Today fields with `sb: 1` |
| Someday | `st: 2`, `sr: null`, `tir: null`, `sb: 0` |
| Specific day | `sr`/`tir`: day timestamp; `st: 2` if future, otherwise `1`; `sb: 0` |
| Clear deadline | `dd: null` |
| Replace tags | `tg: [ids...]`; `[]` removes all |

Moving to Inbox also clears incompatible location/scheduling relationships;
use the move planner in [`edit.rs`](../src/commands/edit.rs), not just `st=0`.
Projects and headings use task records
and relationships rather than separate HTTP resources.
Headings must not be created as Inbox items.
Tasks assigned to projects/areas normally use `st=1` after being triaged.

The CLI's Today view sorts nonzero `ti` in descending order;
structural views use container ordering and other view-specific keys.
Do not infer that an increase of `ix` or `ti` has the same effect in every list.
See [`store/mod.rs`](../src/store/mod.rs)
and [`reorder.rs`](../src/commands/reorder.rs).

## Notes

The reader accepts legacy strings and structured text.
Current full-text writes:

```json
{"_t":"tx","t":1,"ch":0,"v":""}
```

`ch` is unsigned CRC32 of the UTF-8 bytes of `v`;
zero is correct for the empty string only.
Use [`task6_note`](../src/common.rs) for populated notes.
The function's historical name does not mean the task should be written
as Task6.

Delta notes use `t=2` and an ordered `ps` array:

```json
{"_t":"tx","t":2,"ps":[{"p":0,"l":0,"r":"Example"}]}
```

For each patch, `p` is a **UTF-8 byte offset**, `l` is the byte length removed,
and `r` is replacement text.
Optional patch `ch` checks the resulting entire text after that patch.
Apply sequentially, validate bounds and UTF-8,
and verify checksums when supplied.
Do not interpret offsets as Unicode character counts.
Unknown notes formats cannot safely be treated as empty strings.

The current reader validates patch checksums,
but a full-text `t=1` read takes `v` without validating its top-level checksum.
Recovery tooling performed additional checks.
Empty rendered notes become `None` in the materialized store;
explicitly writing an empty structured object is still meaningful.

## ChecklistItem3

| Key | Type | Meaning |
| --- | --- | --- |
| `tt` | string | Title |
| `ss` | integer | Same status enum as tasks |
| `sp` | number or null | Completion/cancellation date |
| `ts` | ID array | Parent task; reader also accepts a historical single ID |
| `ix` | integer | Checklist order |
| `cd`, `md` | number | Creation/modification dates |
| `lt` | boolean | Tombstone behavior |
| `xx` | opaque JSON | Conflict metadata |

Checklist items are independent entities, not embedded task arrays.
Rename and status changes use `t=1`; removal uses `t=2`.
Patch fields currently cover all listed fields except `lt` and `xx`.
Reopening clears `sp` with null.

## Area3 and Tag4

| Entity | Read properties | Patch properties |
| --- | --- | --- |
| Area3 | `tt` title, `tg` tag IDs, `ix` order, `xx` metadata | `tt`, `tg`, `ix`, `md` |
| Tag4 | `tt` title, `sh` shortcut or null, `pn` parent-tag IDs, `ix` order, `xx` metadata | `tt`, `sh`, `pn`, `ix`, `md` |

Use `pn: []` to remove tag parents and `sh: null` to clear a shortcut.
Empty relationship arrays differ from omitted relationships.

## Recurrence, settings, tombstones, and commands

Legacy `rr` fields:

| Key | Meaning |
| --- | --- |
| `tp` | `0` fixed schedule, `1` after completion |
| `fu` | Frequency bitmask: yearly `4`, monthly `8`, daily `16`, weekly `256` |
| `fa` | Every N units |
| `of` | Offset selector objects; weekday/day/ordinal structures |
| `sr` | Start-day timestamp |
| `ia` | Interval-anchor day |
| `ed` | End day; `64092211200` is an effectively-never sentinel |
| `rc` | Repeat count |
| `ts` | Time span in days; `-1` is an observed sentinel |
| `rrv` | Rule version |

See [`recurrence.rs`](../src/wire/recurrence.rs) for supported offset parsing.
Task7 `rp` remains opaque.
Recurrence changes may require template/instance bookkeeping;
ordinary status changes are not a substitute.
The CLI rejects several repeater mutations rather than claiming complete
recurrence support.

`Tombstone2` properties are `dloid` (deleted entity ID) and `dld`
(deletion timestamp).
A tombstone record, hard-delete operation,
and task trash flag are different concepts.

`Command` models `tp` (integer command type), `cd` (creation date), and `if`
(initial-field object).
Individual command-type meanings are not established here.
Settings3/4/5 properties are not documented as an editable API.
Preserve these records when archiving history instead of inventing replacement
settings.
