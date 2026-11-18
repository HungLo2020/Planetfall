//! The horde: flow-field steering toward the nearest player, separation,
//! barriers, behaviour-specific attacks, deaths, drops and explosions.
//! Numbers come from `enemies.toml`.

use super::{
    Affix, BARRIER_RADIUS, Blast, BlastKind, Corpse, DeployableKind, Enemy, EnemyShot, Hazard,
    HazardKind, Hit, PickupKind, Run,
};
use crate::content::{BossSpecial, DT, EnemyBehavior, EnemyId, Material, Rarity, content, seconds};
use crate::events::GameEvent;
use crate::math::{self, V2};
use crate::terrain::TILE;

impl Run {
    pub(super) fn update_enemies(&mut self, events: &mut Vec<GameEvent>) {
        let player_radius = content().tuning.player.radius;
        let mut melee = vec![0.0f32; self.players.len()];
        let extra = self.mission_targets();
        let mut aux_hits: Vec<(V2, f32)> = Vec::new();
        let mut detonate = Vec::new();
        let barriers: Vec<V2> = self
            .deployables
            .iter()
            .filter(|d| d.kind == DeployableKind::Barrier)
            .map(|d| d.pos)
            .collect();
        for i in 0..self.enemies.len() {
            let e = self.enemies[i];
            if e.health <= 0.0 {
                continue;
            }
            let Some(target) = self.nearest_player(e.pos) else {
                continue;
            };
            // A survivor or a running drill rig draws the horde too.
            let mut player = self.players[target].pos;
            let mut aux = None;
            for (pos, _) in &extra {
                if pos.distance(e.pos) < player.distance(e.pos) {
                    player = *pos;
                    aux = Some(*pos);
                }
            }
            let info = e.kind.info();
            let to_player = player - e.pos;
            let dist = to_player.length();
            self.enemies[i].state_ticks = e.state_ticks.saturating_sub(1);
            self.enemies[i].special_ticks = e.special_ticks.saturating_sub(1);
            if e.elite == Some(Affix::Regenerating) && e.burning == 0 {
                let en = &mut self.enemies[i];
                en.health = (en.health + en.max_health * content().tuning.elites.regen * DT)
                    .min(en.max_health);
            }
            if self.update_role(i, player, dist, events) {
                continue;
            }
            let e = self.enemies[i];
            let direct = (dist < 5.0 && self.terrain.line_clear(e.pos, player)) || e.flying();
            let goal = if direct {
                player
            } else {
                self.flow
                    .as_ref()
                    .and_then(|f| f.next_waypoint(&self.terrain, e.pos))
                    .unwrap_or(player)
            };
            let dir = (goal - e.pos).normalize_or_zero();
            // Separation from neighbours so hordes spread around the player.
            let mut push = V2::ZERO;
            for j in self.enemies_near(e.pos, info.radius + 1.0) {
                if j == i {
                    continue;
                }
                let o = self.enemies[j];
                let min = info.radius + o.kind.info().radius;
                let d = e.pos - o.pos;
                let l = d.length();
                if l < min && l > 1e-4 {
                    push += d * (min - l) / l;
                } else if l <= 1e-4 {
                    push += V2::new(((i as f32) * 1.7).sin(), ((i as f32) * 1.3).cos());
                }
            }
            let reach = info.radius + player_radius + 0.2;
            // Chill slows; a frozen enemy stands still and cannot attack.
            let mut speed = info.speed * e.speed_scale * (1.0 - 0.6 * e.chill.min(1.0));
            if dist < reach || e.spawn_ticks > 0 || e.frozen > 0 {
                speed = 0.0;
            }
            if let Some(lunge) = info.lunge
                && info.behavior == EnemyBehavior::Lunger
                && dist < lunge.range
                && dist > reach
            {
                speed *= lunge.speed;
            }
            if info.behavior == EnemyBehavior::Spitter
                && let Some(r) = info.ranged
                && dist < r.keep
                && self.terrain.line_clear(e.pos, player)
            {
                speed = 0.0;
            }
            let step = dir * speed * DT + push * 0.35;
            let (mut pos, _) = if e.flying() {
                // Flyers pass over rock, but stay over the map.
                let edge = self.terrain.world_size() - V2::splat(TILE);
                let p = e.pos + step;
                (
                    V2::new(p[0].clamp(TILE, edge[0]), p[1].clamp(TILE, edge[1])),
                    None,
                )
            } else {
                self.terrain.resolve_circle(e.pos + step, info.radius * 0.9)
            };
            // Barrier posts are solid to the horde.
            for &post in &barriers {
                let d = pos - post;
                let min = BARRIER_RADIUS + info.radius;
                let l = d.length();
                if l < min {
                    let out = if l > 1e-4 { d / l } else { V2::X };
                    let pushed = post + out * min;
                    if !self.terrain.solid_at(pushed) {
                        pos = pushed;
                    } else {
                        pos = e.pos;
                    }
                }
            }
            let en = &mut self.enemies[i];
            en.pos = pos;
            en.spawn_ticks = en.spawn_ticks.saturating_sub(1);
            en.hit_ticks = en.hit_ticks.saturating_sub(1);
            en.attack_ticks = en.attack_ticks.saturating_sub(1);
            en.attack_cooldown = en.attack_cooldown.saturating_sub(1);
            en.drone_cooldown = en.drone_cooldown.saturating_sub(1);
            if to_player.length() > 1e-3 {
                en.yaw = math::turn_toward(en.yaw, math::yaw_of(to_player), 0.18);
            }
            en.walk += speed * DT * 1.7;
            if !en.alerted && dist < 12.0 {
                en.alerted = true;
                if info.alert_sound.is_some() && self.rng.chance(f64::from(info.alert_chance)) {
                    events.push(GameEvent::EnemyAlerted {
                        kind: e.kind,
                        at: e.pos,
                    });
                }
            }
            if dist < reach && en.spawn_ticks == 0 && en.frozen == 0 {
                if info.behavior == EnemyBehavior::Exploder {
                    detonate.push(i);
                } else if en.attack_cooldown == 0 {
                    en.attack_cooldown = 60;
                    en.attack_ticks = 14;
                    let frenzy = if en.elite == Some(Affix::Frenzied) {
                        content().tuning.elites.frenzied_damage
                    } else {
                        1.0
                    };
                    let hit = info.damage * frenzy * self.damage_scale();
                    match aux {
                        Some(at) => aux_hits.push((at, hit)),
                        None => melee[target] += hit,
                    }
                }
            }
        }
        for (player, amount) in melee.into_iter().enumerate() {
            if amount > 0.0 {
                self.hurt_player(player, amount, events);
            }
        }
        for (at, amount) in aux_hits {
            self.hurt_mission_target(at, amount, events);
        }
        for i in detonate {
            self.enemies[i].health = 0.0;
        }
    }

