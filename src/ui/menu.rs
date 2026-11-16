//! Interactive screens: title, pause, options, confirmations, and the
//! in-world terminals (navigation, fabricator, level-up, debrief).
//! Keyboard, mouse and gamepad all navigate through `quasar-ui` focus.

use super::{Builder, Layer, format_cost, format_materials, hud::roman};
use crate::bindings::Prompts;
use planetfall_sim::content::{PlanetId, ShipUpgrade};
use planetfall_sim::run::RunOutcome;
use planetfall_sim::{Command, Game, Terminal};
use quasar_ui::{
    Align, LayoutItem, Length, NodeId, Rect, TextMeasure, TextStyle, UiEvent, UiInput, UiText,
    Widget,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Title,
    Pause { surface: bool },
    Options { from_title: bool },
    ConfirmAbandon,
    ConfirmReset,
    ConfirmQuit { from_title: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptionsState {
    pub fullscreen: bool,
    pub master: f32,
    pub music: f32,
    pub effects: f32,
    pub shake: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    None,
    Launch,
    Resume,
    OpenOptions,
    Back,
    Apply(OptionsState),
    AskReset,
    ResetProgress,
    AskAbandon,
    Abandon,
    QuitToTitle,
    AskQuit,
    Quit,
    Game(Command),
}

/// What the menu should currently show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Content {
    Screen(Screen),
    Terminal,
    LevelUp,
}

pub struct Menu {
    layer: Option<Layer>,
    signature: String,
    actions: Vec<MenuAction>,
    cancel: MenuAction,
    sliders: Vec<(NodeId, Slot)>,
    checks: Vec<(NodeId, Slot)>,
    pub options: OptionsState,
}

#[derive(Clone, Copy, Debug)]
enum Slot {
    Master,
    Music,
    Effects,
    Fullscreen,
    Shake,
}

impl Menu {
    pub fn new(options: OptionsState) -> Self {
        Self {
            layer: None,
            signature: String::new(),
            actions: Vec::new(),
            cancel: MenuAction::None,
            sliders: Vec::new(),
            checks: Vec::new(),
            options,
        }
    }

    pub fn layer(&self) -> Option<&Layer> {
        self.layer.as_ref()
    }

    /// Move the built layer out (for one-shot offscreen renders).
    #[cfg_attr(feature = "desktop", allow(dead_code))]
    pub fn take_layer(&mut self) -> Option<Layer> {
        self.layer.take()
    }

    pub fn is_open(&self) -> bool {
        self.layer.is_some()
    }

    /// Show `content` (rebuilding only when what it displays changed).
    pub fn show(
        &mut self,
        content: Option<Content>,
        game: &Game,
        measure: &dyn TextMeasure,
        scale: f32,
        prompts: &Prompts,
    ) {
        let Some(content) = content else {
            self.layer = None;
            self.signature.clear();
            return;
        };
        let signature = signature(content, game, prompts);
        if self.layer.is_some() && signature == self.signature {
            return;
        }
        let focus_index = self.layer.as_ref().and_then(|l| {
            l.ui.focused()
                .and_then(|f| l.ui.focus_order().iter().position(|&n| n == f))
        });
        let same_screen = self.signature.split('|').next() == signature.split('|').next();
        self.signature = signature;
        let mut b = Builder::default();
        self.actions.clear();
        self.sliders.clear();
        self.checks.clear();
        self.cancel = MenuAction::None;
        self.build(&mut b, content, game, prompts);
        let mut layer = b.finish();
        if same_screen && let Some(i) = focus_index {
            // Keep the cursor in place when only numbers changed (e.g. after a purchase).
            let viewport = Rect::new(0.0, 0.0, 1920.0, 1080.0);
            layer
                .ui
                .layout(viewport, scale, quasar_ui::Edges::ZERO, measure);
            let target = layer.ui.focus_order().get(i).copied();
            layer.ui.focus(target);
        }
        self.layer = Some(layer);
    }

    pub fn update(
        &mut self,
        input: &UiInput,
        viewport: Rect,
        scale: f32,
        measure: &dyn TextMeasure,
        dt: f32,
    ) -> MenuAction {
        let Some(layer) = &mut self.layer else {
            return MenuAction::None;
        };
        let frame = layer
            .ui
            .update(input, viewport, scale, quasar_ui::Edges::ZERO, measure, dt);
        for (id, slot) in &self.sliders {
            if let Some(node) = layer.ui.node(*id)
                && let Widget::Slider { value, .. } = node.widget
            {
                match slot {
                    Slot::Master => self.options.master = value,
                    Slot::Music => self.options.music = value,
                    Slot::Effects => self.options.effects = value,
                    _ => {}
                }
            }
        }
        for (id, slot) in &self.checks {
            if let Some(node) = layer.ui.node(*id)
                && let Widget::Checkbox { checked, .. } = node.widget
            {
                match slot {
                    Slot::Fullscreen => self.options.fullscreen = checked,
                    Slot::Shake => self.options.shake = checked,
                    _ => {}
                }
            }
        }
        for event in frame.events {
            match event {
                UiEvent::Pressed(action) => {
                    if let Some(a) = self.actions.get(action.0 as usize) {
                        return a.clone();
                    }
                }
                UiEvent::Cancelled => return self.cancel.clone(),
                _ => {}
            }
        }
        MenuAction::None
    }

    fn action(&mut self, action: MenuAction) -> u64 {
        self.actions.push(action);
        (self.actions.len() - 1) as u64
    }

    fn card(b: &mut Builder, width: f32) -> NodeId {
        let root = b.root(true);
        b.card(
            root,
            LayoutItem::size(Length::fixed(width), Length::Auto).with_align(Align::Center),
            7.0,
            22.0,
        )
    }

    fn button(
        &mut self,
        b: &mut Builder,
        parent: NodeId,
        text: impl Into<String>,
        action: MenuAction,
        enabled: bool,
    ) {
        let id = self.action(action);
        b.button(parent, text, id, enabled);
    }

    fn build(&mut self, b: &mut Builder, content: Content, game: &Game, prompts: &Prompts) {
        match content {
            Content::Screen(screen) => self.build_screen(b, screen, game, prompts),
            Content::LevelUp => {
                let card = Self::card(b, 560.0);
                let level = game.run.as_ref().map_or(1, |r| r.player.level);
                b.label(
                    card,
                    format!("LEVEL {level} — FIELD UPGRADE"),
                    TextStyle::Heading,
                );
                b.label(
                    card,
                    "Choose one. Field upgrades last until this drop ends.",
                    TextStyle::Small,
                );
                let choices = game
                    .run
                    .as_ref()
                    .and_then(|r| r.level_up_choices())
                    .unwrap_or(&[])
                    .to_vec();
                for (i, perk) in choices.iter().enumerate() {
                    self.button(
                        b,
                        card,
                        format!("{}  —  {}", perk.title(), perk.description()),
                        MenuAction::Game(Command::ChoosePerk(i)),
                        true,
                    );
                }
            }
            Content::Terminal => match &game.terminal {
                Some(Terminal::Navigation) => self.navigation(b, game),
                Some(Terminal::Fabricator) => self.fabricator(b, game),
                Some(Terminal::Debrief(summary)) => {
                    let card = Self::card(b, 560.0);
                    let (title, tone) = match summary.outcome {
                        RunOutcome::Extracted => {
                            ("EXTRACTION SUCCESSFUL", "Cargo transferred to the hold.")
                        }
                        RunOutcome::Died => (
                            "SIGNAL LOST",
                            "You were recovered by suit beacon. All cargo was lost.",
                        ),
                        RunOutcome::Abandoned => {
                            ("DROP ABORTED", "Emergency recall. All cargo was lost.")
                        }
                    };
                    b.label(card, title, TextStyle::Heading);
                    b.label(
                        card,
                        format!("{} · {}", summary.planet.info().name, tone),
                        TextStyle::Small,
                    );
                    b.label(
                        card,
                        format!(
                            "Time {:02}:{:02}   ·   Kills {}   ·   Level {}",
                            summary.seconds / 60,
                            summary.seconds % 60,
                            summary.kills,
                            summary.level
                        ),
                        TextStyle::Body,
                    );
                    let weapons: Vec<String> = summary
                        .weapons
                        .iter()
                        .map(|(w, l)| format!("{} {}", w.name(), roman(*l)))
                        .collect();
                    b.label(
                        card,
                        format!("Loadout: {}", weapons.join(", ")),
                        TextStyle::Small,
                    );
                    if summary.outcome == RunOutcome::Extracted {
                        b.label(
                            card,
                            format!("Banked: {}", format_materials(&summary.banked)),
                            TextStyle::Body,
                        );
                        if summary.salvage.total() > 0 {
                            b.label(
                                card,
                                format!("Ship AI salvage: {}", format_materials(&summary.salvage)),
                                TextStyle::Small,
                            );
                        }
                    } else if summary.lost.total() > 0 {
                        b.label(
                            card,
                            format!("Lost: {}", format_materials(&summary.lost)),
                            TextStyle::Body,
                        );
                    }
                    b.label(
                        card,
                        format!("Hold: {}", format_materials(&game.profile.materials)),
                        TextStyle::Small,
                    );
                    self.cancel = MenuAction::Game(Command::Close);
                    self.button(b, card, "CONTINUE", MenuAction::Game(Command::Close), true);
                }
                Some(Terminal::Module(upgrade)) => {
                    let card = Self::card(b, 520.0);
                    b.label(card, upgrade.name().to_uppercase(), TextStyle::Heading);
                    b.label(
                        card,
                        format!(
                            "Level {} / {}",
                            game.profile.level(*upgrade),
                            upgrade.max_level()
                        ),
                        TextStyle::Body,
                    );
                    b.label(card, upgrade.effect(), TextStyle::Small);
                    b.label(
                        card,
                        "Upgrade this module at the Fabricator.",
                        TextStyle::Small,
                    );
                    self.cancel = MenuAction::Game(Command::Close);
                    self.button(b, card, "CLOSE", MenuAction::Game(Command::Close), true);
                }
                None => {}
            },
        }
    }

    fn navigation(&mut self, b: &mut Builder, game: &Game) {
        let card = Self::card(b, 640.0);
        b.label(card, "NAVIGATION CONSOLE", TextStyle::Heading);
        b.label(
            card,
            "Plot an FTL jump. Dangerous worlds pay better.",
            TextStyle::Small,
        );
        for planet in PlanetId::ALL {
            let info = planet.info();
            let unlocked = game.profile.unlocked(planet);
            let here = game.profile.orbiting == Some(planet);
            let label = if !unlocked {
                format!(
                    "{}  —  LOCKED (Reactor & Nav Core level {})",
                    info.name, info.nav_level
                )
            } else if here {
                format!("{}  —  IN ORBIT", info.name)
            } else {
                format!(
                    "JUMP TO {}  —  {} · hazard x{:.1}",
                    info.name.to_uppercase(),
                    info.kind,
                    info.hazard
                )
            };
            self.button(
                b,
                card,
                label,
                MenuAction::Game(Command::TravelTo(planet)),
                unlocked && !here,
            );
            if unlocked {
                b.label(
                    card,
                    format!("{} Objective: {} lumite.", info.description, info.objective),
                    TextStyle::Small,
                );
            }
        }
        self.cancel = MenuAction::Game(Command::Close);
        self.button(b, card, "CLOSE", MenuAction::Game(Command::Close), true);
    }

    fn fabricator(&mut self, b: &mut Builder, game: &Game) {
        let card = Self::card(b, 700.0);
        b.label(card, "FABRICATOR", TextStyle::Heading);
        b.label(
            card,
            format!("Hold: {}", format_materials(&game.profile.materials)),
            TextStyle::Body,
        );
        for upgrade in ShipUpgrade::ALL {
            let level = game.profile.level(upgrade);
            let label = match game.profile.next_cost(upgrade) {
                None => format!(
                    "{}  —  COMPLETE ({}/{})",
                    upgrade.name(),
                    level,
                    upgrade.max_level()
                ),
                Some(cost) => {
                    let verb = if level == 0 { "BUILD" } else { "UPGRADE" };
                    format!(
                        "{verb} {} {}/{}  —  {}",
                        upgrade.name(),
                        level,
                        upgrade.max_level(),
                        format_cost(&cost)
                    )
                }
            };
            let affordable = game
                .profile
                .next_cost(upgrade)
                .is_some_and(|c| game.profile.materials.covers(c));
            self.button(
                b,
                card,
                label,
                MenuAction::Game(Command::Purchase(upgrade)),
                affordable,
            );
            b.label(card, upgrade.effect(), TextStyle::Small);
        }
        self.cancel = MenuAction::Game(Command::Close);
        self.button(b, card, "CLOSE", MenuAction::Game(Command::Close), true);
    }

    fn build_screen(&mut self, b: &mut Builder, screen: Screen, game: &Game, prompts: &Prompts) {
        match screen {
            Screen::Title => {
                let card = Self::card(b, 520.0);
                b.label(card, "PLANETFALL", TextStyle::Heading);
                b.label(
                    card,
                    "Build your ship. Drop. Mine. Survive. Extract.",
                    TextStyle::Small,
                );
                let s = &game.profile.stats;
                let progress = if s.drops == 0 {
                    "New commander".to_string()
                } else {
                    format!(
                        "{} drops · {} extractions · {}",
                        s.drops,
                        s.extractions,
                        format_materials(&game.profile.materials)
                    )
                };
                b.label(card, progress, TextStyle::Small);
                self.button(
                    b,
                    card,
                    if s.drops == 0 { "BEGIN" } else { "CONTINUE" },
                    MenuAction::Launch,
                    true,
                );
                self.button(b, card, "OPTIONS", MenuAction::OpenOptions, true);
                self.button(b, card, "QUIT", MenuAction::AskQuit, true);
                b.paragraph(
                    card,
                    &prompts.format(
                        "{move} move · {dash} dash · {interact} use · {extract} call pod · weapons fire automatically",
                    ),
                    TextStyle::Small,
                    476.0,
                );
                self.cancel = MenuAction::AskQuit;
            }
            Screen::Pause { surface } => {
                let card = Self::card(b, 460.0);
                b.label(card, "PAUSED", TextStyle::Heading);
                self.button(b, card, "RESUME", MenuAction::Resume, true);
                self.button(b, card, "OPTIONS", MenuAction::OpenOptions, true);
                if surface {
                    self.button(b, card, "ABANDON DROP", MenuAction::AskAbandon, true);
                }
                self.button(b, card, "QUIT TO TITLE", MenuAction::QuitToTitle, true);
                self.button(b, card, "QUIT GAME", MenuAction::AskQuit, true);
                if surface {
                    b.label(
                        card,
                        "Quitting the game mid-drop forfeits the cargo you carry.",
                        TextStyle::Small,
                    );
                }
                self.cancel = MenuAction::Resume;
            }
            Screen::Options { from_title } => {
                let card = Self::card(b, 560.0);
                b.label(card, "OPTIONS", TextStyle::Heading);
                let o = self.options.clone();
                let fullscreen = self.check(b, card, "Fullscreen", o.fullscreen);
                self.checks.push((fullscreen, Slot::Fullscreen));
                for (label, value, slot) in [
                    ("Master volume", o.master, Slot::Master),
                    ("Music volume", o.music, Slot::Music),
                    ("Effects volume", o.effects, Slot::Effects),
                ] {
                    let id =
                        b.ui.add_child(
                            card,
                            Widget::Slider {
                                label: UiText::literal(label),
                                value,
                                min: 0.0,
                                max: 1.0,
                                step: 0.05,
                                action: None,
                            },
                            LayoutItem::size(Length::Fill, Length::fixed(32.0)),
                        )
                        .expect("valid parent");
                    self.sliders.push((id, slot));
                }
                let shake = self.check(b, card, "Screen shake", o.shake);
                self.checks.push((shake, Slot::Shake));
                self.button(b, card, "APPLY", MenuAction::Apply(o), true);
                if from_title {
                    self.button(b, card, "RESET ALL PROGRESS", MenuAction::AskReset, true);
                }
                self.button(b, card, "BACK", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
            Screen::ConfirmAbandon => {
                let card = Self::card(b, 500.0);
                b.label(card, "ABANDON THIS DROP?", TextStyle::Heading);
                b.label(
                    card,
                    "You return to the ship. Everything you carry is lost.",
                    TextStyle::Small,
                );
                self.button(b, card, "ABANDON", MenuAction::Abandon, true);
                self.button(b, card, "KEEP FIGHTING", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
            Screen::ConfirmReset => {
                let card = Self::card(b, 520.0);
                b.label(card, "RESET ALL PROGRESS?", TextStyle::Heading);
                b.label(
                    card,
                    "Your ship, materials and stats are erased. The tutorial restarts.",
                    TextStyle::Small,
                );
                self.button(b, card, "ERASE EVERYTHING", MenuAction::ResetProgress, true);
                self.button(b, card, "CANCEL", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
            Screen::ConfirmQuit { .. } => {
                let card = Self::card(b, 460.0);
                b.label(card, "QUIT PLANETFALL?", TextStyle::Heading);
                self.button(b, card, "QUIT", MenuAction::Quit, true);
                self.button(b, card, "CANCEL", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
        }
    }

    fn check(&mut self, b: &mut Builder, parent: NodeId, label: &str, checked: bool) -> NodeId {
        b.ui.add_child(
            parent,
            Widget::Checkbox {
                label: UiText::literal(label),
                checked,
                action: None,
            },
            LayoutItem::size(Length::Fill, Length::fixed(32.0)),
        )
        .expect("valid parent")
    }
}

/// Everything a screen displays; a change triggers a rebuild.
fn signature(content: Content, game: &Game, prompts: &Prompts) -> String {
    let mut s = format!("{content:?}|{:?}", prompts.family);
    match content {
        Content::Terminal => {
            s.push_str(&format!(
                "|{:?}|{:?}|{:?}|{:?}",
                game.terminal, game.profile.materials, game.profile.upgrades, game.profile.orbiting
            ));
        }
        Content::LevelUp => {
            s.push_str(&format!(
                "|{:?}",
                game.run
                    .as_ref()
                    .and_then(|r| r.level_up_choices().map(|c| c.to_vec()))
            ));
        }
        Content::Screen(Screen::Title) => s.push_str(&format!("|{:?}", game.profile.stats)),
        Content::Screen(_) => {}
    }
    s
}
