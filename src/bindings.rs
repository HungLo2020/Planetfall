//! Action bindings for keyboard/mouse and controllers, and the button
//! prompts derived from them through `quasar-input`.
//!
//! Prompt templates (tutorial lines, hints, menus) name actions as
//! `{interact}`, `{dash}`, `{extract}`, `{build}`, `{ability}` and `{move}`. [`Prompts`] replaces them
//! with the active device family's binding names (`E`, `A`, `Cross`, ...).

use quasar_input::{Binding, Bindings, DeviceFamily, GamepadAxis, GamepadButton, InputDevice, Key};

/// Gameplay actions on the controls screen, with their labels. Movement is
/// rebindable on the keyboard only (the stick is an analogue axis).
pub const REBINDABLE: [(&str, &str, bool); 8] = [
    ("move_x", "Move left / right", false),
    ("move_z", "Move up / down", false),
    ("interact", "Use / deploy", true),
    ("dash", "Dash", true),
    ("ability", "Class ability", true),
    ("extract", "Call extraction pod", true),
    ("build", "Ship build mode", true),
    ("pause", "Pause", true),
];

/// Every default binding, keyboard/mouse and controller together. The
/// controls screen edits this one set; each device reads its own part.
pub fn defaults() -> Bindings {
    let mut all = keyboard();
    for (action, list) in gamepad().actions {
        for binding in list {
            all.bind(action.clone(), binding);
        }
    }
    all
}

/// The bindings `device` reads (device-less bindings go to both).
pub fn for_device(all: &Bindings, device: InputDevice) -> Bindings {
    let mut out = Bindings::default();
    for (action, list) in &all.actions {
        for binding in list {
            if binding.device().is_none_or(|d| d == device) {
                out.bind(action.clone(), binding.clone());
            }
        }
    }
    out
}

/// Menu navigation shared by both devices.
fn ui_actions(b: &mut Bindings, gamepad: bool) {
    if gamepad {
        b.bind("ui_up", Binding::gamepad_button(GamepadButton::DPadUp))
            .bind("ui_down", Binding::gamepad_button(GamepadButton::DPadDown))
            .bind("ui_left", Binding::gamepad_button(GamepadButton::DPadLeft))
            .bind(
                "ui_right",
                Binding::gamepad_button(GamepadButton::DPadRight),
            )
            .bind("ui_activate", Binding::gamepad_button(GamepadButton::South))
            .bind("ui_cancel", Binding::gamepad_button(GamepadButton::East))
            .bind("ui_cancel", Binding::gamepad_button(GamepadButton::Start));
    } else {
        b.bind("ui_up", Binding::key(Key::ArrowUp))
            .bind("ui_up", Binding::key(Key::KeyW))
            .bind("ui_down", Binding::key(Key::ArrowDown))
            .bind("ui_down", Binding::key(Key::KeyS))
            .bind("ui_left", Binding::key(Key::ArrowLeft))
            .bind("ui_right", Binding::key(Key::ArrowRight))
            .bind("ui_next", Binding::key(Key::Tab))
            .bind("ui_previous", Binding::chord([Key::ShiftLeft, Key::Tab]))
            .bind("ui_activate", Binding::key(Key::Enter))
            .bind("ui_activate", Binding::key(Key::Space))
            .bind("ui_cancel", Binding::key(Key::Escape));
    }
}

/// Actions installed on the desktop window.
pub fn keyboard() -> Bindings {
    let mut b = Bindings::default();
    b.bind("move_x", Binding::axis(Key::KeyA, Key::KeyD))
        .bind("move_x", Binding::axis(Key::ArrowLeft, Key::ArrowRight))
        .bind("move_z", Binding::axis(Key::KeyW, Key::KeyS))
        .bind("move_z", Binding::axis(Key::ArrowUp, Key::ArrowDown))
        .bind("interact", Binding::key(Key::KeyE))
        .bind("interact", Binding::key(Key::Enter))
        .bind("dash", Binding::key(Key::Space))
        .bind("dash", Binding::key(Key::ShiftLeft))
        .bind("extract", Binding::key(Key::KeyF))
        .bind("build", Binding::key(Key::KeyB))
        .bind("ability", Binding::key(Key::KeyQ))
        .bind("pause", Binding::key(Key::Escape));
    ui_actions(&mut b, false);
    b
}