    /// Enemy damage grows with planet hazard and the threat level.
    pub(super) fn damage_scale(&self) -> f32 {
        let ramp = content().tuning.threat.damage_per_level;
        (0.75 + 0.25 * self.hazard)
            * (1.0 + self.threat_curve() * ramp)
            * self.hazard_k(&content().tuning.hazard.enemy_damage)
    }

    pub(super) fn remove_dead(&mut self, events: &mut Vec<GameEvent>) {
        let player_radius = content().tuning.player.radius;
        // Explosion chains can kill more enemies, so loop until stable.
        for _ in 0..4 {
            let dead: Vec<usize> = (0..self.enemies.len())
                .filter(|&i| self.enemies[i].health <= 0.0)
                .collect();
            if dead.is_empty() {
                break;
            }
            let mut blasts = Vec::new();
            for &i in &dead {
                let e = self.enemies[i];
                let info = e.kind.info();
                self.kills += 1;
                let targeted = self.mission.target == Some(e.id);
                self.mission_kill(&e, events);
                events.push(GameEvent::EnemyKilled {
                    kind: e.kind,
                    at: e.pos,
                });
                self.corpses.push(Corpse {
                    kind: e.kind,
                    pos: e.pos,
                    yaw: e.yaw,
                    ticks: 0,
                });
                // Splitting elites break into smaller copies.
                if e.elite == Some(Affix::Splitting) && !e.split {
                    let t = &content().tuning.elites;
                    for k in 0..t.split_count {
                        let a = k as f32 * std::f32::consts::TAU / t.split_count as f32;
                        let at = e.pos + V2::new(a.cos(), a.sin()) * 0.8;
                        let at = if self.terrain.solid_at(at) { e.pos } else { at };
                        let id = self.next_id;
                        self.next_id += 1;
                        let hp = e.max_health * t.split_health;
                        self.enemies.push(Enemy {
                            split: true,
                            spawn_ticks: 8,
                            ..Enemy::fresh(id, e.kind, at, hp)
                        });
                    }
                    events.push(GameEvent::Split { at: e.pos });
                }
                // Elites are worth more and always drop a little ore.
                let elites = &content().tuning.elites;
                let xp = if e.elite.is_some() {
                    info.xp * elites.xp
                } else {
                    info.xp
                };
                self.drop_pickup(PickupKind::Xp(xp), e.pos);
                if e.elite.is_some() {
                    self.drop_pickup(PickupKind::Material(Material::Ferrite, 2), e.pos);
                    self.drop_pickup(PickupKind::Material(Material::Lumite, 1), e.pos);
                }
                // Gear: always from bosses and hunted targets, sometimes
                // from elites; credited to the nearest living player.
                let gear = &content().tuning.equipment;
                let finder = self.nearest_player(e.pos);
                if let Some(finder) = finder {
                    if info.boss || targeted {
                        self.find_gear(finder, gear.boss_min, events);
                    } else if e.elite.is_some() && self.rng.chance(f64::from(gear.elite_drop)) {
                        self.find_gear(finder, Rarity::Common, events);
                    }
                }
                for drop in &info.drops {
                    if drop.chance >= 1.0 || self.rng.chance(f64::from(drop.chance)) {
                        self.drop_pickup(PickupKind::Material(drop.material, drop.amount), e.pos);
                    }
                }
                if let Some(explosion) = info.explosion {
                    blasts.push((e.kind, e.pos, explosion));
                }
            }
            self.enemies.retain(|e| e.health > 0.0);
            self.rebuild_index();
            for (kind, at, explosion) in blasts {
                self.blasts.push(Blast {
                    kind: BlastKind::Enemy(kind),
                    pos: at,
                    radius: explosion.radius,
                    ttl: 24,
                    total: 24,
                });
                events.push(GameEvent::Explosion {
                    at,
                    radius: explosion.radius,
                });
                for player in 0..self.players.len() {
                    if at.distance(self.players[player].pos) < explosion.radius + player_radius {
                        self.hurt_player(player, kind.info().damage * self.damage_scale(), events);
                    }
                }
                for j in self.enemies_near(at, explosion.radius) {
                    self.damage_enemy(j, Hit::plain(explosion.enemy_damage), events);
                }
            }
        }
        for c in &mut self.corpses {
            c.ticks += 1;
        }
        self.corpses.retain(|c| c.ticks < 50);
    }

