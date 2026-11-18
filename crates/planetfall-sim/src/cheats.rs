//! Developer console commands for testing (`/give weapon sentry`). Parsing,
//! validation and Minecraft-style suggestions are pure functions of the
//! typed line and the game. A parsed [`Cheat`] is applied through
//! `Command::Cheat`, so it runs on a tick like any other input and replays.

use crate::content::{EnemyId, Material, ModId, ModTarget, PerkId, Rarity, WeaponId, listed_stat};
use crate::game::{Game, Phase};
use serde::{Deserialize, Serialize};

/// Most copies of an upgrade or perk one command adds.
pub const MAX_COUNT: u32 = 20;
/// Most enemies one command spawns.
pub const MAX_SPAWN: u32 = 60;

/// A test command, validated against the game when it was typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cheat {
    /// A weapon in a free slot.
    GiveWeapon(WeaponId),
    /// Drop a carried weapon.
    RemoveWeapon(WeaponId),
    /// Upgrades on a carried weapon.
    WeaponMod {
        weapon: WeaponId,
        upgrade: ModId,
        rarity: Rarity,
        count: u8,
    },
    /// Upgrades on the class ability.
    AbilityMod {
        upgrade: ModId,
        rarity: Rarity,
        count: u8,
    },
    Perk {
        perk: PerkId,
        rarity: Rarity,
        count: u8,
    },
    /// Materials banked on the ship.
    Material {
        material: Material,
        amount: u32,
    },
    Credits(u32),
    /// Dive XP (levels up as if collected).
    Xp(u32),
    /// Refill health and revive downed players.
    Heal,
    /// Toggle taking no damage.
    God,
    /// Kill every enemy.
    Kill,
    /// Enemies a few metres from the player.
    Spawn {
        enemy: EnemyId,
        count: u8,
    },
    /// Set the threat level.
    Threat(u32),
}

/// What a typed line asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed {
    Cheat(Cheat),
    /// `/help`: usage lines to print.
    Help(Vec<String>),
}

/// Completions for the word being typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Suggestions {
    /// Byte offset in the line where the word being completed starts.
    pub from: usize,
    /// Completions: the text to insert and a short description.
    pub items: Vec<(String, String)>,
    /// Usage of every command the line still matches.
    pub usage: Vec<String>,
}

/// Argument kinds in a command's syntax.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Literal(&'static str),
    /// Any weapon.
    Weapon,
    /// A weapon the player carries.
    Carried,
    /// A weapon upgrade that fits the carried weapon before it.
    Upgrade,
    /// An upgrade for the player's class ability.
    AbilityUpgrade,
    Perk,
    Material,
    Enemy,
    Rarity,
    Number,
}

#[derive(Clone, Copy, Debug)]
enum Value {
    Weapon(WeaponId),
    Mod(ModId),
    Perk(PerkId),
    Material(Material),
    Enemy(EnemyId),
    Rarity(Rarity),
    Number(u32),
}

type Build = fn(&[Option<Value>], &Game) -> Result<Parsed, String>;

/// A command: its syntax (`<required>`, `[optional]`), help and builder.
struct Spec {
    syntax: &'static str,
    help: &'static str,
    build: Build,
}

