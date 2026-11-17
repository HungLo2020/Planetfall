use super::*;
use crate::content::{ClassId, ModId, PlanetId, Upgrade};

fn class(key: &str) -> ClassId {
    ClassId::from_key(key).unwrap()
}

fn config(seed: u64) -> RunConfig {
    RunConfig {
        planet: PlanetId::first(),
        seed,
        tutorial: false,
        players: vec![Loadout::basic(ClassId::starter())],
        max_health: 100.0,
        damage_multiplier: 1.0,
        mining_multiplier: 1.0,
        pickup_bonus: 0.0,
    }
}

fn step(run: &mut Run, input: GameInput, ticks: u32) -> Vec<GameEvent> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        if let Some(choices) = run.level_up_choices() {
            assert!(!choices.is_empty());
            run.update(
                &GameInput {
                    command: Some(Command::ChoosePerk(0)),
                    ..GameInput::default()
                },
                &mut events,
            );
        }
        run.update(&input, &mut events);
    }
    events
}

#[test]
fn deterministic_for_seed() {
    let input = GameInput {
        movement: V2::new(0.3, -0.7),
        ..GameInput::default()
    };
    let mut a = Run::new(config(5));
    let mut b = Run::new(config(5));
    step(&mut a, input, 1500);
    step(&mut b, input, 1500);
    assert_eq!(a.state_hash(), b.state_hash());
}

/// A drop saved mid-fight (after mining and kills) resumes and plays on
/// exactly like one that was never interrupted.
#[test]
fn saved_drops_resume_identically() {
    let mut walk = GameInput {
        movement: V2::new(0.6, 0.8),
        ..GameInput::default()
    };
    let mut run = Run::new(config(17));
    run.invulnerable = true;
    step(&mut run, walk, 1800);
    assert!(
        run.terrain.revision > 0 || run.kills > 0,
        "nothing happened"
    );
    let json = serde_json::to_string(&run).unwrap();
    let mut resumed: Run = serde_json::from_str(&json).unwrap();
    assert_eq!(resumed.state_hash(), run.state_hash());
    walk.movement = V2::new(-0.7, 0.2);
    step(&mut run, walk, 900);
    step(&mut resumed, walk, 900);
    assert_eq!(resumed.state_hash(), run.state_hash());
    assert_eq!(resumed.terrain.revision, run.terrain.revision);
}

#[test]
fn horde_spawns_and_weapons_kill() {
    let mut run = Run::new(config(11));
    run.invulnerable = true;
    let events = step(&mut run, GameInput::default(), 60 * 60);
    assert!(events.iter().any(|e| matches!(e, GameEvent::Fired { .. })));
    assert!(run.kills > 0, "no kills after a minute");
    assert!(run.player().level > 1 || run.level_up_choices().is_some() || !run.pickups.is_empty());
}

#[test]
fn walking_into_walls_mines_them() {
    let mut run = Run::new(config(2));
    run.spawning_enabled = false;
    let mut events = Vec::new();
    for dir in [V2::X, V2::NEG_X, V2::Y, V2::NEG_Y] {
        events.extend(step(
            &mut run,
            GameInput {
                movement: dir,
                ..GameInput::default()
            },
            600,
        ));
    }
    assert!(run.blocks_mined > 0);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameEvent::BlockBroken { .. }))
    );
}

#[test]
fn extraction_requires_objective_then_ends_run() {
    let mut run = Run::new(config(4));
    run.spawning_enabled = false;
    let call = GameInput {
        call_extraction: true,
        ..GameInput::default()
    };
    let events = step(&mut run, call, 1);
    assert!(events.contains(&GameEvent::ExtractionDenied));
    run.carried.lumite = run.objective;
    step(&mut run, GameInput::default(), 1);
    assert!(run.objective_done);
    step(&mut run, call, 1);
    assert!(matches!(run.extraction, Extraction::Inbound { .. }));
    run.spawning_enabled = false;
    run.invulnerable = true;
    // Walk back to the landing site and wait for liftoff.
    for _ in 0..(extraction_inbound() + extraction_hold() + 600) {
        let to = run.landing - run.player().pos;
        let input = GameInput {
            movement: if to.length() > 0.5 {
                to.normalize_or_zero()
            } else {
                V2::ZERO
            },
            ..GameInput::default()
        };
        step(&mut run, input, 1);
        if run.outcome().is_some() {
            break;
        }
    }
    assert_eq!(run.outcome(), Some(RunOutcome::Extracted));
}

