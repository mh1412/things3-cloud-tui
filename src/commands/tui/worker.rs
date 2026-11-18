use std::{sync::mpsc, thread, time::Duration};

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;

use crate::{
    app::Cli,
    cmd_ctx::{CmdCtx, DefaultCmdCtx},
    ids::ThingsId,
    store::{RawState, ThingsStore, fold_item},
    wire::wire_object::{OperationType, WireItem},
};

pub enum Request {
    Activity,
    Quit,
    Reorder(Box<super::mutations::ReorderSelection>),
    Refresh,
    Save(Box<super::editor::Draft>),
    Edit {
        draft: Box<super::editor::Draft>,
        close: bool,
    },
    Batch {
        ids: Vec<ThingsId>,
        action: super::mutations::Action,
    },
}

pub enum Update {
    Status(String),
    Quit,
    Loaded {
        store: Box<ThingsStore>,
        today: DateTime<Utc>,
        message: String,
        keep_editor: bool,
        keep_selection: bool,
    },
    Failed(String),
}

pub fn start(cli: &Cli) -> Result<(mpsc::Sender<Request>, smol::channel::Receiver<Update>)> {
    let mut options = Cli::parse_from(["things3"]);
    options.no_cloud = cli.no_cloud || cli.load_journal.is_some();
    options.no_sync = cli.no_sync;
    options.load_journal = cli.load_journal.clone();
    options.today_ts = cli.today_ts;
    options.now_ts = cli.now_ts;
    let (send, receive) = mpsc::channel();
    let (updates, events) = smol::channel::unbounded();
    thread::Builder::new()
        .name("things3-tui-sync".into())
        .spawn(move || {
            // Errors are returned to the status bar; stderr logs would corrupt
            // the alternate screen (especially with --log-level debug).
            let _logging =
                tracing::subscriber::set_default(tracing::subscriber::NoSubscriber::default());
            let mut state = RawState::new();
            let mut pending = WireItem::new();
            let mut first = true;
            let mut retry = true;
            let mut ctx = DefaultCmdCtx::from_cli(&options);
            let mut invalid_edit = false;
            loop {
                let request = if first {
                    first = false;
                    Request::Refresh
                } else {
                    let received = if pending.is_empty() || !retry {
                        receive
                            .recv()
                            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                    } else {
                        receive.recv_timeout(Duration::from_secs(2))
                    };
                    match received {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let message = match flush_pending(
                                &mut pending,
                                &mut ctx,
                                &options,
                                &mut state,
                            ) {
                                Ok(()) => "Saved to Things Cloud".into(),
                                Err(error) => {
                                    retry = false;
                                    format!(
                                        "Error: {error:#} · {} pending records retained; r retries",
                                        pending.len()
                                    )
                                }
                            };
                            if updates.send_blocking(Update::Status(message)).is_err() {
                                break;
                            }
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                };
                if matches!(request, Request::Activity) {
                    retry = true;
                    continue;
                }
                if matches!(request, Request::Quit) {
                    if invalid_edit {
                        let _ = updates.send_blocking(Update::Failed(
                            "Fix the invalid edit before quitting".into(),
                        ));
                        continue;
                    }
                    match flush_pending(&mut pending, &mut ctx, &options, &mut state) {
                        Ok(()) => {
                            let _ = updates.send_blocking(Update::Quit);
                            break;
                        }
                        Err(error) => {
                            retry = false;
                            if updates
                                .send_blocking(Update::Failed(format!(
                                    "{error:#} · Quit canceled; pending changes retained"
                                )))
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                    }
                }
                let editing = matches!(request, Request::Edit { .. });
                let keep_selection = matches!(request, Request::Reorder(_));
                let keep_editor = matches!(request, Request::Edit { close: false, .. });
                let result: Result<String> = match request {
                    Request::Refresh => (|| {
                        flush_pending(&mut pending, &mut ctx, &options, &mut state)?;
                        state = options.load_state()?;
                        ctx = DefaultCmdCtx::from_cli(&options);
                        Ok(if options.no_cloud {
                            "Local preview loaded"
                        } else if options.no_sync {
                            "Cache loaded"
                        } else {
                            "Synced with Things Cloud"
                        }
                        .to_string())
                    })(),
                    request => (|| {
                        // Plan against the optimistic state, including earlier queued edits.
                        let changes = match request {
                            Request::Reorder(selection) => super::mutations::reorder_selection(
                                &state,
                                &selection,
                                ctx.now_timestamp(),
                                ctx.today_timestamp(),
                            )?,
                            Request::Save(draft) => super::mutations::save_plan(
                                &state,
                                &draft,
                                ctx.now_timestamp(),
                                ctx.today_timestamp(),
                            )?,
                            Request::Edit { draft, .. } => super::mutations::autosave_plan(
                                &state,
                                &draft,
                                ctx.now_timestamp(),
                                ctx.today_timestamp(),
                            )?,
                            Request::Batch { ids, action } => super::mutations::batch_plan(
                                &state,
                                &ids,
                                action,
                                ctx.now_timestamp(),
                                ctx.today_timestamp(),
                            )?,
                            Request::Refresh | Request::Activity | Request::Quit => unreachable!(),
                        };
                        if !changes.is_empty() {
                            if !options.no_cloud {
                                enqueue(&mut pending, changes.clone());
                            }
                            fold_item(changes, &mut state);
                        }
                        Ok(if options.no_cloud {
                            "Updated in local preview".to_string()
                        } else {
                            format!("{} pending records · saves after 2s idle", pending.len())
                        })
                    })(),
                };
                retry = result.is_ok();
                if editing {
                    invalid_edit = result.is_err();
                }
                let update = match result {
                    Ok(message) => Update::Loaded {
                        store: Box::new(ThingsStore::from_raw_state(&state)),
                        today: ctx.today(),
                        message,
                        keep_editor,
                        keep_selection,
                    },
                    Err(error) => Update::Failed(format!("{error:#}")),
                };
                if updates.send_blocking(update).is_err() {
                    break;
                }
            }
        })?;
    Ok((send, events))
}

fn flush(pending: &mut WireItem, ctx: &mut dyn CmdCtx) -> Result<()> {
    if !pending.is_empty() {
        ctx.commit_changes(pending.clone(), None)?;
        pending.clear();
    }
    Ok(())
}

fn flush_pending(
    pending: &mut WireItem,
    ctx: &mut DefaultCmdCtx,
    options: &Cli,
    state: &mut RawState,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    // Refresh once per idle batch, not per keystroke/action. Recreate the writer
    // only after the append-log cursor has advanced to the current cloud head.
    if !options.no_cloud && !options.no_sync {
        let mut latest = options.load_state()?;
        fold_item(pending.clone(), &mut latest);
        *state = latest;
        *ctx = DefaultCmdCtx::from_cli(options);
    }
    match flush(pending, ctx) {
        Ok(()) => Ok(()),
        Err(error)
            if !options.no_cloud
                && error
                    .chain()
                    .any(|cause| cause.to_string().starts_with("HTTP 409 ")) =>
        {
            // Another client may write between sync and commit. Fetch a fresh
            // cursor and retry once; keep the queue intact if that also fails.
            let mut sync = Cli::parse_from(["things3"]);
            sync.no_sync = false;
            let mut latest = sync.load_state()?;
            fold_item(pending.clone(), &mut latest);
            *state = latest;
            *ctx = DefaultCmdCtx::from_cli(options);
            flush(pending, ctx)
        }
        Err(error) => Err(error),
    }
}

fn enqueue(pending: &mut WireItem, changes: WireItem) {
    for (id, change) in changes {
        if let Some(previous) = pending.get_mut(&id) {
            if change.operation_type == OperationType::Delete
                && previous.operation_type == OperationType::Create
            {
                pending.remove(&id);
            } else if change.operation_type == OperationType::Update
                && previous.operation_type != OperationType::Delete
            {
                let mut properties = previous.properties_map();
                properties.extend(change.properties_map());
                previous.payload = properties.into();
                previous.entity_type = change.entity_type.or(previous.entity_type.clone());
            } else {
                pending.insert(id, change);
            }
        } else {
            pending.insert(id, change);
        }
    }
}

#[cfg(test)]
fn apply_status(
    state: &mut RawState,
    id: &ThingsId,
    status: crate::wire::task::TaskStatus,
    ctx: &mut dyn CmdCtx,
) -> Result<()> {
    let store = ThingsStore::from_raw_state(state);
    let changes =
        crate::commands::mark::status_change(&store, &id.to_string(), status, ctx.now_timestamp())?;
    ctx.commit_changes(changes.clone(), None)?;
    // Never report a local success before the server has accepted the change.
    fold_item(changes, state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::task::TaskStatus;
    use crate::{
        store::fold_items,
        wire::wire_object::{WireItem, WireObject},
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn queue_coalesces_edits_and_retains_failed_saves() {
        use crate::wire::{
            task::{TaskPatch, TaskProps},
            wire_object::EntityType,
        };
        let id = "A7h5eCi24RvAWKC3Hv3muf".to_string();
        let mut pending = WireItem::new();
        enqueue(
            &mut pending,
            BTreeMap::from([(
                id.clone(),
                WireObject::create(
                    EntityType::Task7,
                    TaskProps {
                        title: "Original".into(),
                        ..Default::default()
                    },
                ),
            )]),
        );
        enqueue(
            &mut pending,
            BTreeMap::from([(
                id.clone(),
                WireObject::update(
                    EntityType::Task7,
                    TaskPatch {
                        title: Some("Edited".into()),
                        ..Default::default()
                    },
                ),
            )]),
        );
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[&id].operation_type, OperationType::Create);
        assert_eq!(pending[&id].properties_map()["tt"], "Edited");
        let mut ctx = TestCtx {
            fail: true,
            commits: 0,
        };
        assert!(flush(&mut pending, &mut ctx).is_err());
        assert_eq!(pending.len(), 1);
        ctx.fail = false;
        flush(&mut pending, &mut ctx).unwrap();
        assert!(pending.is_empty());
        assert_eq!(ctx.commits, 2);
        flush(&mut pending, &mut ctx).unwrap();
        assert_eq!(ctx.commits, 2);
    }

    #[test]
    fn create_then_delete_needs_no_cloud_write() {
        use crate::wire::{checklist::ChecklistItemProps, wire_object::EntityType};
        let id = "A7h5eCi24RvAWKC3Hv3muf".to_string();
        let mut pending = WireItem::new();
        enqueue(
            &mut pending,
            BTreeMap::from([(
                id.clone(),
                WireObject::create(EntityType::ChecklistItem3, ChecklistItemProps::default()),
            )]),
        );
        enqueue(
            &mut pending,
            BTreeMap::from([(id, WireObject::delete(EntityType::ChecklistItem3))]),
        );
        assert!(pending.is_empty());
    }

    struct TestCtx {
        fail: bool,
        commits: usize,
    }
    impl CmdCtx for TestCtx {
        fn now_timestamp(&self) -> f64 {
            100.0
        }
        fn today_timestamp(&self) -> i64 {
            0
        }
        fn next_id(&mut self) -> String {
            unreachable!()
        }
        fn current_head_index(&self) -> i64 {
            0
        }
        fn commit_changes(
            &mut self,
            changes: BTreeMap<String, WireObject>,
            _: Option<i64>,
        ) -> Result<i64> {
            self.commits += 1;
            assert_eq!(changes.len(), 1);
            if self.fail {
                anyhow::bail!("network unavailable");
            }
            Ok(1)
        }
    }

    #[test]
    fn failed_write_does_not_change_local_task_and_reopen_clears_completion() {
        let item: WireItem = serde_json::from_value(json!({
            "A7h5eCi24RvAWKC3Hv3muf": {"t":0,"e":"Task7","p":{"tt":"Task","st":1,"ss":0}}
        }))
        .unwrap();
        let mut state = fold_items([item]);
        let id = "A7h5eCi24RvAWKC3Hv3muf".parse().unwrap();
        let mut ctx = TestCtx {
            fail: true,
            commits: 0,
        };
        assert!(apply_status(&mut state, &id, TaskStatus::Completed, &mut ctx).is_err());
        assert_eq!(
            ThingsStore::from_raw_state(&state).tasks_by_uuid[&id].status,
            TaskStatus::Incomplete
        );
        ctx.fail = false;
        apply_status(&mut state, &id, TaskStatus::Completed, &mut ctx).unwrap();
        assert_eq!(
            ThingsStore::from_raw_state(&state).tasks_by_uuid[&id]
                .stop_date
                .unwrap()
                .timestamp(),
            100
        );
        apply_status(&mut state, &id, TaskStatus::Incomplete, &mut ctx).unwrap();
        assert!(
            ThingsStore::from_raw_state(&state).tasks_by_uuid[&id]
                .stop_date
                .is_none()
        );
    }

    #[test]
    fn legacy_entity_cannot_bypass_cli_status_validation() {
        let item: WireItem = serde_json::from_value(json!({
            "BFD7264C-2C2F-4400-B6D5-AE0F5F985ADB": {"t":0,"e":"Task3","p":{"tt":"Legacy","ss":0}}
        }))
        .unwrap();
        let mut state = fold_items([item]);
        let id = "BFD7264C-2C2F-4400-B6D5-AE0F5F985ADB".parse().unwrap();
        let mut ctx = TestCtx {
            fail: false,
            commits: 0,
        };
        assert!(apply_status(&mut state, &id, TaskStatus::Completed, &mut ctx).is_err());
        assert_eq!(ctx.commits, 0);
    }
}
