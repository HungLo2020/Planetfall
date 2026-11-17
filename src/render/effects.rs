//! Event-driven surface effects on Quasar's GPU passes:
//!
//! - **Particles** (`ParticleSystem`): blood, rock dust, ore glints, wall
//!   impacts, explosions and pod-landing dust, emitted from the game's
//!   events. An additive system carries the glowing bursts and an
//!   alpha-blended one the dust and blood. They are presentation only: the
//!   simulation never sees them.
//! - **Decals** (`DecalRenderer`): scorch marks under explosions and blood
//!   splats where enemies fall, projected onto whatever the opaque pass drew
//!   (floor voxels, rock faces). They last for the drop.
//! - **Outlines** (`OutlineRenderer`): rings and a fill over the parts of the
//!   player and the horde hidden behind rock, so nothing fights unseen.

use planetfall_sim::content::{AbilityKind, Element, EnemyBehavior, Material, PlanetId};
use planetfall_sim::events::GameEvent;
use planetfall_sim::math::V2;
use planetfall_sim::{Game, Phase};
use quasar_frame::{Gpu, GraphicsResult, RenderTarget, texture_2d};
use quasar_render_3d::{
    BlendMode, Decal, DecalRenderer, Mat4, OutlineDraw, OutlineRenderer, OutlineStyle,
    ParticleBurst, ParticleCamera, ParticleSettings, ParticleSystem, SceneTarget,
};
use std::collections::VecDeque;
use vulkanic_gal::{
    CommandBuffer, TexelCopyBufferLayout, Texture, TextureFormat, TextureUsages, TextureView,
};

/// Outline styles, indexed by `SkinRequest::outline`.
pub mod outline {
    use quasar_render_3d::OutlineStyle;

    pub const PLAYER: u8 = 0;
    pub const ENEMY: u8 = 1;
    pub const EXPLODER: u8 = 2;

    /// Only hidden parts are marked: the visible silhouette stays clean.
    pub fn styles() -> [OutlineStyle; 3] {
        let hidden = |color: [f32; 3], fill: f32| OutlineStyle {
            outline: [0.0; 4],
            occluded_outline: [color[0], color[1], color[2], 0.9],
            occluded_fill: [color[0], color[1], color[2], fill],
            width: 2.0,
        };
        [
            hidden([0.35, 0.85, 1.0], 0.3),
            hidden([1.0, 0.25, 0.2], 0.18),
            hidden([1.0, 0.6, 0.15], 0.22),
        ]
    }
}

/// Persistent marks per drop (oldest are replaced first).
const MAX_MARKS: usize = 96;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkKind {
    Scorch,
    Splat,
}

#[derive(Clone, Copy)]
struct Mark {
    kind: MarkKind,
    pos: V2,
    size: f32,
    yaw: f32,
    tint: [f32; 4],
}

pub struct Effects {
    glow: ParticleSystem,
    dust: ParticleSystem,
    decals: DecalRenderer,
    outlines: OutlineRenderer,
    styles: [OutlineStyle; 3],
    marks: VecDeque<Mark>,
    /// The drop the marks belong to.
    drop: Option<(PlanetId, u32)>,
    _scorch: Texture,
    scorch: TextureView,
    _splat: Texture,
    splat: TextureView,
    /// Varies mark rotation without touching the simulation's RNG.
    counter: u32,
}

impl Effects {
    pub fn new(gal: &Gpu) -> GraphicsResult<Self> {
        let glow = ParticleSystem::new(
            gal,
            8192,
            ParticleSettings {
                gravity: [0.0, -7.0, 0.0],
                drag: 1.6,
                blend: BlendMode::Additive,
            },
            0x9e37_79b9,
        )?;
        let dust = ParticleSystem::new(
            gal,
            4096,
            ParticleSettings {
                gravity: [0.0, -11.0, 0.0],
                drag: 1.2,
                blend: BlendMode::Alpha,
            },
            0x85eb_ca6b,
        )?;
        let (_scorch, scorch) = mark_texture(gal, 64, |d, n| {
            // Charred centre fading out through a ragged edge.
            let edge = 0.42 + n * 0.12;
            let a = (1.0 - (d / edge).powf(2.5)).clamp(0.0, 1.0);
            ([0.03, 0.025, 0.02], a * 0.85)
        })?;
        let (_splat, splat) = mark_texture(gal, 64, |d, n| {
            // A white blob with lobes; the tint gives it the blood colour.
            let edge = 0.3 + n * 0.18;
            let a = if d < edge { 1.0 } else { 0.0 };
            ([1.0, 1.0, 1.0], a * 0.8)
        })?;
        Ok(Self {
            glow,
            dust,
            decals: DecalRenderer::new(gal)?,
            outlines: OutlineRenderer::new(gal)?,
            styles: outline::styles(),
            marks: VecDeque::with_capacity(MAX_MARKS),
            drop: None,
            _scorch,
            scorch,
            _splat,
            splat,
            counter: 0,
        })
    }

