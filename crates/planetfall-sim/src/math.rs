//! Minimal 2D vector helpers on the XZ ground plane. `[x, z]`, +Z points "down" the screen.

pub type V2 = [f32; 2];

pub fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}

pub fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

pub fn scale(a: V2, s: f32) -> V2 {
    [a[0] * s, a[1] * s]
}

pub fn dot(a: V2, b: V2) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

pub fn length(a: V2) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

pub fn distance(a: V2, b: V2) -> f32 {
    length(sub(a, b))
}

pub fn distance_sq(a: V2, b: V2) -> f32 {
    let d = sub(a, b);
    d[0] * d[0] + d[1] * d[1]
}

/// Unit vector, or zero for (near) zero input.
pub fn normalize(a: V2) -> V2 {
    let l = length(a);
    if l > 1e-6 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0]
    }
}

/// Clamp a stick/keyboard vector to unit length, keeping analog magnitudes below 1.
pub fn clamp_unit(a: V2) -> V2 {
    let l = length(a);
    if l > 1.0 { scale(a, 1.0 / l) } else { a }
}

/// Yaw that rotates a model facing +Z toward `dir` (matches `Mat4::rotation_y`).
pub fn yaw_of(dir: V2) -> f32 {
    dir[0].atan2(dir[1])
}

pub fn from_yaw(yaw: f32) -> V2 {
    [yaw.sin(), yaw.cos()]
}

pub fn rotate(a: V2, angle: f32) -> V2 {
    let (s, c) = angle.sin_cos();
    [a[0] * c + a[1] * s, -a[0] * s + a[1] * c]
}

/// Shortest signed angle from `from` to `to`.
pub fn angle_delta(from: f32, to: f32) -> f32 {
    let mut d = (to - from) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// Turn `current` toward `target` by at most `max_step` radians.
pub fn turn_toward(current: f32, target: f32, max_step: f32) -> f32 {
    let d = angle_delta(current, target);
    current + d.clamp(-max_step, max_step)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_round_trips() {
        for dir in [[0.0, 1.0], [1.0, 0.0], [-0.6, -0.8]] {
            let back = from_yaw(yaw_of(dir));
            assert!(distance(back, dir) < 1e-5);
        }
    }

    #[test]
    fn angle_delta_is_shortest() {
        assert!((angle_delta(3.0, -3.0) - (std::f32::consts::TAU - 6.0)).abs() < 1e-5);
        assert!((turn_toward(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
    }
}