/// Actions for each controller player.
pub fn gamepad() -> Bindings {
    let mut b = Bindings::default();
    b.bind("move_x", Binding::gamepad_axis(GamepadAxis::LeftStickX))
        .bind(
            "move_x",
            Binding::gamepad_button_axis(GamepadButton::DPadLeft, GamepadButton::DPadRight),
        )
        .bind(
            "move_z",
            Binding::gamepad_axis_scaled(GamepadAxis::LeftStickY, -1.0),
        )
        .bind(
            "move_z",
            Binding::gamepad_button_axis(GamepadButton::DPadUp, GamepadButton::DPadDown),
        )
        .bind("interact", Binding::gamepad_button(GamepadButton::South))
        .bind("dash", Binding::gamepad_button(GamepadButton::East))
        .bind("dash", Binding::gamepad_button(GamepadButton::RightBumper))
        .bind("extract", Binding::gamepad_button(GamepadButton::North))
        .bind("build", Binding::gamepad_button(GamepadButton::West))
        .bind(
            "ability",
            Binding::gamepad_button(GamepadButton::LeftBumper),
        )
        .bind(
            "ability",
            Binding::gamepad_button(GamepadButton::RightTrigger),
        )
        .bind("pause", Binding::gamepad_button(GamepadButton::Start));
    ui_actions(&mut b, true);
    b
}

/// Formats prompt templates for the device family the player last used.
pub struct Prompts {
    pub family: DeviceFamily,
    keyboard: Bindings,
    gamepad: Bindings,
}

impl Default for Prompts {
    fn default() -> Self {
        Self {
            family: DeviceFamily::KeyboardMouse,
            keyboard: keyboard(),
            gamepad: gamepad(),
        }
    }
}

impl Prompts {
    /// Name the player's own bindings (after a rebind).
    pub fn set_bindings(&mut self, all: &Bindings) {
        self.keyboard = for_device(all, InputDevice::KeyboardMouse);
        self.gamepad = for_device(all, InputDevice::Gamepad);
    }

    pub fn gamepad(&self) -> bool {
        self.family != DeviceFamily::KeyboardMouse
    }

    /// `{action}` → `[name]` for the active family; `{move}` becomes the
    /// bound movement keys (`[W][A][S][D]`) or the left stick (`[LS]`). The
    /// HUD and menus draw each `[name]` as a key cap or button glyph (see
    /// [`glyph`] and `ui::kit`).
    pub fn format(&self, template: &str) -> String {
        let mut text = template.replace("{move}", &self.movement());
        for action in ["interact", "dash", "extract", "build", "ability"] {
            text = text.replace(&format!("{{{action}}}"), &format!("[{{{action}}}]"));
        }
        let bindings = if self.gamepad() {
            &self.gamepad
        } else {
            &self.keyboard
        };
        bindings.format_prompt(&text, self.family)
    }

    /// The movement controls as prompt tokens.
    fn movement(&self) -> String {
        if self.gamepad() {
            return "[LS]".to_owned();
        }
        // First keyboard axis of each: "W / S" and "A / D".
        let pair = |action: &str| {
            self.keyboard
                .display_name(action, DeviceFamily::KeyboardMouse)
                .and_then(|n| {
                    n.split_once(" / ")
                        .map(|(a, b)| (a.to_owned(), b.to_owned()))
                })
        };
        match (pair("move_z"), pair("move_x")) {
            (Some((up, down)), Some((left, right))) => format!("[{up}][{left}][{down}][{right}]"),
            _ => "[W][A][S][D]".to_owned(),
        }
    }

    /// Menu navigation hints ("[Enter] select   [Esc] back").
    pub fn menu_hints(&self) -> String {
        let (bindings, family) = if self.gamepad() {
            (&self.gamepad, self.family)
        } else {
            (&self.keyboard, DeviceFamily::KeyboardMouse)
        };
        let name = |action: &str, fallback: &str| {
            bindings
                .display_name(action, family)
                .unwrap_or_else(|| fallback.to_owned())
        };
        format!(
            "[{}] select    [{}] back",
            name("ui_activate", "Enter"),
            name("ui_cancel", "Esc")
        )
    }
}

