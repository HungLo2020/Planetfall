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

    /// `{action}` → `[name]` for the active family; `{move}` names the
    /// movement controls as a whole.
    pub fn format(&self, template: &str) -> String {
        let movement = if self.gamepad() {
            "the left stick"
        } else {
            "WASD"
        };
        let mut text = template.replace("{move}", movement);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_follow_the_device_family() {
        let mut prompts = Prompts::default();
        assert_eq!(prompts.format("press {interact}"), "press [E]");
        assert_eq!(prompts.format("{move} to walk"), "WASD to walk");
        prompts.family = DeviceFamily::Xbox;
        assert_eq!(prompts.format("press {interact}"), "press [A]");
        assert_eq!(prompts.format("{move}"), "the left stick");
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