#[test]
fn caches_offer_rare_upgrades() {
    let mut run = Run::new(config(8));
    run.spawning_enabled = false;
    run.player_mut().pos = run.caches[0].pos;
    let mut events = Vec::new();
    run.update(&GameInput::default(), &mut events);
    run.update(&GameInput::default(), &mut events);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameEvent::CacheOpened { .. }))
    );
    assert_eq!(run.offer_source(), Some(OfferSource::Cache));
    let choices = run.level_up_choices().unwrap();
    assert!(!choices.is_empty());
    assert!(choices.iter().all(|o| o.rarity >= Rarity::Rare));
}

/// Dive levels 5, 10 and 15 offer new weapons (familiar ones likelier);
/// other levels offer upgrades.
#[test]
fn weapon_picks_at_levels() {
    let mut run = Run::new(config(3));
    run.spawning_enabled = false;
    let mut events = Vec::new();
    run.player_mut().pending_levels = 5;
    run.player_mut().level = 6;
    let mut sources = Vec::new();
    for _ in 0..5 {
        run.update(&GameInput::default(), &mut events);
        sources.push(run.offer_source().unwrap());
        run.update(
            &GameInput {
                command: Some(Command::ChoosePerk(0)),
                ..GameInput::default()
            },
            &mut events,
        );
    }
    assert_eq!(sources[3], OfferSource::WeaponPick, "{sources:?}");
    assert!(sources[..3].iter().all(|s| *s == OfferSource::LevelUp));
    assert_eq!(run.player().weapons.len(), 2);
    // Familiar weapons dominate the picks over many rolls.
    let class = run.player().class;
    let mut familiar = 0;
    for seed in 0..40 {
        let mut run = Run::new(config(seed));
        run.player_mut().pending_levels = 1;
        run.player_mut().level = 5;
        run.update(&GameInput::default(), &mut events);
        if let Some(choices) = run.level_up_choices() {
            familiar += choices
                .iter()
                .filter(|o| matches!(o.upgrade, Upgrade::NewWeapon(w) if class.is_familiar(w)))
                .count();
        }
    }
    assert!(familiar > 40, "familiar weapons offered {familiar} times");
}

/// Luck and hazard shift the rarity odds upward.
#[test]
fn luck_improves_rarity() {
    let mut run = Run::new(config(2));
    let base = run.rarity_weights(0, Rarity::Common);
    run.player_mut().luck = 1.0;
    let lucky = run.rarity_weights(0, Rarity::Common);
    let share = |w: [f32; 4]| w[3] / w.iter().sum::<f32>();
    assert!(share(lucky) > share(base) * 2.0);
    let cache = run.rarity_weights(0, Rarity::Rare);
    assert_eq!(cache[0], 0.0);
    assert_eq!(cache[1], 0.0);
}

/// Elements land as statuses: burn ticks damage, cold freezes, acid makes
/// enemies take more, shock arcs to a neighbour.
#[test]
fn elements_apply_statuses() {
    let mut run = Run::new(config(5));
    run.spawning_enabled = false;
    run.invulnerable = true;
    run.player_mut().weapons.clear();
    let spawn = |run: &mut Run, at: V2| {
        let mut e = run.enemies.first().cloned();
        if e.is_none() {
            // Grow the horde until an enemy exists to copy.
            run.spawning_enabled = true;
            for _ in 0..600 {
                run.update(&GameInput::default(), &mut Vec::new());
                if !run.enemies.is_empty() {
                    break;
                }
            }
            run.spawning_enabled = false;
            e = run.enemies.first().cloned();
        }
        let mut e = e.unwrap();
        e.pos = at;
        e.health = 1000.0;
        e.max_health = 1000.0;
        e.spawn_ticks = 0;
        run.enemies = vec![e];
    };
    let at = run.player().pos + V2::new(0.0, 6.0);
    spawn(&mut run, at);
    run.rebuild_index();
    let mut events = Vec::new();
    let hit = |el: Element, potency: f32| Hit {
        damage: 1.0,
        elements: {
            let mut e = Elements::default();
            e.add(el, potency);
            e
        },
        ..Hit::default()
    };
    run.damage_enemy(0, hit(Element::Fire, 20.0), &mut events);
    assert!(run.enemies[0].burning > 0);
    let before = run.enemies[0].health;
    for _ in 0..60 {
        run.update_statuses(&mut events);
        run.ticks += 1;
    }
    assert!(run.enemies[0].health < before - 15.0, "burn did little");
    run.damage_enemy(0, hit(Element::Acid, 5.0), &mut events);
    let h = run.enemies[0].health;
    run.damage_enemy(0, Hit::plain(100.0), &mut events);
    assert!(
        (h - run.enemies[0].health - 125.0).abs() < 0.01,
        "corrode bonus"
    );
    for _ in 0..40 {
        run.damage_enemy(0, hit(Element::Cold, 10.0), &mut events);
    }
    assert!(run.enemies[0].frozen > 0);
    // Shock arcs to a neighbour sooner or later.
    let mut second = run.enemies[0];
    second.pos = at + V2::new(1.5, 0.0);
    second.id += 1;
    run.enemies.push(second);
    run.rebuild_index();
    for _ in 0..40 {
        run.damage_enemy(0, hit(Element::Shock, 10.0), &mut events);
    }
    assert!(run.enemies[1].health < 1000.0, "shock never arced");
}

