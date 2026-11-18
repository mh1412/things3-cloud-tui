use std::sync::mpsc;

use iocraft::prelude::*;

use super::{
    calendar::Calendar,
    editor::Editor,
    model::{List, Model, text},
    mutations::Action,
    theme::{Icons, Palette},
    worker::{Request, Update},
};
use crate::wire::task::TaskStatus;

#[derive(Default, Props)]
pub struct AppProps {
    pub icons: Icons,
    pub initial_view: List,
    pub requests: Option<mpsc::Sender<Request>>,
    pub events: Option<smol::channel::Receiver<Update>>,
    pub local: bool,
    pub cached: bool,
    pub no_color: bool,
}

fn send(model: &mut Model, requests: &mpsc::Sender<Request>, request: Request, message: &str) {
    if model.busy {
        return;
    }
    match requests.send(request) {
        Ok(()) => {
            model.busy = true;
            model.message = message.into();
        }
        Err(_) => model.message = "Sync worker stopped; exit and reopen the TUI".into(),
    }
}

fn action_ids(model: &Model) -> Vec<crate::ids::ThingsId> {
    if model.checked.is_empty() {
        model
            .selected_task()
            .map(|task| vec![task.uuid.clone()])
            .unwrap_or_default()
    } else {
        model
            .tasks
            .iter()
            .filter(|task| model.checked.contains(&task.uuid))
            .map(|task| task.uuid.clone())
            .collect()
    }
}

fn request_quit(model: &mut Model, requests: &mpsc::Sender<Request>) {
    if !model.quick_create
        && let Some(editor) = &model.editor
    {
        let _ = requests.send(Request::Edit {
            draft: Box::new(editor.draft.clone()),
            close: false,
        });
        model.edit_changed_at = None;
    }
    match requests.send(Request::Quit) {
        Ok(()) => {
            model.busy = true;
            model.message = "Saving pending changes before exit…".into();
        }
        Err(_) => model.message = "Sync worker stopped".into(),
    }
}