    /// Role behaviour before the common walk: burrowing (returns true while
    /// underground, which replaces the walk), ranged volleys, jamming pulses
    /// and boss abilities.
    fn update_role(
        &mut self,
        i: usize,
        player: V2,
        dist: f32,
        events: &mut Vec<GameEvent>,
    ) -> bool {
        let e = self.enemies[i];
        let info = e.kind.info();
        if e.spawn_ticks > 0 || e.frozen > 0 {
            return false;
        }
        match info.behavior {
            EnemyBehavior::Burrower => {
                let burrow = info.burrow.unwrap_or(crate::content::Burrow {
                    surface_range: 4.0,
                    surfaced: 5.0,
                });
                if e.burrowed {
                    // Tunnel straight at the prey, unseen.
                    let dir = (player - e.pos).normalize_or_zero();
                    let en = &mut self.enemies[i];
                    en.pos += dir * info.speed * 1.2 * DT;
                    en.yaw = crate::math::yaw_of(dir);
                    if dist < burrow.surface_range {
                        let (pos, _) = self.terrain.resolve_circle(en.pos, info.radius);
                        if !self.terrain.solid_at(pos) {
                            let en = &mut self.enemies[i];
                            en.pos = pos;
                            en.burrowed = false;
                            en.spawn_ticks = 20;
                            en.state_ticks = seconds(burrow.surfaced);
                            events.push(GameEvent::Burrow {
                                at: pos,
                                surfaced: true,
                            });
                        }
                    }
                    return true;
                }
                if e.state_ticks == 0 && dist > burrow.surface_range + 2.0 {
                    self.enemies[i].burrowed = true;
                    events.push(GameEvent::Burrow {
                        at: e.pos,
                        surfaced: false,
                    });
                    return true;
                }
            }
            EnemyBehavior::Spitter => {
                if let Some(r) = info.ranged
                    && dist < r.range
                    && e.state_ticks == 0
                    && self.terrain.line_clear(e.pos, player)
                {
                    self.enemies[i].state_ticks = seconds(r.cooldown);
                    self.enemies[i].attack_ticks = 14;
                    let damage = r.damage * self.damage_scale();
                    self.spit(e.kind, e.pos, player, r.count, r.speed, damage, events);
                }
            }
            EnemyBehavior::Disruptor => {
                if let Some(pulse) = info.pulse
                    && dist < pulse.radius
                    && e.state_ticks == 0
                {
                    self.enemies[i].state_ticks = seconds(pulse.cooldown);
                    for p in &mut self.players {
                        if p.alive() && p.pos.distance(e.pos) < pulse.radius {
                            p.jammed = seconds(pulse.jam);
                        }
                    }
                    events.push(GameEvent::Jammed { at: e.pos });
                }
            }
            _ => {}
        }
        if let Some(special) = &info.special
            && e.special_ticks == 0
            && dist < 24.0
        {
            self.boss_special(i, special, player, events);
        }
        false
    }

