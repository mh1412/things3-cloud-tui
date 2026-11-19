use chrono::{DateTime, Utc};
use clap::ValueEnum;

use crate::{
    ids::ThingsId,
    store::{Task, ThingsStore},
    wire::task::TaskStatus,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum List {
    Inbox,
    #[default]
    Today,
    Upcoming,
    Anytime,
    Someday,
    Logbook,
    Projects,
}

impl List {
    pub const ALL: [Self; 7] = [
        Self::Inbox,
        Self::Today,
        Self::Upcoming,
        Self::Anytime,
        Self::Someday,
        Self::Logbook,
        Self::Projects,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Today => "Today",
            Self::Upcoming => "Upcoming",
            Self::Anytime => "Anytime",
            Self::Someday => "Someday",
            Self::Logbook => "Logbook",
            Self::Projects => "Projects",
        }
    }

    pub fn tasks(self, store: &ThingsStore, today: &DateTime<Utc>) -> Vec<Task> {
        match self {
            Self::Inbox => store.inbox(),
            Self::Today => store.today(today),
            Self::Anytime => store.anytime(today),
            Self::Someday => store.someday(),
            Self::Logbook => store.logbook(None, None),
            Self::Projects => store.projects(Some(TaskStatus::Incomplete)),
            Self::Upcoming => {
                let mut tasks = store.tasks(Some(TaskStatus::Incomplete), Some(false), None);
                tasks.retain(|t| !t.in_someday() && t.start_date.is_some_and(|d| d > *today));
                tasks.sort_by_key(|t| (t.start_date, t.index, t.uuid.clone()));
                tasks
            }
        }
    }
}

pub struct Model {
    pub edit_changed_at: Option<std::time::Instant>,
    pub quick_create: bool,
    pub calendar: Option<super::calendar::Calendar>,
    pub checked: std::collections::HashSet<ThingsId>,
    pub prefix: Option<char>,
    pub editor: Option<super::editor::Editor>,
    pub store: ThingsStore,
    pub today: DateTime<Utc>,
    pub list: List,
    pub project: Option<ThingsId>,
    pub tasks: Vec<Task>,
    pub selected: usize,
    pub query: String,
    pub searching: bool,
    pub details: bool,
    pub detail_scroll: usize,
    pub help: bool,
    pub busy: bool,
    pub message: String,
}

impl Model {
    pub fn new(list: List) -> Self {
        Self {
            edit_changed_at: None,
            quick_create: false,
            calendar: None,
            checked: Default::default(),
            prefix: None,
            editor: None,
            store: ThingsStore::default(),
            today: crate::common::today_utc(),
            list,
            project: None,
            tasks: Vec::new(),
            selected: 0,
            query: String::new(),
            searching: false,
            details: false,
            detail_scroll: 0,
            help: false,
            busy: true,
            message: "Loading…".into(),
        }
    }

    pub fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.selected)
    }

    pub fn title(&self) -> String {
        self.project
            .as_ref()
            .map(|id| self.store.resolve_project_title(id))
            .unwrap_or_else(|| self.list.title().into())
    }

    pub fn rebuild(&mut self, preserve: Option<ThingsId>) {
        let query = self.query.to_lowercase();
        let mut tasks = if let Some(project) = &self.project {
            self.store
                .tasks(None, Some(false), None)
                .into_iter()
                .filter(|t| self.store.effective_project_uuid(t).as_ref() == Some(project))
                .collect()
        } else {
            self.list.tasks(&self.store, &self.today)
        };
        tasks.retain(|t| {
            t.title.to_lowercase().contains(&query)
                || t.notes
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
                || t.checklist_items
                    .iter()
                    .any(|c| c.title.to_lowercase().contains(&query))
        });
        self.selected = preserve
            .and_then(|id| tasks.iter().position(|t| t.uuid == id))
            .unwrap_or(self.selected)
            .min(tasks.len().saturating_sub(1));
        self.tasks = tasks;
        self.checked
            .retain(|id| self.tasks.iter().any(|task| &task.uuid == id));
        self.detail_scroll = 0;
    }

    pub fn change_list(&mut self, list: List) {
        self.checked.clear();
        self.list = list;
        self.project = None;
        self.selected = 0;
        self.query.clear();
        self.details = false;
        self.rebuild(None);
    }

    pub fn cycle_list(&mut self, delta: isize) {
        let index = List::ALL.iter().position(|v| *v == self.list).unwrap_or(0);
        self.change_list(
            List::ALL[(index as isize + delta).rem_euclid(List::ALL.len() as isize) as usize],
        );
    }

    pub fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.tasks.len().saturating_sub(1));
        self.detail_scroll = 0;
    }

    pub fn enter(&mut self) {
        if let Some(task) = self.selected_task() {
            if task.is_project() {
                self.project = Some(task.uuid.clone());
                self.selected = 0;
                self.query.clear();
                self.rebuild(None);
            } else {
                self.details = !self.details;
            }
        }
    }

    pub fn escape(&mut self) {
        if self.help {
            self.help = false;
        } else if self.searching || !self.query.is_empty() {
            self.searching = false;
            self.query.clear();
            self.rebuild(None);
        } else if self.details {
            self.details = false;
        } else if self.project.take().is_some() {
            self.rebuild(None);
        }
    }
}

