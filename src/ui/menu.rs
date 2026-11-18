//! Interactive screens: the main menu, save slots, settings, controls
//! (Quasar's `ControlsScreen`: rebind, conflicts, reset), pause and
//! confirmations, and the in-world terminals (navigation, fabricator,
//! level-up, debrief).
//! Keyboard, mouse and gamepad all navigate through `quasar-ui` focus.

use super::hud::{hazard_color, objective_icon, roman};
use super::kit::{self, Kind, Window};
use super::rarity_color;
use super::{Builder, Layer};
use crate::bindings::{self, Prompts};
use crate::storage::{SlotState, SlotSummary};
use planetfall_sim::content::{
    ClassId, GearSlot, Material, PlanetId, Rarity, Upgrade, UpgradeId, class_xp_to_next, content,
};
use planetfall_sim::game::RunSummary;
use planetfall_sim::mission::{MAX_HAZARD, hazard_name};
use planetfall_sim::run::OfferSource;
use planetfall_sim::run::RunOutcome;
use planetfall_sim::{Command, Game, Terminal};
use quasar_input::{Bindings, Button, DeviceFamily, InputDevice};
use quasar_ui::{
    ActionId, Align, Color, ControlEntry, ControlsOutcome, ControlsScreen, LayoutItem, Length,
    NodeId, Rect, TextMeasure, TextStyle, UiEvent, UiInput, UiText, Vec2, Widget,
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
    /// Select/back prompts for window footers.
    hints: String,
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
            hints: String::new(),
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

    /// Position of the focused control in focus order.
    pub fn focus_index(&self) -> Option<usize> {
        let ui = &self.layer.as_ref()?.ui;
        let focused = ui.focused()?;
        ui.focus_order().iter().position(|&n| n == focused)
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
        layer.refresh_overlays(viewport, scale, measure);
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

    /// A modal window centred over a dimmed screen.
    fn window(b: &mut Builder, width: f32, icon: &str, title: &str, subtitle: &str) -> Window {
        let root = b.root(true);
        b.window(root, width, icon, title, subtitle)
    }

    fn button_as(
        &mut self,
        b: &mut Builder,
        parent: NodeId,
        text: impl Into<String>,
        action: MenuAction,
        enabled: bool,
        kind: Kind,
    ) -> NodeId {
        let id = self.action(action);
        b.button_kind(
            parent,
            text,
            id,
            enabled,
            kind,
            LayoutItem::size(Length::Fill, Length::fixed(42.0)),
        )
    }

    /// A fixed-width button (right column of list rows). It sits in a sized
    /// holder so auto-sized rows make room for it.
    #[allow(clippy::too_many_arguments)]
    fn small_button(
        &mut self,
        b: &mut Builder,
        parent: NodeId,
        text: impl Into<String>,
        action: MenuAction,
        enabled: bool,
        kind: Kind,
        width: f32,
    ) -> NodeId {
        let id = self.action(action);
        let holder = b.layered(
            parent,
            LayoutItem::size(Length::fixed(width), Length::fixed(42.0)).with_align(Align::Center),
        );
        b.strut(
            holder,
            LayoutItem::size(Length::fixed(width), Length::fixed(42.0)),
            width,
            42.0,
        );
        b.button_kind(
            holder,
            text,
            id,
            enabled,
            kind,
            LayoutItem::fill().with_anchor(quasar_ui::Anchor::Stretch),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn icon_button(
        &mut self,
        b: &mut Builder,
        parent: NodeId,
        icon: &str,
        text: impl Into<String>,
        action: MenuAction,
        enabled: bool,
        kind: Kind,
    ) -> NodeId {
        let id = self.action(action);
        b.icon_button(parent, icon, text, id, enabled, kind)
    }

    /// A row of buttons sharing the width.
    fn button_bar(
        &mut self,
        b: &mut Builder,
        parent: NodeId,
        buttons: &[(&str, MenuAction, bool, Kind)],
    ) {
        let row = b.hrow(
            parent,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        for (text, action, enabled, kind) in buttons {
            self.button_as(b, row, *text, action.clone(), *enabled, *kind);
        }
    }

    /// The footer strip: select/back prompts plus a hint.
    fn footer(&self, b: &mut Builder, w: &Window, hint: &str) {
        if hint.is_empty() {
            b.footer(w, self.hints.clone());
        } else {
            b.footer(w, format!("{}   ·   {hint}", self.hints));
        }
    }

    /// The close button every terminal ends with.
    fn close(&mut self, b: &mut Builder, w: &Window) {
        self.cancel = MenuAction::Game(Command::Close);
        self.button_as(
            b,
            w.body,
            "CLOSE",
            MenuAction::Game(Command::Close),
            true,
            Kind::Ghost,
        );
    }

    /// An item row: icon, rarity tag and coloured name with its stat line,
    /// and the action button on the right.
    fn item_row(
        &mut self,
        b: &mut Builder,
        parent: NodeId,
        view: &super::ItemView,
        button: impl Into<String>,
        action: MenuAction,
        enabled: bool,
    ) -> NodeId {
        let row = b.hrow(
            parent,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            6.0,
        );
        b.style(
            row,
            kit::PANEL_ROW,
            Some((1.0, rarity_color(view.rarity).with_alpha(110))),
            3.0,
        );
        b.icon(row, view.icon, super::ITEM_ICON, rarity_color(view.rarity));
        let text = b.column(
            row,
            LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
            0.0,
            0.0,
        );
        b.tinted(
            text,
            view.tag.to_uppercase(),
            TextStyle::Small,
            super::rarity_text(view.rarity),
        );
        b.tinted(
            text,
            &view.name,
            TextStyle::Body,
            super::rarity_text(view.rarity),
        );
        if !view.detail.is_empty() {
            b.paragraph(text, &view.detail, TextStyle::Small);
        }
        let kind = if enabled {
            Kind::Primary
        } else {
            Kind::Secondary
        };
        self.small_button(b, row, button, action, enabled, kind, 160.0);
        row
    }

    fn build(
        &mut self,
        b: &mut Builder,
        content: Content,
        game: &Game,
        prompts: &Prompts,
        context: &MenuContext,
    ) {
        b.family = prompts.family;
        self.hints = prompts.menu_hints();
        match content {
            Content::Screen(screen) => self.build_screen(b, screen, prompts, context),
            Content::LevelUp => self.level_up(b, game),
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

    fn level_up(&mut self, b: &mut Builder, game: &Game) {
        let run = game.run.as_ref();
        let level = run.map_or(1, |r| r.player().level);
        let source = run.and_then(|r| r.offer_source());
        let (icon, title, hint) = match source {
            Some(OfferSource::WeaponPick) => (
                "level",
                format!("Level {level} · new weapon"),
                "Choose a weapon for a free slot. Familiar weapons turn up more often.",
            ),
            Some(OfferSource::Cache) => (
                "cargo",
                "Supply cache".to_owned(),
                "Choose one rare or better upgrade.",
            ),
            _ => (
                "level",
                format!("Level {level} · upgrade"),
                "Choose one. Upgrades last until this drop ends.",
            ),
        };
        let w = Self::window(b, 760.0, icon, &title, hint);
        let choices = run
            .and_then(|r| r.level_up_choices())
            .unwrap_or(&[])
            .to_vec();
        for (i, offer) in choices.iter().enumerate() {
            let (icon, kind) = match offer.upgrade {
                Upgrade::NewWeapon(w) => (w.key(), "new weapon"),
                Upgrade::WeaponMod(w, _) => (w.key(), "weapon mod"),
                Upgrade::AbilityMod(_) => ("upgrade", "ability mod"),
                Upgrade::Field(_) => ("upgrade", "perk"),
            };
            let name = match offer.upgrade {
                Upgrade::NewWeapon(w) => w.info().name.clone(),
                _ => offer.title(),
            };
            let view = super::ItemView {
                icon,
                rarity: offer.rarity,
                tag: if source == Some(OfferSource::WeaponPick) {
                    kind.to_owned()
                } else {
                    format!("{} {kind}", offer.rarity.name())
                },
                name,
                detail: offer.description(),
            };
            self.item_row(
                b,
                w.body,
                &view,
                "TAKE",
                MenuAction::Game(Command::ChoosePerk(i)),
                true,
            );
        }
        self.footer(b, &w, "");
    }

    fn terminal(&mut self, b: &mut Builder, game: &Game) {
        match &game.terminal {
            Some(Terminal::Navigation) => self.navigation(b, game),
            Some(Terminal::Fabricator) => self.fabricator(b, game),
            Some(Terminal::Locker) => self.locker(b, game),
            Some(Terminal::MissionBoard) => self.mission_board(b, game),
            Some(Terminal::Trade) => self.trade(b, game),
            Some(Terminal::Debrief(summary)) => self.debrief(b, game, summary),
            Some(Terminal::Module(upgrade)) => {
                let level = game.profile.level(*upgrade);
                let w = Self::window(
                    b,
                    560.0,
                    &format!("module-{}", upgrade.key()),
                    upgrade.name(),
                    &format!("Level {level} of {}", upgrade.max_level()),
                );
                b.pips(
                    w.body,
                    u32::from(level),
                    u32::from(upgrade.max_level()),
                    kit::ACCENT,
                    12.0,
                );
                b.paragraph(w.body, upgrade.info().description.as_str(), TextStyle::Body);
                b.label(
                    w.body,
                    "Upgrade this module at the Fabricator.",
                    TextStyle::Small,
                );
                self.close(b, &w);
                self.footer(b, &w, "");
            }
            None => {}
        }
    }

    fn debrief(&mut self, b: &mut Builder, game: &Game, summary: &RunSummary) {
        let (icon, title, tone) = match summary.outcome {
            RunOutcome::Extracted => (
                "check",
                "Extraction successful",
                "Cargo transferred to the hold.",
            ),
            RunOutcome::Died => (
                "threat",
                "Signal lost",
                "Recovered by suit beacon. All cargo was lost.",
            ),
            RunOutcome::Abandoned => (
                "back",
                "Drop aborted",
                "Emergency recall. All cargo was lost.",
            ),
        };
        let w = Self::window(
            b,
            680.0,
            icon,
            title,
            &format!("{} · {tone}", summary.planet.info().name),
        );
        let body = w.body;
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            22.0,
            0.0,
        );
        b.stat(
            row,
            "timer",
            format!("{:02}:{:02}", summary.seconds / 60, summary.seconds % 60),
            TextStyle::Body,
            None,
        );
        b.stat(
            row,
            "threat",
            format!("Threat {}", summary.threat),
            TextStyle::Body,
            None,
        );
        b.stat(
            row,
            "obj-cull",
            format!("{} kills", summary.kills),
            TextStyle::Body,
            None,
        );
        b.stat(
            row,
            "level",
            format!("Level {}", summary.level),
            TextStyle::Body,
            None,
        );
        let loadout = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
        for (weapon, level) in &summary.weapons {
            b.icon(loadout, weapon.key(), 44.0, kit::ACCENT_SOFT);
            b.label_fit(loadout, roman(*level), TextStyle::Small);
        }

        b.section(body, "Contract");
        let tick = |done: bool| {
            if done {
                ("check", kit::GOOD)
            } else {
                ("quit", kit::BAD)
            }
        };
        let primary = summary.contract.primary;
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        let (mark, color) = tick(summary.primary_done);
        b.glyph(row, mark, 24.0);
        b.glyph(row, objective_icon(primary.info().kind), 30.0);
        b.tinted_fit(row, primary.name(), TextStyle::Body, color);
        b.chip(
            row,
            &format!(
                "Hazard {} · {}",
                summary.hazard,
                hazard_name(summary.hazard)
            ),
            hazard_color(summary.hazard),
        );
        if let (Some(secondary), Some(done)) = (summary.contract.secondary, summary.secondary_done)
        {
            let row = b.hrow(
                body,
                LayoutItem::size(Length::Fill, Length::Auto),
                10.0,
                0.0,
            );
            let (mark, color) = tick(done);
            b.glyph(row, mark, 20.0);
            b.glyph(row, objective_icon(secondary.info().kind), 24.0);
            b.tinted_fit(
                row,
                format!("Optional · {}", secondary.name()),
                TextStyle::Small,
                color,
            );
        }
        if summary.bonus.total() > 0 {
            let row = b.hrow(
                body,
                LayoutItem::size(Length::Fill, Length::Auto),
                12.0,
                0.0,
            );
            b.label_fit(row, "Contract bonus", TextStyle::Small);
            materials_row(b, row, &summary.bonus, None);
        }
        if let Some(level) = summary.unlocked {
            let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
            b.glyph(row, "hazard", 26.0);
            b.tinted_fit(
                row,
                format!(
                    "HAZARD {level} UNLOCKED · {}",
                    hazard_name(level).to_uppercase()
                ),
                TextStyle::Body,
                hazard_color(level),
            );
        }

        b.section(body, "Class");
        let (before, after) = summary.class_levels;
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        b.glyph(row, &format!("class-{}", summary.class.key()), 36.0);
        b.label_fit(
            row,
            format!("{}  +{} class XP", summary.class.name(), summary.class_xp),
            TextStyle::Body,
        );
        if after > before {
            b.chip(row, &format!("Level up {before} → {after}"), kit::GOOD);
        } else {
            b.chip(row, &format!("Level {after}"), kit::DIM);
        }

        if !summary.finds.is_empty() {
            b.section(body, "Finds · kept");
            let shown = summary.finds.len().min(4);
            for find in &summary.finds[..shown] {
                b.item_view(body, &super::find_view(find), true);
            }
            if summary.finds.len() > shown {
                b.label(
                    body,
                    format!("…and {} more in the stash", summary.finds.len() - shown),
                    TextStyle::Small,
                );
            }
        }
        if summary.salvaged > 0 {
            let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
            b.stat(
                row,
                "credits",
                format!(
                    "Stash full: finds salvaged for {} credits",
                    summary.salvaged
                ),
                TextStyle::Small,
                Some(kit::WARN),
            );
        }

        b.section(body, "Cargo");
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            0.0,
        );
        if summary.outcome == RunOutcome::Extracted {
            b.label_fit(row, "Banked", TextStyle::Body);
            materials_row(b, row, &summary.banked, Some(kit::GOOD));
            if summary.salvage.total() > 0 {
                let row = b.hrow(
                    body,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    12.0,
                    0.0,
                );
                b.label_fit(row, "Ship AI salvage", TextStyle::Small);
                materials_row(b, row, &summary.salvage, None);
            }
        } else {
            b.label_fit(row, "Lost", TextStyle::Body);
            materials_row(b, row, &summary.lost, Some(kit::BAD));
        }
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            0.0,
        );
        b.label_fit(row, "Hold", TextStyle::Small);
        materials_row(b, row, &game.profile.materials, None);

        self.cancel = MenuAction::Game(Command::Close);
        self.button_as(
            b,
            body,
            "CONTINUE",
            MenuAction::Game(Command::Close),
            true,
            Kind::Primary,
        );
        self.footer(b, &w, "");
    }

    fn navigation(&mut self, b: &mut Builder, game: &Game) {
        let w = Self::window(
            b,
            760.0,
            "star-map",
            "Navigation",
            "Plot an FTL jump. Dangerous worlds pay better.",
        );
        for planet in PlanetId::all() {
            let info = planet.info();
            let unlocked = game.profile.unlocked(planet);
            let here = game.profile.orbiting == Some(planet);
            let subtitle = if unlocked {
                format!("{} · {}", info.kind, info.description)
            } else {
                info.kind.clone()
            };
            let (_, right) = b.entry(
                w.body,
                &format!("planet-{}", planet.key()),
                &info.name.to_uppercase(),
                Some(if unlocked { kit::TEXT } else { kit::DIM }),
                &subtitle,
            );
            if !unlocked {
                b.glyph(right, "lock", 22.0);
                b.chip(right, &format!("Nav core {}", info.nav_level), kit::DIM);
            } else if here {
                b.chip(right, "In orbit", kit::GOOD);
            } else {
                b.chip(right, &format!("Pay ×{:.1}", info.hazard), kit::WARN);
                self.small_button(
                    b,
                    right,
                    "JUMP",
                    MenuAction::Game(Command::TravelTo(planet)),
                    true,
                    Kind::Primary,
                    110.0,
                );
            }
        }
        let docked = game.profile.docked;
        let (_, right) = b.entry(
            w.body,
            "planet-station",
            "KESTREL EXCHANGE",
            Some(kit::WARN),
            "Trade station · sell finds, buy gear, rework items and trade materials.",
        );
        if docked {
            b.chip(right, "Docked", kit::GOOD);
        } else {
            self.small_button(
                b,
                right,
                "DOCK",
                MenuAction::Game(Command::Dock),
                true,
                Kind::Primary,
                110.0,
            );
        }
        self.close(b, &w);
        self.footer(b, &w, "");
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
        let planet = profile
            .orbiting
            .map_or("Deep space".to_owned(), |p| p.name().to_owned());
        let w = Self::window(
            b,
            860.0,
            "board",
            "Mission board",
            &format!("Contracts on {planet}"),
        );
        let body = w.body;
        b.section(body, "Hazard level");
        let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
        for level in 1..=MAX_HAZARD {
            let open = level <= unlocked;
            let label = if open {
                format!("{level} · {}", hazard_name(level).to_uppercase())
            } else {
                format!("{level} · LOCKED")
            };
            let kind = if level == hazard {
                Kind::Selected
            } else {
                Kind::Secondary
            };
            let action = if open {
                MenuAction::SetHazard(level)
            } else {
                MenuAction::None
            };
            self.button_as(b, row, label, action, open, kind);
        }
        let h = &content().tuning.hazard;
        let i = usize::from(hazard - 1);
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            18.0,
            0.0,
        );
        b.pips(
            row,
            u32::from(hazard),
            u32::from(MAX_HAZARD),
            hazard_color(hazard),
            10.0,
        );
        b.stat(
            row,
            "health",
            format!("Enemy health ×{:.2}", h.enemy_health[i]),
            TextStyle::Small,
            None,
        );
        b.stat(
            row,
            "threat",
            format!("Damage ×{:.2}", h.enemy_damage[i]),
            TextStyle::Small,
            None,
        );
        b.stat(
            row,
            "cargo",
            format!("Haul ×{:.2}", h.reward[i]),
            TextStyle::Small,
            Some(kit::GOOD),
        );
        b.stat(
            row,
            "xp",
            format!("Class XP ×{:.2}", h.class_xp[i]),
            TextStyle::Small,
            Some(kit::INFO),
        );
        if unlocked < MAX_HAZARD {
            b.label(
                body,
                format!(
                    "Complete a contract at hazard {unlocked} to unlock hazard {}.",
                    unlocked + 1
                ),
                TextStyle::Small,
            );
        }
        b.section(body, "Contracts");
        for (n, contract) in profile.board.iter().enumerate() {
            let card = b.column(
                body,
                LayoutItem::size(Length::Fill, Length::Auto),
                6.0,
                10.0,
            );
            b.style(card, kit::PANEL_ROW, Some((1.0, kit::LINE)), 3.0);
            let head = b.hrow(
                card,
                LayoutItem::size(Length::Fill, Length::Auto),
                12.0,
                0.0,
            );
            b.glyph(head, objective_icon(contract.primary.info().kind), 48.0);
            let text = b.column(
                head,
                LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
                2.0,
                0.0,
            );
            b.tinted(
                text,
                contract.primary.name().to_uppercase(),
                TextStyle::Body,
                kit::ACCENT,
            );
            b.paragraph(text, &contract.primary.describe(hazard), TextStyle::Small);
            self.small_button(
                b,
                head,
                "DEPLOY",
                MenuAction::Game(Command::Deploy {
                    contract: n,
                    hazard,
                }),
                true,
                Kind::Primary,
                130.0,
            );
            let tags = b.hrow(
                card,
                LayoutItem::size(Length::Fill, Length::Auto),
                14.0,
                0.0,
            );
            if let Some(boss) = contract.boss {
                b.stat(
                    tags,
                    "boss",
                    boss.name(),
                    TextStyle::Small,
                    Some(Color::rgb(220, 160, 255)),
                );
            }
            if let Some(secondary) = contract.secondary {
                b.stat(
                    tags,
                    objective_icon(secondary.info().kind),
                    format!("Optional · {}", secondary.name()),
                    TextStyle::Small,
                    Some(kit::GOOD),
                );
            }
            let site = contract.site.info();
            b.stat(
                tags,
                "site",
                site.name.as_str(),
                TextStyle::Small,
                Some(kit::DIM),
            );
            if let Some(mutator) = contract.mutator {
                b.stat(
                    tags,
                    "mutator",
                    mutator.name(),
                    TextStyle::Small,
                    Some(kit::WARN),
                );
            }
            let mut notes = vec![site.description.clone()];
            if let Some(mutator) = contract.mutator {
                notes.push(mutator.info().description.clone());
            }
            if let Some(secondary) = contract.secondary {
                notes.insert(0, format!("Optional: {}", secondary.describe(hazard)));
            }
            b.paragraph(card, &notes.join("  ·  "), TextStyle::Small);
        }
        self.close(b, &w);
        self.footer(b, &w, "");
    }

    /// Locker room: pick a class, see its kit and progress, choose the
    /// starting weapon. Every class keeps its own progress.
    fn locker(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let w = Self::window(
            b,
            820.0,
            &format!("class-{}", profile.class.key()),
            "Locker room",
            "Suit up as any class. Each keeps its own level, loadout and gear.",
        );
        let worn = profile.equipped(profile.class).len();
        self.tabs(
            b,
            w.body,
            &[
                "CREW".to_owned(),
                format!("GEAR {worn}/4  ·  STASH {}", profile.stash.len()),
            ],
        );
        if self.view.tab == 1 {
            self.gear(b, w.body, game);
            self.close(b, &w);
            self.footer(b, &w, "");
            return;
        }
        let grid =
            b.ui.add_child(
                w.body,
                Widget::Panel {
                    layout: quasar_ui::PanelLayout::Grid {
                        columns: 2,
                        gap: 8.0,
                    },
                    padding: quasar_ui::Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                LayoutItem::size(Length::Fill, Length::Auto),
            )
            .expect("valid parent");
        for class in ClassId::all() {
            let (level, into) = profile.class_level(class);
            let info = class.info();
            let active = profile.class == class;
            let card = b.hrow(
                grid,
                LayoutItem::size(Length::Fill, Length::Auto),
                10.0,
                8.0,
            );
            b.style(
                card,
                kit::PANEL_ROW,
                Some((
                    if active { 2.0 } else { 1.0 },
                    if active { kit::ACCENT } else { kit::LINE },
                )),
                3.0,
            );
            b.glyph(card, &format!("class-{}", class.key()), 48.0);
            let text = b.column(
                card,
                LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
                3.0,
                0.0,
            );
            b.tinted(
                text,
                info.name.to_uppercase(),
                TextStyle::Body,
                if active { kit::ACCENT } else { kit::TEXT },
            );
            b.label(text, info.role.as_str(), TextStyle::Small);
            let max = content().tuning.class.max_level;
            let row = b.hrow(text, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
            b.label_fit(row, format!("Lv {level}"), TextStyle::Small);
            let f = if level >= max {
                1.0
            } else {
                into as f32 / class_xp_to_next(level).max(1) as f32
            };
            b.bar(row, Length::Fill, 6.0, f, kit::INFO);
            if active {
                b.chip(card, "Active", kit::ACCENT);
            } else {
                self.small_button(
                    b,
                    card,
                    "SELECT",
                    MenuAction::Game(Command::SelectClass(class)),
                    true,
                    Kind::Secondary,
                    96.0,
                );
            }
        }
        let class = profile.class;
        let info = class.info();
        let (level, _) = profile.class_level(class);
        b.section(w.body, &format!("{} kit", info.name));
        b.paragraph(w.body, &info.description, TextStyle::Small);
        let (_, right) = b.entry(
            w.body,
            super::hud::ability_icon(info.ability.kind),
            &format!("{} {}", info.ability.name, roman(class.ability_tier(level))),
            Some(kit::INFO),
            &info.ability.description,
        );
        b.chip(right, "Ability", kit::INFO);
        let (_, right) = b.entry(
            w.body,
            "level",
            &format!("{} {}", info.passive.name, roman(class.passive_tier(level))),
            Some(kit::GOOD),
            &info.passive.description,
        );
        b.chip(right, "Passive", kit::GOOD);

        b.section(w.body, "Starting weapon");
        let options = profile.start_options(class);
        let current = profile.start_weapon(class);
        let weapons: Vec<_> = class
            .familiar()
            .chain(std::iter::once(class.signature()))
            .collect();
        for weapon in weapons.into_iter().rev() {
            let unlocked = options.contains(&weapon);
            let tag = if weapon == class.signature() {
                "Signature weapon"
            } else {
                "Familiar weapon"
            };
            let (_, right) = b.entry(
                w.body,
                weapon.key(),
                weapon.name(),
                Some(if unlocked { kit::TEXT } else { kit::DIM }),
                &format!("{tag} · {}", weapon.info().description),
            );
            if weapon == current {
                b.chip(right, "Equipped", kit::GOOD);
            } else if unlocked {
                self.small_button(
                    b,
                    right,
                    "EQUIP",
                    MenuAction::Game(Command::SetStartWeapon(weapon)),
                    true,
                    Kind::Secondary,
                    100.0,
                );
            } else {
                let at = info
                    .unlocks
                    .iter()
                    .find(|u| u.weapon == weapon.key())
                    .map_or("Find on a dive".to_owned(), |u| {
                        format!("Class level {}", u.level)
                    });
                b.glyph(right, "lock", 20.0);
                b.chip(right, &at, kit::DIM);
            }
        }
        self.close(b, &w);
        self.footer(b, &w, "");
    }

    /// A row of tab buttons; the current tab is highlighted.
    fn tabs(&mut self, b: &mut Builder, card: NodeId, labels: &[String]) {
        let row = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
        for (i, label) in labels.iter().enumerate() {
            let current = usize::from(self.view.tab) == i;
            let action = MenuAction::SetView(View {
                tab: i as u8,
                ..View::default()
            });
            self.button_as(
                b,
                row,
                label.clone(),
                action,
                true,
                if current {
                    Kind::Selected
                } else {
                    Kind::Secondary
                },
            );
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
        let row = b.hrow(
            card,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        self.button_as(
            b,
            row,
            "◀ PREVIOUS",
            MenuAction::SetView(prev),
            page > 0,
            Kind::Secondary,
        );
        let label = b.column(
            row,
            LayoutItem::size(Length::fixed(140.0), Length::Auto).with_align(Align::Center),
            0.0,
            0.0,
        );
        b.label(
            label,
            format!("Page {} of {pages}", page + 1),
            TextStyle::Small,
        );
        self.button_as(
            b,
            row,
            "NEXT ▶",
            MenuAction::SetView(next),
            page + 1 < pages,
            Kind::Secondary,
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
        b.section(card, &format!("{} wears", class.name()));
        b.paragraph(
            card,
            &if stats.is_empty() {
                "Nothing yet. Gear drops from caches, elites and bosses.".to_owned()
            } else {
                stats.join("  ·  ")
            },
            TextStyle::Small,
        );
        for slot in GearSlot::ALL {
            let current = worn.iter().find(|g| g.slot() == slot);
            let open = self.view.slot == Some(slot);
            let toggle = View {
                slot: (!open).then_some(slot),
                ..self.view
            };
            let group = b.column(
                card,
                LayoutItem::size(Length::Fill, Length::Auto),
                6.0,
                if open { 8.0 } else { 0.0 },
            );
            let toggle_text = if open { "CLOSE" } else { "CHANGE" };
            match current {
                Some(g) => {
                    self.item_row(
                        b,
                        group,
                        &super::gear_view(g),
                        toggle_text,
                        MenuAction::SetView(toggle),
                        true,
                    );
                }
                None => {
                    let (_, right) = b.entry(
                        group,
                        "lock",
                        &format!("{} · empty", slot.name()),
                        Some(kit::DIM),
                        "No item worn in this slot.",
                    );
                    self.small_button(
                        b,
                        right,
                        toggle_text,
                        MenuAction::SetView(toggle),
                        true,
                        Kind::Secondary,
                        160.0,
                    );
                }
            }
            if !open {
                continue;
            }
            b.style(group, kit::PANEL_INSET, Some((1.0, kit::ACCENT_SOFT)), 3.0);
            if current.is_some() {
                self.button_as(
                    b,
                    group,
                    "TAKE OFF",
                    MenuAction::Game(Command::Unequip(slot)),
                    true,
                    Kind::Ghost,
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
                b.label(
                    group,
                    "Nothing in the stash for this slot.",
                    TextStyle::Small,
                );
            }
            for g in options.into_iter().take(PAGE) {
                let compare = if g.score() > base + 0.05 {
                    "EQUIP ▲"
                } else if g.score() < base - 0.05 {
                    "EQUIP ▼"
                } else {
                    "EQUIP ="
                };
                let mut view = super::gear_view(g);
                if let Some(c) = profile.wearer(g.id) {
                    view.tag = format!("{} · worn by {}", view.tag, c.name());
                }
                self.item_row(
                    b,
                    group,
                    &view,
                    compare,
                    MenuAction::Game(Command::Equip(g.id)),
                    true,
                );
            }
        }
    }

    /// The trade station: sell, buy, rework and trade materials.
    fn trade(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let t = &content().tuning.trade;
        let w = Self::window(
            b,
            860.0,
            "trade",
            "Kestrel Exchange",
            "Trade station · credits for finds, gear for credits",
        );
        let card = w.body;
        let row = b.hrow(
            card,
            LayoutItem::size(Length::Fill, Length::Auto),
            18.0,
            0.0,
        );
        b.stat(
            row,
            "credits",
            profile.credits.to_string(),
            TextStyle::Body,
            Some(kit::WARN),
        );
        b.stat(
            row,
            "cargo",
            format!(
                "Stash {}/{}",
                profile.stash.len(),
                content().tuning.equipment.stash_size
            ),
            TextStyle::Body,
            None,
        );
        materials_row(b, row, &profile.materials, None);
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
            menu.item_row(
                b,
                card,
                &super::gear_view(g),
                text,
                MenuAction::Game(action),
                enabled,
            );
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
                        Some(c) => format!("WORN · {}", c.name().to_uppercase()),
                        None => format!("SELL +{}", g.sell_value()),
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
                    let text = format!("BUY {}", g.price());
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
                b.section(card, "Fuse");
                for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
                    let have = profile.fusible(rarity).len();
                    let next = Rarity::ALL[rarity.index() + 1];
                    let cost = t.fuse_cost[rarity.index()];
                    let (_, right) = b.entry(
                        card,
                        "fabricator",
                        &format!("{} {} → 1 {}", t.fuse, rarity.name(), next.name()),
                        Some(super::rarity_text(next)),
                        &format!(
                            "{have} unworn {} items in the stash · {cost} credits",
                            rarity.name().to_lowercase()
                        ),
                    );
                    let ok = have >= t.fuse && profile.credits >= cost;
                    self.small_button(
                        b,
                        right,
                        "FUSE",
                        MenuAction::Game(Command::Fuse(rarity)),
                        ok,
                        if ok { Kind::Primary } else { Kind::Secondary },
                        120.0,
                    );
                }
                b.label(
                    card,
                    "Fusing uses the weakest unworn items. Rerolling keeps the base and rarity.",
                    TextStyle::Small,
                );
                b.section(card, "Reroll");
                let items: Vec<_> = profile.stash.iter().collect();
                for g in self.page(&items).to_vec() {
                    let cost = t.reroll[g.rarity.index()];
                    item_row(
                        self,
                        b,
                        g,
                        format!("REROLL {cost}"),
                        Command::Reroll(g.id),
                        profile.credits >= cost,
                    );
                }
                self.pager(b, card, items.len());
            }
            _ => {
                for (i, material) in Material::ALL.into_iter().enumerate() {
                    let (_, right) = b.entry(
                        card,
                        super::hud::material_icon(material),
                        material.name(),
                        None,
                        &format!(
                            "You hold {} · lots of {}",
                            profile.materials.get(material),
                            t.lot
                        ),
                    );
                    let sell = profile.materials.get(material) >= t.lot;
                    self.small_button(
                        b,
                        right,
                        format!("SELL +{}", t.material_sell[i]),
                        MenuAction::Game(Command::TradeMaterial {
                            material,
                            buy: false,
                        }),
                        sell,
                        Kind::Secondary,
                        130.0,
                    );
                    let buy = profile.credits >= t.material_buy[i];
                    self.small_button(
                        b,
                        right,
                        format!("BUY {}", t.material_buy[i]),
                        MenuAction::Game(Command::TradeMaterial {
                            material,
                            buy: true,
                        }),
                        buy,
                        Kind::Secondary,
                        130.0,
                    );
                }
            }
        }
        self.close(b, &w);
        self.footer(b, &w, "Prices in credits");
    }

    fn fabricator(&mut self, b: &mut Builder, game: &Game) {
        let w = Self::window(
            b,
            820.0,
            "fabricator",
            "Fabricator",
            "Build and upgrade ship modules. Upgrades are permanent.",
        );
        let row = b.hrow(
            w.body,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            0.0,
        );
        b.label_fit(row, "Hold", TextStyle::Small);
        materials_row(b, row, &game.profile.materials, None);
        for upgrade in UpgradeId::all() {
            let level = game.profile.level(upgrade);
            let card = b.hrow(
                w.body,
                LayoutItem::size(Length::Fill, Length::Auto),
                12.0,
                8.0,
            );
            b.style(card, kit::PANEL_ROW, Some((1.0, kit::LINE)), 3.0);
            b.glyph(card, &format!("module-{}", upgrade.key()), 52.0);
            let text = b.column(
                card,
                LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
                3.0,
                0.0,
            );
            let head = b.hrow(
                text,
                LayoutItem::size(Length::Fill, Length::Auto),
                10.0,
                0.0,
            );
            b.tinted_fit(
                head,
                upgrade.name().to_uppercase(),
                TextStyle::Body,
                if level > 0 { kit::ACCENT } else { kit::TEXT },
            );
            b.pips(
                head,
                u32::from(level),
                u32::from(upgrade.max_level()),
                kit::ACCENT,
                9.0,
            );
            b.paragraph(text, upgrade.info().description.as_str(), TextStyle::Small);
            let right = b.column(
                card,
                LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
                6.0,
                0.0,
            );
            match game.profile.next_cost(upgrade) {
                None => {
                    b.chip(right, "Complete", kit::GOOD);
                }
                Some(cost) => {
                    let affordable = game.profile.materials.covers(cost);
                    let costs = b.hrow(
                        right,
                        LayoutItem::size(Length::Auto, Length::Auto),
                        8.0,
                        0.0,
                    );
                    for m in Material::ALL {
                        let need = cost.get(m);
                        if need > 0 {
                            let color = if game.profile.materials.get(m) >= need {
                                kit::TEXT
                            } else {
                                kit::BAD
                            };
                            b.stat(
                                costs,
                                super::hud::material_icon(m),
                                need.to_string(),
                                TextStyle::Small,
                                Some(color),
                            );
                        }
                    }
                    let verb = if level == 0 { "BUILD" } else { "UPGRADE" };
                    self.small_button(
                        b,
                        right,
                        verb,
                        MenuAction::Game(Command::Purchase(upgrade)),
                        affordable,
                        if affordable {
                            Kind::Primary
                        } else {
                            Kind::Secondary
                        },
                        150.0,
                    );
                }
            }
        }
        self.close(b, &w);
        self.footer(b, &w, "");
    }

    fn build_screen(
        &mut self,
        b: &mut Builder,
        screen: Screen,
        prompts: &Prompts,
        context: &MenuContext,
    ) {
        match screen {
            Screen::Main => self.main_menu(b, prompts, context),
            Screen::Saves(mode) => {
                let (icon, title, hint) = match mode {
                    SavesMode::New => ("new", "New game", "Choose a slot for the new commander."),
                    SavesMode::Load => ("load", "Load game", "Continue a saved commander."),
                    SavesMode::Delete => ("trash", "Delete a save", "Choose a save to delete."),
                };
                let w = Self::window(b, 700.0, icon, title, hint);
                if let Some(error) = &context.error {
                    let row = b.hrow(
                        w.body,
                        LayoutItem::size(Length::Fill, Length::Auto),
                        8.0,
                        0.0,
                    );
                    b.glyph(row, "hazard", 24.0);
                    b.tinted(
                        row,
                        format!("Could not load: {error}"),
                        TextStyle::Small,
                        kit::BAD,
                    );
                }
                for (slot, state) in &context.slots {
                    let (icon, title, subtitle, enabled) = match state {
                        SlotState::Empty => (
                            "new",
                            format!("SLOT {slot}"),
                            "Empty".to_owned(),
                            mode == SavesMode::New,
                        ),
                        SlotState::Saved(s) => ("save", format!("SLOT {slot}"), slot_line(s), true),
                        SlotState::Unusable(reason) => (
                            "hazard",
                            format!("SLOT {slot}"),
                            format!("Unusable ({reason})"),
                            mode != SavesMode::Load,
                        ),
                    };
                    let color = if matches!(state, SlotState::Empty) {
                        kit::DIM
                    } else {
                        kit::TEXT
                    };
                    let (_, right) = b.entry(w.body, icon, &title, Some(color), &subtitle);
                    let (verb, kind) = match (mode, state) {
                        (SavesMode::Delete, _) => ("DELETE", Kind::Danger),
                        (SavesMode::New, SlotState::Empty) => ("START", Kind::Primary),
                        (SavesMode::New, _) => ("OVERWRITE", Kind::Danger),
                        (SavesMode::Load, _) => ("LOAD", Kind::Primary),
                    };
                    self.small_button(
                        b,
                        right,
                        verb,
                        MenuAction::PickSlot(*slot),
                        enabled,
                        if enabled { kind } else { Kind::Secondary },
                        140.0,
                    );
                }
                let row = b.hrow(
                    w.body,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    10.0,
                    0.0,
                );
                if mode == SavesMode::Load {
                    self.button_as(
                        b,
                        row,
                        "DELETE A SAVE…",
                        MenuAction::OpenSaves(SavesMode::Delete),
                        true,
                        Kind::Danger,
                    );
                }
                self.button_as(b, row, "BACK", MenuAction::Back, true, Kind::Ghost);
                self.cancel = MenuAction::Back;
                self.footer(b, &w, "");
            }
            Screen::ConfirmOverwrite(slot) => {
                let w = Self::window(b, 560.0, "hazard", &format!("Overwrite slot {slot}?"), "");
                b.paragraph(
                    w.body,
                    "The commander in this slot is replaced by a new game.",
                    TextStyle::Body,
                );
                self.button_bar(
                    b,
                    w.body,
                    &[
                        (
                            "START NEW GAME",
                            MenuAction::Overwrite(slot),
                            true,
                            Kind::Danger,
                        ),
                        ("CANCEL", MenuAction::Back, true, Kind::Ghost),
                    ],
                );
                self.cancel = MenuAction::Back;
                self.footer(b, &w, "");
            }
            Screen::ConfirmDelete(slot) => {
                let w = Self::window(b, 560.0, "trash", &format!("Delete slot {slot}?"), "");
                b.paragraph(
                    w.body,
                    "This commander's ship and progress are erased.",
                    TextStyle::Body,
                );
                self.button_bar(
                    b,
                    w.body,
                    &[
                        ("DELETE", MenuAction::DeleteSlot(slot), true, Kind::Danger),
                        ("CANCEL", MenuAction::Back, true, Kind::Ghost),
                    ],
                );
                self.cancel = MenuAction::Back;
                self.footer(b, &w, "");
            }
            Screen::Settings => {
                let w = Self::window(
                    b,
                    640.0,
                    "settings",
                    "Settings",
                    "Audio, display and gameplay",
                );
                let o = self.settings.clone();
                let head = b.hrow(
                    w.body,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    8.0,
                    0.0,
                );
                b.glyph(head, "sound", 22.0);
                b.section(head, "Audio");
                for (label, value, field) in [
                    ("Master volume", o.master, Field::Master),
                    ("Music volume", o.music, Field::Music),
                    ("Effects volume", o.effects, Field::Effects),
                ] {
                    let id =
                        b.ui.add_child(
                            w.body,
                            Widget::Slider {
                                label: UiText::literal(label),
                                value,
                                min: 0.0,
                                max: 1.0,
                                step: 0.05,
                                action: None,
                            },
                            LayoutItem::size(Length::Fill, Length::fixed(36.0)),
                        )
                        .expect("valid parent");
                    self.sliders.push((id, field));
                }
                let head = b.hrow(
                    w.body,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    8.0,
                    0.0,
                );
                b.glyph(head, "display", 22.0);
                b.section(head, "Display");
                for (label, value, field) in [
                    ("Fullscreen", o.fullscreen, Field::Fullscreen),
                    ("Shadows", o.shadows, Field::Shadows),
                    ("Bloom", o.bloom, Field::Bloom),
                ] {
                    let id = self.check(b, w.body, label, value);
                    self.checks.push((id, field));
                }
                let head = b.hrow(
                    w.body,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    8.0,
                    0.0,
                );
                b.glyph(head, "controller", 22.0);
                b.section(head, "Gameplay");
                let shake = self.check(b, w.body, "Screen shake", o.shake);
                self.checks.push((shake, Field::Shake));
                self.icon_button(
                    b,
                    w.body,
                    "keyboard",
                    "CONTROLS…",
                    MenuAction::OpenControls { gamepad: false },
                    true,
                    Kind::Secondary,
                );
                self.button_bar(
                    b,
                    w.body,
                    &[
                        ("APPLY", MenuAction::ApplySettings, true, Kind::Primary),
                        ("BACK", MenuAction::Back, true, Kind::Ghost),
                    ],
                );
                self.cancel = MenuAction::Back;
                self.footer(b, &w, "");
            }
            Screen::Controls { gamepad } => {
                let (device, other, icon, other_icon) = if gamepad {
                    (
                        InputDevice::Gamepad,
                        "SHOW KEYBOARD AND MOUSE",
                        "controller",
                        "keyboard",
                    )
                } else {
                    (
                        InputDevice::KeyboardMouse,
                        "SHOW CONTROLLER",
                        "keyboard",
                        "controller",
                    )
                };
                let w = Self::window(
                    b,
                    660.0,
                    icon,
                    "Controls",
                    "Select an action, then press the new button. Esc cancels.",
                );
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
                    .build(&mut b.ui, w.body, ActionId::new(self.controls_reset))
                    .expect("controls screen");
                self.icon_button(
                    b,
                    w.body,
                    other_icon,
                    other,
                    MenuAction::OpenControls { gamepad: !gamepad },
                    true,
                    Kind::Secondary,
                );
                self.button_as(b, w.body, "BACK", MenuAction::Back, true, Kind::Ghost);
                self.controls = Some(controls);
                self.cancel = MenuAction::Back;
                self.footer(b, &w, "");
            }
            Screen::Pause { surface } => {
                let note = match (context.active_slot, surface) {
                    (Some(slot), true) => {
                        format!("Slot {slot} saves this drop; it resumes where you left off.")
                    }
                    (Some(slot), false) => format!("Progress saves to slot {slot}."),
                    (None, _) => "This game is not being saved.".to_owned(),
                };
                let w = Self::window(b, 500.0, "pause", "Paused", &note);
                self.icon_button(
                    b,
                    w.body,
                    "play",
                    "RESUME",
                    MenuAction::Resume,
                    true,
                    Kind::Primary,
                );
                self.icon_button(
                    b,
                    w.body,
                    "settings",
                    "SETTINGS",
                    MenuAction::OpenSettings,
                    true,
                    Kind::Secondary,
                );
                if surface {
                    self.icon_button(
                        b,
                        w.body,
                        "back",
                        "ABANDON DROP",
                        MenuAction::AskAbandon,
                        true,
                        Kind::Danger,
                    );
                }
                let label = if context.active_slot.is_some() {
                    "SAVE AND QUIT TO MENU"
                } else {
                    "QUIT TO MENU"
                };
                self.icon_button(
                    b,
                    w.body,
                    "save",
                    label,
                    MenuAction::SaveAndQuit,
                    true,
                    Kind::Secondary,
                );
                self.icon_button(
                    b,
                    w.body,
                    "quit",
                    "QUIT GAME",
                    MenuAction::AskQuit,
                    true,
                    Kind::Ghost,
                );
                self.cancel = MenuAction::Resume;
                self.footer(b, &w, "");
            }
            Screen::ConfirmAbandon => {
                let w = Self::window(b, 560.0, "hazard", "Abandon this drop?", "");
                b.paragraph(
                    w.body,
                    "You return to the ship. Everything you carry is lost.",
                    TextStyle::Body,
                );
                self.button_bar(
                    b,
                    w.body,
                    &[
                        ("ABANDON", MenuAction::Abandon, true, Kind::Danger),
                        ("KEEP FIGHTING", MenuAction::Back, true, Kind::Primary),
                    ],
                );
                self.cancel = MenuAction::Back;
                self.footer(b, &w, "");
            }
            Screen::ConfirmQuit => {
                let w = Self::window(b, 520.0, "quit", "Quit Planetfall?", "");
                if let Some(slot) = context.active_slot {
                    b.paragraph(
                        w.body,
                        &format!("Your game is saved to slot {slot}."),
                        TextStyle::Body,
                    );
                }
                self.button_bar(
                    b,
                    w.body,
                    &[
                        ("QUIT", MenuAction::Quit, true, Kind::Danger),
                        ("CANCEL", MenuAction::Back, true, Kind::Ghost),
                    ],
                );
                self.cancel = MenuAction::Back;
                self.footer(b, &w, "");
            }
        }
    }

    /// The title screen: the logo and a left-aligned menu over the ship.
    fn main_menu(&mut self, b: &mut Builder, prompts: &Prompts, context: &MenuContext) {
        let root = b.root(true);
        let _ = b.ui.set_style(
            root,
            Some(quasar_ui::NodeStyle::filled(Color::rgba(0, 3, 8, 110))),
        );
        let column = b.column(
            root,
            LayoutItem::size(Length::fixed(600.0), Length::Auto)
                .with_anchor(quasar_ui::Anchor::Left)
                .with_offset(Vec2::new(90.0, 0.0)),
            10.0,
            0.0,
        );
        b.logo(column, 600.0);
        let menu = b.column(
            column,
            LayoutItem::size(Length::fixed(420.0), Length::Auto),
            8.0,
            16.0,
        );
        b.style(menu, kit::PANEL, Some((1.0, kit::LINE)), 3.0);
        b.brackets(menu, kit::ACCENT);
        let resume = context.continue_slot.and_then(|slot| {
            context.slots.iter().find_map(|(s, state)| match state {
                SlotState::Saved(summary) if *s == slot => Some((slot, summary)),
                _ => None,
            })
        });
        self.icon_button(
            b,
            menu,
            "play",
            "CONTINUE",
            MenuAction::Continue,
            resume.is_some(),
            Kind::Primary,
        );
        if let Some((slot, summary)) = resume {
            b.label(
                menu,
                format!(
                    "Slot {slot} · {} · {}",
                    summary.location,
                    play_time(summary.meta.play_seconds)
                ),
                TextStyle::Small,
            );
        }
        self.icon_button(
            b,
            menu,
            "new",
            "NEW GAME",
            MenuAction::OpenSaves(SavesMode::New),
            true,
            Kind::Secondary,
        );
        let any = context
            .slots
            .iter()
            .any(|(_, s)| matches!(s, SlotState::Saved(_)));
        self.icon_button(
            b,
            menu,
            "load",
            "LOAD GAME",
            MenuAction::OpenSaves(SavesMode::Load),
            any,
            Kind::Secondary,
        );
        self.icon_button(
            b,
            menu,
            "settings",
            "SETTINGS",
            MenuAction::OpenSettings,
            true,
            Kind::Secondary,
        );
        self.icon_button(
            b,
            menu,
            "quit",
            "QUIT",
            MenuAction::AskQuit,
            true,
            Kind::Ghost,
        );
        let hint = b.hrow(
            column,
            LayoutItem::size(Length::fixed(600.0), Length::Auto),
            8.0,
            0.0,
        );
        b.glyph(
            hint,
            if prompts.gamepad() {
                "controller"
            } else {
                "keyboard"
            },
            22.0,
        );
        b.rich(
            hint,
            &prompts.format("{move} move   {dash} dash   {interact} use   {extract} call pod"),
            TextStyle::Small,
            560.0,
        );
        self.cancel = MenuAction::AskQuit;
    }

    fn check(&mut self, b: &mut Builder, parent: NodeId, label: &str, checked: bool) -> NodeId {
        let id =
            b.ui.add_child(
                parent,
                Widget::Checkbox {
                    label: UiText::literal(label),
                    checked,
                    action: None,
                },
                LayoutItem::size(Length::Fill, Length::fixed(34.0)),
            )
            .expect("valid parent");
        // The row is lit so the (track-coloured) box shows against it.
        b.style(id, kit::PANEL_ROW, Some((1.0, kit::LINE)), 3.0);
        id
    }
}

/// Materials as icon chips in a row (`color` tints the numbers).
fn materials_row(
    b: &mut Builder,
    row: NodeId,
    m: &planetfall_sim::content::Materials,
    color: Option<Color>,
) {
    for material in Material::ALL {
        b.stat(
            row,
            super::hud::material_icon(material),
            m.get(material).to_string(),
            TextStyle::Body,
            color,
        );
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
