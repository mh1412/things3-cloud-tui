use clap::ValueEnum;
use iocraft::prelude::Color;

use super::model::List;
use crate::{store::Task, wire::task::TaskStatus};

#[derive(Debug, Default, Clone, Copy, ValueEnum)]
pub enum Icons {
    #[default]
    Nerd,
    Emoji,
    Plain,
}

impl Icons {
    pub fn field(self, index: usize) -> &'static str {
        match self {
            Self::Nerd => [
                "\u{f031}", "\u{f036}", "\u{f073}", "\u{f11e}", "\u{f02c}", "\u{f07b}", "\u{f0ae}",
            ][index],
            Self::Emoji => ["✏️", "📝", "📅", "🚩", "🏷️", "📁", "☑️"][index],
            Self::Plain => ["T", "N", "@", "!", "#", "/", "+"][index],
        }
    }
    pub fn list(self, list: List) -> &'static str {
        let index = List::ALL.iter().position(|value| *value == list).unwrap();
        match self {
            Self::Nerd => [
                "\u{f01c}", "\u{f005}", "\u{f073}", "\u{f0c9}", "\u{f186}", "\u{f02d}", "\u{f07b}",
            ][index],
            Self::Emoji => ["📥", "⭐", "📅", "📋", "🌙", "📖", "📁"][index],
            Self::Plain => ["I", "T", "U", "A", "S", "L", "P"][index],
        }
    }

    pub fn task(self, task: &Task) -> &'static str {
        let index = match task.status {
            TaskStatus::Completed => 0,
            TaskStatus::Canceled => 1,
            _ if task.is_project() => 2,
            _ => 3,
        };
        match self {
            Self::Nerd => ["\u{f058}", "\u{f05e}", "\u{f07b}", "\u{f10c}"][index],
            Self::Emoji => ["✅", "🚫", "📁", "⚪"][index],
            Self::Plain => ["[x]", "[-]", "[+]", "[ ]"][index],
        }
    }
}

pub struct Palette {
    pub background: Option<Color>,
    pub panel: Option<Color>,
    pub focus: Option<Color>,
    pub selection: Option<Color>,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub green: Color,
    pub red: Color,
    pub border: Color,
}

impl Palette {
    pub fn new(no_color: bool) -> Self {
        let color = |r, g, b| {
            if no_color {
                Color::Reset
            } else {
                Color::Rgb { r, g, b }
            }
        };
        let background = |r, g, b| (!no_color).then(|| color(r, g, b));
        Self {
            background: background(24, 24, 37),
            panel: background(30, 30, 46),
            focus: background(49, 50, 68),
            selection: background(69, 71, 110),
            text: color(205, 214, 244),
            muted: color(147, 153, 178),
            accent: color(137, 180, 250),
            green: color(166, 227, 161),
            red: color(243, 139, 168),
            border: color(69, 71, 90),
        }
    }
}