/// Strip terminal control characters from cloud-supplied text.
pub fn text(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::fold_items;
    use crate::wire::wire_object::WireItem;
    use serde_json::json;

    fn model() -> Model {
        let item: WireItem = serde_json::from_value(json!({
            "A7h5eCi24RvAWKC3Hv3muf": {"t":0,"e":"Task7","p":{"tt":"First","st":0,"cd":1,"ix":0}},
            "KGvAPpMrzHAKMdgMiERP1V": {"t":0,"e":"Task7","p":{"tt":"Second","st":0,"cd":1,"ix":1,"nt":"Café notes"}}
        })).unwrap();
        let mut m = Model::new(List::Inbox);
        m.store = ThingsStore::from_raw_state(&fold_items([item]));
        m.rebuild(None);
        m
    }

    #[test]
    fn today_uses_reference_date_then_ascending_manual_order() {
        let mut m = model();
        let first = m.tasks[0].uuid.clone();
        let second = m.tasks[1].uuid.clone();
        for task in m.store.tasks_by_uuid.values_mut() {
            task.start = crate::wire::task::TaskStart::Anytime;
            task.start_date = Some(m.today);
            task.today_index_reference = Some(100);
            task.today_index = if task.uuid == first { 1 } else { 2 };
        }
        assert_eq!(List::Today.tasks(&m.store, &m.today)[0].uuid, first);
        m.store
            .tasks_by_uuid
            .get_mut(&second)
            .unwrap()
            .today_index_reference = Some(200);
        assert_eq!(List::Today.tasks(&m.store, &m.today)[0].uuid, second);
        // An unassigned index must not jump ahead of a newer reference date.
        m.store.tasks_by_uuid.get_mut(&first).unwrap().today_index = 0;
        assert_eq!(List::Today.tasks(&m.store, &m.today)[0].uuid, second);
    }

    #[test]
    fn selection_survives_refresh_and_clamps_when_item_disappears() {
        let mut m = model();
        m.move_selection(1);
        let id = m.selected_task().unwrap().uuid.clone();
        m.store.tasks_by_uuid.get_mut(&id).unwrap().index = -1;
        m.rebuild(Some(id.clone()));
        assert_eq!(m.selected, 0);
        assert_eq!(m.selected_task().unwrap().uuid, id);
        m.store.tasks_by_uuid.remove(&id);
        m.rebuild(Some(id));
        assert_eq!(m.selected_task().unwrap().title, "First");
    }

    #[test]
    fn search_handles_notes_empty_results_and_escape() {
        let mut m = model();
        m.query = "CAFÉ".into();
        m.rebuild(None);
        assert_eq!(m.tasks.len(), 1);
        assert_eq!(m.tasks[0].title, "Second");
        m.query = "not found".into();
        m.rebuild(None);
        m.move_selection(20);
        assert!(m.selected_task().is_none());
        m.enter();
        m.escape();
        assert_eq!(m.tasks.len(), 2);
        m.cycle_list(-1);
        assert_eq!(m.list, List::Projects);
    }
}
