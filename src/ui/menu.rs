//! Interactive screens: the main menu, save slots, settings, controls
//! (Quasar's `ControlsScreen`: rebind, conflicts, reset), pause and
//! confirmations, and the in-world terminals (navigation, fabricator,
//! level-up, debrief).
//! Keyboard, mouse and gamepad all navigate through `quasar-ui` focus.

use super::{Builder, Layer, format_cost, format_materials, hud::roman};
use crate::bindings::{self, Prompts};
use crate::storage::{SlotState, SlotSummary};
use planetfall_sim::content::{ClassId, PlanetId, Rarity, UpgradeId, class_xp_to_next, content};
use planetfall_sim::run::OfferSource;
use planetfall_sim::run::RunOutcome;
use planetfall_sim::{Command, Game, Terminal};
use quasar_input::{Bindings, Button, DeviceFamily, InputDevice};
use quasar_ui::{
    ActionId, Align, Border, Color, ControlEntry, ControlsOutcome, ControlsScreen, LayoutItem,
    Length, NodeId, NodeStyle, Rect, TextMeasure, TextStyle, UiEvent, UiInput, UiText, Widget,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The main menu: continue, new game, load, settings, quit.
    Main,
    /// The five save slots, for starting, loading or deleting a game.
    Saves(SavesMode),
    ConfirmOverwrite(usize),
    ConfirmDelete(usize),
    Settings,
    /// Rebind the keyboard/mouse or the controller column.
    Controls {
        gamepad: bool,
    },
    Pause {
        surface: bool,
    },
    ConfirmAbandon,
    ConfirmQuit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavesMode {
    New,
    Load,
    Delete,
}

/// Settings as edited on the Settings screen.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsState {
    pub fullscreen: bool,
    pub shadows: bool,
    pub bloom: bool,
    pub master: f32,
    pub music: f32,
    pub effects: f32,
    pub shake: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    None,
    /// Load the most recent slot.
    Continue,
    OpenSaves(SavesMode),
    /// A slot chosen on the saves screen (meaning depends on the mode).
    PickSlot(usize),
    Overwrite(usize),
    DeleteSlot(usize),
    OpenSettings,
    OpenControls {
        gamepad: bool,
    },
    /// The controls screen changed the bindings: install and save them.
    ApplyBindings(Bindings),
    Back,
    ApplySettings,
    Resume,
    AskAbandon,
    Abandon,
    SaveAndQuit,
    AskQuit,
    Quit,
    Game(Command),
}

/// App facts the menus show beyond the game itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuContext {
    pub slots: Vec<(usize, SlotState)>,
    /// The slot Continue loads.
    pub continue_slot: Option<usize>,
    /// The slot being played (None at the main menu).
    pub active_slot: Option<usize>,
    /// Shown on the saves screen after a failed load.
    pub error: Option<String>,
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
    sliders: Vec<(NodeId, Field)>,
    checks: Vec<(NodeId, Field)>,
    pub settings: SettingsState,
    /// The player's bindings (both devices), edited by the controls screen.
    pub bindings: Bindings,
    controls: Option<ControlsScreen>,
    /// Action id of the controls screen's reset button.
    controls_reset: u64,
}

/// A settings control.
#[derive(Clone, Copy, Debug)]
enum Field {
    Master,
    Music,
    Effects,
    Fullscreen,
    Shadows,
    Bloom,
    Shake,
}

impl Menu {
    pub fn new(settings: SettingsState) -> Self {
        Self {
            layer: None,
            signature: String::new(),
            actions: Vec::new(),
            cancel: MenuAction::None,
            sliders: Vec::new(),
            checks: Vec::new(),
            settings,
            bindings: bindings::defaults(),
            controls: None,
            controls_reset: u64::MAX,
        }
    }

    /// Whether a controls row waits for a button (raw presses go to
    /// [`Self::capture`] instead of menu navigation).
    pub fn is_capturing(&self) -> bool {
        self.controls
            .as_ref()
            .is_some_and(ControlsScreen::is_capturing)
    }

