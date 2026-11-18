//! Developer console: `/` opens a command line for testing (`/give weapon
//! sentry`). It suggests completions for the word being typed as you type
//! (Tab or a click-free Up/Down picks one), keeps a history and a short
//! log. Parsing and suggestions come from `planetfall_sim::cheats`; the
//! game holds still while it is open.

use super::kit::{ACCENT, BAD, DIM, PANEL, TEXT};
use super::{Builder, Layer};
use planetfall_sim::cheats::{self, Parsed};
use planetfall_sim::{Command, Game};
use quasar_input::{FrameInput, Key};
use quasar_ui::{Anchor, Color, Edges, LayoutItem, Length, TextMeasure, TextStyle, Vec2};

/// Log lines kept on screen.
const LOG_LINES: usize = 8;
/// Suggestions listed at once.
const SHOWN: usize = 10;

/// What the console asks of the game this frame.
pub enum ConsoleAction {
    Close,
    Run(Command),
}

#[derive(Default)]
pub struct Console {
    line: String,
    history: Vec<String>,
    /// Position while browsing history with Up/Down (no suggestions shown).
    browsing: Option<usize>,
    /// Highlighted suggestion.
    selected: usize,
    log: Vec<(String, Color)>,
    /// Skip the text of the key press that opened the console.
    fresh: bool,
}

impl Console {
    pub fn open(&mut self) {
        self.line = "/".into();
        self.selected = 0;
        self.browsing = None;
        self.fresh = true;
    }

    /// A console that has run `lines` on `game` and has `typing` on its
    /// line (for screenshots).
    pub fn preview(game: &mut Game, lines: &[&str], typing: &str) -> Self {
        let mut console = Console::default();
        for line in lines {
            console.line = (*line).to_owned();
            if let Some(ConsoleAction::Run(command)) = console.submit(game) {
                game.update(&planetfall_sim::GameInput {
                    command: Some(command),
                    ..Default::default()
                });
            }
        }
        console.line = typing.to_owned();
        console
    }

    /// Edit the line from this frame's keys and text.
    pub fn update(&mut self, input: &FrameInput, game: &Game) -> Option<ConsoleAction> {
        if input.pressed(Key::Escape) {
            return Some(ConsoleAction::Close);
        }
        let suggestions = cheats::suggest(&self.line, game);
        let count = suggestions.items.len().min(SHOWN);
        if input.pressed(Key::Tab) {
            if let Some((word, _)) = suggestions.items.get(self.selected.min(count.max(1) - 1)) {
                self.line.truncate(suggestions.from);
                self.line.push_str(word);
                self.line.push(' ');
                self.selected = 0;
            }
            return None;
        }
        if input.pressed(Key::ArrowUp) || input.pressed(Key::ArrowDown) {
            let up = input.pressed(Key::ArrowUp);
            if count > 0 && self.browsing.is_none() {
                self.selected = if up {
                    (self.selected + count - 1) % count
                } else {
                    (self.selected + 1) % count
                };
            } else if !self.history.is_empty() {
                let last = self.history.len() - 1;
                let at = match (self.browsing, up) {
                    (None, true) => last,
                    (None, false) => return None,
                    (Some(i), true) => i.saturating_sub(1),
                    (Some(i), false) if i < last => i + 1,
                    (Some(_), false) => {
                        self.browsing = None;
                        self.line = "/".into();
                        return None;
                    }
                };
                self.browsing = Some(at);
                self.line = self.history[at].clone();
            }
            return None;
        }
        if input.pressed(Key::Enter) || input.pressed(Key::NumpadEnter) {
            return self.submit(game);
        }
        let mut edited = false;
        if input.pressed(Key::Backspace) {
            self.line.pop();
            edited = true;
        }
        if !self.fresh {
            for c in input.text().chars().filter(|c| !c.is_control()) {
                self.line.push(c);
                edited = true;
            }
        }
        self.fresh = false;
        if edited {
            self.selected = 0;
            self.browsing = None;
        }
        None
    }