    /// Forget the marks (a different game was loaded).
    pub fn clear(&mut self) {
        self.marks.clear();
        self.drop = None;
    }

    /// Turn this frame's events into particles and marks.
    pub fn observe(&mut self, game: &Game, events: &[GameEvent]) -> GraphicsResult<()> {
        self.observe_with(game, events, true)
    }

    /// Only the lasting marks of earlier events (offscreen renders replay
    /// the recent past so a single frame shows its scorches and splats).
    pub fn remember(&mut self, game: &Game, events: &[GameEvent]) -> GraphicsResult<()> {
        self.observe_with(game, events, false)
    }

    fn observe_with(
        &mut self,
        game: &Game,
        events: &[GameEvent],
        particles: bool,
    ) -> GraphicsResult<()> {
        let mut glow = Bursts::new(particles);
        let mut dust = Bursts::new(particles);
        let Some(run) = game.run.as_ref().filter(|_| {
            matches!(
                game.phase,
                Phase::Surface | Phase::Ascent { .. } | Phase::Lost { .. }
            )
        }) else {
            self.marks.clear();
            self.drop = None;
            return Ok(());
        };
        let drop = (run.planet, game.profile.stats.drops);
        if self.drop != Some(drop) {
            self.marks.clear();
            self.drop = Some(drop);
        }
        // Palette and blood colours are linear, like the voxel textures.
        let rock = run.planet.info().palette.rock;
        for event in events {
            match *event {
                GameEvent::EnemyKilled { kind, at } => {
                    let info = kind.info();
                    let blood = info.blood;
                    let exploder = info.behavior == EnemyBehavior::Exploder;
                    dust.emit(&ParticleBurst {
                        position: [at[0], 0.9, at[1]],
                        position_jitter: info.radius * 0.6,
                        count: if exploder { 10 } else { 18 },
                        direction: [0.0, 1.0, 0.0],
                        spread: 1.2,
                        speed: [2.0, 5.0],
                        life: [0.45, 0.9],
                        size: [0.16, 0.05],
                        color_start: [blood[0], blood[1], blood[2], 1.0],
                        color_end: [blood[0] * 0.6, blood[1] * 0.6, blood[2] * 0.6, 0.0],
                    })?;
                    if !exploder {
                        // Decals are not lit: darken to sit on shaded ground.
                        let tint = [blood[0] * 0.3, blood[1] * 0.3, blood[2] * 0.3, 0.55];
                        self.mark(MarkKind::Splat, at, 0.7 + info.radius * 1.3, tint);
                    }
                }
                GameEvent::EnemyHit { at } => {
                    glow.emit(&ParticleBurst {
                        position: [at[0], 1.0, at[1]],
                        position_jitter: 0.15,
                        count: 4,
                        direction: [0.0, 1.0, 0.0],
                        spread: 1.4,
                        speed: [2.0, 4.0],
                        life: [0.12, 0.25],
                        size: [0.09, 0.02],
                        color_start: [2.2, 1.6, 0.9, 1.0],
                        color_end: [1.0, 0.3, 0.1, 0.0],
                    })?;
                }
                GameEvent::MineHit { at, material } => {
                    dust.emit(&chips(at, 1.0, rock, 5, 0.12))?;
                    if let Some(m) = material {
                        glow.emit(&glints(at, 1.0, ore_glow(m), 4))?;
                    }
                }
                GameEvent::BlockBroken { at, material } => {
                    let mut burst = chips(at, 1.2, rock, 26, 0.22);
                    burst.position_jitter = 0.7;
                    burst.speed = [2.5, 6.5];
                    dust.emit(&burst)?;
                    dust.emit(&ParticleBurst {
                        position: [at[0], 0.6, at[1]],
                        position_jitter: 0.8,
                        count: 14,
                        direction: [0.0, 1.0, 0.0],
                        spread: 1.5,
                        speed: [0.6, 1.6],
                        life: [0.8, 1.6],
                        size: [0.35, 0.9],
                        color_start: [rock[0], rock[1], rock[2], 0.45],
                        color_end: [rock[0], rock[1], rock[2], 0.0],
                    })?;
                    if let Some(m) = material {
                        glow.emit(&glints(at, 1.2, ore_glow(m), 16))?;
                    }
                }
                GameEvent::Impact { weapon, at, height } => {
                    let c = weapon.info().color;
                    glow.emit(&ParticleBurst {
                        position: [at[0], height, at[1]],
                        position_jitter: 0.05,
                        count: 7,
                        direction: [0.0, 1.0, 0.0],
                        spread: std::f32::consts::PI,
                        speed: [1.5, 4.0],
                        life: [0.12, 0.3],
                        size: [0.08, 0.01],
                        color_start: [c[0] * 2.5, c[1] * 2.5, c[2] * 2.5, 1.0],
                        color_end: [c[0], c[1], c[2], 0.0],
                    })?;
                }
                GameEvent::Explosion { at, radius } => {
                    glow.emit(&ParticleBurst {
                        position: [at[0], 0.8, at[1]],
                        position_jitter: radius * 0.25,
                        count: 60,
                        direction: [0.0, 1.0, 0.0],
                        spread: 1.6,
                        speed: [3.0, 3.0 + radius * 2.5],
                        life: [0.25, 0.6],
                        size: [0.3, 0.05],
                        color_start: [3.0, 1.6, 0.5, 1.0],
                        color_end: [1.2, 0.2, 0.05, 0.0],
                    })?;
                    dust.emit(&ParticleBurst {
                        position: [at[0], 0.8, at[1]],
                        position_jitter: radius * 0.4,
                        count: 18,
                        direction: [0.0, 1.0, 0.0],
                        spread: 0.9,
                        speed: [0.8, 2.2],
                        life: [0.9, 1.8],
                        size: [0.5, 1.4],
                        color_start: [0.08, 0.07, 0.06, 0.55],
                        color_end: [0.05, 0.05, 0.05, 0.0],
                    })?;
                    self.mark(MarkKind::Scorch, at, radius * 2.3, [1.0, 1.0, 1.0, 0.9]);
                }
                GameEvent::PodLanded { at } => {
                    dust.emit(&ParticleBurst {
                        position: [at[0], 0.3, at[1]],
                        position_jitter: 1.2,
                        count: 70,
                        direction: [0.0, 1.0, 0.0],
                        spread: 1.45,
                        speed: [3.0, 7.0],
                        life: [0.8, 1.6],
                        size: [0.45, 1.2],
                        color_start: [rock[0], rock[1], rock[2], 0.6],
                        color_end: [rock[0], rock[1], rock[2], 0.0],
                    })?;
                    self.mark(MarkKind::Scorch, at, 6.0, [1.0, 1.0, 1.0, 0.8]);
                }
                GameEvent::AbilityUsed {
                    player,
                    kind,
                    from,
                    to,
                } => {
                    let c = run
                        .players
                        .get(player)
                        .map_or([1.0; 3], |p| p.class.info().color);
                    let hot = [c[0] * 2.4, c[1] * 2.4, c[2] * 2.4];
                    let length = from.distance(to);
                    let steps = ((length / 0.6) as u32).clamp(1, 24);
                    match kind {
                        AbilityKind::Bore | AbilityKind::Blink => {
                            // A streak of sparks along the path.
                            for n in 0..=steps {
                                let p = from.lerp(to, n as f32 / steps as f32);
                                glow.emit(&ParticleBurst {
                                    position: [p[0], 1.0, p[1]],
                                    position_jitter: 0.3,
                                    count: if kind == AbilityKind::Bore { 6 } else { 3 },
                                    direction: [0.0, 1.0, 0.0],
                                    spread: 1.4,
                                    speed: [1.0, 3.5],
                                    life: [0.2, 0.5],
                                    size: [0.12, 0.02],
                                    color_start: [hot[0], hot[1], hot[2], 1.0],
                                    color_end: [c[0], c[1], c[2], 0.0],
                                })?;
                            }
                            if kind == AbilityKind::Bore {
                                for n in 0..=steps {
                                    let p = from.lerp(to, n as f32 / steps as f32);
                                    dust.emit(&chips(p, 1.0, rock, 6, 0.2))?;
                                }
                            }
                        }
                        AbilityKind::Barrier | AbilityKind::Turret => {
                            glow.emit(&ParticleBurst {
                                position: [to[0], 0.4, to[1]],
                                position_jitter: if kind == AbilityKind::Barrier {
                                    1.6
                                } else {
                                    0.4
                                },
                                count: 30,
                                direction: [0.0, 1.0, 0.0],
                                spread: 0.8,
                                speed: [1.5, 4.0],
                                life: [0.3, 0.7],
                                size: [0.12, 0.02],
                                color_start: [hot[0], hot[1], hot[2], 1.0],
                                color_end: [c[0], c[1], c[2], 0.0],
                            })?;
                        }
                    }
                }
                GameEvent::Status { element, at } => {
                    let c = element_color(element);
                    glow.emit(&ParticleBurst {
                        position: [at[0], 1.0, at[1]],
                        position_jitter: 0.35,
                        count: 12,
                        direction: [0.0, 1.0, 0.0],
                        spread: 1.2,
                        speed: [1.0, 3.0],
                        life: [0.3, 0.6],
                        size: [0.12, 0.02],
                        color_start: [c[0], c[1], c[2], 1.0],
                        color_end: [c[0] * 0.4, c[1] * 0.4, c[2] * 0.4, 0.0],
                    })?;
                }
                GameEvent::Dashed => {
                    let p = run.player().pos;
                    dust.emit(&ParticleBurst {
                        position: [p[0], 0.2, p[1]],
                        position_jitter: 0.3,
                        count: 10,
                        direction: [0.0, 1.0, 0.0],
                        spread: 1.3,
                        speed: [0.8, 2.0],
                        life: [0.4, 0.8],
                        size: [0.25, 0.6],
                        color_start: [rock[0], rock[1], rock[2], 0.4],
                        color_end: [rock[0], rock[1], rock[2], 0.0],
                    })?;
                }
                _ => {}
            }
        }
        // Burning and corroded enemies shed embers and drips while it lasts.
        if particles {
            self.counter = self.counter.wrapping_add(1);
            for (n, e) in run
                .enemies
                .iter()
                .filter(|e| e.burning > 0 || e.corroded > 0)
                .take(40)
                .enumerate()
            {
                if !(self.counter as usize + n).is_multiple_of(4) {
                    continue;
                }
                let element = if e.burning > 0 {
                    Element::Fire
                } else {
                    Element::Acid
                };
                let c = element_color(element);
                glow.emit(&ParticleBurst {
                    position: [e.pos[0], 1.0, e.pos[1]],
                    position_jitter: 0.3,
                    count: 2,
                    direction: [0.0, 1.0, 0.0],
                    spread: 0.5,
                    speed: [0.6, 1.6],
                    life: [0.3, 0.6],
                    size: [0.1, 0.02],
                    color_start: [c[0], c[1], c[2], 1.0],
                    color_end: [c[0] * 0.3, c[1] * 0.3, c[2] * 0.3, 0.0],
                })?;
            }
        }
        for burst in &glow.bursts {
            self.glow.emit(burst)?;
        }
        for burst in &dust.bursts {
            self.dust.emit(burst)?;
        }
        Ok(())
    }

