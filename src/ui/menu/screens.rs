//! App screens: the title menu, save slots, settings, controls, pause
//! (with the drop summary) and confirmations.

use super::{Field, Menu, MenuAction, MenuContext, SavesMode, Screen, play_time, slot_line};
use crate::bindings::{self, Prompts};
use crate::storage::SlotState;
use crate::ui::Builder;
use crate::ui::hud::{hazard_color, objective_icon};
use crate::ui::kit::{self, Kind};
use planetfall_sim::Game;
use planetfall_sim::mission::hazard_name;
use quasar_input::{DeviceFamily, InputDevice};
use quasar_ui::{
    ActionId, Color, ControlEntry, ControlsScreen, LayoutItem, Length, NodeId, TextStyle, UiText,
    Vec2, Widget,
};

impl Menu {
    pub(super) fn build_screen(
        &mut self,
        b: &mut Builder,
        screen: Screen,
        game: &Game,
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
                let w = Self::window(b, 540.0, "pause", "Paused", &note);
                if let (true, Some(run)) = (surface, game.run.as_ref()) {
                    self.drop_details(b, w.body, run);
                }
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

    /// The pause screen's summary of the drop: planet, hazard, contract,
    /// site, mutator and threat (kept off the HUD).
    pub(super) fn drop_details(
        &mut self,
        b: &mut Builder,
        body: NodeId,
        run: &planetfall_sim::run::Run,
    ) {
        let m = &run.mission;
        let info = run.planet.info();
        b.section(body, "This drop");
        let head = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        b.glyph(head, &format!("planet-{}", run.planet.key()), 32.0);
        b.tinted_fit(head, info.name.to_uppercase(), TextStyle::Body, kit::TEXT);
        if !run.tutorial {
            b.chip(
                head,
                &format!("Hazard {} · {}", m.hazard, hazard_name(m.hazard)),
                hazard_color(m.hazard),
            );
        }
        let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
        b.glyph(row, objective_icon(m.primary_kind()), 26.0);
        b.paragraph(
            row,
            &m.contract.primary.describe(m.hazard),
            TextStyle::Small,
        );
        if let Some(secondary) = m.contract.secondary {
            let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
            b.glyph(row, objective_icon(secondary.info().kind), 22.0);
            b.paragraph(
                row,
                &format!("Optional: {}", secondary.describe(m.hazard)),
                TextStyle::Small,
            );
        }
        if !run.tutorial {
            let tags = b.hrow(
                body,
                LayoutItem::size(Length::Fill, Length::Auto),
                16.0,
                0.0,
            );
            b.stat(
                tags,
                "site",
                m.contract.site.name(),
                TextStyle::Small,
                Some(kit::DIM),
            );
            if let Some(mutator) = m.contract.mutator {
                b.stat(
                    tags,
                    "mutator",
                    mutator.name(),
                    TextStyle::Small,
                    Some(kit::GOOD),
                );
            }
            b.stat(
                tags,
                "threat",
                format!(
                    "Threat {} · next in {}",
                    run.threat_level(),
                    crate::ui::hud::threat_eta(run)
                ),
                TextStyle::Small,
                Some(kit::WARN),
            );
        }
        b.rule(body);
    }

    /// The title screen: the logo and a left-aligned menu over the ship.
    pub(super) fn main_menu(&mut self, b: &mut Builder, prompts: &Prompts, context: &MenuContext) {
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
}