    fn submit(&mut self, game: &Game) -> Option<ConsoleAction> {
        let line = self.line.trim().to_owned();
        self.line = "/".into();
        self.selected = 0;
        self.browsing = None;
        if line.trim_start_matches('/').is_empty() {
            return None;
        }
        if self.history.last() != Some(&line) {
            self.history.push(line.clone());
        }
        self.say(line.clone(), TEXT);
        match cheats::parse(&line, game) {
            Ok(Parsed::Cheat(cheat)) => Some(ConsoleAction::Run(Command::Cheat(cheat))),
            Ok(Parsed::Help(lines)) => {
                for l in lines {
                    self.say(format!("  {l}"), DIM);
                }
                None
            }
            Err(e) => {
                self.say(format!("  {e}"), BAD);
                None
            }
        }
    }

    fn say(&mut self, text: impl Into<String>, color: Color) {
        self.log.push((text.into(), color));
        let excess = self.log.len().saturating_sub(40);
        self.log.drain(..excess);
    }

    /// The console over the bottom of the screen: log, suggestions, usage
    /// and the input line.
    pub fn build(
        &self,
        game: &Game,
        time: f32,
        size: (f32, f32),
        scale: f32,
        measure: &dyn TextMeasure,
    ) -> Layer {
        let mut b = Builder::default();
        let root = b.root(false);
        let width = 780.0;
        let column = b.column(
            root,
            LayoutItem::size(Length::fixed(width), Length::Auto)
                .with_anchor(Anchor::BottomLeft)
                .with_margin(Edges::new(18.0, 0.0, 0.0, 120.0))
                .with_offset(Vec2::new(0.0, 0.0)),
            4.0,
            0.0,
        );
        let start = self.log.len().saturating_sub(LOG_LINES);
        if start < self.log.len() {
            let log = b.column(
                column,
                LayoutItem::size(Length::Fill, Length::Auto),
                2.0,
                8.0,
            );
            b.style(log, Color::rgba(5, 9, 15, 170), None, 3.0);
            for (text, color) in &self.log[start..] {
                b.tinted(log, text.clone(), TextStyle::Small, *color);
            }
        }
        let suggestions = cheats::suggest(&self.line, game);
        if !suggestions.items.is_empty() && self.browsing.is_none() {
            let list = b.column(
                column,
                LayoutItem::size(Length::fixed(520.0), Length::Auto),
                1.0,
                6.0,
            );
            b.style(
                list,
                PANEL,
                Some((1.0, Color::rgba(130, 156, 182, 70))),
                3.0,
            );
            let selected = self.selected.min(suggestions.items.len().min(SHOWN) - 1);
            for (i, (word, detail)) in suggestions.items.iter().take(SHOWN).enumerate() {
                let row = b.hrow(
                    list,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    10.0,
                    2.0,
                );
                if i == selected {
                    b.style(row, Color::rgba(255, 142, 48, 40), None, 2.0);
                }
                let color = if i == selected { ACCENT } else { TEXT };
                b.tinted(row, word.clone(), TextStyle::Body, color);
                if !detail.is_empty() {
                    b.tinted(row, detail.clone(), TextStyle::Small, DIM);
                }
            }
            if suggestions.items.len() > SHOWN {
                b.tinted(
                    list,
                    format!("+{} more: keep typing", suggestions.items.len() - SHOWN),
                    TextStyle::Small,
                    DIM,
                );
            }
        }
        if !suggestions.usage.is_empty() {
            let usage = b.column(
                column,
                LayoutItem::size(Length::Fill, Length::Auto),
                1.0,
                6.0,
            );
            b.style(usage, Color::rgba(5, 9, 15, 200), None, 3.0);
            for line in suggestions.usage.iter().take(3) {
                b.tinted(usage, line.clone(), TextStyle::Small, DIM);
            }
        }
        let input = b.hrow(
            column,
            LayoutItem::size(Length::Fill, Length::Auto),
            0.0,
            8.0,
        );
        b.style(input, PANEL, Some((1.0, ACCENT)), 3.0);
        let cursor = if (time * 2.0).fract() < 0.5 { "_" } else { " " };
        b.tinted(
            input,
            format!("{}{cursor}", self.line),
            TextStyle::Body,
            TEXT,
        );
        let keys = b.hrow(
            column,
            LayoutItem::size(Length::Fill, Length::Auto),
            0.0,
            4.0,
        );
        b.style(keys, Color::rgba(5, 9, 15, 200), None, 3.0);
        b.tinted(
            keys,
            "Tab complete · Up/Down choose · Enter run · Esc close",
            TextStyle::Small,
            DIM,
        );
        b.finish_laid_out(size, scale, measure)
    }
}
