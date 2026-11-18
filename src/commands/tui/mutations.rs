use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, anyhow, ensure};

use super::editor::Draft;
use crate::{
    commands::{
        TagDeltaArgs,
        edit::{EditArgs, build_edit_plan},
        mark::status_change,
        new::{NewArgs, build_new_plan},
        schedule::{ScheduleArgs, build_schedule_plan},
    },
    common::{resolve_tag_ids, task6_note},
    ids::ThingsId,
    store::{RawState, ThingsStore, fold_item},
    wire::{
        checklist::{ChecklistItemPatch, ChecklistItemProps},
        task::{TaskPatch, TaskStatus},
        wire_object::{EntityType, WireItem, WireObject},
    },
};

#[derive(Clone, Copy)]
pub enum Action {
    Date(chrono::NaiveDate),
    Toggle,
    Complete,
    Reopen,
    Cancel,
    Today,
    Someday,
    Trash,
}

pub struct ReorderSelection {
    pub visible: Vec<ThingsId>,
    pub selected: Vec<ThingsId>,
    pub down: bool,
    pub today_order: bool,
    pub preserve_dates: bool,
}

pub fn reorder_selection(
    state: &RawState,
    selection: &ReorderSelection,
    now: f64,
    today: i64,
) -> Result<WireItem> {
    use crate::commands::reorder::{ReorderArgs, reorder_changes};
    let selected: HashSet<_> = selection.selected.iter().collect();
    let mut order = selection.visible.clone();
    let mut staged = state.clone();
    let mut changes = WireItem::new();
    let mut positions: Vec<_> = (0..order.len().saturating_sub(1)).collect();
    if selection.down {
        positions.reverse();
    }
    for position in positions {
        let (from, to) = if selection.down {
            (position, position + 1)
        } else {
            (position + 1, position)
        };
        if !selected.contains(&order[from]) || selected.contains(&order[to]) {
            continue;
        }
        let store = ThingsStore::from_raw_state(&staged);
        if selection.preserve_dates {
            let task = store
                .get_task(&order[from].to_string())
                .ok_or_else(|| anyhow!("Task no longer exists"))?;
            let anchor = store
                .get_task(&order[to].to_string())
                .ok_or_else(|| anyhow!("Anchor no longer exists"))?;
            ensure!(
                task.start_date == anchor.start_date,
                "Upcoming tasks can be reordered within the same date"
            );
        }
        let args = ReorderArgs {
            item_id: order[from].to_string(),
            before_id: (!selection.down).then(|| order[to].to_string()),
            after_id: selection.down.then(|| order[to].to_string()),
        };
        let step = reorder_changes(&args, &store, now, today, selection.today_order)?;
        fold_item(step.clone(), &mut staged);
        for (id, object) in step {
            merge(&mut changes, id, object);
        }
        order.swap(from, to);
    }
    Ok(changes)
}

fn merge(changes: &mut WireItem, id: String, object: WireObject) {
    if let Some(existing) = changes.get_mut(&id) {
        let mut props = existing.properties_map();
        props.extend(object.properties_map());
        existing.payload = props.into();
    } else {
        changes.insert(id, object);
    }
}

fn location(store: &ThingsStore, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.eq_ignore_ascii_case("inbox")
        || value.eq_ignore_ascii_case("clear")
    {
        return Ok(String::new());
    }
    let projects = store.projects(Some(TaskStatus::Incomplete));
    let areas = store.areas();
    let mut matches: HashSet<String> = projects
        .iter()
        .filter(|p| p.uuid.to_string().starts_with(value) || p.title.eq_ignore_ascii_case(value))
        .map(|p| p.uuid.to_string())
        .collect();
    matches.extend(
        areas
            .iter()
            .filter(|a| {
                a.uuid.to_string().starts_with(value) || a.title.eq_ignore_ascii_case(value)
            })
            .map(|a| a.uuid.to_string()),
    );
    ensure!(
        matches.len() == 1,
        "Location must match one project/area name or ID: {value}"
    );
    Ok(matches.into_iter().next().unwrap())
}

