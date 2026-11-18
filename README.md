# things3-cloud-tui

[![Main](https://github.com/evanpurkhiser/things3-cloud/actions/workflows/main.yml/badge.svg)](https://github.com/evanpurkhiser/things3-cloud/actions/workflows/main.yml)

A Rust command-line client for [Things 3](https://culturedcode.com/things/)
that talks directly to the Things Cloud API.

This project is a TUI-focused fork of
[evanpurkhiser/things3-cloud](https://github.com/evanpurkhiser/things3-cloud),
created by [Evan Purkhiser](https://github.com/evanpurkhiser). The original
project provides the foundation: the Rust CLI, Things Cloud client, sync and
cache infrastructure, and task-management commands. This fork builds on that
work with an interactive terminal interface, keyboard-driven editing,
autosave, and batch actions.

Thanks to Evan and the upstream contributors for making this project possible.
The original [MIT license](LICENSE) and copyright notice are retained.

Protocol reference: [Things Cloud API documentation](doc/README.md).

## TUI demo

Browse Inbox and Today, view and edit task details, and schedule tasks with
the calendar picker. Recorded with synthetic tasks in local-preview mode.

<img src=".github/assets/things3-tui.gif" width="720" alt="Things3 TUI demo: Inbox and Today, detail view and editing, and calendar date selection">

[Watch or download the MP4](.github/assets/things3-tui.mp4).

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
things3 tui --icons emoji
things3 tui --icons plain
things3 --no-sync tui
```

Browse Inbox, Today, Upcoming, Anytime, Someday, Logbook, and Projects.
The dark theme includes colored panels, focus and selection highlights, and
status colors. Nerd Font icons are enabled by default: select a Nerd Font
(such as JetBrainsMono Nerd Font) in your terminal's font settings. Use
`--icons emoji` for emoji or `--icons plain` for text symbols. The global
`--no-color` option uses your terminal's default colors.
Wide terminals show task details alongside the list; on smaller terminals,
press `Enter` or `d` to see notes, dates, tags, and checklists.

| Key | Action |
| --- | --- |
| `1`–`7`, `Tab`, `←` / `→` | Switch lists |
| `j` / `k`, `↓` / `↑` | Move the cursor |
| `c` / `e` | Quick create / edit details |
| `x` | Toggle selection of the cursor task |
| `* a` / `* n` | Select all visible tasks / clear selection |
| `y` / `v` / `-` | Complete / reopen / cancel (`!` also cancels) |
| `t` / `s` / `#` | Schedule today / someday / trash |
| `w` | Open calendar to schedule selected tasks (or the focused task) |
| `g i`, `g t`, `g a`, `g s`, `g l`, `g p` | Jump to Inbox, Today, Anytime, Someday, Logbook, Projects |
| `Home` / `End`, `PgUp` / `PgDn` | Jump through the list |
| `o` / `Enter` | View task details or open a project |
| `e` | Enter detail edit mode |
| `/` | Search the current list, including notes and checklists |
| `Enter` / `Esc` while searching | Keep / clear the filter |
| `Space` | Complete or reopen the selected task |
| `Shift-J` / `Shift-K` | Move selected tasks (or the focused task) down / up |
| `Ctrl-D` / `Ctrl-U` | Scroll task details down / up |
| `u` / `Esc` | Clear the filter or go back |
| `r` | Refresh |
| `?` | Show help |
| `q` / `Ctrl-C` | Quit |

Cloud operations run in a background worker. Changes appear locally immediately
and are queued in memory, then saved together after two seconds without keyboard
activity. Repeated edits to the same item are combined. The status bar distinguishes
pending changes from saved changes. Refresh and quit flush the queue first;
failed saves retain the queue, and a failed quit-save keeps the TUI open.
Status changes use the same validation as `mark`.
As with other commands, `--no-sync` skips cloud reads but still permits writes.

Actions apply to selected tasks, or the cursor task when nothing is selected.
Each batch is validated in full before entering the save queue. Changing lists
clears the selection.

Selected rows are highlighted. Reordering moves each selected block one row
down/up while preserving relative order and selection. Without a selection,
it moves the focused task. Reordering stays within containers; Upcoming
preserves date groups and Logbook stays date-ordered.

Press `c` to type a title directly above the list. `Enter` creates the task;
`Esc` cancels. The new task stays focused, so press `e` to add details.

Details have **view** and **edit** modes in the same panel. On wide terminals,
opening details focuses the existing right-hand panel; on narrow terminals,
it fills the screen.

In view mode, `j/k` or arrows move between fields and checklist items. `Space`
toggles the focused checklist item done/open; `y` completes it, `v` reopens it,
and `-` cancels it. `t` schedules Today, `s` schedules
Someday, and `w` opens the calendar (for the deadline when that field is focused,
otherwise for scheduling). `e` enters edit mode; `u`/`Esc` returns to the list.

On a checklist item, `Shift-J` / `Shift-K` moves it down / up, keeping focus
on the moved item. Checklist order autosaves with the other detail changes.
It includes title, multiline notes, schedule, deadline, tags,
project/area location, and checklist titles and statuses. Use `Tab`/`Shift-Tab`
to switch fields and `Ctrl-U` to clear a field. Edits automatically enter the
save queue after a brief pause; no save shortcut is required. `Esc` returns to
view mode while keeping edits (`Ctrl-S` remains an optional equivalent). In notes,
`Enter` inserts a newline. `Ctrl-N` adds a checklist item,
`Ctrl-D` removes the focused item, and `Ctrl-T` cycles its status. Failed saves
keep the editor open. Invalid fields must be corrected before leaving.
Project metadata editing is not included.

Idle saves refresh cloud history first. If another device advances history
before the commit, the TUI refreshes and retries once; failed retries keep the
pending queue.

In the calendar, arrows (or `h/j/k/l`) move by day/week, `PgUp`/`PgDn`
(or `[`/`]`) change months, and `t` jumps to Today. `Enter` applies the
date; `Esc`/`u` goes back. In details, `Ctrl-P` opens the calendar for the
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
