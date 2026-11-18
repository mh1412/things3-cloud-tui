use std::{sync::mpsc, thread};

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;

use crate::{
    app::Cli,
    cmd_ctx::{CmdCtx, DefaultCmdCtx},
    ids::ThingsId,
    store::{RawState, ThingsStore, fold_item},
};

pub enum Request {
    Reorder {
        id: ThingsId,
        anchor: ThingsId,
        down: bool,
        today_order: bool,
    },
    Refresh,
    Save(Box<super::editor::Draft>),
    Batch {
        ids: Vec<ThingsId>,
        action: super::mutations::Action,
    },
}

pub enum Update {
    Loaded {
        store: Box<ThingsStore>,
        today: DateTime<Utc>,
        message: String,
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
            for request in std::iter::once(Request::Refresh).chain(receive) {
                let mut ctx = DefaultCmdCtx::from_cli(&options);
                let result = match request {
                    Request::Refresh => options.load_state().map(|loaded| {
                        state = loaded;
                        if options.no_cloud {
                            "Local preview loaded"
                        } else if options.no_sync {
                            "Cache loaded"
                        } else {
                            "Synced with Things Cloud"
                        }
                        .to_string()
                    }),
                    request => (|| {
                        // Revalidate against the latest state before writing. Keep simulated
                        // changes in memory when running against a fixture or local preview.
                        if !options.no_cloud && !options.no_sync {
                            state = options.load_state()?;
                        }
                        let changes = match request {
                            Request::Reorder {
                                id,
                                anchor,
                                down,
                                today_order,
                            } => {
                                let args = crate::commands::reorder::ReorderArgs {
                                    item_id: id.to_string(),
                                    before_id: (!down).then(|| anchor.to_string()),
                                    after_id: down.then(|| anchor.to_string()),
                                };
                                crate::commands::reorder::reorder_changes(
                                    &args,
                                    &ThingsStore::from_raw_state(&state),
                                    ctx.now_timestamp(),
                                    ctx.today_timestamp(),
                                    today_order,
                                )?
                            }
                            Request::Save(draft) => super::mutations::save_plan(
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
                            Request::Refresh => unreachable!(),
                        };
                        if !changes.is_empty() {
                            ctx.commit_changes(changes.clone(), None)?;
                            fold_item(changes, &mut state);
                        }
                        Ok(if options.no_cloud {
                            "Updated in local preview"
                        } else {
                            "Saved to Things Cloud"
                        }
                        .to_string())
                    })(),
                };
                let update = match result {
                    Ok(message) => Update::Loaded {
                        store: Box::new(ThingsStore::from_raw_state(&state)),
                        today: ctx.today(),
                        message,
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