pub fn save_plan(state: &RawState, draft: &Draft, now: f64, today: i64) -> Result<WireItem> {
    let store = ThingsStore::from_raw_state(state);
    let mut changes = BTreeMap::new();
    let id = draft.id.to_string();
    let when = draft.value(2).trim().to_lowercase();
    ensure!(
        !draft.value(0).trim().is_empty(),
        "Task title cannot be empty"
    );
    ensure!(
        !when.is_empty(),
        "When is required (inbox, anytime, today, evening, someday, or YYYY-MM-DD)"
    );
    let original = draft.original.as_ref();
    let creating = original.is_none();
    let target = if creating || draft.changed(5) {
        Some(location(&store, draft.value(5))?)
    } else {
        None
    };
    let tags = if creating || draft.changed(4) {
        if draft.value(4).trim().is_empty() {
            Some(Vec::new())
        } else {
            let (ids, error) = resolve_tag_ids(&store, draft.value(4));
            ensure!(error.is_empty(), "{error}");
            Some(ids)
        }
    } else {
        None
    };

    if let Some(original) = original {
        let (current, error, _) = store.resolve_mark_identifier(&id);
        let current = current.ok_or_else(|| anyhow!(error))?;
        ensure!(
            current.is_todo(),
            "This editor edits to-dos; open a project to edit its tasks"
        );
        let add_tags = tags.as_ref().map(|ids| {
            ids.iter()
                .filter(|id| !original.tags.contains(id))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        });
        let remove_tags = tags.as_ref().map(|ids| {
            original
                .tags
                .iter()
                .filter(|id| !ids.contains(id))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        });
        let move_target = if draft.changed(2) && when == "inbox" {
            Some("inbox".into())
        } else {
            target
                .clone()
                .map(|s| if s.is_empty() { "clear".into() } else { s })
        };
        if draft.changed(0) || draft.changed(1) || draft.changed(4) || move_target.is_some() {
            let args = EditArgs {
                task_ids: vec![id.clone().into()],
                title: draft.changed(0).then(|| draft.value(0).to_string()),
                notes: draft.changed(1).then(|| draft.value(1).to_string()),
                move_target,
                tag_delta: TagDeltaArgs {
                    add_tags: add_tags.filter(|v| !v.is_empty()),
                    remove_tags: remove_tags.filter(|v| !v.is_empty()),
                },
                add_checklist: vec![],
                remove_checklist: None,
                rename_checklist: vec![],
            };
            let basic_change = args.title.is_some()
                || args.notes.is_some()
                || args.move_target.is_some()
                || args.tag_delta.add_tags.is_some()
                || args.tag_delta.remove_tags.is_some();
            if basic_change {
                changes.extend(
                    build_edit_plan(&args, &store, now, &mut || ThingsId::random().to_string())
                        .map_err(|e| anyhow!(e))?
                        .changes,
                );
            }
        }
    } else {
        ensure!(
            store.get_task(&id).is_none(),
            "This task already exists. Close the editor, refresh, and reopen it to edit."
        );
        let target = target.clone().unwrap_or_default();
        let args = NewArgs {
            title: draft.value(0).into(),
            in_target: if target.is_empty() {
                "inbox".into()
            } else {
                target
            },
            when: None,
            before_id: None,
            after_id: None,
            notes: draft.value(1).into(),
            tags: None,
            deadline_date: None,
        };
        let plan = build_new_plan(&args, &store, now, today, &mut || id.clone())
            .map_err(|e| anyhow!(e))?;
        ensure!(plan.new_uuid == id, "Unexpected task identifier");
        changes = plan.changes;
        merge(
            &mut changes,
            id.clone(),
            WireObject::update(
                EntityType::Task7,
                TaskPatch {
                    notes: Some(task6_note(draft.value(1))),
                    tag_ids: tags,
                    ..Default::default()
                },
            ),
        );
    }

    // Plan scheduling against the task after its create/move patch, then merge it
    // into the same commit as the rest of the editor changes.
    let schedule_when = (creating || draft.changed(2))
        .then(|| when.clone())
        .filter(|s| s != "inbox");
    let deadline_changed = creating || draft.changed(3);
    if schedule_when.is_some() || deadline_changed {
        let mut staged = state.clone();
        fold_item(changes.clone(), &mut staged);
        let staged_store = ThingsStore::from_raw_state(&staged);
        let deadline = draft.value(3).trim();
        let args = ScheduleArgs {
            task_id: id.clone(),
            when: schedule_when,
            deadline_date: (deadline_changed && !deadline.is_empty()).then(|| deadline.to_string()),
            clear_deadline: deadline_changed && deadline.is_empty(),
        };
        let plan = build_schedule_plan(&args, &staged_store, now, today).map_err(|e| anyhow!(e))?;
        merge(
            &mut changes,
            id.clone(),
            WireObject::update(EntityType::Task7, plan.update),
        );
    }

    let original_items = original
        .map(|t| t.checklist_items.as_slice())
        .unwrap_or_default();
    let current = store.get_task(&id);
    let current_items = current
        .as_ref()
        .map(|t| t.checklist_items.as_slice())
        .unwrap_or_default();
    let desired_ids: Vec<_> = draft
        .checklist
        .iter()
        .filter(|item| !item.title.value.trim().is_empty())
        .map(|item| &item.id)
        .collect();
    let current_ids: Vec<_> = current_items.iter().map(|item| &item.uuid).collect();
    let order_changed = desired_ids != current_ids;
    let mut seen = HashSet::new();
    for (position, item) in draft.checklist.iter().enumerate() {
        let position =
            i32::try_from(position).map_err(|_| anyhow!("Checklist ordering overflow"))?;
        ensure!(
            seen.insert(item.id.clone()),
            "Duplicate checklist identifier"
        );
        let title = item.title.value.trim();
        if let Some(before) = original_items.iter().find(|c| c.uuid == item.id) {
            let changed_title = title != before.title;
            let changed_status = item.status != before.status;
            let changed_index = order_changed && before.index != position;
            ensure!(
                !title.is_empty(),
                "Checklist titles cannot be empty; use Ctrl-D to remove an item"
            );
            if changed_title || changed_status || changed_index {
                ensure!(
                    current_items.iter().any(|c| c.uuid == item.id),
                    "A checklist item was removed elsewhere; refresh before editing it"
                );
                changes.insert(
                    item.id.to_string(),
                    WireObject::update(
                        EntityType::ChecklistItem3,
                        ChecklistItemPatch {
                            title: changed_title.then(|| title.to_string()),
                            status: changed_status.then_some(item.status),
                            sort_index: changed_index.then_some(position),
                            stop_date: changed_status.then_some(
                                if item.status == TaskStatus::Incomplete {
                                    None
                                } else {
                                    Some(now)
                                },
                            ),
                            modification_date: Some(now),
                            ..Default::default()
                        },
                    ),
                );
            }
        } else if !title.is_empty() {
            changes.insert(
                item.id.to_string(),
                WireObject::create(
                    EntityType::ChecklistItem3,
                    ChecklistItemProps {
                        title: title.into(),
                        task_ids: vec![draft.id.clone()],
                        status: item.status,
                        sort_index: position,
                        creation_date: Some(now),
                        modification_date: Some(now),
                        stop_date: if item.status == TaskStatus::Incomplete {
                            None
                        } else {
                            Some(now)
                        },
                        ..Default::default()
                    },
                ),
            );
        }
    }
    for item in original_items {
        if !seen.contains(&item.uuid) && current_items.iter().any(|c| c.uuid == item.uuid) {
            changes.insert(
                item.uuid.to_string(),
                WireObject::delete(EntityType::ChecklistItem3),
            );
        }
    }
    Ok(changes)
}

