# things3-cloud

[![Main](https://github.com/evanpurkhiser/things3-cloud/actions/workflows/main.yml/badge.svg)](https://github.com/evanpurkhiser/things3-cloud/actions/workflows/main.yml)

A Rust command-line client for [Things 3](https://culturedcode.com/things/)
that talks directly to the Things Cloud API.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/demo-dark.gif">
  <source media="(prefers-color-scheme: light)" srcset=".github/assets/demo-light.gif">
  <img src=".github/assets/demo-light.gif" alt="Demo of the things3 command-line interface">
</picture>

```bash
things3 find --query "rent" --deadline "<=2026-03-31"
things3 new "Follow up with team" --when today
things3 schedule <task-id> --deadline 2026-04-10
things3 mark <task-id> --done
```

## Install

Via Homebrew:

```bash
brew install evanpurkhiser/personal/things3-cloud
```

Via Arch Linux AUR:

```bash
yay -S things3-cloud
```

From crates.io:

```bash
cargo install things3-cloud
```

From source:

```bash
cargo install --path .
```

## Configure auth

```bash
things3 set-auth
```

You can also set credentials via environment variables:

```bash
export THINGS3_EMAIL="you@example.com"
export THINGS3_PASSWORD="your-password"
```

Environment variables override values in the auth file.

## Interactive mode

```bash
things3 tui
things3 tui --view inbox
things3 --no-sync tui
```

Browse Inbox, Today, Upcoming, Anytime, Someday, Logbook, and Projects.
Wide terminals show task details alongside the list; on smaller terminals,
press `Enter` or `d` to see notes, dates, tags, and checklists.

| Key | Action |
| --- | --- |
| `1`–`7`, `Tab`, `←` / `→` | Switch lists |
| `j` / `k`, `↓` / `↑` | Move the cursor |
| `c` / `e` | Create / edit a task |
| `x` | Toggle selection of the cursor task |
| `* a` / `* n` | Select all visible tasks / clear selection |
| `y` / `v` / `!` | Complete / reopen / cancel |
| `t` / `s` / `#` | Schedule today / someday / trash |
| `w` | Open calendar to schedule selected tasks (or the focused task) |
| `g i`, `g t`, `g a`, `g s`, `g l`, `g p` | Jump to Inbox, Today, Anytime, Someday, Logbook, Projects |
| `Home` / `End`, `PgUp` / `PgDn` | Jump through the list |
| `o` / `Enter` | Open a project or toggle task details |
| `/` | Search the current list, including notes and checklists |
| `Enter` / `Esc` while searching | Keep / clear the filter |
| `Space` | Complete or reopen the selected task |
| `Shift-J` / `Shift-K` | Move the focused task down / up |
| `Ctrl-D` / `Ctrl-U` | Scroll task details down / up |
| `u` / `Esc` | Clear the filter or go back |
| `r` | Refresh |
| `?` | Show help |
| `q` / `Ctrl-C` | Quit |

Cloud operations run in a background worker. Status changes use the same
validation as `mark`, and are shown as saved only after a successful commit.
As with other commands, `--no-sync` skips cloud reads but still permits writes.

Actions apply to selected tasks, or the cursor task when nothing is selected.
Each batch is validated in full and submitted in one commit. Changing lists
clears the selection.

Selected rows are highlighted. Reordering applies to the focused task within
its container; Upcoming preserves date groups and Logbook stays date-ordered.

The task editor includes title, multiline notes, schedule, deadline, tags,
project/area location, and checklist titles and statuses. Use `Tab`/`Shift-Tab`
to switch fields, `Ctrl-U` to clear a field, `Ctrl-S` to save, and `Esc` to
discard. In notes, `Enter` inserts a newline. `Ctrl-N` adds a checklist item,
`Ctrl-D` removes the focused item, and `F2` cycles its status. Failed saves
keep the draft open. Project metadata editing is not included.

In the calendar, arrows (or `h/j/k/l`) move by day/week, `PgUp`/`PgDn`
(or `[`/`]`) change months, and `t` jumps to Today. `Enter` applies the
date; `Esc`/`u` goes back. In the editor, `F4` opens the calendar for the
deadline when that field is focused, otherwise for the schedule field.

To try the interface with sample data from a source checkout:

```bash
cargo run -- --load-journal tests/fixtures/tui.json tui --view inbox
```

A loaded journal (or `--no-cloud`) enables **local preview**: status changes
stay in memory, and `r` reloads the original fixture/cache.

## Supported features

Roadmap items are tracked in [`ROADMAP.md`](ROADMAP.md).

- [x] Configure auth with `set-auth` (stored in XDG state)
- [x] Incremental sync with local cache for fast startup
- [x] `--no-sync` flag to skip cloud sync and use local cache only

### Views

- [x] `inbox`, `today`, `upcoming`, `anytime`, `someday`, `logbook`
- [x] `projects` / `project <id>`, `areas` / `area <id>`, `tags`
- [x] Show notes via `--detailed`
- [x] Show checklist items via `--detailed`
- [x] `find` with title/notes/checklists
      and tag/area/project/status/date filters
- [x] `tui` — interactive lists, search, details, and completion toggles

### Tasks

- [x] `new` — create tasks with title, notes, tags, when, deadline, and position
- [x] `edit` — rename, set/remove notes, move, add/remove tags
      (multi-ID supported)
- [x] `edit --add-checklist/--remove-checklist/--rename-checklist`
- [x] `mark` — done/incomplete/canceled (multi-ID supported)
- [x] `mark --check/--uncheck/--check-cancel` for checklist toggles
- [x] `schedule` — when/start date, deadline, today/evening/someday
- [ ] Read and update recurrence schedules
- [x] `reorder` — reorder tasks within lists
- [x] `delete` — trash tasks

### Projects / Areas / Tags

- [x] `projects new` / `projects edit`
- [x] `areas new` / `areas edit`
- [x] `tags new` / `tags edit` / `tags delete`

## Testing

Run all Rust tests:

```bash
cargo test --all-targets
```

The CLI snapshot suite uses `trycmd` test cases in `trycmd/`.

## Related projects

**Things Cloud API clients** (same approach as this project)

- [wbopan/things-cloud-mcp](https://github.com/wbopan/things-cloud-mcp)
  (Go MCP server via Cloud API)
- [arthursoares/things-cloud-sdk](https://github.com/arthursoares/things-cloud-sdk)
  (Go SDK for Things Cloud)

**Local database / URL scheme clients**
(read from local SQLite, write via URL scheme or AppleScript)

- [ossianhempel/things3-cli](https://github.com/ossianhempel/things3-cli)
  (Go CLI, local database)
- [thingsapi/things.py](https://github.com/thingsapi/things.py)
  (Python library, reads database)
- [thingsapi/things-cli](https://github.com/thingsapi/things-cli)
  (Python CLI built on things.py)
- [GarthDB/rust-things3](https://github.com/GarthDB/rust-things3)
  (Rust library and CLI with MCP)

**MCP servers** (local, via AppleScript or URL scheme)

- [drjforrest/mcp-things3](https://github.com/drjforrest/mcp-things3)
  (Python MCP, AppleScript + x-call)
- [rossshannon/Things3-MCP](https://github.com/rossshannon/Things3-MCP)
  (Python MCP with read/write)