    /// Offer this frame's pressed buttons to a running rebind.
    pub fn capture(&mut self, pressed: &[Button]) -> MenuAction {
        let (Some(controls), Some(layer)) = (&mut self.controls, &mut self.layer) else {
            return MenuAction::None;
        };
        match controls.capture(&mut layer.ui, pressed) {
            Ok(ControlsOutcome::Rebound { .. }) => {
                self.bindings = controls.bindings().clone();
                MenuAction::ApplyBindings(self.bindings.clone())
            }
            _ => MenuAction::None,
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
        context: &MenuContext,
    ) {
        let Some(content) = content else {
            self.layer = None;
            self.signature.clear();
            return;
        };
        // A rebind in progress owns the screen until it finishes.
        if self.is_capturing() {
            return;
        }
        let signature = signature(content, game, prompts, context);
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
        self.controls = None;
        self.controls_reset = u64::MAX;
        self.cancel = MenuAction::None;
        self.build(&mut b, content, game, prompts, context);
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
                    Field::Master => self.settings.master = value,
                    Field::Music => self.settings.music = value,
                    Field::Effects => self.settings.effects = value,
                    _ => {}
                }
            }
        }
        for (id, slot) in &self.checks {
            if let Some(node) = layer.ui.node(*id)
                && let Widget::Checkbox { checked, .. } = node.widget
            {
                match slot {
                    Field::Fullscreen => self.settings.fullscreen = checked,
                    Field::Shadows => self.settings.shadows = checked,
                    Field::Bloom => self.settings.bloom = checked,
                    Field::Shake => self.settings.shake = checked,
                    _ => {}
                }
            }
        }
        for event in frame.events {
            if let Some(controls) = &mut self.controls {
                match controls.handle_event(&mut layer.ui, &event) {
                    Ok(ControlsOutcome::Reset) => {
                        self.bindings = controls.bindings().clone();
                        return MenuAction::ApplyBindings(self.bindings.clone());
                    }
                    Ok(ControlsOutcome::Capturing { .. }) => continue,
                    _ => {}
                }
            }
            match event {
                UiEvent::Pressed(action) if action.0 == self.controls_reset => {}
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
    ) -> NodeId {
        let id = self.action(action);
        b.button(parent, text, id, enabled)
    }

    fn build(
        &mut self,
        b: &mut Builder,
        content: Content,
        game: &Game,
        prompts: &Prompts,
        context: &MenuContext,
    ) {
        match content {
            Content::Screen(screen) => self.build_screen(b, screen, prompts, context),
            Content::LevelUp => {
                let card = Self::card(b, 680.0);
                let run = game.run.as_ref();
                let level = run.map_or(1, |r| r.player().level);
                let source = run.and_then(|r| r.offer_source());
                let (title, hint) = match source {
                    Some(OfferSource::WeaponPick) => (
                        format!("LEVEL {level} — NEW WEAPON"),
                        "Choose a weapon for a free slot. Familiar weapons turn up more often.",
                    ),
                    Some(OfferSource::Cache) => (
                        "SUPPLY CACHE".to_owned(),
                        "Choose one rare or better upgrade.",
                    ),
                    _ => (
                        format!("LEVEL {level} — UPGRADE"),
                        "Choose one. Upgrades last until this drop ends.",
                    ),
                };
                b.label(card, title, TextStyle::Heading);
                b.label(card, hint, TextStyle::Small);
                let choices = run
                    .and_then(|r| r.level_up_choices())
                    .unwrap_or(&[])
                    .to_vec();
                for (i, offer) in choices.iter().enumerate() {
                    let pick = source == Some(OfferSource::WeaponPick);
                    let tag = if pick {
                        String::new()
                    } else {
                        format!("[{}]  ", offer.rarity.name().to_uppercase())
                    };
                    // Each offer sits in a panel bordered in its rarity colour.
                    let slot =
                        b.panel(card, LayoutItem::size(Length::Fill, Length::Auto), 3.0, 5.0);
                    let color = if pick {
                        Color::rgba(140, 150, 160, 160)
                    } else {
                        rarity_color(offer.rarity)
                    };
                    let _ = b.ui.set_style(
                        slot,
                        Some(NodeStyle {
                            background: None,
                            border: Some(Border { width: 2.0, color }),
                            radius: 3.0,
                        }),
                    );
                    self.button(
                        b,
                        slot,
                        format!("{tag}{}", offer.title()),
                        MenuAction::Game(Command::ChoosePerk(i)),
                        true,
                    );
                    b.paragraph(slot, &offer.description(), TextStyle::Small);
                }
            }
            Content::Terminal => match &game.terminal {
                Some(Terminal::Navigation) => self.navigation(b, game),
                Some(Terminal::Fabricator) => self.fabricator(b, game),
                Some(Terminal::Locker) => self.locker(b, game),
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
                    let (before, after) = summary.class_levels;
                    let class_line = if after > before {
                        format!(
                            "{} +{} class XP  ·  LEVEL UP: {} → {}",
                            summary.class.name(),
                            summary.class_xp,
                            before,
                            after
                        )
                    } else {
                        format!(
                            "{} +{} class XP  ·  level {}",
                            summary.class.name(),
                            summary.class_xp,
                            after
                        )
                    };
                    b.label(card, class_line, TextStyle::Body);
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
                    b.label(card, upgrade.info().description.as_str(), TextStyle::Small);
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
        for planet in PlanetId::all() {
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

    /// Locker room: pick a class, see its kit and progress, choose the
    /// starting weapon. Every class keeps its own progress.
    fn locker(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let card = Self::card(b, 720.0);
        b.label(card, "LOCKER ROOM", TextStyle::Heading);
        b.label(
            card,
            "Suit up as any class. Each keeps its own level and loadout.",
            TextStyle::Small,
        );
        for class in ClassId::all() {
            let (level, into) = profile.class_level(class);
            let info = class.info();
            let active = profile.class == class;
            let progress = if level >= content().tuning.class.max_level {
                "MAX".to_owned()
            } else {
                format!("{into}/{} XP", class_xp_to_next(level))
            };
            let label = format!(
                "{}{}  —  {}  ·  level {level}  ·  {progress}",
                if active { "▶ " } else { "" },
                info.name.to_uppercase(),
                info.role,
            );
            self.button(
                b,
                card,
                label,
                MenuAction::Game(Command::SelectClass(class)),
                !active,
            );
        }
        let class = profile.class;
        let info = class.info();
        let (level, _) = profile.class_level(class);
        let ability_tier = class.ability_tier(level);
        let passive_tier = class.passive_tier(level);
        b.label(card, info.name.to_uppercase(), TextStyle::Body);
        b.paragraph(card, &info.description, TextStyle::Small);
        b.paragraph(
            card,
            &format!(
                "Ability: {} {} — {}",
                info.ability.name,
                roman(ability_tier),
                info.ability.description
            ),
            TextStyle::Small,
        );
        b.paragraph(
            card,
            &format!(
                "Passive: {} {} — {}",
                info.passive.name,
                roman(passive_tier),
                info.passive.description
            ),
            TextStyle::Small,
        );
        let familiar: Vec<&str> = class.familiar().map(|w| w.name()).collect();
        b.paragraph(
            card,
            &format!("Familiar weapons: {}", familiar.join(", ")),
            TextStyle::Small,
        );
        b.label(card, "STARTING WEAPON", TextStyle::Small);
        let options = profile.start_options(class);
        let current = profile.start_weapon(class);
        for weapon in class
            .familiar()
            .chain(std::iter::once(class.signature()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let unlocked = options.contains(&weapon);
            let tag = if weapon == class.signature() {
                " (signature)"
            } else {
                ""
            };
            let label = if weapon == current {
                format!("▶ {}{tag}  —  EQUIPPED", weapon.name())
            } else if unlocked {
                format!("{}{tag}", weapon.name())
            } else {
                let at = info
                    .unlocks
                    .iter()
                    .find(|u| u.weapon == weapon.key())
                    .map_or("find it on a dive".to_owned(), |u| {
                        format!("class level {}", u.level)
                    });
                format!("{}  —  LOCKED ({at})", weapon.name())
            };
            self.button(
                b,
                card,
                label,
                MenuAction::Game(Command::SetStartWeapon(weapon)),
                unlocked && weapon != current,
            );
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
        for upgrade in UpgradeId::all() {
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
            b.label(card, upgrade.info().description.as_str(), TextStyle::Small);
        }
        self.cancel = MenuAction::Game(Command::Close);
        self.button(b, card, "CLOSE", MenuAction::Game(Command::Close), true);
    }

    fn build_screen(
        &mut self,
        b: &mut Builder,
        screen: Screen,
        prompts: &Prompts,
        context: &MenuContext,
    ) {
        match screen {
            Screen::Main => {
                let card = Self::card(b, 520.0);
                b.label(card, "PLANETFALL", TextStyle::Heading);
                b.label(
                    card,
                    "Build your ship. Drop. Mine. Survive. Extract.",
                    TextStyle::Small,
                );
                let resume = context.continue_slot.and_then(|slot| {
                    context.slots.iter().find_map(|(s, state)| match state {
                        SlotState::Saved(summary) if *s == slot => Some((slot, summary)),
                        _ => None,
                    })
                });
                match resume {
                    Some((slot, summary)) => {
                        self.button(b, card, "CONTINUE", MenuAction::Continue, true);
                        b.label(
                            card,
                            format!(
                                "Slot {slot} · {} · {}",
                                summary.location,
                                play_time(summary.meta.play_seconds)
                            ),
                            TextStyle::Small,
                        );
                    }
                    None => {
                        self.button(b, card, "CONTINUE", MenuAction::Continue, false);
                    }
                }
                self.button(
                    b,
                    card,
                    "NEW GAME",
                    MenuAction::OpenSaves(SavesMode::New),
                    true,
                );
                let any = context
                    .slots
                    .iter()
                    .any(|(_, s)| matches!(s, SlotState::Saved(_)));
                self.button(
                    b,
                    card,
                    "LOAD GAME",
                    MenuAction::OpenSaves(SavesMode::Load),
                    any,
                );
                self.button(b, card, "SETTINGS", MenuAction::OpenSettings, true);
                self.button(b, card, "QUIT", MenuAction::AskQuit, true);
                b.paragraph(
                    card,
                    &prompts.format(
                        "{move} move · {dash} dash · {interact} use · {extract} call pod · weapons fire automatically",
                    ),
                    TextStyle::Small,
                );
                self.cancel = MenuAction::AskQuit;
            }
            Screen::Saves(mode) => {
                let card = Self::card(b, 640.0);
                let (title, hint) = match mode {
                    SavesMode::New => ("NEW GAME", "Choose a slot for the new commander."),
                    SavesMode::Load => ("LOAD GAME", "Continue a saved commander."),
                    SavesMode::Delete => ("DELETE A SAVE", "Choose a save to delete."),
                };
                b.label(card, title, TextStyle::Heading);
                b.label(card, hint, TextStyle::Small);
                if let Some(error) = &context.error {
                    b.paragraph(card, &format!("Could not load: {error}"), TextStyle::Small);
                }
                for (slot, state) in &context.slots {
                    let (label, enabled) = match state {
                        SlotState::Empty => {
                            (format!("SLOT {slot} — Empty"), mode == SavesMode::New)
                        }
                        SlotState::Saved(s) => (format!("SLOT {slot} — {}", slot_line(s)), true),
                        SlotState::Unusable(reason) => (
                            format!("SLOT {slot} — Unusable ({reason})"),
                            mode != SavesMode::Load,
                        ),
                    };
                    self.button(b, card, label, MenuAction::PickSlot(*slot), enabled);
                }
                if mode == SavesMode::Load {
                    self.button(
                        b,
                        card,
                        "DELETE A SAVE…",
                        MenuAction::OpenSaves(SavesMode::Delete),
                        true,
                    );
                }
                self.button(b, card, "BACK", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
            Screen::ConfirmOverwrite(slot) => {
                let card = Self::card(b, 520.0);
                b.label(card, format!("OVERWRITE SLOT {slot}?"), TextStyle::Heading);
                b.label(
                    card,
                    "The commander in this slot is replaced by a new game.",
                    TextStyle::Small,
                );
                self.button(b, card, "START NEW GAME", MenuAction::Overwrite(slot), true);
                self.button(b, card, "CANCEL", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
            Screen::ConfirmDelete(slot) => {
                let card = Self::card(b, 520.0);
                b.label(card, format!("DELETE SLOT {slot}?"), TextStyle::Heading);
                b.label(
                    card,
                    "This commander's ship and progress are erased.",
                    TextStyle::Small,
                );
                self.button(b, card, "DELETE", MenuAction::DeleteSlot(slot), true);
                self.button(b, card, "CANCEL", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
            Screen::Settings => {
                let card = Self::card(b, 560.0);
                b.label(card, "SETTINGS", TextStyle::Heading);
                let o = self.settings.clone();
                b.label(card, "AUDIO", TextStyle::Small);
                for (label, value, field) in [
                    ("Master volume", o.master, Field::Master),
                    ("Music volume", o.music, Field::Music),
                    ("Effects volume", o.effects, Field::Effects),
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
                    self.sliders.push((id, field));
                }
                b.label(card, "DISPLAY", TextStyle::Small);
                for (label, value, field) in [
                    ("Fullscreen", o.fullscreen, Field::Fullscreen),
                    ("Shadows", o.shadows, Field::Shadows),
                    ("Bloom", o.bloom, Field::Bloom),
                ] {
                    let id = self.check(b, card, label, value);
                    self.checks.push((id, field));
                }
                b.label(card, "GAMEPLAY", TextStyle::Small);
                let shake = self.check(b, card, "Screen shake", o.shake);
                self.checks.push((shake, Field::Shake));
                self.button(
                    b,
                    card,
                    "CONTROLS…",
                    MenuAction::OpenControls { gamepad: false },
                    true,
                );
                self.button(b, card, "APPLY", MenuAction::ApplySettings, true);
                self.button(b, card, "BACK", MenuAction::Back, true);
                self.cancel = MenuAction::Back;
            }
            Screen::Controls { gamepad } => {
                let card = Self::card(b, 600.0);
                b.label(card, "CONTROLS", TextStyle::Heading);
                let (device, other) = if gamepad {
                    (InputDevice::Gamepad, "SHOW KEYBOARD & MOUSE")
                } else {
                    (InputDevice::KeyboardMouse, "SHOW CONTROLLER")
                };
                let entries = bindings::REBINDABLE
                    .iter()
                    .filter(|(_, _, pad)| !gamepad || *pad)
                    .map(|(action, label, _)| ControlEntry::new(*action, UiText::literal(*label)))
                    .collect();
                let mut controls =
                    ControlsScreen::new(entries, &self.bindings, bindings::defaults());
                let family = if gamepad && prompts.gamepad() {
                    prompts.family
                } else if gamepad {
                    DeviceFamily::GenericGamepad
                } else {
                    DeviceFamily::KeyboardMouse
                };
                self.controls_reset = self.action(MenuAction::None);
                let _ = controls.set_device(&mut b.ui, device, family);
                controls
                    .build(&mut b.ui, card, ActionId::new(self.controls_reset))
                    .expect("controls screen");
                b.label(
                    card,
                    "Select an action, then press the new button. Esc cancels.",
                    TextStyle::Small,
                );
                self.button(
                    b,
                    card,
                    other,
                    MenuAction::OpenControls { gamepad: !gamepad },
                    true,
                );
                self.button(b, card, "BACK", MenuAction::Back, true);
                self.controls = Some(controls);
                self.cancel = MenuAction::Back;
            }
            Screen::Pause { surface } => {
                let card = Self::card(b, 460.0);
                b.label(card, "PAUSED", TextStyle::Heading);
                self.button(b, card, "RESUME", MenuAction::Resume, true);
                self.button(b, card, "SETTINGS", MenuAction::OpenSettings, true);
                if surface {
                    self.button(b, card, "ABANDON DROP", MenuAction::AskAbandon, true);
                }
                let saving = context.active_slot.is_some();
                let label = if saving {
                    "SAVE & QUIT TO MENU"
                } else {
                    "QUIT TO MENU"
                };
                self.button(b, card, label, MenuAction::SaveAndQuit, true);
                self.button(b, card, "QUIT GAME", MenuAction::AskQuit, true);
                let note = match (context.active_slot, surface) {
                    (Some(slot), true) => {
                        format!("Slot {slot} saves this drop; it resumes where you left off.")
                    }
                    (Some(slot), false) => format!("Progress saves to slot {slot}."),
                    (None, _) => "This game is not being saved.".to_owned(),
                };
                b.label(card, note, TextStyle::Small);
                self.cancel = MenuAction::Resume;
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
            Screen::ConfirmQuit => {
                let card = Self::card(b, 460.0);
                b.label(card, "QUIT PLANETFALL?", TextStyle::Heading);
                if let Some(slot) = context.active_slot {
                    b.label(
                        card,
                        format!("Your game is saved to slot {slot}."),
                        TextStyle::Small,
                    );
                }
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
fn signature(content: Content, game: &Game, prompts: &Prompts, context: &MenuContext) -> String {
    let mut s = format!("{content:?}|{:?}", prompts.family);
    match content {
        Content::Terminal => {
            s.push_str(&format!(
                "|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                game.terminal,
                game.profile.materials,
                game.profile.upgrades,
                game.profile.orbiting,
                game.profile.class,
                game.profile.classes
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
        Content::Screen(_) => s.push_str(&format!("|{context:?}")),
    }
    s
}

/// Border colour of an upgrade by rarity.
fn rarity_color(rarity: Rarity) -> Color {
    match rarity {
        Rarity::Common => Color::rgba(140, 150, 160, 200),
        Rarity::Uncommon => Color::rgba(90, 215, 100, 230),
        Rarity::Rare => Color::rgba(80, 150, 255, 240),
        Rarity::Legendary => Color::rgba(255, 165, 40, 255),
    }
}

/// "1h 05m" / "12m".
fn play_time(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}h {:02}m", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

/// One line describing a saved commander.
fn slot_line(s: &SlotSummary) -> String {
    let progress = if s.tutorial {
        "tutorial".to_owned()
    } else {
        format!("{} drops · {} modules", s.drops, s.modules)
    };
    let drop = if s.mid_drop { " (mid-drop)" } else { "" };
    format!(
        "{}{drop} · {progress} · {}",
        s.location,
        play_time(s.meta.play_seconds)
    )
}
