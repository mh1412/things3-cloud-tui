use iocraft::prelude::{KeyCode, KeyModifiers};

use super::model::{List, Model, text};
use crate::{
    ids::ThingsId,
    store::Task,
    wire::task::{TaskStart, TaskStatus},
};

pub const CHECKLIST: usize = 6;
pub const LABELS: [&str; CHECKLIST] = ["Title", "Notes", "When", "Deadline", "Tags", "Location"];

#[derive(Clone, Debug, Default)]
pub struct Input {
    pub value: String,
    cursor: usize,
}

impl Input {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            cursor: value.len(),
            value,
        }
    }

    pub fn insert(&mut self, value: &str, multiline: bool) {
        let value = text(value).replace('\r', "");
        let value = if multiline {
            value
        } else {
            value.replace(['\n', '\t'], " ")
        };
        self.value.insert_str(self.cursor, &value);
        self.cursor += value.len();
    }

    pub fn display(&self, focused: bool) -> String {
        if focused {
            format!(
                "{}▏{}",
                text(&self.value[..self.cursor]),
                text(&self.value[self.cursor..])
            )
        } else {
            text(&self.value)
        }
    }

    pub fn key(&mut self, code: KeyCode, modifiers: KeyModifiers, multiline: bool) {
        if modifiers.contains(KeyModifiers::CONTROL) {
            match code {
                KeyCode::Char('u') => {
                    self.value.clear();
                    self.cursor = 0;
                }
                KeyCode::Char('a') => self.cursor = 0,
                KeyCode::Char('e') => self.cursor = self.value.len(),
                _ => {}
            }
            return;
        }
        match code {
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&c.to_string(), multiline)
            }
            KeyCode::Left => {
                self.cursor = self.value[..self.cursor]
                    .char_indices()
                    .last()
                    .map_or(0, |(i, _)| i)
            }
            KeyCode::Right => {
                self.cursor += self.value[self.cursor..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
            }
            KeyCode::Home => {
                self.cursor = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
            }
            KeyCode::End => {
                self.cursor += self.value[self.cursor..]
                    .find('\n')
                    .unwrap_or(self.value.len() - self.cursor)
            }
            KeyCode::Backspace => {
                if let Some((start, _)) = self.value[..self.cursor].char_indices().last() {
                    self.value.drain(start..self.cursor);
                    self.cursor = start;
                }
            }
            KeyCode::Delete => {
                if let Some(c) = self.value[self.cursor..].chars().next() {
                    self.value.drain(self.cursor..self.cursor + c.len_utf8());
                }
            }
            KeyCode::Enter if multiline => self.insert("\n", true),
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub struct CheckDraft {
    pub id: ThingsId,
    pub title: Input,
    pub status: TaskStatus,
}

impl Default for CheckDraft {
    fn default() -> Self {
        Self {
            id: ThingsId::random(),
            title: Input::default(),
            status: TaskStatus::Incomplete,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Draft {
    pub id: ThingsId,
    pub original: Option<Task>,
    pub fields: [Input; CHECKLIST],
    pub initial: [String; CHECKLIST],
    pub checklist: Vec<CheckDraft>,
}

impl Draft {
    pub fn changed(&self, field: usize) -> bool {
        self.fields[field].value != self.initial[field]
    }
    pub fn value(&self, field: usize) -> &str {
        &self.fields[field].value
    }
}

pub struct Editor {
    pub editing: bool,
    pub draft: Draft,
    pub focus: usize,
}

impl Editor {
    pub fn create(model: &Model) -> Self {
        let when = match model.list {
            List::Today => "today".into(),
            List::Anytime => "anytime".into(),
            List::Someday => "someday".into(),
            List::Upcoming => (model.today + chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string(),
            _ if model.project.is_some() => "anytime".into(),
            _ => "inbox".into(),
        };
        let values = [
            String::new(),
            String::new(),
            when,
            String::new(),
            String::new(),
            model
                .project
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ];
        Self {
            draft: Draft {
                id: ThingsId::random(),
                original: None,
                fields: values.clone().map(Input::new),
                initial: values,
                checklist: Vec::new(),
            },
            focus: 0,
            editing: true,
        }
    }

    pub fn edit(model: &Model, task: Task) -> Self {
        let when = if task.evening {
            "evening".into()
        } else if let Some(day) = task.start_date {
            day.format("%Y-%m-%d").to_string()
        } else {
            match task.start {
                TaskStart::Inbox => "inbox",
                TaskStart::Someday => "someday",
                _ => "anytime",
            }
            .into()
        };
        let tags = task
            .tags
            .iter()
            .map(|id| {
                let title = model.store.resolve_tag_title(id);
                if title.contains(',') {
                    id.to_string()
                } else {
                    title
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let values = [
            task.title.clone(),
            task.notes.clone().unwrap_or_default(),
            when,
            task.deadline
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            tags,
            model
                .store
                .effective_project_uuid(&task)
                .or_else(|| model.store.effective_area_uuid(&task))
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ];
        let checklist = task
            .checklist_items
            .iter()
            .map(|c| CheckDraft {
                id: c.uuid.clone(),
                title: Input::new(&c.title),
                status: c.status,
            })
            .collect();
        Self {
            draft: Draft {
                id: task.uuid.clone(),
                original: Some(task),
                fields: values.clone().map(Input::new),
                initial: values,
                checklist,
            },
            focus: 0,
            editing: true,
        }
    }

    pub fn input(&mut self) -> Option<&mut Input> {
        if self.focus < CHECKLIST {
            Some(&mut self.draft.fields[self.focus])
        } else {
            self.draft
                .checklist
                .get_mut(self.focus - CHECKLIST)
                .map(|c| &mut c.title)
        }
    }

    pub fn view_mode(&mut self) {
        self.editing = false;
        self.focus = self
            .focus
            .min((CHECKLIST + self.draft.checklist.len()).saturating_sub(1));
    }

    /// View navigation never changes text. Return true only for a mutation.
    pub fn view_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return false;
        }
        let count = CHECKLIST + self.draft.checklist.len();
        match code {
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                self.focus = (self.focus + 1).min(count - 1)
            }
            KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                self.focus = self.focus.saturating_sub(1)
            }
            KeyCode::Home => self.focus = 0,
            KeyCode::End => self.focus = count - 1,
            KeyCode::Char('e') => self.editing = true,
            KeyCode::Char(c @ ('J' | 'K')) => {
                let Some(index) = self.focus.checked_sub(CHECKLIST) else {
                    return false;
                };
                if index >= self.draft.checklist.len() {
                    return false;
                }
                let target = if c == 'J' {
                    index.checked_add(1)
                } else {
                    index.checked_sub(1)
                };
                if let Some(target) = target.filter(|target| *target < self.draft.checklist.len()) {
                    self.draft.checklist.swap(index, target);
                    self.focus = CHECKLIST + target;
                    return true;
                }
            }
            KeyCode::Char(' ' | '-' | 'y' | 'v') => {
                if let Some(item) = self
                    .draft
                    .checklist
                    .get_mut(self.focus.wrapping_sub(CHECKLIST))
                {
                    item.status = if code == KeyCode::Char('-') {
                        TaskStatus::Canceled
                    } else if code == KeyCode::Char('y') {
                        TaskStatus::Completed
                    } else if code == KeyCode::Char('v') || item.status == TaskStatus::Completed {
                        TaskStatus::Incomplete
                    } else {
                        TaskStatus::Completed
                    };
                    return true;
                }
            }
            KeyCode::Char('t' | 's') => {
                self.draft.fields[2] = Input::new(if code == KeyCode::Char('t') {
                    "today"
                } else {
                    "someday"
                });
                return true;
            }
            _ => {}
        }
        false
    }

    pub fn footer(&self) -> &'static str {
        if self.editing {
            "EDIT · Tab  move · Esc  view · Autosave on"
        } else {
            "VIEW · j/k move · y/v/- status · Space toggle · e edit · u back"
        }
    }

    pub fn add_item(&mut self) {
        let index = if self.focus >= CHECKLIST {
            (self.focus - CHECKLIST + 1).min(self.draft.checklist.len())
        } else {
            self.draft.checklist.len()
        };
        self.draft.checklist.insert(index, CheckDraft::default());
        self.focus = CHECKLIST + index;
    }

    pub fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let count = CHECKLIST + self.draft.checklist.len() + 1; // final Add item row
        match code {
            KeyCode::Tab => self.focus = (self.focus + 1) % count,
            KeyCode::BackTab => self.focus = (self.focus + count - 1) % count,
            KeyCode::Up => self.focus = self.focus.saturating_sub(1),
            KeyCode::Down => self.focus = (self.focus + 1).min(count - 1),
            KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => self.add_item(),
            KeyCode::Char('d')
                if modifiers.contains(KeyModifiers::CONTROL)
                    && self.focus >= CHECKLIST
                    && self.focus < count - 1 =>
            {
                self.draft.checklist.remove(self.focus - CHECKLIST);
                self.focus = self.focus.min(count - 2);
            }
            KeyCode::Char('t') if modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(item) = self
                    .draft
                    .checklist
                    .get_mut(self.focus.wrapping_sub(CHECKLIST))
                {
                    item.status = match item.status {
                        TaskStatus::Incomplete => TaskStatus::Completed,
                        TaskStatus::Completed => TaskStatus::Canceled,
                        _ => TaskStatus::Incomplete,
                    };
                }
            }
            KeyCode::Enter if self.focus >= CHECKLIST => self.add_item(),
            KeyCode::Enter if self.focus != 1 => self.focus = (self.focus + 1).min(count - 1),
            _ => {
                let multiline = self.focus == 1;
                if let Some(input) = self.input() {
                    input.key(code, modifiers, multiline);
                }
            }
        }
    }

    pub fn hint(&self) -> &'static str {
        if !self.editing {
            return match self.focus {
                3 => "Deadline · w / Ctrl-P: calendar · e: edit",
                CHECKLIST.. => "y/v/- status · Space toggle · Shift-J/K reorder · e edit",
                _ => "t: Today · s: Someday · w / Ctrl-P: calendar · e: edit",
            };
        }
        match self.focus {
            0 => "Task title",
            1 => "Enter: new line",
            2 => "Ctrl-P: calendar · today / evening / anytime / someday / inbox",
            3 => "Ctrl-P: calendar · YYYY-MM-DD · blank: none",
            4 => "Names or IDs, comma-separated",
            5 => "Project or area name / ID · blank: none",
            _ => "Enter / Ctrl-N: add · Ctrl-D: remove · Ctrl-T: status",
        }
    }

    pub fn lines(&self, width: usize, icons: super::theme::Icons) -> (Vec<(String, bool)>, usize) {
        self.render_lines(width, icons, true)
    }

    pub fn preview_lines(&self, width: usize, icons: super::theme::Icons) -> Vec<String> {
        self.render_lines(width, icons, false)
            .0
            .into_iter()
            .map(|(line, _)| line)
            .collect()
    }

    fn render_lines(
        &self,
        width: usize,
        icons: super::theme::Icons,
        active: bool,
    ) -> (Vec<(String, bool)>, usize) {
        let mut lines = Vec::new();
        let mut cursor_line = 0;
        for (index, label) in LABELS.iter().enumerate() {
            let focused = active && index == self.focus;
            if focused {
                cursor_line = lines.len();
            }
            let value = self.draft.fields[index].display(focused && self.editing);
            let value = if value.is_empty() {
                "—".into()
            } else {
                value
            };
            let value = format!(" {} {label:<9}  {value}", icons.field(index));
            for line in super::screen::wrap_lines(&value, width.max(1)) {
                if focused && line.contains('▏') {
                    cursor_line = lines.len();
                }
                lines.push((line, focused));
            }
        }
        lines.push((String::new(), false));
        lines.push((format!(" {} Checklist", icons.field(CHECKLIST)), false));
        for (index, item) in self.draft.checklist.iter().enumerate() {
            let focused = active && self.focus == CHECKLIST + index;
            if focused {
                cursor_line = lines.len();
            }
            let status = match item.status {
                TaskStatus::Completed => "x",
                TaskStatus::Canceled => "-",
                _ => " ",
            };
            let value = format!("[{status}] {}", item.title.display(focused && self.editing));
            for line in super::screen::wrap_lines(&value, width.max(1)) {
                if focused && line.contains('▏') {
                    cursor_line = lines.len();
                }
                lines.push((line, focused));
            }
        }
        let add_focused = active && self.focus == CHECKLIST + self.draft.checklist.len();
        if add_focused {
            cursor_line = lines.len();
        }
        if self.editing && active {
            lines.push((
                format!("{} Add item", if add_focused { ">" } else { "+" }),
                add_focused,
            ));
        }
        (lines, cursor_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_keys_navigate_and_toggle_without_editing_text() {
        let mut editor = Editor::create(&Model::new(List::Inbox));
        editor.draft.fields[0] = Input::new("Original title");
        editor.add_item();
        editor.input().unwrap().insert("Step", false);
        editor.view_mode();
        for (key, expected) in [
            ('-', TaskStatus::Canceled),
            ('v', TaskStatus::Incomplete),
            ('v', TaskStatus::Incomplete),
            ('y', TaskStatus::Completed),
            ('y', TaskStatus::Completed),
            ('v', TaskStatus::Incomplete),
        ] {
            assert!(editor.view_key(KeyCode::Char(key), KeyModifiers::NONE));
            assert_eq!(editor.draft.checklist[0].status, expected);
        }
        assert!(editor.view_key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(editor.draft.checklist[0].status, TaskStatus::Completed);
        assert!(editor.view_key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(editor.draft.checklist[0].status, TaskStatus::Incomplete);
        editor.view_key(KeyCode::Home, KeyModifiers::NONE);
        assert!(!editor.view_key(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(editor.focus, 1);
        editor.view_key(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(editor.focus, 0);
        assert!(!editor.view_key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(editor.view_key(KeyCode::Char('t'), KeyModifiers::NONE));
        assert_eq!(editor.draft.value(2), "today");
        assert_eq!(editor.draft.value(0), "Original title");
        let rendered = editor.lines(80, super::super::theme::Icons::Plain).0;
        assert!(!rendered.iter().any(|(line, _)| line.contains('▏')));
        editor.view_key(KeyCode::Char('e'), KeyModifiers::NONE);
        assert!(editor.editing);
        editor.key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(editor.draft.value(0), "Original titlej");
    }

    #[test]
    fn unicode_cursor_insertion_and_deletion_preserve_boundaries() {
        let mut input = Input::new("café你好");
        input.key(KeyCode::Left, KeyModifiers::NONE, false);
        input.key(KeyCode::Backspace, KeyModifiers::NONE, false);
        input.insert("🙂", false);
        assert_eq!(input.value, "café🙂好");
        input.key(KeyCode::Delete, KeyModifiers::NONE, false);
        assert_eq!(input.value, "café🙂");
        input.insert("\nnotes", true);
        assert_eq!(input.value, "café🙂\nnotes");
    }

    #[test]
    fn checklist_draft_keeps_ids_while_renaming_and_toggling() {
        let mut editor = Editor::create(&Model::new(List::Inbox));
        editor.add_item();
        let id = editor.draft.checklist[0].id.clone();
        editor.input().unwrap().insert("Step", false);
        editor.key(KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert_eq!(editor.draft.checklist[0].id, id);
        assert_eq!(editor.draft.checklist[0].status, TaskStatus::Completed);
        editor.key(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(editor.draft.checklist.is_empty());
    }
}