    /// A fan of `count` spit shots from `from` toward `at`.
    #[allow(clippy::too_many_arguments)]
    fn spit(
        &mut self,
        kind: EnemyId,
        from: V2,
        at: V2,
        count: u32,
        speed: f32,
        damage: f32,
        events: &mut Vec<GameEvent>,
    ) {
        let dir = (at - from).normalize_or_zero();
        for n in 0..count.max(1) {
            let spread = (n as f32 - (count as f32 - 1.0) * 0.5) * 0.22;
            let aim = crate::math::rotate(dir, spread);
            self.enemy_shots.push(EnemyShot {
                pos: from + aim * 0.6,
                vel: aim * speed,
                damage,
                ttl: seconds(2.5),
                kind,
            });
        }
        events.push(GameEvent::Spat { at: from });
    }

    fn boss_special(
        &mut self,
        i: usize,
        special: &BossSpecial,
        player: V2,
        events: &mut Vec<GameEvent>,
    ) {
        let e = self.enemies[i];
        let t = &content().tuning.hazards;
        let every = match special {
            BossSpecial::Brood {
                enemy,
                count,
                every,
            } => {
                if let Some(kind) = EnemyId::from_key(enemy) {
                    self.spawn_group(kind, e.pos, *count);
                }
                *every
            }
            BossSpecial::LavaTrail { every } => {
                self.hazards.push(Hazard {
                    kind: HazardKind::Embers,
                    pos: e.pos,
                    timer: seconds(8.0),
                    dormant: 0,
                });
                *every
            }
            BossSpecial::FrostNova { every, radius } => {
                for p in &mut self.players {
                    if p.alive() && p.pos.distance(e.pos) < *radius {
                        p.chilled = seconds(t.chill_seconds * 1.5);
                    }
                }
                *every
            }
            BossSpecial::Volley { every, count } => {
                let damage = e.kind.info().damage * 0.35 * self.damage_scale();
                self.spit(e.kind, e.pos, player, *count, 9.0, damage, events);
                *every
            }
        };
        self.enemies[i].special_ticks = seconds(every);
        events.push(GameEvent::BossAbility { at: e.pos });
    }

    /// Spit in flight: walls stop it, players take it.
    pub(super) fn update_enemy_shots(&mut self, events: &mut Vec<GameEvent>) {
        let radius = content().tuning.player.radius + 0.25;
        let mut shots = std::mem::take(&mut self.enemy_shots);
        shots.retain_mut(|s| {
            s.ttl = s.ttl.saturating_sub(1);
            s.pos += s.vel * DT;
            if s.ttl == 0 || self.terrain.solid_at(s.pos) {
                return false;
            }
            if let Some(p) = (0..self.players.len())
                .find(|&p| self.players[p].alive() && self.players[p].pos.distance(s.pos) < radius)
            {
                self.hurt_player(p, s.damage, events);
                return false;
            }
            true
        });
        self.enemy_shots = shots;
    }
}
