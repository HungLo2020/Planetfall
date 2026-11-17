//! Input: keyboard/mouse actions installed on the window's `InputState`, plus
//! controllers through `quasar-input`'s `GamepadHub` and gilrs backend. Both
//! feed the same named actions (see `bindings.rs`). Controller edges are
//! latched until a fixed tick uses them; the window already buffers keyboard
//! edges between ticks.

use crate::bindings::{self, Prompts};
use planetfall_sim::math::V2;
use planetfall_sim::{Command, GameInput};
use quasar_input::{
    DeviceFamily, FrameInput, GamepadHub, GilrsGamepads, InputDevice, InputState, PlayerId,
};
use quasar_ui::{UiBindings, UiInput, from_frame_input};

const PLAYER: PlayerId = PlayerId::new(0);

#[derive(Default, Clone, Copy)]
struct Latched {
    ability: bool,
    interact: bool,
    dash: bool,
    extract: bool,
    build: bool,
}

pub struct Controls {
    hub: GamepadHub,
    gilrs: Option<GilrsGamepads>,
    latched: Latched,
    pub command: Option<Command>,
    /// Button prompts for the device family that last produced input.
    pub prompts: Prompts,
}

impl Controls {
    pub fn new() -> Self {
        let mut hub = GamepadHub::new();
        let _ = hub.add_player(PLAYER);
        if let Some(p) = hub.player_mut(PLAYER) {
            let _ = p.input.set_bindings(bindings::gamepad());
        }

        let gilrs = match GilrsGamepads::new() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("planetfall: controllers unavailable ({e}); keyboard only");
                None
            }
        };
        Self {
            hub,
            gilrs,
            latched: Latched::default(),
            command: None,
            prompts: Prompts::default(),
        }
    }

    /// Install the player's bindings on the controller and in the prompts;
    /// returns the keyboard/mouse part for the window.
    pub fn set_bindings(&mut self, all: &quasar_input::Bindings) -> quasar_input::Bindings {
        if let Some(p) = self.hub.player_mut(PLAYER) {
            let _ = p
                .input
                .set_bindings(bindings::for_device(all, InputDevice::Gamepad));
        }
        self.prompts.set_bindings(all);
        bindings::for_device(all, InputDevice::KeyboardMouse)
    }

    /// Buttons pressed this frame on either device (rebind capture).
    pub fn pressed_buttons(&self, keyboard: &FrameInput) -> Vec<quasar_input::Button> {
        let mut pressed = keyboard.pressed_buttons();
        if let Some(p) = self.pad() {
            pressed.extend(p.pressed_buttons());
        }
        pressed
    }

    fn pad(&self) -> Option<&InputState> {
        self.hub.player(PLAYER).map(|p| &p.input)
    }

    /// Poll controllers and latch this frame's edges. Call once per frame.
    pub fn begin_frame(&mut self, keyboard: &FrameInput) {
        if let Some(g) = &mut self.gilrs {
            let _ = g.poll(&mut self.hub);
        }
        // Every input state stamps activity from one process-wide counter,
        // so the most recent device across the window and the hub decides
        // which button names the prompts use.
        let recent =
            quasar_input::most_recent_device([keyboard.last_activity(), self.hub.last_activity()]);
        let pad_used = recent == Some(InputDevice::Gamepad);
        let key_used = recent == Some(InputDevice::KeyboardMouse);
        if pad_used {
            self.prompts.family = self
                .hub
                .assignment(PLAYER)
                .and_then(|id| self.hub.device(id))
                .map_or(DeviceFamily::GenericGamepad, |info| {
                    info.fingerprint.family()
                });
        } else if key_used {
            self.prompts.family = DeviceFamily::KeyboardMouse;
        }
    }

    pub fn latch_gameplay(&mut self, keyboard: &FrameInput) {
        let pad = |name: &str| self.pad().is_some_and(|p| p.action(name).pressed);
        let interact = keyboard.action("interact").pressed || pad("interact");
        let dash = keyboard.action("dash").pressed || pad("dash");
        let extract = keyboard.action("extract").pressed || pad("extract");
        let build = keyboard.action("build").pressed || pad("build");
        let ability = keyboard.action("ability").pressed || pad("ability");
        self.latched.interact |= interact;
        self.latched.dash |= dash;
        self.latched.extract |= extract;
        self.latched.build |= build;
        self.latched.ability |= ability;
    }

    pub fn pause_pressed(&self, keyboard: &FrameInput) -> bool {
        keyboard.action("pause").pressed || self.pad().is_some_and(|p| p.action("pause").pressed)
    }

    /// Clear controller edges after the frame used them.
    pub fn end_frame(&mut self) {
        if let Some(p) = self.hub.player_mut(PLAYER) {
            p.input.consume_edges();
        }
    }

    /// Input for one fixed tick: held movement plus any latched edges.
    pub fn take_game_input(&mut self, keyboard: &FrameInput) -> GameInput {
        let pad = |name: &str| self.pad().map_or(0.0, |p| p.action(name).value);
        let movement = V2::new(
            (keyboard.action("move_x").value + pad("move_x")).clamp(-1.0, 1.0),
            (keyboard.action("move_z").value + pad("move_z")).clamp(-1.0, 1.0),
        );
        let latched = std::mem::take(&mut self.latched);
        GameInput {
            movement,
            interact: latched.interact,
            dash: latched.dash,
            call_extraction: latched.extract,
            build: latched.build,
            ability: latched.ability,
            command: self.command.take(),
        }
    }

    pub fn clear(&mut self) {
        self.latched = Latched::default();
    }

    /// Menu navigation from keyboard/mouse and the controller.
    pub fn ui_input(&self, keyboard: &FrameInput) -> UiInput {
        let mut ui = from_frame_input(keyboard, &UiBindings::default());
        if let Some(p) = self.pad() {
            let pressed = |n: &str| p.action(n).pressed;
            ui.up |= pressed("ui_up");
            ui.down |= pressed("ui_down");
            ui.left |= pressed("ui_left");
            ui.right |= pressed("ui_right");
            ui.activate |= pressed("ui_activate");
            ui.cancel |= pressed("ui_cancel");
        }
        ui
    }
}