/// The atlas glyph for a prompt token, when it has one: controller face
/// buttons by family, shoulders, sticks, the d-pad, start/select and mouse
/// buttons. Keyboard keys have none (they are drawn as key caps).
pub fn glyph(label: &str, family: DeviceFamily) -> Option<&'static str> {
    use DeviceFamily::*;
    let face = match (family, label) {
        (Xbox | GenericGamepad, "A" | "South") => Some("xbox-a"),
        (Xbox | GenericGamepad, "B" | "East") => Some("xbox-b"),
        (Xbox | GenericGamepad, "X" | "West") => Some("xbox-x"),
        (Xbox | GenericGamepad, "Y" | "North") => Some("xbox-y"),
        (Nintendo, "A") => Some("nin-a"),
        (Nintendo, "B") => Some("nin-b"),
        (Nintendo, "X") => Some("nin-x"),
        (Nintendo, "Y") => Some("nin-y"),
        (_, "Cross") => Some("ps-cross"),
        (_, "Circle") => Some("ps-circle"),
        (_, "Square") => Some("ps-square"),
        (_, "Triangle") => Some("ps-triangle"),
        _ => None,
    };
    if face.is_some() {
        return face;
    }
    Some(match label {
        "LB" | "L1" | "L" | "Left Bumper" => "pad-lb",
        "RB" | "R1" | "R" | "Right Bumper" => "pad-rb",
        "LT" | "L2" | "ZL" | "Left Trigger" => "pad-lt",
        "RT" | "R2" | "ZR" | "Right Trigger" => "pad-rt",
        "LS" | "L3" | "Left Stick X" | "Left Stick Y" => "pad-ls",
        "RS" | "R3" | "Right Stick X" | "Right Stick Y" => "pad-rs",
        "Menu" | "Options" | "Plus" | "Start" => "pad-start",
        "View" | "Create" | "Minus" | "Select" => "pad-select",
        "Left Mouse" => "mouse-left",
        "Right Mouse" => "mouse-right",
        "Middle Mouse" => "mouse-middle",
        l if l.starts_with("D-Pad") => "pad-dpad",
        _ => return None,
    })
}

/// Short key-cap text for a keyboard key label.
pub fn keycap_text(label: &str) -> String {
    match label {
        "Up" => "↑".into(),
        "Down" => "↓".into(),
        "Left" => "←".into(),
        "Right" => "→".into(),
        "Left Shift" | "Right Shift" => "Shift".into(),
        "Left Ctrl" | "Right Ctrl" => "Ctrl".into(),
        "Left Alt" | "Right Alt" => "Alt".into(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_follow_the_device_family() {
        let mut prompts = Prompts::default();
        assert_eq!(prompts.format("press {interact}"), "press [E]");
        assert_eq!(prompts.format("{move} to walk"), "[W][A][S][D] to walk");
        assert_eq!(glyph("E", DeviceFamily::KeyboardMouse), None);
        prompts.family = DeviceFamily::Xbox;
        assert_eq!(prompts.format("press {interact}"), "press [A]");
        assert_eq!(prompts.format("{move}"), "[LS]");
        assert_eq!(glyph("A", DeviceFamily::Xbox), Some("xbox-a"));
        assert!(prompts.menu_hints().contains("[A]"));
        prompts.family = DeviceFamily::PlayStation;
        assert_ne!(prompts.format("{dash}"), prompts.format("{interact}"));
    }

    #[test]
    fn rebinds_reach_prompts_and_split_by_device() {
        let mut all = defaults();
        all.rebind(
            "interact",
            InputDevice::KeyboardMouse,
            0,
            Binding::key(Key::KeyG),
        );
        let mut prompts = Prompts::default();
        prompts.set_bindings(&all);
        assert_eq!(prompts.format("press {interact}"), "press [G]");
        let pad = for_device(&all, InputDevice::Gamepad);
        assert!(
            pad.actions["interact"]
                .iter()
                .all(|b| b.device() == Some(InputDevice::Gamepad))
        );
        assert_eq!(
            for_device(&defaults(), InputDevice::KeyboardMouse).actions,
            keyboard().actions
        );
    }
}
