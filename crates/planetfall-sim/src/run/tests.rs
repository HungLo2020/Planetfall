use super::*;
use crate::content::PlanetId;

fn config(seed: u64) -> RunConfig {
    RunConfig {
        planet: PlanetId::first(),
        seed,
        tutorial: false,
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
    assert!(run.player.level > 1 || run.level_up_choices().is_some() || !run.pickups.is_empty());
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
        let to = run.landing - run.player.pos;
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
fn caches_grant_weapons() {
    let mut run = Run::new(config(8));
    run.spawning_enabled = false;
    run.player.pos = run.caches[0].pos;
    let events = step(&mut run, GameInput::default(), 2);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameEvent::WeaponFound { .. }))
    );
    assert_eq!(run.weapons.len(), 2);
}

/// Every enemy and planet in the data can actually appear.
#[test]
fn spawn_tables_cover_the_content() {
    for planet in PlanetId::all() {
        let mut run = Run::new(RunConfig {
            planet,
            ..config(3)
        });
        run.ticks = 60 * 60 * 6;
        run.extraction = Extraction::Inbound {
            ticks: 999,
            pos: run.landing,
        };
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
