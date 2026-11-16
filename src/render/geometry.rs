//! Procedural GPU resources: a unit cube and small tiling textures.

use quasar_frame::{Gpu, GraphicsResult, texture_2d};
use quasar_render_3d::{Mesh, Vertex};
use vulkanic_gal::{TexelCopyBufferLayout, Texture, TextureFormat, TextureUsages, TextureView};

pub struct Textures3D {
    _white: Texture,
    pub white: TextureView,
    _rock: Texture,
    pub rock_normal: TextureView,
    _metal: Texture,
    pub metal_normal: TextureView,
}

impl Textures3D {
    pub fn new(gal: &Gpu) -> GraphicsResult<Self> {
        let (_white, white) = upload_rgba(gal, 1, 1, &[255; 4])?;
        let (_rock, rock_normal) = upload_normal(gal, 64, |u, v| {
            // Chunky voxel-like rock: layered value noise with blocky steps.
            let n = |x: f32, y: f32, f: f32| value_noise(x * f, y * f);
            let h = n(u, v, 4.0) * 0.6 + n(u, v, 9.0) * 0.3 + n(u, v, 17.0) * 0.1;
            (h * 6.0).floor() / 6.0 * 0.05
        })?;
        let (_metal, metal_normal) = upload_normal(gal, 64, |u, v| {
            // Deck plating: panel seams and rivets.
            let seam = |t: f32| {
                let f = (t * 2.0).fract();
                if !(0.03..=0.97).contains(&f) {
                    -0.02
                } else {
                    0.0
                }
            };
            let rivet = {
                let (fx, fy) = ((u * 2.0).fract(), (v * 2.0).fract());
                let d = ((fx - 0.1).powi(2) + (fy - 0.1).powi(2)).sqrt();
                if d < 0.04 { 0.012 } else { 0.0 }
            };
            seam(u) + seam(v) + rivet
        })?;
        Ok(Self {
            _white,
            white,
            _rock,
            rock_normal,
            _metal,
            metal_normal,
        })
    }
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Tileable (period 64 lattice) value noise in 0..1.
fn value_noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let p = |a: i32, b: i32| hash(a.rem_euclid(64), b.rem_euclid(64));
    let a = p(xi, yi) + (p(xi + 1, yi) - p(xi, yi)) * s(fx);
    let b = p(xi, yi + 1) + (p(xi + 1, yi + 1) - p(xi, yi + 1)) * s(fx);
    a + (b - a) * s(fy)
}

pub fn tile_hash(x: i32, y: i32) -> f32 {
    hash(x, y)
}

fn upload_normal(
    gal: &Gpu,
    size: u32,
    height: impl Fn(f32, f32) -> f32,
) -> GraphicsResult<(Texture, TextureView)> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let step = 1.0 / size as f32;
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 * step, y as f32 * step);
            let wrap = |t: f32| t.rem_euclid(1.0);
            let dx = (height(wrap(u + step), v) - height(wrap(u - step), v)) * size as f32 * 0.5;
            let dy = (height(u, wrap(v + step)) - height(u, wrap(v - step))) * size as f32 * 0.5;
            let len = (dx * dx + dy * dy + 1.0).sqrt();
            for n in [-dx / len, -dy / len, 1.0 / len] {
                pixels.push(((n * 0.5 + 0.5) * 255.0).round() as u8);
            }
            pixels.push(255);
        }
    }
    upload_rgba(gal, size, size, &pixels)
}

fn upload_rgba(
    gal: &Gpu,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> GraphicsResult<(Texture, TextureView)> {
    let texture = texture_2d(
        &gal.device,
        "Planetfall procedural texture",
        width,
        height,
        TextureFormat::Rgba8Unorm,
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
    )?;
    gal.queue.write_texture(
        texture.as_image_copy(),
        pixels,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        texture.size(),
    );
    let view = texture.create_view(&Default::default());
    Ok((texture, view))
}

/// Unit cube spanning [-1, 1] with per-face UVs.
pub fn cube(gal: &Gpu) -> GraphicsResult<Mesh> {
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0., 0., 1.],
            [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
        ),
        (
            [0., 0., -1.],
            [
                [1., -1., -1.],
                [-1., -1., -1.],
                [-1., 1., -1.],
                [1., 1., -1.],
            ],
        ),
        (
            [1., 0., 0.],
            [[1., -1., 1.], [1., -1., -1.], [1., 1., -1.], [1., 1., 1.]],
        ),
        (
            [-1., 0., 0.],
            [
                [-1., -1., -1.],
                [-1., -1., 1.],
                [-1., 1., 1.],
                [-1., 1., -1.],
            ],
        ),
        (
            [0., 1., 0.],
            [[-1., 1., 1.], [1., 1., 1.], [1., 1., -1.], [-1., 1., -1.]],
        ),
        (
            [0., -1., 0.],
            [
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., -1., 1.],
                [-1., -1., 1.],
            ],
        ),
    ];
    let uvs = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corners) in faces {
        let base = vertices.len() as u32;
        for (position, uv) in corners.into_iter().zip(uvs) {
            vertices.push(Vertex {
                position,
                normal,
                uv,
            });
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::upload(gal, &vertices, &indices)
}
