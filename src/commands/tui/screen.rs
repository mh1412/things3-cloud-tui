use std::sync::mpsc;

use iocraft::prelude::*;

use super::{
    calendar::Calendar,
    editor::Editor,
    model::{List, Model, text},
    mutations::Action,
    worker::{Request, Update},
};
use crate::{store::Task, wire::task::TaskStatus};

#[derive(Default, Props)]
pub struct AppProps {
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

fn status_icon(task: &Task) -> &'static str {
    match task.status {
        TaskStatus::Completed => "[x]",
        TaskStatus::Canceled => "[-]",
        _ if task.is_project() => "[+]",
        _ => "[ ]",
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

fn details(model: &Model) -> String {
    let Some(task) = model.selected_task() else {
        return "Select a task to view its details.".into();
    };
    let mut lines = vec![
        format!("{} {}", status_icon(task), text(&task.title)),
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

fn detail_lines(model: &Model, terminal_width: u16) -> Vec<String> {
    let width = if terminal_width >= 100 {
        terminal_width / 2 - 4
    } else {
        terminal_width.saturating_sub(if terminal_width >= 60 { 19 } else { 2 })
    };
    wrap_lines(&details(model), usize::from(width).max(1))
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

const HELP: &str = "Keyboard shortcuts\n\nc / e             Create / edit task\nj k / ↑ ↓         Move cursor\nShift-J / Shift-K Move focused task down / up\no / Enter         Open project / task details\nu / Esc           Back / clear search\nx                 Toggle selection (row highlight)\n* a / * n         Select all visible / clear selection\ny / v / !         Complete / reopen / cancel\nt / s / #         Today / someday / trash\nSpace             Toggle completion\nActions apply to selection, or cursor if none selected.\ng i/t/a/s/l/p     Inbox/Today/Anytime/Someday/Logbook/Projects\n1–7 / Tab         Switch lists\n/                 Search; Enter keeps filter\nCtrl-D / Ctrl-U   Scroll details down / up\nr                 Refresh\n? / q             Help / quit\n\nEditor: Tab fields · Ctrl-S save · Esc discard\nCtrl-N add checklist item · Ctrl-D remove · F2 status\nCtrl-U clears field · Enter adds a line in notes\nLocal previews never write to Things Cloud.";

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
                Update::Loaded {
                    store,
                    today,
                    message,
                } => {
                    let selected = state.selected_task().map(|t| t.uuid.clone());
                    state.store = *store;
                    state.today = today;
                    state.message = message;
                    state.editor = None;
                    state.calendar = None;
                    state.checked.clear();
                    state.rebuild(selected);
                }
                Update::Failed(error) => state.message = format!("Error: {}", text(&error)),
            }
        }
    });
    let requests = props.requests.clone().expect("TUI request sender");
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
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            quit.set(true);
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
            if code == KeyCode::F(4) {
                let editor = state.editor.as_ref().unwrap();
                let field = if editor.focus == 3 { 3 } else { 2 };
                let date = chrono::NaiveDate::parse_from_str(editor.draft.value(field), "%Y-%m-%d")
                    .unwrap_or(state.today.date_naive());
                state.calendar = Some(Calendar {
                    date,
                    field: Some(field),
                });
            } else if code == KeyCode::Esc {
                state.editor = None;
                state.message = "Draft discarded".into();
            } else if code == KeyCode::Char('s') && modifiers.contains(KeyModifiers::CONTROL) {
                let draft = state.editor.as_ref().unwrap().draft.clone();
                send(
                    &mut state,
                    &requests,
                    Request::Save(Box::new(draft)),
                    "Saving…",
                );
            } else {
                state.editor.as_mut().unwrap().key(code, modifiers);
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
                KeyCode::Char('q') => quit.set(true),
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
                state.detail_scroll = state
                    .detail_scroll
                    .saturating_add(1)
                    .min(detail_lines(&state, width).len().saturating_sub(rows));
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                state.detail_scroll = state.detail_scroll.saturating_sub(1);
            }
            KeyCode::Char(c @ ('J' | 'K')) => {
                let down = c == 'J';
                let adjacent = if down {
                    state.selected.checked_add(1)
                } else {
                    state.selected.checked_sub(1)
                };
                if state.list == List::Logbook {
                    state.message = "Logbook is ordered by completion date".into();
                } else if let Some((task, anchor)) = state
                    .selected_task()
                    .zip(adjacent.and_then(|i| state.tasks.get(i)))
                {
                    if state.list == List::Upcoming && task.start_date != anchor.start_date {
                        state.message =
                            "Upcoming tasks can be reordered within the same date".into();
                    } else {
                        let request = Request::Reorder {
                            id: task.uuid.clone(),
                            anchor: anchor.uuid.clone(),
                            down,
                            today_order: state.list == List::Today && state.project.is_none(),
                        };
                        send(&mut state, &requests, request, "Reordering…");
                    }
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
            KeyCode::Char('c') if !state.busy => state.editor = Some(Editor::create(&state)),
            KeyCode::Char('e') if !state.busy => {
                if let Some(task) = state.selected_task().cloned() {
                    if task.is_todo() {
                        state.editor = Some(Editor::edit(&state, task));
                    } else {
                        state.message = "Open the project to edit its tasks".into();
                    }
                }
            }
            KeyCode::Char('q') => quit.set(true),
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
            KeyCode::Enter | KeyCode::Char('o') => state.enter(),
            KeyCode::Char('d') => state.details = !state.details,
            KeyCode::Char('r') => send(&mut state, &requests, Request::Refresh, "Refreshing…"),
            KeyCode::Char(c @ (' ' | 'y' | 'v' | '!' | 't' | 's' | '#')) => {
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
                        '!' => Action::Cancel,
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
    if let Some(calendar) = &state.calendar {
        return element! {
            View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                Text(content: if calendar.field == Some(3) { "Choose deadline" } else { "Choose schedule date" }, weight: Weight::Bold)
                Text(content: calendar.display())
                Text(content: "Arrows / h j k l: day · PgUp/PgDn or [ ]: month · t: today")
                Text(content: "Enter: apply · Esc / u: back")
                Text(content: text(&state.message))
            }
        };
    }
    if let Some(editor) = &state.editor {
        let (lines, cursor) = editor.lines(usize::from(width.saturating_sub(2)));
        let count = usize::from(height.saturating_sub(4)).max(1);
        let start = cursor.saturating_sub(count.saturating_sub(1));
        return element! {
            View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                Text(content: if editor.draft.original.is_some() { "Edit task" } else { "Create task" }, weight: Weight::Bold)
                View(flex_grow: 1.0, min_height: 0, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                    #(lines.into_iter().skip(start).take(count).map(|(line, focused)| element! {
                        View(height: 1, flex_shrink: 0.0) { Text(content: line, weight: if focused { Weight::Bold } else { Weight::Normal }, wrap: TextWrap::NoWrap) }
                    }))
                }
                Text(content: text(&state.message).replace('\n', " "), wrap: TextWrap::NoWrap)
                Text(content: "Tab: field · Ctrl-S: save · Esc: discard · F4: calendar · Ctrl-N: add item · Ctrl-D: delete · F2: status", wrap: TextWrap::NoWrap)
            }
        };
    }
    let wide = width >= 100;
    let show_details = wide || state.details;
    let accent = if props.no_color {
        Color::Reset
    } else {
        Color::Cyan
    };
    let mode = if props.local {
        "LOCAL PREVIEW"
    } else if props.cached {
        "CACHED READS"
    } else {
        "THINGS CLOUD"
    };
    let heading = format!(
        "things3  ·  {}  ·  {} items  ·  {mode}",
        text(&state.title()),
        state.tasks.len()
    );
    let start = state.selected.saturating_sub(rows.saturating_sub(1));
    let detail_lines = detail_lines(&state, width);
    let scroll = state
        .detail_scroll
        .min(detail_lines.len().saturating_sub(rows));
    let detail_text = detail_lines
        .into_iter()
        .skip(scroll)
        .collect::<Vec<_>>()
        .join("\n");
    let search = if !state.checked.is_empty() && !state.searching {
        format!(
            "{} selected · y done · v reopen · ! cancel · t today · w date · s someday · # trash",
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
        View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
            View(height: 1, flex_shrink: 0.0) { Text(content: heading, color: accent, weight: Weight::Bold, wrap: TextWrap::NoWrap) }
            View(height: 1, flex_shrink: 0.0) { Text(content: search, wrap: TextWrap::NoWrap) }
            View(flex_grow: 1.0, min_height: 0, overflow: Overflow::Hidden) {
                #(if width >= 60 { Some(element! {
                    View(width: 17, flex_shrink: 0.0, flex_direction: FlexDirection::Column, padding_top: 1) {
                        #(List::ALL.into_iter().enumerate().map(|(i, list)| element! {
                            Text(content: format!("{} {} {}", if state.list == list { ">" } else { " " }, i + 1, list.title()), color: if state.list == list { accent } else { Color::Reset })
                        }))
                    }
                }) } else { None })
                View(flex_grow: 1.0, min_width: 0, border_style: BorderStyle::Round, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                    #(if state.help { Some(element! { Text(content: format!("w: calendar · t: today · Editor F4: calendar\n{HELP}")) }) } else { None })
                    #(if !state.help && (!show_details || wide) { Some(element! {
                        View(flex_direction: FlexDirection::Column, flex_grow: 1.0, overflow: Overflow::Hidden) {
                            #(if state.tasks.is_empty() { Some(element! { Text(content: if state.busy { "Loading…" } else { "No tasks in this view." }) }) } else { None })
                            #(state.tasks.iter().enumerate().skip(start).take(rows).map(|(index, task)| {
                                let selected = index == state.selected;
                                let checked = state.checked.contains(&task.uuid);
                                element! {
                                    View(height: 1, flex_shrink: 0.0, background_color: if checked && !props.no_color { Some(Color::DarkBlue) } else { None }) {
                                        Text(content: format!("{} {} {}", if selected { ">" } else { " " }, status_icon(task), text(&task.title).replace('\n', " ")),
                                            color: if checked && !props.no_color { Color::White } else if selected { accent } else { Color::Reset }, weight: if selected || checked { Weight::Bold } else { Weight::Normal }, wrap: TextWrap::NoWrap)
                                    }
                                }
                            }))
                        }
                    }) } else { None })
                    #(if !state.help && show_details && !wide { Some(element! { Text(content: detail_text.clone()) }) } else { None })
                }
                #(if wide && !state.help { Some(element! {
                    View(width: u32::from(width / 2), flex_shrink: 0.0, border_style: BorderStyle::Round, padding_left: 1, padding_right: 1, overflow: Overflow::Hidden) {
                        Text(content: detail_text)
                    }
                }) } else { None })
            }
            View(height: 1, flex_shrink: 0.0) { Text(content: text(&state.message).replace('\n', " "), wrap: TextWrap::NoWrap) }
            View(height: 1, flex_shrink: 0.0) { Text(content: "c create · e edit · x select · o open · u back · t today · w date · ? help · q quit", wrap: TextWrap::NoWrap) }
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
