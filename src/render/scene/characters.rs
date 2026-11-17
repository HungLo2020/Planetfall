//! The player and the horde, as skinned characters. Enemy models, clips and
//! held weapons come from content data.

use super::*;
use crate::render::effects::outline;

impl Painter<'_, '_> {
    #[allow(
        clippy::too_many_arguments,
        reason = "pose inputs read straight from game state"
    )]
    pub(super) fn astronaut(
        &mut self,
        class: ClassId,
        pos: V2,
        y: f32,
        yaw: f32,
        walk: f32,
        moving: bool,
        flash: f32,
        weapon: Option<WeaponId>,
    ) {
        let root = at(pos, y) * Mat4::rotation_y(yaw);
        let mut layers = vec![if moving {
            Layer::full("walk", walk)
        } else {
            Layer::full("idle", self.time)
        }];
        if weapon.is_some() {
            layers.push(Layer::full("aim", 0.0));
        }
        let posed = self.suit(class, root, &layers, flash, Some(outline::PLAYER));
        if let Some(weapon) = weapon {
            let info = weapon.info();
            let k = info.model_scale;
            // Held in the right hand: follow the arm joint, then level the gun.
            let arm = posed.bone("arm_r").unwrap_or(Mat4::IDENTITY);
            let gun = root
                * arm
                * t(0.0, -0.62, 0.05)
                * Mat4::rotation_x(1.2)
                * s(k, k, k)
                * Mat4::rotation_y(info.model_yaw.to_radians());
            self.model(&info.model, gun);
        }
        // Suit lamp so the player always reads in dark caves.
        self.point(
            [pos[0], y + 3.6, pos[1] + 0.6],
            [0.85, 0.92, 1.0],
            1.15,
            10.0,
        );
    }
}

impl Painter<'_, '_> {
    /// The astronaut in a class's colours: the suit takes a share of the
    /// class colour and the visor and lamps glow in it.
    pub(super) fn suit(
        &mut self,
        class: ClassId,
        root: Mat4,
        layers: &[Layer],
        flash: f32,
        outline: Option<u8>,
    ) -> Posed {
        let c = class.info().color;
        let tint = mix3([1.0; 3], c, 0.45);
        self.character_tinted(names::ASTRONAUT, root, layers, flash, outline, tint, c)
    }
}

pub(super) fn enemies(p: &mut Painter<'_, '_>, run: &Run) {
    for e in &run.enemies {
        let info = e.kind.info();
        let rise = if e.spawn_ticks > 0 {
            -(e.spawn_ticks as f32 / 24.0) * 2.2
        } else {
            0.0
        };
        let root = at(e.pos, rise) * Mat4::rotation_y(e.yaw);
        let mut glow = if e.hit_ticks > 0 {
            e.hit_ticks as f32 / 6.0 * 0.9
        } else {
            0.0
        };
        let lunge = e.attack_ticks as f32 / 14.0;
        let mut layers = vec![Layer::full("walk", e.walk)];
        let character = p.skins.character(&info.model);
        // Body-only clips (a bloater's pulse) layer over the gait.
        if character.has("pulse") {
            layers.push(Layer::full("pulse", p.time + e.id as f32 * 0.37));
        }
        if lunge > 0.0 && character.has("attack") {
            layers.push(Layer {
                clip: "attack",
                time: 0.0,
                weight: lunge,
            });
        }
        // Exploders flash faster the closer they get.
        if info.behavior == EnemyBehavior::Exploder {
            let danger = (1.0 - (e.pos.distance(run.player().pos) / 5.0)).clamp(0.0, 1.0);
            glow += danger * 0.4 * ((p.time * 14.0).sin() * 0.5 + 0.5);
        }
        let style = if info.behavior == EnemyBehavior::Exploder {
            outline::EXPLODER
        } else {
            outline::ENEMY
        };
        p.character(&info.model, root, &layers, glow, Some(style));
    }
    for c in &run.corpses {
        let info = c.kind.info();
        // Exploders leave nothing behind.
        if info.behavior == EnemyBehavior::Exploder {
            continue;
        }
        let k = (c.ticks as f32 / 14.0).min(1.0);
        let sink = ((c.ticks as f32 - 30.0) / 20.0).clamp(0.0, 1.0) * 1.2;
        let root = at(c.pos, -sink) * Mat4::rotation_y(c.yaw) * Mat4::rotation_x(k * 1.5);
        p.character(&info.model, root, &[Layer::full("idle", 0.0)], 0.0, None);
    }
}
