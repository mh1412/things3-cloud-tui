# things3-cloud-tui roadmap

This fork focuses on a keyboard-first terminal interface for Things Cloud.
The [README feature overview](README.md#features) describes what is available
today. The upstream project's development plans are maintained separately in
[evanpurkhiser/things3-cloud](https://github.com/evanpurkhiser/things3-cloud).

## Current gaps and potential follow-up work

These are candidates for future work, not scheduled releases or commitments.

- **Durable pending changes:** the autosave queue currently lives in memory.
  Persisting it would allow recovery after an interrupted session.
- **Concurrent-edit reconciliation:** stale-history retries refresh the cloud
  head, but the TUI does not provide a complete three-way merge or conflict UI.
- **Project, area, and tag management in the TUI:** tasks can use these
  containers and tags; their metadata management remains a CLI workflow.
- **Recurrence and reminders:** no full TUI editor or complete repeater
  bookkeeping is implemented. Existing protocol support does not imply all
  recurring-task mutations are available.

## Implementation references

- [Interactive usage and keybindings](README.md#interactive-mode)
- [Things Cloud API reference](doc/README.md)
- [Sync behavior and known limitations](doc/sync-and-recovery.md)