/// Rebase an editor snapshot on our latest optimistic state so that reverting
/// a field to its original value also undoes an earlier autosaved edit.
pub fn autosave_plan(state: &RawState, draft: &Draft, now: f64, today: i64) -> Result<WireItem> {
    let mut model = super::model::Model::new(super::model::List::Inbox);
    model.store = ThingsStore::from_raw_state(state);
    let task = model
        .store
        .get_task(&draft.id.to_string())
        .ok_or_else(|| anyhow!("Task no longer exists"))?;
    let mut current = super::editor::Editor::edit(&model, task).draft;
    current.fields = draft.fields.clone();
    current.checklist = draft.checklist.clone();
    save_plan(state, &current, now, today)
}

pub fn batch_plan(
    state: &RawState,
    ids: &[ThingsId],
    action: Action,
    now: f64,
    today: i64,
) -> Result<WireItem> {
    let store = ThingsStore::from_raw_state(state);
    let mut changes = BTreeMap::new();
    for id in ids {
        let id = id.to_string();
        let (task, error, _) = store.resolve_mark_identifier(&id);
        let task = task.ok_or_else(|| anyhow!(error))?;
        match action {
            Action::Today | Action::Someday | Action::Date(_) => {
                let args = ScheduleArgs {
                    task_id: id.clone(),
                    when: Some(match action {
                        Action::Today => "today".into(),
                        Action::Date(date) => date.format("%Y-%m-%d").to_string(),
                        _ => "someday".into(),
                    }),
                    deadline_date: None,
                    clear_deadline: false,
                };
                let plan =
                    build_schedule_plan(&args, &store, now, today).map_err(|e| anyhow!(e))?;
                changes.insert(id, WireObject::update(EntityType::Task7, plan.update));
            }
            Action::Trash => {
                ensure!(
                    !task.has_repeater(),
                    "Repeater tasks cannot be trashed until repeater bookkeeping is supported"
                );
                changes.insert(
                    id,
                    WireObject::update(
                        EntityType::Task7,
                        TaskPatch {
                            trashed: Some(true),
                            modification_date: Some(Some(now)),
                            ..Default::default()
                        },
                    ),
                );
            }
            _ => {
                let status = match action {
                    Action::Complete => TaskStatus::Completed,
                    Action::Reopen => TaskStatus::Incomplete,
                    Action::Cancel => TaskStatus::Canceled,
                    _ if task.status == TaskStatus::Incomplete => TaskStatus::Completed,
                    _ => TaskStatus::Incomplete,
                };
                if status != task.status {
                    changes.extend(status_change(&store, &id, status, now)?);
                }
            }
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tui::{
        editor::Editor,
        model::{List, Model},
    };
    use crate::store::fold_items;

    fn fixture() -> (RawState, Model) {
        let items: Vec<WireItem> =
            serde_json::from_str(include_str!("../../../tests/fixtures/tui.json")).unwrap();
        let state = fold_items(items);
        let mut model = Model::new(List::Inbox);
        model.store = ThingsStore::from_raw_state(&state);
        model.rebuild(None);
        (state, model)
    }

    #[test]
    fn create_then_edit_notes_and_checklist_in_one_plan() {
        let (mut state, mut model) = fixture();
        let mut editor = Editor::create(&model);
        editor.draft.fields[0] = super::super::editor::Input::new("New task");
        editor.draft.fields[1] = super::super::editor::Input::new("Notes\n你好");
        editor.add_item();
        editor.input().unwrap().insert("First step", false);
        let id = editor.draft.id.clone();
        fold_item(
            save_plan(&state, &editor.draft, 100.0, 0).unwrap(),
            &mut state,
        );
        model.store = ThingsStore::from_raw_state(&state);
        let task = model.store.get_task(&id.to_string()).unwrap();
        assert_eq!(task.notes.as_deref(), Some("Notes\n你好"));
        assert_eq!(task.checklist_items.len(), 1);
        let mut editor = Editor::edit(&model, task);
        editor.draft.checklist[0].title = super::super::editor::Input::new("Renamed");
        editor.draft.checklist[0].status = TaskStatus::Completed;
        fold_item(
            save_plan(&state, &editor.draft, 200.0, 0).unwrap(),
            &mut state,
        );
        let task = ThingsStore::from_raw_state(&state)
            .get_task(&id.to_string())
            .unwrap();
        assert_eq!(task.checklist_items[0].title, "Renamed");
        assert_eq!(task.checklist_items[0].status, TaskStatus::Completed);
        let mut editor = Editor::edit(&model, task);
        editor.draft.checklist.clear();
        fold_item(
            save_plan(&state, &editor.draft, 300.0, 0).unwrap(),
            &mut state,
        );
        assert!(
            ThingsStore::from_raw_state(&state)
                .get_task(&id.to_string())
                .unwrap()
                .checklist_items
                .is_empty()
        );
    }

    #[test]
    fn batch_validates_every_target_before_returning_changes() {
        let (mut state, model) = fixture();
        let ids: Vec<_> = model.tasks.iter().map(|t| t.uuid.clone()).collect();
        assert_eq!(ids.len(), 2);
        let changes = batch_plan(&state, &ids, Action::Complete, 100.0, 0).unwrap();
        assert_eq!(changes.len(), 2);
        let mut invalid = ids.clone();
        invalid.push(ThingsId::random());
        assert!(batch_plan(&state, &invalid, Action::Complete, 100.0, 0).is_err());
        fold_item(changes, &mut state);
        let store = ThingsStore::from_raw_state(&state);
        assert!(
            ids.iter()
                .all(|id| store.tasks_by_uuid[id].status == TaskStatus::Completed)
        );
    }

    #[test]
    fn reorder_moves_focus_down_and_back_up() {
        use crate::commands::reorder::{ReorderArgs, reorder_changes};
        let (mut state, mut model) = fixture();
        let id = model.tasks[0].uuid.to_string();
        let anchor = model.tasks[1].uuid.to_string();
        for down in [true, false] {
            let args = ReorderArgs {
                item_id: id.clone(),
                before_id: (!down).then(|| anchor.clone()),
                after_id: down.then(|| anchor.clone()),
            };
            let changes = reorder_changes(&args, &model.store, 100.0, 0, false).unwrap();
            fold_item(changes, &mut state);
            model.store = ThingsStore::from_raw_state(&state);
            model.rebuild(Some(id.parse().unwrap()));
            assert_eq!(model.selected, usize::from(down));
            assert_eq!(model.selected_task().unwrap().uuid.to_string(), id);
        }
    }

    #[test]
    fn calendar_batch_and_today_reordering_follow_display_order() {
        use crate::commands::reorder::{ReorderArgs, reorder_changes};
        let (mut state, model) = fixture();
        let ids: Vec<_> = model.tasks.iter().map(|task| task.uuid.clone()).collect();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 12, 25).unwrap();
        let today = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
        fold_item(
            batch_plan(&state, &ids, Action::Date(date), 100.0, today.timestamp()).unwrap(),
            &mut state,
        );
        let store = ThingsStore::from_raw_state(&state);
        assert!(
            ids.iter()
                .all(|id| store.tasks_by_uuid[id].start_date.unwrap().date_naive() == date)
        );
        fold_item(
            batch_plan(&state, &ids, Action::Today, 100.0, today.timestamp()).unwrap(),
            &mut state,
        );
        let store = ThingsStore::from_raw_state(&state);
        let tasks = store.today(&today);
        let first = tasks[0].uuid.clone();
        let args = ReorderArgs {
            item_id: first.to_string(),
            before_id: None,
            after_id: Some(tasks[1].uuid.to_string()),
        };
        fold_item(
            reorder_changes(&args, &store, 200.0, today.timestamp(), true).unwrap(),
            &mut state,
        );
        assert_eq!(
            ThingsStore::from_raw_state(&state).today(&today)[1].uuid,
            first
        );
    }

    #[test]
    fn autosave_can_revert_title_and_edit_new_checklist_items() {
        let (mut state, model) = fixture();
        let task = model.tasks[0].clone();
        let original_title = task.title.clone();
        let mut editor = Editor::edit(&model, task);
        editor.draft.fields[0] = super::super::editor::Input::new("Changed");
        fold_item(
            autosave_plan(&state, &editor.draft, 100.0, 0).unwrap(),
            &mut state,
        );
        editor.draft.fields[0] = super::super::editor::Input::new(&original_title);
        editor.add_item();
        editor.input().unwrap().insert("New step", false);
        fold_item(
            autosave_plan(&state, &editor.draft, 101.0, 0).unwrap(),
            &mut state,
        );
        editor.input().unwrap().insert(" edited", false);
        fold_item(
            autosave_plan(&state, &editor.draft, 102.0, 0).unwrap(),
            &mut state,
        );
        let task = ThingsStore::from_raw_state(&state)
            .get_task(&editor.draft.id.to_string())
            .unwrap();
        assert_eq!(task.title, original_title);
        assert!(
            task.checklist_items
                .iter()
                .any(|item| item.title == "New step edited")
        );
        assert!(
            autosave_plan(&state, &editor.draft, 103.0, 0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn multi_reorder_preserves_blocks_gaps_and_boundaries() {
        use crate::wire::task::TaskProps;
        let ids: Vec<ThingsId> = (0..4).map(|_| ThingsId::random()).collect();
        let item = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                (
                    id.to_string(),
                    WireObject::create(
                        EntityType::Task7,
                        TaskProps {
                            title: format!("Task {i}"),
                            sort_index: i as i32,
                            creation_date: Some(1.0),
                            ..Default::default()
                        },
                    ),
                )
            })
            .collect::<WireItem>();
        let original = fold_items([item]);
        for (selected, down, expected) in [
            (vec![0, 1], true, vec![2, 0, 1, 3]),
            (vec![1, 2], false, vec![1, 2, 0, 3]),
            (vec![0, 2], true, vec![1, 0, 3, 2]),
            (vec![1, 3], false, vec![1, 0, 3, 2]),
            (vec![2, 3], true, vec![0, 1, 2, 3]),
            (vec![0, 1], false, vec![0, 1, 2, 3]),
            (vec![0, 1, 2, 3], true, vec![0, 1, 2, 3]),
        ] {
            let selection = ReorderSelection {
                visible: ids.clone(),
                selected: selected.iter().map(|i| ids[*i].clone()).collect(),
                down,
                today_order: false,
                preserve_dates: false,
            };
            let mut state = original.clone();
            fold_item(
                reorder_selection(&state, &selection, 100.0, 0).unwrap(),
                &mut state,
            );
            let result: Vec<_> = ThingsStore::from_raw_state(&state)
                .inbox()
                .into_iter()
                .map(|t| t.uuid)
                .collect();
            let expected: Vec<_> = expected.into_iter().map(|i| ids[i].clone()).collect();
            assert_eq!(result, expected, "selected={selected:?}, down={down}");
        }
    }

    #[test]
    fn checklist_reordering_autosaves_order_and_preserves_identity() {
        use iocraft::prelude::{KeyCode, KeyModifiers};
        let (mut state, mut model) = fixture();
        let mut editor = Editor::edit(&model, model.tasks[0].clone());
        let first = editor.draft.checklist[0].id.clone();
        editor.add_item();
        editor.input().unwrap().insert("Second step", false);
        let second = editor.draft.checklist[1].id.clone();
        fold_item(
            autosave_plan(&state, &editor.draft, 100.0, 0).unwrap(),
            &mut state,
        );
        editor.view_mode();
        assert!(editor.view_key(KeyCode::Char('K'), KeyModifiers::NONE));
        assert_eq!(editor.focus, super::super::editor::CHECKLIST);
        fold_item(
            autosave_plan(&state, &editor.draft, 101.0, 0).unwrap(),
            &mut state,
        );
        model.store = ThingsStore::from_raw_state(&state);
        let task = model.store.get_task(&editor.draft.id.to_string()).unwrap();
        assert_eq!(
            task.checklist_items
                .iter()
                .map(|item| item.uuid.clone())
                .collect::<Vec<_>>(),
            vec![second.clone(), first.clone()]
        );
        assert!(!editor.view_key(KeyCode::Char('K'), KeyModifiers::NONE));
        assert!(editor.view_key(KeyCode::Char('J'), KeyModifiers::NONE));
        fold_item(
            autosave_plan(&state, &editor.draft, 102.0, 0).unwrap(),
            &mut state,
        );
        let task = ThingsStore::from_raw_state(&state)
            .get_task(&editor.draft.id.to_string())
            .unwrap();
        assert_eq!(
            task.checklist_items
                .iter()
                .map(|item| item.uuid.clone())
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert!(!editor.view_key(KeyCode::Char('J'), KeyModifiers::NONE));
        assert!(
            autosave_plan(&state, &editor.draft, 103.0, 0)
                .unwrap()
                .is_empty()
        );
    }
}
