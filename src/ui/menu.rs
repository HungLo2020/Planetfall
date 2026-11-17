//! Interactive screens: the main menu, save slots, settings, controls
//! (Quasar's `ControlsScreen`: rebind, conflicts, reset), pause and
//! confirmations, and the in-world terminals (navigation, fabricator,
//! level-up, debrief).
//! Keyboard, mouse and gamepad all navigate through `quasar-ui` focus.

use super::rarity_color;
use super::{Builder, Layer, format_cost, format_materials, hud::roman};
use crate::bindings::{self, Prompts};
use crate::storage::{SlotState, SlotSummary};
use planetfall_sim::content::{
    ClassId, GearSlot, Material, PlanetId, Rarity, UpgradeId, class_xp_to_next, content,
};
use planetfall_sim::mission::{MAX_HAZARD, hazard_name};
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
    /// Mission board: choose the hazard level (handled by the menu).
    SetHazard(u8),
    /// Switch a terminal's tab, open gear slot or page (handled by the menu).
    SetView(View),
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

/// Terminal view state kept by the menu: the tab, the gear slot opened in
/// the locker and the page of a long list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View {
    pub tab: u8,
    pub slot: Option<GearSlot>,
    pub page: usize,
}

/// Which terminal a [`View`] belongs to.
fn view_key(terminal: &Option<Terminal>) -> &'static str {
    match terminal {
        Some(Terminal::Locker) => "locker",
        Some(Terminal::Trade) => "trade",
        _ => "",
    }
}