const SPECS: &[Spec] = &[
    Spec {
        syntax: "give weapon <weapon>",
        help: "Add a weapon to a free slot",
        build: |v, g| {
            let weapon = weapon(v, 0);
            let run = on_drop(g)?;
            let max = crate::content::content().tuning.weapons.max_carried;
            if run.player().carries(weapon) {
                Err(format!("Already carrying {}", weapon.info().name))
            } else if run.player().weapons.len() >= max {
                Err(format!(
                    "All {max} weapon slots are full: /remove a weapon first"
                ))
            } else {
                Ok(Parsed::Cheat(Cheat::GiveWeapon(weapon)))
            }
        },
    },
    Spec {
        syntax: "give upgrade <carried-weapon> <upgrade> [rarity] [count]",
        help: "Add upgrades to a carried weapon",
        build: |v, g| {
            let (weapon, upgrade) = (weapon(v, 0), modid(v, 1));
            let run = on_drop(g)?;
            let slot = run
                .player()
                .weapons
                .iter()
                .find(|w| w.kind == weapon)
                .ok_or("Not carrying that weapon")?;
            let count = stacks_left(upgrade, slot.stacks(upgrade), count(v, 3))?;
            Ok(Parsed::Cheat(Cheat::WeaponMod {
                weapon,
                upgrade,
                rarity: rarity(v, 2, upgrade.info().min_rarity),
                count,
            }))
        },
    },
    Spec {
        syntax: "give ability <ability-upgrade> [rarity] [count]",
        help: "Add upgrades to the class ability",
        build: |v, g| {
            let upgrade = modid(v, 0);
            let run = on_drop(g)?;
            let count = stacks_left(upgrade, run.player().ability.stacks(upgrade), count(v, 2))?;
            Ok(Parsed::Cheat(Cheat::AbilityMod {
                upgrade,
                rarity: rarity(v, 1, upgrade.info().min_rarity),
                count,
            }))
        },
    },
    Spec {
        syntax: "give perk <perk> [rarity] [count]",
        help: "Take a field or class perk",
        build: |v, g| {
            on_drop(g)?;
            let Some(Value::Perk(perk)) = v[0] else {
                unreachable!()
            };
            Ok(Parsed::Cheat(Cheat::Perk {
                perk,
                rarity: rarity(v, 1, Rarity::Common),
                count: count(v, 2).clamp(1, MAX_COUNT) as u8,
            }))
        },
    },
    Spec {
        syntax: "give material <material> [amount]",
        help: "Bank materials on the ship",
        build: |v, _| {
            let Some(Value::Material(material)) = v[0] else {
                unreachable!()
            };
            Ok(Parsed::Cheat(Cheat::Material {
                material,
                amount: number(v, 1).unwrap_or(100),
            }))
        },
    },
    Spec {
        syntax: "give credits <amount>",
        help: "Add trade credits",
        build: |v, _| Ok(Parsed::Cheat(Cheat::Credits(number(v, 0).unwrap_or(0)))),
    },
    Spec {
        syntax: "give xp <amount>",
        help: "Add dive XP (levels up)",
        build: |v, g| {
            on_drop(g)?;
            Ok(Parsed::Cheat(Cheat::Xp(number(v, 0).unwrap_or(0))))
        },
    },
    Spec {
        syntax: "remove <carried-weapon>",
        help: "Drop a carried weapon",
        build: |v, g| {
            on_drop(g)?;
            Ok(Parsed::Cheat(Cheat::RemoveWeapon(weapon(v, 0))))
        },
    },
    Spec {
        syntax: "spawn <enemy> [count]",
        help: "Spawn enemies near you",
        build: |v, g| {
            on_drop(g)?;
            let Some(Value::Enemy(enemy)) = v[0] else {
                unreachable!()
            };
            Ok(Parsed::Cheat(Cheat::Spawn {
                enemy,
                count: number(v, 1).unwrap_or(1).clamp(1, MAX_SPAWN) as u8,
            }))
        },
    },
    Spec {
        syntax: "kill",
        help: "Kill every enemy",
        build: |_, g| on_drop(g).map(|_| Parsed::Cheat(Cheat::Kill)),
    },
    Spec {
        syntax: "heal",
        help: "Refill health",
        build: |_, g| on_drop(g).map(|_| Parsed::Cheat(Cheat::Heal)),
    },
    Spec {
        syntax: "god",
        help: "Toggle taking no damage",
        build: |_, g| on_drop(g).map(|_| Parsed::Cheat(Cheat::God)),
    },
    Spec {
        syntax: "threat <level>",
        help: "Set the threat level",
        build: |v, g| {
            on_drop(g)?;
            Ok(Parsed::Cheat(Cheat::Threat(number(v, 0).unwrap_or(0))))
        },
    },
    Spec {
        syntax: "help",
        help: "List commands",
        build: |_, _| {
            Ok(Parsed::Help(
                SPECS
                    .iter()
                    .map(|s| format!("/{}  {}", s.syntax, s.help))
                    .collect(),
            ))
        },
    },
];