fn details(model: &Model, icons: Icons) -> String {
    let Some(task) = model.selected_task() else {
        return "Select a task to view its details.".into();
    };
    let mut lines = vec![
        format!("{} {}", icons.task(task), text(&task.title)),
        String::new(),
    ];
    if let Some(id) = model.store.effective_project_uuid(task) {
        lines.push(format!(
            "Project: {}",
            text(&model.store.resolve_project_title(id))
        ));
    }
    if let Some(id) = model.store.effective_area_uuid(task) {
        lines.push(format!(
            "Area: {}",
            text(&model.store.resolve_area_title(id))
        ));
    }
    if !task.tags.is_empty() {
        lines.push(format!(
            "Tags: {}",
            task.tags
                .iter()
                .map(|id| text(&model.store.resolve_tag_title(id)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for (name, date) in [
        ("Scheduled", task.start_date),
        ("Deadline", task.deadline),
        ("Completed", task.stop_date),
    ] {
        if let Some(date) = date {
            lines.push(format!("{name}: {}", date.format("%Y-%m-%d")));
        }
    }
    lines.push(format!("ID: {}", task.uuid));
    lines.push(String::new());
    if let Some(notes) = &task.notes {
        lines.push(text(notes));
        lines.push(String::new());
    }
    for item in &task.checklist_items {
        let mark = if item.status == TaskStatus::Completed {
            "x"
        } else if item.status == TaskStatus::Canceled {
            "-"
        } else {
            " "
        };
        lines.push(format!("[{mark}] {}", text(&item.title)));
    }
    lines.join("\n")
}

fn detail_lines(model: &Model, terminal_width: u16, icons: Icons) -> Vec<String> {
    let width = if terminal_width >= 100 {
        terminal_width / 2 - 4
    } else {
        terminal_width.saturating_sub(if terminal_width >= 60 { 19 } else { 2 })
    };
    if let Some(task) = model.selected_task().filter(|task| task.is_todo()) {
        Editor::edit(model, task.clone()).preview_lines(usize::from(width).max(1), icons)
    } else {
        wrap_lines(&details(model, icons), usize::from(width).max(1))
    }
}

pub(super) fn wrap_lines(value: &str, width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for line in value.replace('\t', "    ").split('\n') {
        let mut row = String::new();
        let mut used = 0;
        for ch in line.chars() {
            let size = console::measure_text_width(&ch.to_string());
            if used + size > width && !row.is_empty() {
                result.push(std::mem::take(&mut row));
                used = 0;
            }
            row.push(ch);
            used += size;
        }
        result.push(row);
    }
    result
}

const HELP: &str = "Keyboard shortcuts

c                 Quick create (Enter confirms)
o / Enter         View details / open project
e                 Edit details
j k / ↑ ↓         Move cursor
Shift-J / Shift-K Move selection down / up
u / Esc           Back / clear search
x                 Toggle selection
* a / * n         Select all visible / clear
y / v / -         Complete / reopen / cancel (! also cancels)
t / s / #         Today / someday / trash
w                 Calendar
Space             Toggle completion
Actions apply to selection, or cursor if none selected.
g i/t/a/s/l/p     Inbox/Today/Anytime/Someday/Logbook/Projects
1–7 / Tab         Switch lists
/                 Search; Enter keeps filter
Ctrl-D / Ctrl-U   Scroll preview down / up
r                 Refresh
? / q             Help / quit

Details view: j/k move · y/v/- status · Space toggle · t/s/w dates · e edit
Checklist: Shift-J/K reorder
Details edit: autosave on · Tab fields · Esc view
Ctrl-P calendar · Ctrl-T checklist status
Ctrl-N add checklist item · Ctrl-D remove item
Ctrl-U clear field · Enter new line in notes
Local previews never write to Things Cloud.";

#[component]
pub fn App(mut hooks: Hooks, props: &AppProps) -> impl Into<AnyElement<'static>> {
    let mut model = hooks.use_state(|| Model::new(props.initial_view));
    let mut quit = hooks.use_state(|| false);
    let (width, height) = hooks.use_terminal_size();
    let rows = usize::from(height.saturating_sub(6)).max(1);
    let events = props.events.clone().expect("TUI event receiver");
    hooks.use_future(async move {
        while let Ok(update) = events.recv().await {
            let mut state = model.write();
            state.busy = false;
            match update {
                Update::Status(message) => state.message = message,
                Update::Quit => quit.set(true),
                Update::Loaded {
                    store,
                    today,
                    message,
                    keep_editor,
                    keep_selection,
                } => {
                    let created = state
                        .editor
                        .as_ref()
                        .filter(|editor| editor.draft.original.is_none())
                        .map(|editor| editor.draft.id.clone());
                    let selected = created
                        .clone()
                        .or_else(|| state.selected_task().map(|t| t.uuid.clone()));
                    state.store = *store;
                    state.today = today;
                    state.message = message;
                    if !keep_editor {
                        state.editor = None;
                        state.quick_create = false;
                        state.calendar = None;
                        state.edit_changed_at = None;
                    }
                    if !keep_selection {
                        state.checked.clear();
                    }
                    if created.is_some() {
                        state.query.clear();
                        if state.project.is_none()
                            && matches!(state.list, List::Logbook | List::Projects)
                        {
                            state.change_list(List::Inbox);
                        }
                    }
                    state.rebuild(selected);
                }
                Update::Failed(error) => state.message = format!("Error: {}", text(&error)),
            }
        }
    });
    let requests = props.requests.clone().expect("TUI request sender");
    let autosave = requests.clone();
    hooks.use_future(async move {
        loop {
            smol::Timer::after(std::time::Duration::from_millis(100)).await;
            let ready = {
                let state = model.read();
                !state.quick_create
                    && !state.busy
                    && state.editor.is_some()
                    && state
                        .edit_changed_at
                        .is_some_and(|time| time.elapsed() >= std::time::Duration::from_millis(300))
            };
            if ready {
                let mut state = model.write();
                let draft = state.editor.as_ref().unwrap().draft.clone();
                state.edit_changed_at = None;
                state.message = "Queuing edits…".into();
                if autosave
                    .send(Request::Edit {
                        draft: Box::new(draft),
                        close: false,
                    })
                    .is_err()
                {
                    state.message = "Sync worker stopped".into();
                    break;
                }
            }
        }
    });
    let icons = props.icons;
    hooks.use_terminal_events(move |event| {
        let TerminalEvent::Key(KeyEvent {
            code,
            modifiers,
            kind,
            ..
        }) = event
        else {
            return;
        };
        if kind == KeyEventKind::Release {
            return;
        }
        let _ = requests.send(Request::Activity);
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            request_quit(&mut model.write(), &requests);
            return;
        }
        let mut state = model.write();
        if state.calendar.is_some() {
            if state.busy {
                return;
            }
            match code {
                KeyCode::Esc | KeyCode::Char('u') => state.calendar = None,
                KeyCode::Enter => {
                    let calendar = state.calendar.as_ref().unwrap();
                    let date = calendar.date;
                    if let Some(field) = calendar.field {
                        if let Some(editor) = state.editor.as_mut() {
                            editor.draft.fields[field] =
                                super::editor::Input::new(date.format("%Y-%m-%d").to_string());
                        }
                        state.calendar = None;
                        state.edit_changed_at = Some(std::time::Instant::now());
                    } else {
                        let ids = action_ids(&state);
                        let action = if date == state.today.date_naive() {
                            Action::Today
                        } else {
                            Action::Date(date)
                        };
                        send(
                            &mut state,
                            &requests,
                            Request::Batch { ids, action },
                            "Scheduling…",
                        );
                    }
                }
                _ => {
                    let today = state.today.date_naive();
                    state.calendar.as_mut().unwrap().key(code, today);
                }
            }
            return;
        }
        if state.editor.is_some() {
            if state.busy {
                return;
            }
            if state.quick_create {
                match code {
                    KeyCode::Esc => {
                        state.editor = None;
                        state.quick_create = false;
                        state.message.clear();
                    }
                    KeyCode::Enter => {
                        let draft = state.editor.as_ref().unwrap().draft.clone();
                        if draft.value(0).trim().is_empty() {
                            state.message = "Add a title".into();
                        } else {
                            send(
                                &mut state,
                                &requests,
                                Request::Save(Box::new(draft)),
                                "Creating…",
                            );
                        }
                    }
                    _ => state.editor.as_mut().unwrap().draft.fields[0].key(code, modifiers, false),
                }
                return;
            }
            let view_mode = !state.editor.as_ref().unwrap().editing;
            if (code == KeyCode::Char('p') && modifiers.contains(KeyModifiers::CONTROL))
                || (view_mode && code == KeyCode::Char('w'))
            {
                let editor = state.editor.as_ref().unwrap();
                let field = if editor.focus == 3 { 3 } else { 2 };
                let date = chrono::NaiveDate::parse_from_str(editor.draft.value(field), "%Y-%m-%d")
                    .unwrap_or(state.today.date_naive());
                state.calendar = Some(Calendar {
                    date,
                    field: Some(field),
                });
            } else if code == KeyCode::Esc
                || (view_mode && code == KeyCode::Char('u'))
                || (code == KeyCode::Char('s') && modifiers.contains(KeyModifiers::CONTROL))
            {
                let draft = state.editor.as_ref().unwrap().draft.clone();
                state.editor.as_mut().unwrap().view_mode();
                state.edit_changed_at = None;
                send(
                    &mut state,
                    &requests,
                    Request::Edit {
                        draft: Box::new(draft),
                        close: view_mode,
                    },
                    "Queuing edits…",
                );
            } else if view_mode {
                if code == KeyCode::Char('q') {
                    request_quit(&mut state, &requests);
                } else if state.editor.as_mut().unwrap().view_key(code, modifiers) {
                    state.edit_changed_at = Some(std::time::Instant::now());
                }
            } else {
                state.editor.as_mut().unwrap().key(code, modifiers);
                state.edit_changed_at = Some(std::time::Instant::now());
            }
            return;
        }
        if state.searching {
            match code {
                KeyCode::Esc => state.escape(),
                KeyCode::Enter => state.searching = false,
                KeyCode::Backspace => {
                    state.query.pop();
                    state.selected = 0;
                    state.rebuild(None);
                }
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    state.query.push(c);
                    state.selected = 0;
                    state.rebuild(None);
                }
                _ => {}
            }
            return;
        }
        if state.help {
            match code {
                KeyCode::Char('q') => request_quit(&mut state, &requests),
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('u') => state.help = false,
                _ => {}
            }
            return;
        }
        if let Some(prefix) = state.prefix.take() {
            match (prefix, code) {
                ('*', KeyCode::Char('a')) => {
                    state.checked = state.tasks.iter().map(|t| t.uuid.clone()).collect()
                }
                ('*', KeyCode::Char('n')) => state.checked.clear(),
                ('g', KeyCode::Char('i')) => state.change_list(List::Inbox),
                ('g', KeyCode::Char('t')) => state.change_list(List::Today),
                ('g', KeyCode::Char('a')) => state.change_list(List::Anytime),
                ('g', KeyCode::Char('s')) => state.change_list(List::Someday),
                ('g', KeyCode::Char('l')) => state.change_list(List::Logbook),
                ('g', KeyCode::Char('p')) => state.change_list(List::Projects),
                _ => {}
            }
            state.message.clear();
            return;
        }
        match code {
            KeyCode::Char('w') if !state.busy && !action_ids(&state).is_empty() => {
                let date = state
                    .selected_task()
                    .and_then(|t| t.start_date)
                    .map(|d| d.date_naive())
                    .unwrap_or(state.today.date_naive());
                state.calendar = Some(Calendar { date, field: None });
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                state.detail_scroll = state.detail_scroll.saturating_add(1).min(
                    detail_lines(&state, width, icons)
                        .len()
                        .saturating_sub(rows),
                );
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                state.detail_scroll = state.detail_scroll.saturating_sub(1);
            }
            KeyCode::Char(c @ ('J' | 'K')) => {
                if state.list == List::Logbook {
                    state.message = "Logbook is ordered by completion date".into();
                } else {
                    let request = Request::Reorder(Box::new(super::mutations::ReorderSelection {
                        visible: state.tasks.iter().map(|task| task.uuid.clone()).collect(),
                        selected: action_ids(&state),
                        down: c == 'J',
                        today_order: state.list == List::Today && state.project.is_none(),
                        preserve_dates: state.list == List::Upcoming,
                    }));
                    send(&mut state, &requests, request, "Reordering…");
                }
            }
            KeyCode::Char(c @ ('*' | 'g')) => {
                state.prefix = Some(c);
                state.message = if c == '*' {
                    "* a: select all · * n: clear selection"
                } else {
                    "g i: Inbox · t: Today · a: Anytime · s: Someday · l: Logbook · p: Projects"
                }
                .into();
            }
            KeyCode::Char('x') => {
                if let Some(id) = state.selected_task().map(|t| t.uuid.clone())
                    && !state.checked.remove(&id)
                {
                    state.checked.insert(id);
                }
            }
            KeyCode::Char('c') if !state.busy => {
                state.editor = Some(Editor::create(&state));
                state.quick_create = true;
                state.message.clear();
            }
            KeyCode::Char('e' | 'o' | 'd') | KeyCode::Enter if !state.busy => {
                if let Some(task) = state.selected_task().cloned() {
                    if task.is_todo() {
                        let mut editor = Editor::edit(&state, task);
                        if code != KeyCode::Char('e') {
                            editor.view_mode();
                        }
                        state.editor = Some(editor);
                    } else {
                        state.enter();
                    }
                }
            }
            KeyCode::Char('q') => request_quit(&mut state, &requests),
            KeyCode::Char('?') => state.help = true,
            KeyCode::Char('j') | KeyCode::Down => state.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => state.move_selection(-1),
            KeyCode::PageDown => state.move_selection(rows as isize),
            KeyCode::PageUp => state.move_selection(-(rows as isize)),
            KeyCode::Home => {
                state.selected = 0;
                state.detail_scroll = 0;
            }
            KeyCode::End => {
                state.selected = state.tasks.len().saturating_sub(1);
                state.detail_scroll = 0;
            }
            KeyCode::Tab | KeyCode::Right => state.cycle_list(1),
            KeyCode::BackTab | KeyCode::Left => state.cycle_list(-1),
            KeyCode::Char(c @ '1'..='7') => state.change_list(List::ALL[(c as u8 - b'1') as usize]),
            KeyCode::Char('/') => state.searching = true,
            KeyCode::Esc | KeyCode::Char('u') => state.escape(),
            KeyCode::Char('r') => send(&mut state, &requests, Request::Refresh, "Refreshing…"),
            KeyCode::Char(c @ (' ' | 'y' | 'v' | '!' | '-' | 't' | 's' | '#')) => {
                let ids = if state.checked.is_empty() {
                    state
                        .selected_task()
                        .map(|t| vec![t.uuid.clone()])
                        .unwrap_or_default()
                } else {
                    state
                        .tasks
                        .iter()
                        .filter(|t| state.checked.contains(&t.uuid))
                        .map(|t| t.uuid.clone())
                        .collect()
                };
                if !ids.is_empty() {
                    let action = match c {
                        'y' => Action::Complete,
                        'v' => Action::Reopen,
                        '!' | '-' => Action::Cancel,
                        't' => Action::Today,
                        's' => Action::Someday,
                        '#' => Action::Trash,
                        _ => Action::Toggle,
                    };
                    send(
                        &mut state,
                        &requests,
                        Request::Batch { ids, action },
                        "Saving…",
                    );
                }
            }
            _ => {}
        }
    });

    let mut system = hooks.use_context_mut::<SystemContext>();
    if quit.get() {
        system.exit();
    }
    let state = model.read();
    let palette = Palette::new(props.no_color);
    if let Some(calendar) = &state.calendar {
        return element! {
            View(width: u32::from(width), height: u32::from(height), background_color: palette.background, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden, padding: 1) {
                Text(content: format!("{}  {}", icons.list(List::Upcoming), if calendar.field == Some(3) { "Choose deadline" } else { "Choose schedule date" }), color: palette.accent, weight: Weight::Bold)
                View(border_style: BorderStyle::Round, border_color: palette.accent, background_color: palette.panel, padding: 1, width: 34) { Text(content: calendar.display(), color: palette.text) }
                Text(content: "Arrows / h j k l: day · PgUp/PgDn or [ ]: month · t: today", color: palette.muted)
                Text(content: "Enter: apply · Esc / u: back", color: palette.accent)
                Text(content: text(&state.message), color: if state.message.starts_with("Error:") { palette.red } else { palette.muted })
            }
        };
    }
    if let Some(editor) = &state.editor
        && !state.quick_create
        && width < 100
    {
        let (lines, cursor) = editor.lines(usize::from(width.saturating_sub(4)), icons);
        let count = usize::from(height.saturating_sub(5)).max(1);
        let start = cursor.saturating_sub(count.saturating_sub(1));
        return element! {
            View(width: u32::from(width), height: u32::from(height), background_color: palette.background, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                Text(content: format!(" {} Details · {}", icons.field(0), if editor.editing { "EDIT" } else { "VIEW" }), color: palette.accent, weight: Weight::Bold)
                View(flex_grow: 1.0, min_height: 0, padding_left: 1, padding_right: 1, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                    #(lines.into_iter().skip(start).take(count).map(|(line, focused)| element! {
                        View(height: 1, flex_shrink: 0.0, background_color: if focused { palette.focus } else { palette.panel }) { Text(content: line, color: if focused { palette.accent } else { palette.text }, weight: if focused { Weight::Bold } else { Weight::Normal }, wrap: TextWrap::NoWrap) }
                    }))
                }
                Text(content: editor.hint(), color: palette.muted, wrap: TextWrap::NoWrap)
                Text(content: text(&state.message).replace('\n', " "), color: if state.message.starts_with("Error:") { palette.red } else { palette.muted }, wrap: TextWrap::NoWrap)
                Text(content: editor.footer(), color: palette.accent, wrap: TextWrap::NoWrap)
            }
        };
    }
    let wide = width >= 100;
    let editing = state.editor.as_ref().filter(|_| !state.quick_create);
    let show_details = wide || state.details;
    let accent = palette.accent;
    let mode = if props.local {
        "LOCAL PREVIEW"
    } else if props.cached {
        "CACHED READS"
    } else {
        "THINGS CLOUD"
    };
    let heading = format!(
        "  {}  THINGS  /  {}  ·  {} items  ·  {mode}",
        icons.list(state.list),
        text(&state.title()),
        state.tasks.len()
    );
    let start = state.selected.saturating_sub(rows.saturating_sub(1));
    let (panel_lines, scroll) = if let Some(editor) = editing {
        let (lines, cursor) = editor.lines(usize::from(width / 2 - 4), icons);
        (lines, cursor.saturating_sub(rows.saturating_sub(2)))
    } else {
        let lines = detail_lines(&state, width, icons);
        let scroll = state.detail_scroll.min(lines.len().saturating_sub(rows));
        (
            lines.into_iter().map(|line| (line, false)).collect(),
            scroll,
        )
    };
    let detail_text = panel_lines
        .iter()
        .map(|(line, _)| line.clone())
        .skip(scroll)
        .collect::<Vec<_>>()
        .join("\n");
    let search = if state.quick_create {
        format!(
            "{}  {}",
            icons.field(0),
            state.editor.as_ref().unwrap().draft.fields[0].display(true)
        )
    } else if let Some(editor) = editing {
        editor.hint().into()
    } else if !state.checked.is_empty() && !state.searching {
        format!(
            "{} selected · y done · v reopen · - cancel · t today · w date · s someday · # trash",
            state.checked.len()
        )
    } else if state.searching {
        format!("/{}▏", text(&state.query))
    } else if state.query.is_empty() {
        "/ search  ·  ? help".into()
    } else {
        format!("Filter: {}  (Esc to clear)", text(&state.query))
    };
    element! {
        View(width: u32::from(width), height: u32::from(height), background_color: palette.background, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
            View(height: 1, flex_shrink: 0.0, background_color: palette.focus) { Text(content: heading, color: accent, weight: Weight::Bold, wrap: TextWrap::NoWrap) }
            View(height: 1, flex_shrink: 0.0, padding_left: 1, background_color: if state.quick_create { palette.focus } else { None }) { Text(content: search, color: if state.quick_create || !state.checked.is_empty() { palette.accent } else { palette.muted }, wrap: TextWrap::NoWrap) }
            View(flex_grow: 1.0, min_height: 0, overflow: Overflow::Hidden) {
                #(if width >= 60 { Some(element! {
                    View(width: 17, flex_shrink: 0.0, flex_direction: FlexDirection::Column, padding_top: 1, background_color: palette.panel) {
                        #(List::ALL.into_iter().enumerate().map(|(i, list)| element! {
                            View(height: 2, flex_shrink: 0.0, background_color: if state.list == list { palette.focus } else { palette.panel }) {
                                Text(content: format!(" {} {} {} {}", if state.list == list { "▎" } else { " " }, icons.list(list), i + 1, list.title()), color: if state.list == list { accent } else { palette.muted }, weight: if state.list == list { Weight::Bold } else { Weight::Normal })
                            }
                        }))
                    }
                }) } else { None })
                View(flex_grow: 1.0, min_width: 0, border_style: BorderStyle::Round, border_color: palette.border, background_color: palette.panel, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                    #(if state.help { Some(element! { Text(content: HELP, color: palette.text) }) } else { None })
                    #(if !state.help && (!show_details || wide) { Some(element! {
                        View(flex_direction: FlexDirection::Column, flex_grow: 1.0, overflow: Overflow::Hidden) {
                            #(if state.tasks.is_empty() { Some(element! { Text(content: if state.busy { "  Loading your tasks…" } else { "  All clear. Press c to create a task." }, color: palette.muted) }) } else { None })
                            #(state.tasks.iter().enumerate().skip(start).take(rows).map(|(index, task)| {
                                let selected = index == state.selected;
                                let checked = state.checked.contains(&task.uuid);
                                element! {
                                    View(height: 1, flex_shrink: 0.0, background_color: if checked { palette.selection } else if selected { palette.focus } else { palette.panel }) {
                                        Text(content: format!("{} {} {}", if selected { "▎" } else { " " }, icons.task(task), text(&task.title).replace('\n', " ")),
                                            color: if checked { palette.text } else if selected { accent } else if task.status == TaskStatus::Completed { palette.green } else if task.status == TaskStatus::Canceled { palette.muted } else { palette.text }, weight: if selected || checked { Weight::Bold } else { Weight::Normal }, wrap: TextWrap::NoWrap)
                                    }
                                }
                            }))
                        }
                    }) } else { None })
                    #(if !state.help && show_details && !wide { Some(element! { Text(content: detail_text.clone(), color: palette.text) }) } else { None })
                }
                #(if wide && !state.help { Some(element! {
                    View(width: u32::from(width / 2), flex_shrink: 0.0, border_style: BorderStyle::Round, border_color: if editing.is_some() { palette.accent } else { palette.border }, background_color: palette.panel, padding_left: 1, padding_right: 1, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                        #(panel_lines.iter().skip(scroll).take(rows).map(|(line, focused)| element! {
                            View(height: 1, flex_shrink: 0.0, background_color: if *focused { palette.focus } else { palette.panel }) {
                                Text(content: line.clone(), color: if *focused { palette.accent } else { palette.text }, weight: if *focused { Weight::Bold } else { Weight::Normal }, wrap: TextWrap::NoWrap)
                            }
                        }))
                    }
                }) } else { None })
            }
            View(height: 1, flex_shrink: 0.0, padding_left: 1) { Text(content: text(&state.message).replace('\n', " "), color: if state.message.starts_with("Error:") { palette.red } else if state.busy { palette.accent } else { palette.green }, wrap: TextWrap::NoWrap) }
            View(height: 1, flex_shrink: 0.0, background_color: palette.focus, padding_left: 1) { Text(content: if state.quick_create { "Enter  create     Esc  cancel     Then e for details" } else if let Some(editor) = editing { editor.footer() } else { "c create · e edit · x select · o details · u back · t today · w date · ? help · q quit" }, color: palette.muted, wrap: TextWrap::NoWrap) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_scrolling_counts_wrapped_unicode_lines() {
        assert_eq!(
            wrap_lines("abcdef\n\n你好世界", 4),
            vec!["abcd", "ef", "", "你好", "世界"]
        );
        assert_eq!(wrap_lines("cafe\u{301}", 4), vec!["cafe\u{301}"]);
    }
}
