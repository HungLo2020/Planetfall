//! Ground-plane conventions on top of `quasar-math` (glam).
//!
//! The simulation works on the XZ plane as [`V2`] = `(x, z)`, with +Z pointing
//! "down" the screen. Planetfall models face +Z, so a yaw here is the angle
//! `Mat4::from_rotation_y` needs to turn a model toward a direction. That is
//! Quasar's yaw plus π (Quasar yaws face −Z); the helpers below keep that
//! difference in one place. Everything else is plain glam.

pub use quasar_math::{Vec2, angle_delta, glam::FloatExt};

/// A point or direction on the ground plane: `(x, z)`.
pub type V2 = Vec2;

/// Yaw that rotates a +Z-facing model toward `dir`.
pub fn yaw_of(dir: V2) -> f32 {
    dir.x.atan2(dir.y)
}

/// Unit direction a +Z-facing model looks along at `yaw`.
pub fn from_yaw(yaw: f32) -> V2 {
    V2::new(yaw.sin(), yaw.cos())
}

/// Rotate `a` by `angle` in the same sense as yaw.
pub fn rotate(a: V2, angle: f32) -> V2 {
    V2::from_angle(-angle).rotate(a)
}

/// Turn `current` toward `target` by at most `max_step` radians.
pub fn turn_toward(current: f32, target: f32, max_step: f32) -> f32 {
    quasar_math::turn_towards(current, target, max_step)
}

/// Unsigned angle between two unit vectors, in radians.
pub fn angle_between(a: V2, b: V2) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_round_trips_and_rotates_with_yaw() {
        for dir in [V2::new(0.0, 1.0), V2::new(1.0, 0.0), V2::new(-0.6, -0.8)] {
            assert!(from_yaw(yaw_of(dir)).distance(dir) < 1e-5);
            let turned = rotate(dir, 0.3);
            assert!((yaw_of(turned) - yaw_of(dir) - 0.3).abs() % std::f32::consts::TAU < 1e-4);
        }
    }

    #[test]
    fn turning_is_bounded() {
        assert!((turn_toward(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
        assert!((angle_delta(3.0, -3.0) - (std::f32::consts::TAU - 6.0)).abs() < 1e-5);
    }
}