    fn mark(&mut self, kind: MarkKind, pos: V2, size: f32, tint: [f32; 4]) {
        if self.marks.len() >= MAX_MARKS {
            self.marks.pop_front();
        }
        self.counter = self.counter.wrapping_add(1);
        let yaw = (self.counter as f32 * 2.399_963).rem_euclid(std::f32::consts::TAU);
        self.marks.push_back(Mark {
            kind,
            pos,
            size,
            yaw,
            tint,
        });
    }

    /// Project the drop's marks onto the finished opaque scene.
    pub fn prepare_decals(
        &mut self,
        gal: &Gpu,
        scene: &SceneTarget,
        view_projection: Mat4,
    ) -> GraphicsResult<Option<CommandBuffer>> {
        if self.marks.is_empty() {
            return Ok(None);
        }
        // Scorches first so blood lands on top of burnt ground.
        let mut decals: Vec<Decal<'_>> = Vec::with_capacity(self.marks.len());
        for kind in [MarkKind::Scorch, MarkKind::Splat] {
            for m in self.marks.iter().filter(|m| m.kind == kind) {
                let texture = match kind {
                    MarkKind::Scorch => &self.scorch,
                    MarkKind::Splat => &self.splat,
                };
                // A flat box around the floor (y = 0): deep enough to catch
                // the ground, shallow enough to stay off wall tops.
                let transform = Mat4::translation(m.pos[0], 0.0, m.pos[1])
                    * Mat4::rotation_y(m.yaw)
                    * Mat4::scale(m.size, 1.2, m.size);
                decals.push(Decal {
                    tint: m.tint,
                    fade: 0.6,
                    ..Decal::new(transform, texture)
                });
            }
        }
        self.decals
            .prepare(gal, scene, view_projection, &decals, BlendMode::Alpha)
            .map(Some)
    }

