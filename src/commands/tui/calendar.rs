use chrono::{Datelike, Duration, Months, NaiveDate};
use iocraft::prelude::KeyCode;

pub struct Calendar {
    pub date: NaiveDate,
    pub field: Option<usize>,
}

impl Calendar {
    pub fn key(&mut self, key: KeyCode, today: NaiveDate) {
        let days = match key {
            KeyCode::Left | KeyCode::Char('h') => -1,
            KeyCode::Right | KeyCode::Char('l') => 1,
            KeyCode::Up | KeyCode::Char('k') => -7,
            KeyCode::Down | KeyCode::Char('j') => 7,
            _ => 0,
        };
        self.date = match key {
            KeyCode::PageUp | KeyCode::Char('[') => self.date.checked_sub_months(Months::new(1)),
            KeyCode::PageDown | KeyCode::Char(']') => self.date.checked_add_months(Months::new(1)),
            KeyCode::Char('t') => Some(today),
            _ => self.date.checked_add_signed(Duration::days(days)),
        }
        .unwrap_or(self.date);
    }

    pub fn display(&self) -> String {
        let first = self.date.with_day(1).unwrap();
        let offset = first.weekday().num_days_from_monday();
        let mut lines = vec![
            self.date.format("%B %Y").to_string(),
            " Mo  Tu  We  Th  Fr  Sa  Su".into(),
        ];
        let mut row = "    ".repeat(offset as usize);
        for day in 1..=31 {
            let Some(date) = first.with_day(day) else {
                break;
            };
            row.push_str(&if date == self.date {
                format!("[{day:2}]")
            } else {
                format!(" {day:2} ")
            });
            if (offset + day).is_multiple_of(7) {
                lines.push(std::mem::take(&mut row));
            }
        }
        if !row.is_empty() {
            lines.push(row);
        }
        lines.push(format!("\nSelected: {}", self.date));
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_handles_month_ends_and_leap_days() {
        let today = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();
        let mut calendar = Calendar {
            date: today,
            field: None,
        };
        calendar.key(KeyCode::PageDown, today);
        assert_eq!(calendar.date.to_string(), "2024-02-29");
        assert!(calendar.display().contains("[29]"));
        calendar.key(KeyCode::Right, today);
        assert_eq!(calendar.date.to_string(), "2024-03-01");
        calendar.key(KeyCode::Char('t'), today);
        assert_eq!(calendar.date, today);
    }
}
