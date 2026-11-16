//! Input: keyboard/mouse actions installed on the window's `InputState`, plus
//! controllers through `quasar-input`'s `GamepadHub` and gilrs backend. Both
//! feed the same named actions; edges are latched until a fixed tick uses them.

use planetfall_sim::{Command, GameInput};
use quasar_input::{
    Binding, Bindings, FrameInput, GamepadAxis, GamepadButton, GamepadHub, GilrsGamepads,
    InputState, Key, PlayerId,
};
use quasar_ui::{UiBindings, UiInput, from_frame_input};

const PLAYER: PlayerId = PlayerId::new(0);

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
pub fn keyboard_bindings() -> Bindings {
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
        .bind("pause", Binding::key(Key::Escape));
    ui_actions(&mut b, false);
    b
}

fn gamepad_bindings() -> Bindings {
    let mut b = Bindings::default();
    b.bind("move_x", Binding::gamepad_axis(GamepadAxis::LeftStickX))
        .bind(
            "move_z",
            Binding::gamepad_axis_scaled(GamepadAxis::LeftStickY, -1.0),
        )
        .bind("interact", Binding::gamepad_button(GamepadButton::South))
        .bind("dash", Binding::gamepad_button(GamepadButton::East))
        .bind("dash", Binding::gamepad_button(GamepadButton::RightBumper))
        .bind("extract", Binding::gamepad_button(GamepadButton::North))
        .bind("pause", Binding::gamepad_button(GamepadButton::Start));
    ui_actions(&mut b, true);
    b
}

#[derive(Default, Clone, Copy)]
struct Latched {
    interact: bool,
    dash: bool,
    extract: bool,
}

pub struct Controls {
    hub: GamepadHub,
    gilrs: Option<GilrsGamepads>,
    latched: Latched,
    pub command: Option<Command>,
    /// The last device that produced input, for button labels.
    pub gamepad_active: bool,
}

impl Controls {
    pub fn new() -> Self {
        let mut hub = GamepadHub::new();
        let _ = hub.add_player(PLAYER);
        if let Some(p) = hub.player_mut(PLAYER) {
            let _ = p.input.set_bindings(gamepad_bindings());
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
            gamepad_active: false,
        }
    }

    fn pad(&self) -> Option<&InputState> {
        self.hub.player(PLAYER).map(|p| &p.input)
    }

    /// Poll controllers and latch this frame's edges. Call once per frame.
    pub fn begin_frame(&mut self, keyboard: &FrameInput) {
        if let Some(g) = &mut self.gilrs {
            let _ = g.poll(&mut self.hub);
        }
        let pad_used = self.pad().is_some_and(|p| {
            [
                "move_x",
                "move_z",
                "interact",
                "dash",
                "extract",
                "pause",
                "ui_up",
                "ui_down",
                "ui_activate",
            ]
            .iter()
            .any(|a| {
                let s = p.action(a);
                s.pressed || s.value.abs() > 0.4
            })
        });
        let key_used = [
            "move_x",
            "move_z",
            "interact",
            "dash",
            "extract",
            "pause",
            "ui_activate",
        ]
        .iter()
        .any(|a| keyboard.action(a).pressed || keyboard.action(a).value != 0.0)
            || keyboard.mouse_pressed(quasar_input::MouseButton::Left);
        if pad_used {
            self.gamepad_active = true;
        } else if key_used {
            self.gamepad_active = false;
        }
    }

    pub fn latch_gameplay(&mut self, keyboard: &FrameInput) {
        let pad = |name: &str| self.pad().is_some_and(|p| p.action(name).pressed);
        let interact = keyboard.action("interact").pressed || pad("interact");
        let dash = keyboard.action("dash").pressed || pad("dash");
        let extract = keyboard.action("extract").pressed || pad("extract");
        self.latched.interact |= interact;
        self.latched.dash |= dash;
        self.latched.extract |= extract;
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
        let movement = [
            (keyboard.action("move_x").value + pad("move_x")).clamp(-1.0, 1.0),
            (keyboard.action("move_z").value + pad("move_z")).clamp(-1.0, 1.0),
        ];
        let latched = std::mem::take(&mut self.latched);
        GameInput {
            movement,
            interact: latched.interact,
            dash: latched.dash,
            call_extraction: latched.extract,
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