    /// Advance and draw the particles (both systems).
    pub fn prepare_particles(
        &mut self,
        gal: &Gpu,
        target: &RenderTarget,
        camera: ParticleCamera,
        dt: f32,
    ) -> GraphicsResult<[CommandBuffer; 2]> {
        let dt = dt.clamp(0.0, 0.1);
        Ok([
            self.dust.prepare(gal, target, camera, dt)?,
            self.glow.prepare(gal, target, camera, dt)?,
        ])
    }

    /// Rings and fills over the hidden parts of outlined characters.
    pub fn prepare_outlines(
        &mut self,
        gal: &Gpu,
        scene: &SceneTarget,
        view_projection: Mat4,
        draws: &[OutlineDraw<'_>],
    ) -> GraphicsResult<Option<CommandBuffer>> {
        if draws.is_empty() {
            return Ok(None);
        }
        self.outlines
            .prepare(gal, scene, view_projection, draws, &self.styles)
            .map(Some)
    }

    pub fn release(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        self.glow.release(gal)?;
        self.dust.release(gal)?;
        self.decals.release(gal)?;
        self.outlines.release(gal)
    }
}

/// Bursts gathered while reading events (dropped when only marks are
/// wanted).
struct Bursts {
    enabled: bool,
    bursts: Vec<ParticleBurst>,
}

impl Bursts {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            bursts: Vec::new(),
        }
    }

    fn emit(&mut self, burst: &ParticleBurst) -> GraphicsResult<()> {
        if self.enabled {
            self.bursts.push(*burst);
        }
        Ok(())
    }
}