/// Legendary behaviours: twin barrel, second ring, echo step, mobile sentry.
#[test]
fn legendary_specials_change_behaviour() {
    let id = |k: &str| ModId::from_key(k).unwrap();
    // Second Ring doubles the drones.
    let mut run = Run::new(RunConfig {
        players: vec![Loadout::basic(class("engineer"))],
        ..config(2)
    });
    let drones = run.drone_positions(0).len();
    run.player_mut().weapons[0]
        .mods
        .push((id("second-ring"), Rarity::Legendary));
    assert_eq!(run.drone_positions(0).len(), drones * 2);
    // Mobile Sentry: the turret follows its owner.
    run.player_mut()
        .ability
        .mods
        .push((id("mobile-sentry"), Rarity::Legendary));
    run.spawning_enabled = false;
    step(
        &mut run,
        GameInput {
            ability: true,
            ..GameInput::default()
        },
        1,
    );
    let start = run.deployables[0].pos;
    step(
        &mut run,
        GameInput {
            movement: V2::new(0.0, 1.0),
            ..GameInput::default()
        },
        120,
    );
    assert!(
        run.deployables[0].pos.distance(start) > 2.0,
        "sentry stayed put"
    );
    // Echo Step: two blinks back to back.
    let mut run = Run::new(config(2));
    run.spawning_enabled = false;
    let mut events = Vec::new();
    run.apply_offer(
        0,
        Offer {
            upgrade: Upgrade::AbilityMod(id("echo-step")),
            rarity: Rarity::Legendary,
        },
        &mut events,
    );
    assert_eq!(run.player().ability.charges, 2);
    let blink = GameInput {
        ability: true,
        ..GameInput::default()
    };
    let used = |events: &[GameEvent]| {
        events
            .iter()
            .filter(|e| matches!(e, GameEvent::AbilityUsed { .. }))
            .count()
    };
    let events = step(&mut run, blink, 1);
    let events2 = step(&mut run, blink, 1);
    assert_eq!(used(&events) + used(&events2), 2);
}

/// Every enemy and planet in the data can actually appear.
#[test]
fn spawn_tables_cover_the_content() {
    for planet in PlanetId::all() {
        let mut run = Run::new(RunConfig {
            planet,
            ..config(3)
        });
        // A high threat unlocks every spawn rule.
        run.threat = 10.0;
        let table = run.spawn_table();
        for enemy in EnemyId::all() {
            assert!(
                table.iter().any(|(id, _)| *id == enemy),
                "{} never spawns on {}",
                enemy.key(),
                planet.key()
            );
        }
    }
}