/// Rows per page in stash and shop lists.
const PAGE: usize = 7;

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
    /// Hazard level picked on the mission board (0: not opened yet).
    hazard: u8,
    view: View,
    /// The terminal the view belongs to (reset when another opens).
    view_terminal: String,
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
            hazard: 0,
            view: View::default(),
            view_terminal: String::new(),
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
    /// Open a terminal on a given tab (screenshots).
    pub fn set_view(&mut self, view: View, terminal: &Option<Terminal>) {
        self.view = view;
        self.view_terminal = view_key(terminal).to_owned();
        self.signature.clear();
    }

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
                        if let MenuAction::SetHazard(h) = a {
                            self.hazard = *h;
                            self.signature.clear();
                            return MenuAction::None;
                        }
                        if let MenuAction::SetView(v) = a {
                            self.view = *v;
                            self.signature.clear();
                            return MenuAction::None;
                        }
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
            Content::Terminal => {
                // A different terminal starts on its first tab.
                let key = view_key(&game.terminal);
                if self.view_terminal != key {
                    self.view_terminal = key.to_owned();
                    self.view = View::default();
                }
                self.terminal(b, game);
            }
        }
    }

    fn terminal(&mut self, b: &mut Builder, game: &Game) {
        {
            match &game.terminal {
                Some(Terminal::Navigation) => self.navigation(b, game),
                Some(Terminal::Fabricator) => self.fabricator(b, game),
                Some(Terminal::Locker) => self.locker(b, game),
                Some(Terminal::MissionBoard) => self.mission_board(b, game),
                Some(Terminal::Trade) => self.trade(b, game),
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
                            "Time {:02}:{:02}   ·   Threat {}   ·   Kills {}   ·   Level {}",
                            summary.seconds / 60,
                            summary.seconds % 60,
                            summary.threat,
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
                    // Contract results.
                    let tick = |done: bool| if done { "✓" } else { "✗" };
                    b.label(
                        card,
                        format!(
                            "{} {}  ·  hazard {} {}",
                            tick(summary.primary_done),
                            summary.contract.primary.name(),
                            summary.hazard,
                            hazard_name(summary.hazard)
                        ),
                        TextStyle::Body,
                    );
                    if let (Some(secondary), Some(done)) =
                        (summary.contract.secondary, summary.secondary_done)
                    {
                        b.label(
                            card,
                            format!("{} Optional: {}", tick(done), secondary.name()),
                            TextStyle::Small,
                        );
                    }
                    if summary.bonus.total() > 0 {
                        b.label(
                            card,
                            format!("Contract bonus: {}", format_materials(&summary.bonus)),
                            TextStyle::Small,
                        );
                    }
                    if let Some(level) = summary.unlocked {
                        b.label(
                            card,
                            format!("HAZARD {level} UNLOCKED ({})", hazard_name(level)),
                            TextStyle::Body,
                        );
                    }
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
                    if !summary.finds.is_empty() {
                        b.paragraph(
                            card,
                            &format!("Finds (kept): {}", summary.finds.join(", ")),
                            TextStyle::Small,
                        );
                    }
                    if summary.salvaged > 0 {
                        b.label(
                            card,
                            format!(
                                "Stash full: finds salvaged for {} credits",
                                summary.salvaged
                            ),
                            TextStyle::Small,
                        );
                    }
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
            }
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
                b.label(card, info.description.clone(), TextStyle::Small);
            }
        }
        let docked = game.profile.docked;
        self.button(
            b,
            card,
            if docked {
                "KESTREL EXCHANGE  —  DOCKED".to_owned()
            } else {
                "JUMP TO KESTREL EXCHANGE  —  trade station".to_owned()
            },
            MenuAction::Game(Command::Dock),
            !docked,
        );
        b.label(
            card,
            "Sell finds for credits, buy gear, rework items and trade materials.",
            TextStyle::Small,
        );
        self.cancel = MenuAction::Game(Command::Close);
        self.button(b, card, "CLOSE", MenuAction::Game(Command::Close), true);
    }

    /// Mission board: contracts for the planet in orbit and the hazard
    /// level to take them at.
    fn mission_board(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let unlocked = profile.hazard_unlocked;
        if self.hazard == 0 || self.hazard > unlocked {
            self.hazard = profile.hazard_choice.clamp(1, unlocked);
        }
        let hazard = self.hazard;
        let card = Self::card(b, 760.0);
        let planet = profile.orbiting.map_or("—", |p| p.name());
        b.label(
            card,
            format!("MISSION BOARD  ·  {}", planet.to_uppercase()),
            TextStyle::Heading,
        );
        let h = &content().tuning.hazard;
        let i = usize::from(hazard - 1);
        b.label(
            card,
            format!(
                "Hazard {hazard} {}: enemies ×{:.2} health, ×{:.2} damage  ·  haul ×{:.2}, class XP ×{:.2}",
                hazard_name(hazard),
                h.enemy_health[i],
                h.enemy_damage[i],
                h.reward[i],
                h.class_xp[i]
            ),
            TextStyle::Small,
        );
        let row =
            b.ui.add_child(
                card,
                Widget::Panel {
                    layout: quasar_ui::PanelLayout::Stack {
                        direction: quasar_ui::StackDirection::Horizontal,
                        gap: 4.0,
                    },
                    padding: quasar_ui::Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                LayoutItem::size(Length::Fill, Length::Auto),
            )
            .expect("valid parent");
        for level in 1..=MAX_HAZARD {
            let label = if level == hazard {
                format!("▶ {level}")
            } else if level > unlocked {
                format!("{level} (locked)")
            } else {
                format!("{level}")
            };
            let id = self.action(MenuAction::SetHazard(level));
            b.ui.add_child(
                row,
                Widget::Button {
                    text: UiText::literal(label),
                    action: Some(ActionId::new(id)),
                },
                LayoutItem::size(Length::Fill, Length::fixed(34.0)),
            )
            .expect("valid parent");
            if level > unlocked {
                // Locked levels are shown but cannot be picked.
                self.actions[id as usize] = MenuAction::None;
            }
        }
        if unlocked < MAX_HAZARD {
            b.label(
                card,
                format!(
                    "Complete a contract at hazard {unlocked} to unlock hazard {}.",
                    unlocked + 1
                ),
                TextStyle::Small,
            );
        }
        for (n, contract) in profile.board.iter().enumerate() {
            let slot = b.panel(card, LayoutItem::size(Length::Fill, Length::Auto), 2.0, 6.0);
            let _ = b.ui.set_style(
                slot,
                Some(NodeStyle {
                    background: None,
                    border: Some(Border {
                        width: 1.5,
                        color: Color::rgba(255, 142, 48, 140),
                    }),
                    radius: 3.0,
                }),
            );
            self.button(
                b,
                slot,
                format!("TAKE: {}", contract.primary.name().to_uppercase()),
                MenuAction::Game(Command::Deploy {
                    contract: n,
                    hazard,
                }),
                true,
            );
            b.paragraph(slot, &contract.primary.describe(hazard), TextStyle::Small);
            if let Some(boss) = contract.boss {
                b.label(slot, format!("Target: {}", boss.name()), TextStyle::Small);
            }
            if let Some(secondary) = contract.secondary {
                b.paragraph(
                    slot,
                    &format!("Optional: {}", secondary.describe(hazard)),
                    TextStyle::Small,
                );
            }
            let site = contract.site.info();
            b.paragraph(
                slot,
                &format!("Site: {} — {}", site.name, site.description),
                TextStyle::Small,
            );
            if let Some(mutator) = contract.mutator {
                let m = mutator.info();
                b.paragraph(
                    slot,
                    &format!("Mutator: {} — {}", m.name, m.description),
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
            "Suit up as any class. Each keeps its own level, loadout and gear.",
            TextStyle::Small,
        );
        let worn = profile.equipped(profile.class).len();
        self.tabs(
            b,
            card,
            &[
                "CREW".to_owned(),
                format!("GEAR ({worn}/4)  ·  STASH {}", profile.stash.len()),
            ],
        );
        if self.view.tab == 1 {
            self.gear(b, card, game);
            self.cancel = MenuAction::Game(Command::Close);
            self.button(b, card, "CLOSE", MenuAction::Game(Command::Close), true);
            return;
        }
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

    /// A row of tab buttons; the current tab is marked and disabled.
    fn tabs(&mut self, b: &mut Builder, card: NodeId, labels: &[String]) {
        let row =
            b.ui.add_child(
                card,
                Widget::Panel {
                    layout: quasar_ui::PanelLayout::Stack {
                        direction: quasar_ui::StackDirection::Horizontal,
                        gap: 4.0,
                    },
                    padding: quasar_ui::Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                LayoutItem::size(Length::Fill, Length::Auto),
            )
            .expect("valid parent");
        for (i, label) in labels.iter().enumerate() {
            let current = usize::from(self.view.tab) == i;
            let text = if current {
                format!("▶ {label}")
            } else {
                label.clone()
            };
            let id = self.action(MenuAction::SetView(View {
                tab: i as u8,
                ..View::default()
            }));
            b.ui.add_child(
                row,
                Widget::Button {
                    text: UiText::literal(text),
                    action: Some(ActionId::new(id)),
                },
                LayoutItem::size(Length::Fill, Length::fixed(34.0)),
            )
            .expect("valid parent");
        }
    }

    /// Previous/next page buttons for a list of `len` rows.
    fn pager(&mut self, b: &mut Builder, card: NodeId, len: usize) {
        let pages = len.div_ceil(PAGE).max(1);
        if pages <= 1 {
            return;
        }
        let page = self.view.page.min(pages - 1);
        let prev = View {
            page: page.saturating_sub(1),
            ..self.view
        };
        let next = View {
            page: (page + 1).min(pages - 1),
            ..self.view
        };
        self.button(
            b,
            card,
            format!("◀ PREVIOUS  ·  page {} of {pages}", page + 1),
            MenuAction::SetView(prev),
            page > 0,
        );
        self.button(
            b,
            card,
            "NEXT ▶",
            MenuAction::SetView(next),
            page + 1 < pages,
        );
    }

    /// The rows of the current page.
    fn page<'a, T>(&self, items: &'a [T]) -> &'a [T] {
        let pages = items.len().div_ceil(PAGE).max(1);
        let start = self.view.page.min(pages - 1) * PAGE;
        &items[start.min(items.len())..(start + PAGE).min(items.len())]
    }

    /// Locker GEAR tab: the active class's four slots; opening a slot lists
    /// the stash items that fit it.
    fn gear(&mut self, b: &mut Builder, card: NodeId, game: &Game) {
        let profile = &game.profile;
        let class = profile.class;
        let worn = profile.equipped(class);
        let stats: Vec<String> = worn
            .iter()
            .flat_map(|g| g.stats())
            .map(|(e, a)| e.describe(a))
            .collect();
        b.paragraph(
            card,
            &format!(
                "{} wears: {}",
                class.name(),
                if stats.is_empty() {
                    "nothing yet. Gear drops from caches, elites and bosses.".to_owned()
                } else {
                    stats.join(", ")
                }
            ),
            TextStyle::Small,
        );
        for slot in GearSlot::ALL {
            let current = worn.iter().find(|g| g.slot() == slot);
            let open = self.view.slot == Some(slot);
            let label = match current {
                Some(g) => format!(
                    "{}{}: [{}] {}",
                    if open { "▼ " } else { "" },
                    slot.name().to_uppercase(),
                    g.rarity.name().to_uppercase(),
                    g.name()
                ),
                None => format!(
                    "{}{}: empty",
                    if open { "▼ " } else { "" },
                    slot.name().to_uppercase()
                ),
            };
            let toggle = View {
                slot: (!open).then_some(slot),
                ..self.view
            };
            let row = b.panel(card, LayoutItem::size(Length::Fill, Length::Auto), 2.0, 4.0);
            if let Some(g) = current {
                let _ = b.ui.set_style(
                    row,
                    Some(NodeStyle {
                        background: None,
                        border: Some(Border {
                            width: 1.5,
                            color: rarity_color(g.rarity),
                        }),
                        radius: 3.0,
                    }),
                );
            }
            self.button(b, row, label, MenuAction::SetView(toggle), true);
            if let Some(g) = current {
                b.paragraph(row, &g.summary(), TextStyle::Small);
            }
            if !open {
                continue;
            }
            if current.is_some() {
                self.button(
                    b,
                    row,
                    "TAKE OFF",
                    MenuAction::Game(Command::Unequip(slot)),
                    true,
                );
            }
            let base = current.map_or(0.0, |g| g.score());
            let mut options: Vec<_> = profile
                .stash
                .iter()
                .filter(|g| g.slot() == slot && Some(g.id) != current.map(|c| c.id))
                .collect();
            options.sort_by(|a, b| b.score().total_cmp(&a.score()));
            if options.is_empty() {
                b.label(row, "Nothing in the stash for this slot.", TextStyle::Small);
            }
            for g in options.into_iter().take(PAGE) {
                let compare = if g.score() > base + 0.05 {
                    "▲"
                } else if g.score() < base - 0.05 {
                    "▼"
                } else {
                    "="
                };
                let worn_by = profile
                    .wearer(g.id)
                    .map_or(String::new(), |c| format!("  (worn by {})", c.name()));
                self.button(
                    b,
                    row,
                    format!(
                        "{compare} EQUIP [{}] {}{worn_by}",
                        g.rarity.name().to_uppercase(),
                        g.name()
                    ),
                    MenuAction::Game(Command::Equip(g.id)),
                    true,
                );
                b.paragraph(row, &g.summary(), TextStyle::Small);
            }
        }
    }

    /// The trade station: sell, buy, rework and trade materials.
    fn trade(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let t = &content().tuning.trade;
        let card = Self::card(b, 760.0);
        b.label(card, "KESTREL EXCHANGE", TextStyle::Heading);
        b.label(
            card,
            format!(
                "Credits {}   ·   Stash {}/{}   ·   Hold: {}",
                profile.credits,
                profile.stash.len(),
                content().tuning.equipment.stash_size,
                format_materials(&profile.materials)
            ),
            TextStyle::Body,
        );
        self.tabs(
            b,
            card,
            &[
                "SELL".to_owned(),
                "BUY".to_owned(),
                "WORKSHOP".to_owned(),
                "MATERIALS".to_owned(),
            ],
        );
        let item_row = |menu: &mut Self,
                        b: &mut Builder,
                        g: &planetfall_sim::gear::Gear,
                        text: String,
                        action: Command,
                        enabled: bool| {
            let row = b.panel(card, LayoutItem::size(Length::Fill, Length::Auto), 2.0, 4.0);
            let _ = b.ui.set_style(
                row,
                Some(NodeStyle {
                    background: None,
                    border: Some(Border {
                        width: 1.5,
                        color: rarity_color(g.rarity),
                    }),
                    radius: 3.0,
                }),
            );
            menu.button(b, row, text, MenuAction::Game(action), enabled);
            b.paragraph(row, &g.summary(), TextStyle::Small);
        };
        match self.view.tab {
            0 => {
                b.label(card, "Worn items cannot be sold.", TextStyle::Small);
                let mut items: Vec<_> = profile.stash.iter().collect();
                items.sort_by_key(|g| profile.wearer(g.id).is_some());
                if items.is_empty() {
                    b.label(card, "The stash is empty.", TextStyle::Small);
                }
                for g in self.page(&items).to_vec() {
                    let worn = profile.wearer(g.id);
                    let text = match worn {
                        Some(c) => format!(
                            "[{}] {}  —  worn by {}",
                            g.rarity.name().to_uppercase(),
                            g.name(),
                            c.name()
                        ),
                        None => format!(
                            "SELL [{}] {}  —  +{} cr",
                            g.rarity.name().to_uppercase(),
                            g.name(),
                            g.sell_value()
                        ),
                    };
                    item_row(self, b, g, text, Command::Sell(g.id), worn.is_none());
                }
                self.pager(b, card, items.len());
            }
            1 => {
                if profile.market.is_empty() {
                    b.label(card, "Sold out until the next visit.", TextStyle::Small);
                }
                for (i, g) in profile.market.iter().enumerate() {
                    let text = format!(
                        "BUY [{}] {} ({})  —  {} cr",
                        g.rarity.name().to_uppercase(),
                        g.name(),
                        g.slot().name(),
                        g.price()
                    );
                    item_row(
                        self,
                        b,
                        g,
                        text,
                        Command::Buy(i),
                        g.price() <= profile.credits,
                    );
                }
            }
            2 => {
                for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
                    let have = profile.fusible(rarity).len();
                    let next = Rarity::ALL[rarity.index() + 1];
                    let cost = t.fuse_cost[rarity.index()];
                    self.button(
                        b,
                        card,
                        format!(
                            "FUSE {} {} → 1 {}  (unworn: {have})  —  {cost} cr",
                            t.fuse,
                            rarity.name().to_uppercase(),
                            next.name().to_uppercase()
                        ),
                        MenuAction::Game(Command::Fuse(rarity)),
                        have >= t.fuse && profile.credits >= cost,
                    );
                }
                b.label(
                    card,
                    "Fusing uses the weakest unworn items. Rerolling keeps the base and rarity.",
                    TextStyle::Small,
                );
                let items: Vec<_> = profile.stash.iter().collect();
                for g in self.page(&items).to_vec() {
                    let cost = t.reroll[g.rarity.index()];
                    let text = format!(
                        "REROLL [{}] {}  —  {cost} cr",
                        g.rarity.name().to_uppercase(),
                        g.name()
                    );
                    item_row(
                        self,
                        b,
                        g,
                        text,
                        Command::Reroll(g.id),
                        profile.credits >= cost,
                    );
                }
                self.pager(b, card, items.len());
            }
            _ => {
                for (i, material) in Material::ALL.into_iter().enumerate() {
                    self.button(
                        b,
                        card,
                        format!(
                            "SELL {} {}  —  +{} cr",
                            t.lot,
                            material.name(),
                            t.material_sell[i]
                        ),
                        MenuAction::Game(Command::TradeMaterial {
                            material,
                            buy: false,
                        }),
                        profile.materials.get(material) >= t.lot,
                    );
                    self.button(
                        b,
                        card,
                        format!(
                            "BUY {} {}  —  {} cr",
                            t.lot,
                            material.name(),
                            t.material_buy[i]
                        ),
                        MenuAction::Game(Command::TradeMaterial {
                            material,
                            buy: true,
                        }),
                        profile.credits >= t.material_buy[i],
                    );
                }
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
            s.push_str(&format!(
                "|{:?}|{:?}|{}|{}",
                game.profile
                    .stash
                    .iter()
                    .map(|g| (g.id, &g.affixes))
                    .collect::<Vec<_>>(),
                game.profile.market.len(),
                game.profile.credits,
                game.profile.docked
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
