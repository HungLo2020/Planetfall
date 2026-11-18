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
        // Facing, the walk/idle mix and the lean come from the smoothed
        // pose; `yaw` and `moving` are the raw values it eases toward.
        let _ = (yaw, moving);
        let pose = self.pose;
        let root = at(pos, y) * Mat4::rotation_y(pose.yaw) * Mat4::rotation_x(0.11 * pose.lean);
        let mut layers = vec![Layer::full("idle", self.time)];
        if pose.motion > 0.01 {
            layers.push(Layer {
                clip: "walk",
                time: walk,
                weight: pose.motion,
            });
        }
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
    /// A class suit; the visor and lamps glow in the class colour.
    pub(super) fn suit(
        &mut self,
        class: ClassId,
        root: Mat4,
        layers: &[Layer],
        flash: f32,
        outline: Option<u8>,
    ) -> Posed {
        // The suits are painted in their class colours; only the glow
        // (visor and lamps) takes the class colour here.
        let c = class.info().color;
        let model = &class.info().model;
        self.character_tinted(model, root, layers, flash, outline, [1.0; 3], c)
    }
}

pub(super) fn enemies(p: &mut Painter<'_, '_>, run: &Run) {
    for e in &run.enemies {
        let info = e.kind.info();
        // Burrowers underground: a churning mound of rubble.
        if e.burrowed {
            let k = info.radius * 1.6;
            let wobble = (p.time * 12.0 + e.id as f32).sin() * 0.08;
            p.cube(
                at(e.pos, 0.1 + wobble) * Mat4::rotation_y(p.time * 3.0) * s(k, 0.3, k),
                [0.32, 0.26, 0.2],
            );
            for i in 0..3 {
                let a = p.time * 4.0 + i as f32 * 2.1;
                let q = e.pos + V2::new(a.cos(), a.sin()) * k * 0.6;
                p.cube(at(q, 0.25) * s(0.18, 0.18, 0.18), [0.4, 0.33, 0.25]);
            }
            continue;
        }
        let rise = if e.spawn_ticks > 0 {
            -(e.spawn_ticks as f32 / 24.0) * 2.2
        } else if e.flying() {
            // Flyers hover above the floor (and over rock).
            1.4 + (p.time * 5.0 + e.id as f32).sin() * 0.2
        } else {
            0.0
        };
        // Elites stand a quarter taller.
        let k = info.scale * if e.elite.is_some() { 1.25 } else { 1.0 };
        let root = at(e.pos, rise) * Mat4::rotation_y(e.yaw) * s(k, k, k);
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
        let style = if e.elite.is_some() {
            outline::ELITE
        } else if info.behavior == EnemyBehavior::Exploder {
            outline::EXPLODER
        } else {
            outline::ENEMY
        };
        // Element statuses: frost, flame flicker, acid sheen.
        let mut tint = info.tint.unwrap_or([1.0; 3]);
        if e.wet > 0 {
            tint = [tint[0] * 0.8, tint[1] * 0.9, tint[2] * 1.15];
        }
        if e.frozen > 0 {
            tint = [0.65, 0.9, 1.35];
        } else if e.chill > 0.05 {
            tint = mix3(tint, [0.75, 0.9, 1.2], e.chill.min(1.0));
        }
        if e.corroded > 0 {
            tint = [tint[0] * 0.75, tint[1] * 1.15, tint[2] * 0.6];
        }
        if e.burning > 0 {
            glow += 0.25 + 0.15 * (p.time * 18.0 + e.id as f32).sin();
        }
        // Elite affixes tint the body and light the eyes.
        let mut eyes = [1.0f32; 3];
        match e.elite {
            Some(Affix::Armored) => {
                tint = [tint[0] * 0.7, tint[1] * 0.75, tint[2] * 0.85];
                eyes = [1.0, 0.85, 0.3];
            }
            Some(Affix::Swift) => eyes = [0.3, 1.0, 1.6],
            Some(Affix::Frenzied) => {
                tint = [tint[0] * 1.2, tint[1] * 0.8, tint[2] * 0.75];
                eyes = [2.0, 0.3, 0.2];
            }
            Some(Affix::Splitting) => {
                tint = [tint[0] * 1.1, tint[1] * 1.0, tint[2] * 1.3];
                eyes = [1.4, 0.5, 1.8];
            }
            Some(Affix::Regenerating) => {
                tint = [tint[0] * 0.8, tint[1] * 1.25, tint[2] * 0.8];
                eyes = [0.4, 1.8, 0.5];
            }
            None => {}
        }
        p.character_tinted(&info.model, root, &layers, glow, Some(style), tint, eyes);
        // Frozen solid: a translucent shell of ice.
        if e.frozen > 0 {
            let r = info.radius * k * 1.5;
            let h = 1.9 * k * info.scale.max(0.6) / info.scale.max(0.01);
            p.ghost(
                at(e.pos, h * 0.45) * Mat4::rotation_y(e.id as f32) * s(r, h * 0.55, r),
                [0.6, 0.85, 1.0],
                0.35,
            );
        }
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
