//! Render interpolation. The simulation advances in fixed 60 Hz ticks, but
//! displays refresh faster (120 to 360 Hz), so drawing the latest tick makes
//! characters step once per tick while the smoothed camera glides: motion
//! looks jittery. Before each tick the desktop records where things were;
//! when rendering it moves them to the point between that and the current
//! tick (`interpolation` from the fixed-step clock), draws, and puts the
//! exact simulated values back. The simulation itself is never changed.

use planetfall_sim::Game;
use planetfall_sim::math::V2;
use std::collections::BTreeMap;

/// Positions and walk phases at one tick.
#[derive(Clone, Default)]
pub struct Snapshot {
    players: Vec<(V2, f32)>,
    enemies: BTreeMap<u32, (V2, f32)>,
    pickups: Vec<V2>,
    ship: (V2, f32),
    /// The game tick the snapshot was taken at (stale snapshots are ignored).
    tick: u64,
}

impl Snapshot {
    pub fn capture(game: &Game) -> Self {
        let mut s = Snapshot {
            ship: (game.ship.player.pos, game.ship.player.walk),
            tick: game.tick,
            ..Snapshot::default()
        };
        if let Some(run) = &game.run {
            s.players = run.players.iter().map(|p| (p.pos, p.walk)).collect();
            s.enemies = run
                .enemies
                .iter()
                .map(|e| (e.id, (e.pos, e.walk)))
                .collect();
            s.pickups = run.pickups.iter().map(|p| p.pos).collect();
        }
        s
    }
}

fn lerp(a: V2, b: V2, t: f32) -> V2 {
    // A jump of more than a few metres in one tick is a teleport (blink,
    // respawn): show it at once rather than sliding across the map.
    if a.distance(b) > 4.0 {
        b
    } else {
        a + (b - a) * t
    }
}

/// Move `game` to the in-between state `alpha` of the way from `prev` to the
/// current tick. Returns the exact current values for [`restore`].
pub fn apply(game: &mut Game, prev: &Snapshot, alpha: f32) -> Option<Snapshot> {
    // Only interpolate across exactly one tick.
    if game.tick != prev.tick + 1 {
        return None;
    }
    let alpha = alpha.clamp(0.0, 1.0);
    let now = Snapshot::capture(game);
    let ship = &mut game.ship.player;
    ship.pos = lerp(prev.ship.0, now.ship.0, alpha);
    ship.walk = prev.ship.1 + (now.ship.1 - prev.ship.1) * alpha;
    if let Some(run) = &mut game.run {
        for (p, (pos, walk)) in run.players.iter_mut().zip(&prev.players) {
            p.pos = lerp(*pos, p.pos, alpha);
            p.walk = walk + (p.walk - walk) * alpha;
        }
        for e in &mut run.enemies {
            if let Some((pos, walk)) = prev.enemies.get(&e.id) {
                e.pos = lerp(*pos, e.pos, alpha);
                e.walk = walk + (e.walk - walk) * alpha;
            }
        }
        // Pickups have no ids; pair them only while none came or went.
        if run.pickups.len() == prev.pickups.len() {
            for (p, pos) in run.pickups.iter_mut().zip(&prev.pickups) {
                p.pos = lerp(*pos, p.pos, alpha);
            }
        }
    }
    Some(now)
}

/// Put back the exact simulated values saved by [`apply`].
pub fn restore(game: &mut Game, now: Snapshot) {
    game.ship.player.pos = now.ship.0;
    game.ship.player.walk = now.ship.1;
    if let Some(run) = &mut game.run {
        for (p, (pos, walk)) in run.players.iter_mut().zip(&now.players) {
            p.pos = *pos;
            p.walk = *walk;
        }
        for e in &mut run.enemies {
            if let Some((pos, walk)) = now.enemies.get(&e.id) {
                e.pos = *pos;
                e.walk = *walk;
            }
        }
        if run.pickups.len() == now.pickups.len() {
            for (p, pos) in run.pickups.iter_mut().zip(&now.pickups) {
                p.pos = *pos;
            }
        }
    }
}