/// Rock chips knocked off a face.
fn chips(at: V2, height: f32, rock: [f32; 3], count: u32, size: f32) -> ParticleBurst {
    ParticleBurst {
        position: [at[0], height, at[1]],
        position_jitter: 0.2,
        count,
        direction: [0.0, 1.0, 0.0],
        spread: 1.3,
        speed: [1.5, 4.0],
        life: [0.35, 0.7],
        size: [size, size * 0.5],
        color_start: [rock[0], rock[1], rock[2], 1.0],
        color_end: [rock[0] * 0.8, rock[1] * 0.8, rock[2] * 0.8, 0.0],
    }
}

/// Bright ore flecks.
fn glints(at: V2, height: f32, color: [f32; 3], count: u32) -> ParticleBurst {
    ParticleBurst {
        position: [at[0], height, at[1]],
        position_jitter: 0.3,
        count,
        direction: [0.0, 1.0, 0.0],
        spread: 1.4,
        speed: [1.5, 4.5],
        life: [0.3, 0.7],
        size: [0.1, 0.02],
        color_start: [color[0], color[1], color[2], 1.0],
        color_end: [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, 0.0],
    }
}

/// HDR colour of an element's sparks.
fn element_color(element: Element) -> [f32; 3] {
    match element {
        Element::Fire => [3.0, 1.3, 0.3],
        Element::Acid => [0.9, 2.6, 0.4],
        Element::Cold => [1.2, 2.0, 3.0],
        Element::Shock => [1.6, 2.0, 3.0],
    }
}

/// HDR colour of an ore's glints (blooms in the post chain).
fn ore_glow(material: Material) -> [f32; 3] {
    match material {
        Material::Ferrite => [2.4, 1.3, 0.5],
        Material::Lumite => [0.7, 2.2, 2.6],
        Material::Voidstone => [1.6, 0.7, 2.6],
    }
}

/// A square RGBA mark texture from `pixel(distance from centre, noise)`.
fn mark_texture(
    gal: &Gpu,
    size: u32,
    pixel: impl Fn(f32, f32) -> ([f32; 3], f32),
) -> GraphicsResult<(Texture, TextureView)> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32 - 0.5;
            let v = (y as f32 + 0.5) / size as f32 - 0.5;
            let d = (u * u + v * v).sqrt();
            let angle = v.atan2(u);
            // Lobed, ragged edge: a few angular harmonics plus fine noise.
            let n = 0.5
                + 0.25 * (angle * 3.0 + 0.7).sin()
                + 0.15 * (angle * 7.0 + 1.9).sin()
                + 0.1 * hash(x, y);
            let (rgb, a) = pixel(d, n.clamp(0.0, 1.0));
            for c in rgb {
                data.push((c.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            data.push((a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    let texture = texture_2d(
        &gal.device,
        "Planetfall decal",
        size,
        size,
        TextureFormat::Rgba8Unorm,
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
    )?;
    gal.queue.write_texture(
        texture.as_image_copy(),
        &data,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size * 4),
            rows_per_image: Some(size),
        },
        texture.size(),
    );
    let view = texture.create_view(&Default::default());
    Ok((texture, view))
}

fn hash(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}