fn weapon(v: &[Option<Value>], i: usize) -> WeaponId {
    match v[i] {
        Some(Value::Weapon(w)) => w,
        _ => unreachable!("syntax guarantees a weapon"),
    }
}

fn modid(v: &[Option<Value>], i: usize) -> ModId {
    match v[i] {
        Some(Value::Mod(m)) => m,
        _ => unreachable!("syntax guarantees an upgrade"),
    }
}

fn number(v: &[Option<Value>], i: usize) -> Option<u32> {
    match v.get(i).copied().flatten() {
        Some(Value::Number(n)) => Some(n),
        _ => None,
    }
}

fn count(v: &[Option<Value>], i: usize) -> u32 {
    number(v, i).unwrap_or(1)
}

/// The rarity typed, or `default` (raised to what the upgrade needs).
fn rarity(v: &[Option<Value>], i: usize, default: Rarity) -> Rarity {
    match v.get(i).copied().flatten() {
        Some(Value::Rarity(r)) => r.max(default),
        _ => default,
    }
}

/// How many of `count` copies of `upgrade` still fit.
fn stacks_left(upgrade: ModId, taken: u32, count: u32) -> Result<u8, String> {
    let room = upgrade
        .info()
        .max_stacks
        .map_or(MAX_COUNT, |max| max.saturating_sub(taken));
    if room == 0 {
        return Err(format!("{} is already at its limit", upgrade.info().title));
    }
    Ok(count.clamp(1, room.min(MAX_COUNT)) as u8)
}

fn on_drop(game: &Game) -> Result<&crate::run::Run, String> {
    match (&game.run, game.phase) {
        (Some(run), Phase::Surface) => Ok(run),
        _ => Err("Only during a drop, on the planet surface".into()),
    }
}

impl Spec {
    fn slots(&self) -> Vec<(Kind, bool)> {
        self.syntax
            .split_whitespace()
            .map(|word| {
                let optional = word.starts_with('[');
                let name = word.trim_matches(|c| matches!(c, '<' | '>' | '[' | ']'));
                let kind = if word.starts_with('<') || optional {
                    match name {
                        "weapon" => Kind::Weapon,
                        "carried-weapon" => Kind::Carried,
                        "upgrade" => Kind::Upgrade,
                        "ability-upgrade" => Kind::AbilityUpgrade,
                        "perk" => Kind::Perk,
                        "material" => Kind::Material,
                        "enemy" => Kind::Enemy,
                        "rarity" => Kind::Rarity,
                        _ => Kind::Number,
                    }
                } else {
                    Kind::Literal(name)
                };
                (kind, optional)
            })
            .collect()
    }
}

