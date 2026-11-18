//! Developer console: `/` opens a command line for testing (`/give weapon
//! sentry`). It suggests completions for the word being typed as you type
//! (Tab or a click-free Up/Down picks one), keeps a history and a short
//! log. Line editing (cursor, Home/End, Delete, IME text) is a
//! `quasar-ui` `TextInput`; parsing and suggestions come from
//! `planetfall_sim::cheats`. The game holds still while it is open.

use super::kit::{ACCENT, BAD, DIM, PANEL, TEXT};
use super::{Builder, Layer};
use planetfall_sim::cheats::{self, Parsed};
use planetfall_sim::{Command, Game};
use quasar_input::{FrameInput, Key};
use quasar_ui::{
    Anchor, Color, Edges, Layer as UiLayer, LayoutItem, Length, Rect, TextMeasure, TextStyle,
    Theme, Ui, UiBindings, UiText, Vec2, Widget, from_frame_input,
};

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
    /// Caret position in characters.
    cursor: usize,
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
        self.set_line("/");
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
        console.set_line(typing);
        console
    }

    /// Replace the line, caret at the end.
    fn set_line(&mut self, line: &str) {
        self.line = line.to_owned();
        self.cursor = self.line.chars().count();
    }

    /// Apply this frame's typing, caret keys and deletes through a
    /// one-field `quasar-ui` tree. Returns whether the text changed.
    fn edit(&mut self, input: &FrameInput, measure: &dyn TextMeasure) -> bool {
        let mut ui = Ui::new(Theme::modern());
        let field = ui.add_root(
            Widget::TextInput {
                text: self.line.clone(),
                placeholder: UiText::literal(""),
                cursor: self.cursor,
                anchor: self.cursor,
                max_len: 200,
                password: false,
                action: None,
            },
            LayoutItem::size(Length::fixed(600.0), Length::fixed(30.0)),
            UiLayer::Normal,
            false,
        );
        ui.focus(Some(field));
        // The console owns navigation and draws no selection.
        let edits = quasar_ui::UiInput {
            pointer: None,
            up: false,
            down: false,
            left: input.pressed(Key::ArrowLeft),
            right: input.pressed(Key::ArrowRight),
            next: false,
            previous: false,
            activate: false,
            cancel: false,
            shift: false,
            select_all: false,
            ..from_frame_input(input, &UiBindings::default())
        };
        let edits = if self.fresh {
            quasar_ui::UiInput {
                text: String::new(),
                ..edits
            }
        } else {
            edits
        };
        ui.update(
            &edits,
            Rect::new(0.0, 0.0, 800.0, 100.0),
            1.0,
            Edges::ZERO,
            measure,
            0.0,
        );
        let Some(Widget::TextInput { text, cursor, .. }) = ui.node(field).map(|n| &n.widget) else {
            return false;
        };
        self.cursor = *cursor;
        let changed = *text != self.line;
        if changed {
            self.line = text.clone();
        }
        changed
    }

    /// Edit the line from this frame's keys and text.
    pub fn update(
        &mut self,
        input: &FrameInput,
        game: &Game,
        measure: &dyn TextMeasure,
    ) -> Option<ConsoleAction> {
        if input.pressed(Key::Escape) {
            return Some(ConsoleAction::Close);
        }
        let suggestions = cheats::suggest(&self.line, game);
        let count = suggestions.items.len().min(SHOWN);
        if input.pressed(Key::Tab) {
            if let Some((word, _)) = suggestions.items.get(self.selected.min(count.max(1) - 1)) {
                let mut line = self.line[..suggestions.from].to_owned();
                line.push_str(word);
                line.push(' ');
                self.set_line(&line);
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
                        self.set_line("/");
                        return None;
                    }
                };
                self.browsing = Some(at);
                let line = self.history[at].clone();
                self.set_line(&line);
            }
            return None;
        }
        if input.pressed(Key::Enter) || input.pressed(Key::NumpadEnter) {
            return self.submit(game);
        }
        let edited = self.edit(input, measure);
        self.fresh = false;
        if edited {
            self.selected = 0;
            self.browsing = None;
        }
        None
    }

    fn submit(&mut self, game: &Game) -> Option<ConsoleAction> {
        let line = self.line.trim().to_owned();
        self.set_line("/");
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
        // The caret sits between the text before and after it.
        let split = self
            .line
            .char_indices()
            .nth(self.cursor)
            .map_or(self.line.len(), |(i, _)| i);
        let (before, after) = self.line.split_at(split);
        // Blink only at the end of the line, where it cannot shift text.
        let caret = if !after.is_empty() || (time * 2.0).fract() < 0.5 {
            "|"
        } else {
            ""
        };
        b.tinted(
            input,
            format!("{before}{caret}{after}"),
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
            "Tab complete · Up/Down choose · Left/Right move · Enter run · Esc close",
            TextStyle::Small,
            DIM,
        );
        b.finish_laid_out(size, scale, measure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quasar_input::InputState;
    use quasar_ui::MonospaceMeasure;

    /// One frame with `keys` pressed and `text` typed.
    fn frame(keys: &[Key], text: &str) -> FrameInput {
        let mut state = InputState::default();
        for &key in keys {
            state.key(key, true);
        }
        state.text = text.to_owned();
        let mut input = FrameInput::default();
        input.capture(&state, 1);
        input
    }

    #[test]
    fn edits_mid_line_and_completes() {
        let game = Game::new(planetfall_sim::Profile::default(), 1);
        let measure = MonospaceMeasure;
        let mut console = Console::default();
        console.open();
        // The key press that opened the console types nothing.
        console.update(&frame(&[], "/"), &game, &measure);
        console.update(&frame(&[], "gve"), &game, &measure);
        for _ in 0..2 {
            console.update(&frame(&[Key::ArrowLeft], ""), &game, &measure);
        }
        console.update(&frame(&[], "i"), &game, &measure);
        assert_eq!((console.line.as_str(), console.cursor), ("/give", 3));
        console.update(&frame(&[Key::End], " wea"), &game, &measure);
        console.update(&frame(&[Key::Tab], ""), &game, &measure);
        assert_eq!(console.line, "/give weapon ");
        console.update(&frame(&[Key::Home, Key::Delete], ""), &game, &measure);
        assert_eq!((console.line.as_str(), console.cursor), ("give weapon ", 0));
    }
}
