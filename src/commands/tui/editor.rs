use iocraft::prelude::{KeyCode, KeyModifiers};

use super::model::{List, Model, text};
use crate::{
    ids::ThingsId,
    store::Task,
    wire::task::{TaskStart, TaskStatus},
};

pub const CHECKLIST: usize = 6;
pub const LABELS: [&str; CHECKLIST] = [
    "Title",
    "Notes (Enter inserts a line)",
    "When (inbox / anytime / today / evening / someday / YYYY-MM-DD)",
    "Deadline (YYYY-MM-DD; blank clears)",
    "Tags (comma-separated names or IDs)",
    "Location (project/area name or ID; blank removes location)",
];

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
            KeyCode::F(2) => {
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

    pub fn lines(&self, width: usize) -> (Vec<(String, bool)>, usize) {
        let mut lines = Vec::new();
        let mut cursor_line = 0;
        for (index, label) in LABELS.iter().enumerate() {
            let focused = index == self.focus;
            lines.push((
                format!("{} {label}", if focused { ">" } else { " " }),
                focused,
            ));
            let value = self.draft.fields[index].display(focused);
            for line in super::screen::wrap_lines(&value, width.max(1)) {
                if focused && line.contains('▏') {
                    cursor_line = lines.len();
                }
                lines.push((line, focused));
            }
        }
        lines.push(("Checklist (F2 cycles open / done / canceled)".into(), false));
        for (index, item) in self.draft.checklist.iter().enumerate() {
            let focused = self.focus == CHECKLIST + index;
            let status = match item.status {
                TaskStatus::Completed => "x",
                TaskStatus::Canceled => "-",
                _ => " ",
            };
            let value = format!("[{status}] {}", item.title.display(focused));
            for line in super::screen::wrap_lines(&value, width.max(1)) {
                if focused && line.contains('▏') {
                    cursor_line = lines.len();
                }
                lines.push((line, focused));
            }
        }
        let add_focused = self.focus == CHECKLIST + self.draft.checklist.len();
        if add_focused {
            cursor_line = lines.len();
        }
        lines.push((
            format!(
                "{} Add checklist item (Enter)",
                if add_focused { ">" } else { "+" }
            ),
            add_focused,
        ));
        (lines, cursor_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        editor.key(KeyCode::F(2), KeyModifiers::NONE);
        assert_eq!(editor.draft.checklist[0].id, id);
        assert_eq!(editor.draft.checklist[0].status, TaskStatus::Completed);
        editor.key(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert!(editor.draft.checklist.is_empty());
    }
}
