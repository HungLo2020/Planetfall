//! Interactive screens: the main menu, save slots, settings, controls
//! (Quasar's `ControlsScreen`: rebind, conflicts, reset), pause and
//! confirmations, and the in-world terminals (navigation, fabricator,
//! level-up, debrief).
//! Keyboard, mouse and gamepad all navigate through `quasar-ui` focus.
//! This module holds the menu state, shared widgets and dispatch; each
//! group of screens lives in a submodule.

mod locker;
mod screens;
mod terminals;
mod trade;

use super::kit::{self, Kind, Window};
use super::rarity_color;
use super::{Builder, Layer};
use crate::bindings::{self, Prompts};
use crate::storage::{SlotState, SlotSummary};
use planetfall_sim::content::{GearSlot, Material};
use planetfall_sim::{Command, Game, Terminal};
use quasar_input::{Bindings, Button};
use quasar_ui::{
    Align, Color, ControlsOutcome, ControlsScreen, LayoutItem, Length, NodeId, Rect, TextMeasure,
    TextStyle, UiEvent, UiInput, UiText, Widget,
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
            Content::Screen(screen) => self.build_screen(b, screen, game, prompts, context),
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