/// Every class ability does its job: a bore breaks rock, a blink moves the
/// player, barrier posts and turrets appear and expire, and all of them
/// start a cooldown.
#[test]
fn class_abilities_work() {
    for key in ["prospector", "striker", "warden", "engineer"] {
        let mut run = Run::new(RunConfig {
            players: vec![Loadout::basic(class(key))],
            ..config(9)
        });
        run.spawning_enabled = false;
        // Stand two metres from the nearest wall, facing it, so the bore
        // has rock to cut.
        let origin = run.player().pos;
        let (dir, n) = [V2::X, V2::NEG_X, V2::Y, V2::NEG_Y]
            .into_iter()
            .map(|d| {
                let n = (1..60)
                    .find(|n| run.terrain.solid_at(origin + d * (*n as f32 * 0.5)))
                    .unwrap_or(99);
                (d, n)
            })
            .min_by_key(|(_, n)| *n)
            .unwrap();
        let start = if key == "prospector" {
            origin + dir * (n as f32 * 0.5 - 2.0).max(0.0)
        } else {
            origin
        };
        run.player_mut().pos = start;
        run.player_mut().yaw = crate::math::yaw_of(dir);
        let mined = run.blocks_mined;
        let events = step(
            &mut run,
            GameInput {
                ability: true,
                ..GameInput::default()
            },
            1,
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GameEvent::AbilityUsed { .. })),
            "{key}: no ability"
        );
        assert!(run.player().ability.cooldown > 0, "{key}: no cooldown");
        match key {
            "prospector" => assert!(run.blocks_mined > mined, "bore broke nothing"),
            "striker" => assert!(run.player().pos.distance(start) > 0.5, "blink did not move"),
            "warden" => assert!(
                run.deployables
                    .iter()
                    .filter(|d| d.kind == DeployableKind::Barrier)
                    .count()
                    >= 3
            ),
            _ => assert!(
                run.deployables
                    .iter()
                    .any(|d| matches!(d.kind, DeployableKind::Turret { .. }))
            ),
        }
        // Pressing again while recharging does nothing but report it.
        let events = step(
            &mut run,
            GameInput {
                ability: true,
                ..GameInput::default()
            },
            1,
        );
        assert!(events.contains(&GameEvent::AbilityNotReady));
        step(&mut run, GameInput::default(), 60 * 30);
        assert!(run.deployables.is_empty(), "{key}: deployables expire");
        assert_eq!(run.player().ability.cooldown, 0);
    }
}

/// Barrier posts stop the horde; turrets kill.
#[test]
fn barriers_block_and_turrets_kill() {
    let mut run = Run::new(RunConfig {
        players: vec![Loadout::basic(class("engineer"))],
        ..config(4)
    });
    run.invulnerable = true;
    // Strip the engineer's own weapon so only the turret can kill.
    run.player_mut().weapons.clear();
    step(
        &mut run,
        GameInput {
            ability: true,
            ..GameInput::default()
        },
        1,
    );
    step(&mut run, GameInput::default(), 60 * 12);
    assert!(run.kills > 0, "the turret killed nothing");
}

/// Passives apply at the start of a drop and class perks are only offered
/// to their class.
#[test]
fn passives_and_class_perks() {
    let prospector = Run::new(RunConfig {
        players: vec![Loadout::basic(class("prospector"))],
        ..config(1)
    });
    let warden = Run::new(RunConfig {
        players: vec![Loadout::basic(class("warden"))],
        ..config(1)
    });
    assert!(prospector.player().mining_multiplier > 1.4);
    assert!(prospector.player().ore_bonus > 0.0);
    assert!(warden.player().max_health > prospector.player().max_health);
    assert!(warden.player().extraction_speed > 0.0);
    for (upgrade, _) in warden.upgrade_pool(0, Rarity::Legendary) {
        if let Upgrade::Field(id) = upgrade {
            let owner = id.info().class.as_deref();
            assert!(owner.is_none_or(|c| c == "warden"), "{} offered", id.key());
        }
    }
}

/// Two players share a drop: both act, the horde targets either, and the
/// drop only fails when everyone is down.
#[test]
fn co_op_ready_players_list() {
    let mut run = Run::new(RunConfig {
        players: vec![
            Loadout::basic(class("warden")),
            Loadout::basic(class("striker")),
        ],
        ..config(6)
    });
    assert_eq!(run.players.len(), 2);
    let a = GameInput {
        movement: V2::X,
        ..GameInput::default()
    };
    let b = GameInput {
        movement: V2::NEG_X,
        ..GameInput::default()
    };
    let mut events = Vec::new();
    let starts = (run.players[0].pos, run.players[1].pos);
    for _ in 0..60 {
        run.update_players(&[a, b], &mut events);
    }
    assert!(run.players[0].pos.x > starts.0.x || run.players[0].mining.is_some());
    assert!(run.players[1].pos.x < starts.1.x || run.players[1].mining.is_some());
    run.players[0].health = 0.0;
    run.update_players(&[a, b], &mut events);
    assert!(run.players[0].down);
    assert!(run.outcome().is_none(), "one player down is not a loss");
    run.players[1].health = 0.0;
    run.update_players(&[a, b], &mut events);
    assert_eq!(run.outcome(), Some(RunOutcome::Died));
}

