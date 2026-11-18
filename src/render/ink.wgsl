// Ink and cel pass: comic-book outlines from the scene depth and soft
// banded shading, applied to the opaque scene before translucent draws,
// particles and post.

struct Params {
    size: vec2<f32>,
    near: f32,
    far: f32,
    // Pixel distance between depth samples (line weight).
    thickness: f32,
    // Relative depth jump for a silhouette, and relative slope change for a
    // crease.
    silhouette: f32,
    crease: f32,
    // Brightness bands and how strongly shading snaps to them (0..1).
    bands: f32,
    band_strength: f32,
    // Ink strength (0..1).
    ink_strength: f32,
    _pad: vec2<f32>,
    // Ink colour multiplied into the scene.
    ink: vec4<f32>,
};

@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var<uniform> p: Params;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn linear_depth(d: f32) -> f32 {
    return p.near * p.far / (p.far - d * (p.far - p.near));
}

fn depth_at(c: vec2<i32>) -> f32 {
    let top = vec2<i32>(p.size) - vec2<i32>(1, 1);
    return textureLoad(depth_tex, clamp(c, vec2<i32>(0, 0), top), 0);
}

@fragment
fn fs_ink(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = vec2<i32>(pos.xy);
    var color = textureLoad(color_tex, c, 0);
    let raw = depth_at(c);

    // Cel bands: pull mid-range brightness toward a few soft steps. Bright
    // HDR (emissive, glows) is left alone so bloom still works.
    let l = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    if (l > 0.002 && l < 1.0 && raw < 1.0) {
        let x = l * p.bands;
        let step_up = smoothstep(0.38, 0.62, fract(x));
        let banded = (floor(x) + step_up) / p.bands;
        let k = mix(1.0, max(banded, 0.02) / l, p.band_strength);
        color = vec4<f32>(color.rgb * k, color.a);
    }

    if (raw >= 1.0) {
        return color;
    }
    // Ink: silhouettes where depth jumps, creases where its slope bends.
    let t = max(1, i32(p.thickness));
    let z = linear_depth(raw);
    let zl = linear_depth(depth_at(c + vec2<i32>(-t, 0)));
    let zr = linear_depth(depth_at(c + vec2<i32>(t, 0)));
    let zu = linear_depth(depth_at(c + vec2<i32>(0, -t)));
    let zd = linear_depth(depth_at(c + vec2<i32>(0, t)));
    // Only the nearer side of a jump draws the line, so it hugs the object.
    let jump = max(max(zl - z, zr - z), max(zu - z, zd - z)) / z;
    let bend = max(abs(zl + zr - 2.0 * z), abs(zu + zd - 2.0 * z)) / z;
    let edge = max(
        smoothstep(p.silhouette, p.silhouette * 2.0, jump),
        smoothstep(p.crease, p.crease * 2.5, bend) * 0.8,
    );
    let inked = color.rgb * p.ink.rgb;
    return vec4<f32>(mix(color.rgb, inked, edge * p.ink_strength), color.a);
}