/// The values a slot accepts, given the values before it: (word, description).
fn candidates(kind: Kind, before: &[Option<Value>], game: &Game) -> Vec<(String, String)> {
    let run = game.run.as_ref();
    match kind {
        Kind::Literal(word) => vec![(word.to_owned(), String::new())],
        Kind::Weapon => WeaponId::all()
            .map(|w| (w.key().to_owned(), w.info().name.clone()))
            .collect(),
        Kind::Carried => run
            .map(|r| {
                r.player()
                    .weapons
                    .iter()
                    .map(|w| (w.kind.key().to_owned(), w.kind.info().name.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        Kind::Upgrade => {
            let Some(Value::Weapon(w)) = before.iter().rev().flatten().next().copied() else {
                return Vec::new();
            };
            let weapon = w.info();
            ModId::all()
                .filter(|m| {
                    let info = m.info();
                    info.target == ModTarget::Weapon
                        && (info.behaviors.is_empty() || info.behaviors.contains(&weapon.behavior))
                        && (!listed_stat(info.stat) || weapon.upgrades.get(info.stat).is_some())
                })
                .map(|m| (m.key().to_owned(), describe_mod(m)))
                .collect()
        }
        Kind::AbilityUpgrade => {
            let ability = run.map(|r| r.player().class.info().ability.kind);
            ModId::all()
                .filter(|m| {
                    let info = m.info();
                    info.target == ModTarget::Ability
                        && (info.abilities.is_empty()
                            || ability.is_some_and(|a| info.abilities.contains(&a)))
                })
                .map(|m| (m.key().to_owned(), describe_mod(m)))
                .collect()
        }
        Kind::Perk => PerkId::all()
            .map(|p| {
                let info = p.info();
                let class = info
                    .class
                    .as_deref()
                    .map(|c| format!(" ({c})"))
                    .unwrap_or_default();
                (p.key().to_owned(), format!("{}{class}", info.title))
            })
            .collect(),
        Kind::Material => Material::ALL
            .iter()
            .map(|m| (m.name().to_lowercase(), m.name().to_owned()))
            .collect(),
        Kind::Enemy => EnemyId::all()
            .map(|e| {
                let info = e.info();
                let boss = if info.boss { " (boss)" } else { "" };
                (e.key().to_owned(), format!("{}{boss}", info.name))
            })
            .collect(),
        Kind::Rarity => Rarity::ALL
            .iter()
            .map(|r| (r.name().to_lowercase(), r.name().to_owned()))
            .collect(),
        Kind::Number => Vec::new(),
    }
}

fn describe_mod(m: ModId) -> String {
    let info = m.info();
    if info.min_rarity > Rarity::Common {
        format!("{} ({}+)", info.title, info.min_rarity.name())
    } else {
        info.title.clone()
    }
}

/// Read `word` as a value of `kind`.
fn resolve(kind: Kind, word: &str, before: &[Option<Value>], game: &Game) -> Option<Value> {
    let word = word.to_lowercase();
    if kind == Kind::Number {
        return word.parse().ok().map(Value::Number);
    }
    if let Kind::Literal(l) = kind {
        return (l == word).then_some(Value::Number(0));
    }
    // Only offered values are accepted (an upgrade must fit its weapon).
    if !candidates(kind, before, game)
        .iter()
        .any(|(w, _)| *w == word)
    {
        return None;
    }
    Some(match kind {
        Kind::Weapon | Kind::Carried => Value::Weapon(WeaponId::from_key(&word)?),
        Kind::Upgrade | Kind::AbilityUpgrade => Value::Mod(ModId::from_key(&word)?),
        Kind::Perk => Value::Perk(PerkId::from_key(&word)?),
        Kind::Material => Value::Material(
            *Material::ALL
                .iter()
                .find(|m| m.name().to_lowercase() == word)?,
        ),
        Kind::Enemy => Value::Enemy(EnemyId::from_key(&word)?),
        Kind::Rarity => Value::Rarity(
            *Rarity::ALL
                .iter()
                .find(|r| r.name().to_lowercase() == word)?,
        ),
        Kind::Literal(_) | Kind::Number => unreachable!(),
    })
}

/// Words of a line without its leading slash.
fn words(line: &str) -> Vec<&str> {
    line.trim_start()
        .trim_start_matches('/')
        .split_whitespace()
        .collect()
}

/// How a spec matches `words`: the argument values (`None` for literals and
/// missing optionals), or how many words matched before it failed.
fn match_spec(
    spec: &Spec,
    words: &[&str],
    game: &Game,
) -> Result<Vec<Option<Value>>, (usize, String)> {
    let slots = spec.slots();
    let mut values: Vec<Option<Value>> = Vec::new();
    for (i, (kind, optional)) in slots.iter().enumerate() {
        let Some(word) = words.get(i) else {
            if *optional {
                if !matches!(kind, Kind::Literal(_)) {
                    values.push(None);
                }
                continue;
            }
            return Err((
                i,
                format!("Missing {}. Usage: /{}", slot_name(spec, i), spec.syntax),
            ));
        };
        match resolve(*kind, word, &values, game) {
            Some(v) => {
                if !matches!(kind, Kind::Literal(_)) {
                    values.push(Some(v));
                }
            }
            None => {
                return Err((
                    i,
                    format!(
                        "Unknown {} '{word}'. Usage: /{}",
                        slot_name(spec, i),
                        spec.syntax
                    ),
                ));
            }
        }
    }
    if words.len() > slots.len() {
        return Err((
            slots.len(),
            format!("Too many words. Usage: /{}", spec.syntax),
        ));
    }
    Ok(values)
}

fn slot_name(spec: &Spec, i: usize) -> &'static str {
    spec.syntax
        .split_whitespace()
        .nth(i)
        .map(|w| w.trim_matches(|c| matches!(c, '<' | '>' | '[' | ']')))
        .unwrap_or("word")
}

/// Parse and validate a typed line.
pub fn parse(line: &str, game: &Game) -> Result<Parsed, String> {
    let words = words(line);
    if words.is_empty() {
        return Err("Type a command (/help lists them)".into());
    }
    let mut best: Option<(usize, String)> = None;
    for spec in SPECS {
        match match_spec(spec, &words, game) {
            Ok(values) => return (spec.build)(&values, game),
            Err((matched, message)) => {
                if best.as_ref().is_none_or(|(m, _)| matched > *m) {
                    best = Some((matched, message));
                }
            }
        }
    }
    match best {
        Some((matched, message)) if matched > 0 => Err(message),
        _ => Err(format!("Unknown command '{}' (/help lists them)", words[0])),
    }
}

/// Completions for the end of `line` (the word being typed, or the next
/// word after a space) and the usage of every command it still matches.
pub fn suggest(line: &str, game: &Game) -> Suggestions {
    let trailing = line.is_empty() || line.ends_with(char::is_whitespace) || line.ends_with('/');
    let all = words(line);
    let (done, partial) = if trailing {
        (&all[..], "")
    } else {
        (&all[..all.len() - 1], all[all.len() - 1])
    };
    let from = line.len() - partial.len();
    let partial = partial.to_lowercase();
    let mut out = Suggestions {
        from,
        ..Suggestions::default()
    };
    for spec in SPECS {
        let slots = spec.slots();
        let Some(&(kind, _)) = slots.get(done.len()) else {
            continue;
        };
        // Every finished word must fit this command.
        let mut values = Vec::new();
        let fits =
            done.iter()
                .zip(&slots)
                .all(|(word, (k, _))| match resolve(*k, word, &values, game) {
                    Some(v) => {
                        if !matches!(k, Kind::Literal(_)) {
                            values.push(Some(v));
                        }
                        true
                    }
                    None => false,
                });
        if !fits {
            continue;
        }
        let options: Vec<(String, String)> = candidates(kind, &values, game)
            .into_iter()
            .filter(|(w, _)| w.starts_with(&partial))
            .collect();
        if options.is_empty() && kind != Kind::Number {
            continue;
        }
        if !out.usage.iter().any(|u| u.ends_with(spec.syntax)) {
            out.usage.push(format!("/{}", spec.syntax));
        }
        for (word, detail) in options {
            let detail = if detail.is_empty() && matches!(kind, Kind::Literal(_)) && done.is_empty()
            {
                spec.help.to_owned()
            } else {
                detail
            };
            if !out.items.iter().any(|(w, _)| *w == word) {
                out.items.push((word, detail));
            }
        }
    }
    out
}

impl Game {
    /// Apply a console command (validated when typed; re-checked here since
    /// the game may have moved on since).
    pub(crate) fn cheat(&mut self, cheat: Cheat) {
        let message = match cheat {
            Cheat::Material { material, amount } => {
                self.profile.materials.add(material, amount);
                self.events.push(crate::events::GameEvent::ProfileChanged);
                Ok(format!("+{amount} {}", material.name()))
            }
            Cheat::Credits(amount) => {
                self.profile.credits = self.profile.credits.saturating_add(amount);
                self.events.push(crate::events::GameEvent::ProfileChanged);
                Ok(format!("+{amount} credits"))
            }
            _ => match (&mut self.run, self.phase) {
                (Some(run), Phase::Surface) => run.cheat(cheat, &mut self.events),
                _ => Err("Only during a drop".to_owned()),
            },
        };
        match message {
            Ok(text) => self.notify(text),
            Err(text) => {
                self.events.push(crate::events::GameEvent::Denied);
                self.notify(text);
            }
        }
    }
}

#[cfg(test)]
fn run_cheat_for_tests(run: &mut crate::run::Run, cheat: Cheat) {
    run.cheat(cheat, &mut Vec::new()).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Command, GameInput};

    /// A game on the surface of its first drop, as the Engineer.
    fn on_surface() -> Game {
        let profile = crate::Profile {
            class: crate::content::ClassId::from_key("engineer").unwrap(),
            ..crate::Profile::default()
        };
        let mut game = Game::new(profile, 7);
        let mut pilot = crate::autoplay::Autopilot::new();
        for _ in 0..60 * 60 * 3 {
            if game.phase == Phase::Surface && game.run.as_ref().is_some_and(|r| r.ticks > 30) {
                break;
            }
            let input = pilot.input(&game);
            game.update(&input);
        }
        assert_eq!(game.phase, Phase::Surface);
        let run = game.run.as_mut().unwrap();
        run.spawning_enabled = false;
        // The tutorial drop is the Striker's; hand over the sentries.
        let sentry = WeaponId::from_key("sentry").unwrap();
        run.player_mut().weapons.truncate(1);
        if !run.player().carries(sentry) {
            super::run_cheat_for_tests(run, Cheat::GiveWeapon(sentry));
        }
        game
    }

    fn run(game: &mut Game, line: &str) {
        let Ok(Parsed::Cheat(cheat)) = parse(line, game) else {
            panic!("{line}: {:?}", parse(line, game));
        };
        game.update(&GameInput {
            command: Some(Command::Cheat(cheat)),
            ..GameInput::default()
        });
    }

    #[test]
    fn suggestions_follow_the_line() {
        let game = on_surface();
        let s = suggest("/", &game);
        assert!(s.items.iter().any(|(w, _)| w == "give"));
        assert!(s.items.iter().any(|(w, _)| w == "spawn"));
        let s = suggest("/give ", &game);
        let words: Vec<&str> = s.items.iter().map(|(w, _)| w.as_str()).collect();
        assert!(words.contains(&"weapon") && words.contains(&"upgrade"));
        let s = suggest("/give we", &game);
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.from, "/give ".len());
        // Only upgrades that fit the named weapon.
        let s = suggest("/give upgrade sentry ", &game);
        assert!(s.items.iter().any(|(w, _)| w == "mobile-sentry"));
        assert!(!s.items.iter().any(|(w, _)| w == "second-ring"));
        assert!(s.usage[0].contains("<upgrade>"));
    }

    #[test]
    fn give_sentry_and_its_upgrades() {
        let mut game = on_surface();
        run(&mut game, "/give weapon orbit-drones");
        let p = game.run.as_ref().unwrap().player();
        assert!(p.carries(WeaponId::from_key("orbit-drones").unwrap()));
        run(&mut game, "/give upgrade sentry mobile-sentry");
        run(&mut game, "/give upgrade sentry extra-payload rare 3");
        let sentry = WeaponId::from_key("sentry").unwrap();
        let p = game.run.as_ref().unwrap().player();
        let slot = p.weapons.iter().find(|w| w.kind == sentry).unwrap();
        assert_eq!(slot.stacks(ModId::from_key("mobile-sentry").unwrap()), 1);
        assert_eq!(slot.stacks(ModId::from_key("extra-payload").unwrap()), 3);
        // Errors explain themselves.
        assert!(parse("/give upgrade sentry second-ring", &game).is_err());
        assert!(parse("/give weapon sentry", &game).is_err());
        assert!(parse("/frobnicate", &game).is_err());
        assert!(matches!(parse("/help", &game), Ok(Parsed::Help(_))));
    }

    #[test]
    fn spawn_kill_and_materials() {
        let mut game = on_surface();
        run(&mut game, "/spawn husk 5");
        let run_ = game.run.as_ref().unwrap();
        assert!(run_.enemies.len() >= 5);
        run(&mut game, "/kill");
        game.update(&GameInput::default());
        assert!(
            game.run
                .as_ref()
                .unwrap()
                .enemies
                .iter()
                .all(|e| e.health <= 0.0)
        );
        let before = game.profile.materials.lumite;
        run(&mut game, "/give material lumite 50");
        assert_eq!(game.profile.materials.lumite, before + 50);
    }
}