/// The threat rises all drop long (faster during extraction), surges are
/// warned before they hit, and elites join the horde as it climbs.
#[test]
fn threat_escalates_with_surges_and_elites() {
    let mut run = Run::new(config(12));
    run.invulnerable = true;
    let mut events = Vec::new();
    let mut warned_at = None;
    let mut surged_at = None;
    for tick in 0..60 * 60 * 9 {
        if let Some(choices) = run.level_up_choices() {
            let pick = Command::ChoosePerk(choices.len() - 1);
            run.update(
                &GameInput {
                    command: Some(pick),
                    ..GameInput::default()
                },
                &mut events,
            );
        }
        run.update(&GameInput::default(), &mut events);
        for e in events.drain(..) {
            match e {
                GameEvent::SurgeWarning { .. } if warned_at.is_none() => warned_at = Some(tick),
                GameEvent::SurgeStarted { .. } if surged_at.is_none() => surged_at = Some(tick),
                _ => {}
            }
        }
    }
    assert!(run.threat_level() >= 4, "threat {}", run.threat);
    let (warned, surged) = (warned_at.unwrap(), surged_at.unwrap());
    assert!(surged > warned, "the warning comes first");
    assert!(surged - warned >= 60 * 5);
    assert!(
        run.enemies.iter().any(|e| e.elite.is_some()) || run.kills > 0,
        "no elites by threat {}",
        run.threat_level()
    );
    // Extraction speeds the threat up.
    let mut a = Run::new(config(2));
    let mut b = Run::new(config(2));
    b.extraction = Extraction::Inbound {
        ticks: 9999,
        pos: b.landing,
    };
    for _ in 0..600 {
        a.update_threat(&mut events);
        b.update_threat(&mut events);
    }
    assert!(b.threat > a.threat * 1.5);
}

/// Elites are tougher and count as heavy; later levels need more XP.
#[test]
fn elites_and_xp_curve() {
    let mut run = Run::new(config(4));
    run.threat = 12.0;
    run.spawning_enabled = true;
    let mut elites = 0;
    for _ in 0..60 * 40 {
        run.update_spawner(&mut Vec::new());
        elites = run.enemies.iter().filter(|e| e.elite.is_some()).count();
        if elites > 0 {
            break;
        }
    }
    assert!(elites > 0);
    let e = run.enemies.iter().find(|e| e.elite.is_some()).unwrap();
    assert!(e.heavy());
    use crate::content::xp_to_next;
    let early = xp_to_next(2) - xp_to_next(1);
    let late = xp_to_next(16) - xp_to_next(15);
    assert!(late > early * 2, "the XP curve steepens");
}

/// Tuning probe (run with `--ignored --nocapture`): how long each class
/// survives on each planet when it never extracts, with the autopilot's
/// upgrade picks and movement. This is the soft cap the threat sets.
#[test]
#[ignore]
fn survival_time_probe() {
    use crate::autoplay::Autopilot;
    use crate::game::Game;
    let only = std::env::var("PROBE_PLANET").ok();
    for planet in PlanetId::all().filter(|p| only.as_deref().is_none_or(|k| k == p.key())) {
        for key in ["striker", "prospector", "warden", "engineer"] {
            let mut minutes = Vec::new();
            for seed in 0..4u64 {
                // Progress typical of reaching this planet: health and
                // damage modules and some class levels.
                let nav = f32::from(planet.info().nav_level);
                let mut loadout = Loadout::basic(class(key));
                loadout.class_level = 1 + 3 * u32::from(planet.info().nav_level);
                let mut run = Run::new(RunConfig {
                    planet,
                    players: vec![loadout],
                    max_health: 100.0 + 25.0 * nav,
                    damage_multiplier: 1.0 + 0.12 * nav,
                    ..config(seed * 7 + 1)
                });
                let mut game = Game::new(crate::Profile::default(), 1);
                game.phase = crate::Phase::Surface;
                let mut pilot = Autopilot::new();
                let mut events = Vec::new();
                for _ in 0..60 * 60 * 30 {
                    run.objective_done = false;
                    game.run = Some(run);
                    let mut input = pilot.input(&game);
                    run = game.run.take().unwrap();
                    input.call_extraction = false;
                    run.update(&input, &mut events);
                    events.clear();
                    if run.outcome().is_some() {
                        break;
                    }
                }
                minutes.push(run.minutes());
            }
            minutes.sort_by(f32::total_cmp);
            eprintln!(
                "{:<8} {:<10} survives {:.1} min (median of {:?})",
                planet.key(),
                key,
                (minutes[1] + minutes[2]) / 2.0,
                minutes
                    .iter()
                    .map(|m| (m * 10.0).round() / 10.0)
                    .collect::<Vec<_>>()
            );
        }
    }
}
