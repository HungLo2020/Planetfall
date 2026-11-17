//! Heads-up display: rebuilt from game state every frame (cheap for a small tree).

use super::Layer;
use super::{Builder, format_materials};
use crate::bindings::Prompts;
use planetfall_sim::content::{Material, ObjectiveKind, content};
use planetfall_sim::mission::hazard_name;
use planetfall_sim::run::{Extraction, Run, extraction_hold};
use planetfall_sim::{Game, Phase};
use quasar_ui::{
    Anchor, Color, Edges, LayoutItem, Length, NodeStyle, TextMeasure, TextStyle, Vec2,
};

pub struct HudContext<'a> {
    pub game: &'a Game,
    pub measure: &'a dyn TextMeasure,
    /// Physical viewport size.
    pub size: (f32, f32),
    pub scale: f32,
    pub prompts: &'a Prompts,
    /// Set while a terminal/menu covers the centre of the screen.
    pub menu_open: bool,
}

/// A fixed-width card pinned to a screen edge or corner.
fn pinned(anchor: Anchor, width: f32) -> LayoutItem {
    LayoutItem::size(Length::fixed(width), Length::Auto)
        .with_anchor(anchor)
        .with_margin(Edges::new(18.0, 16.0, 18.0, 16.0))
}

pub fn build(cx: &HudContext) -> Layer {
    let mut b = Builder::default();
    let root = b.root(false);
    let game = cx.game;
    let text = |s: &str| cx.prompts.format(s);
    match game.phase {
        Phase::Ship => {
            let panel = b.card(root, pinned(Anchor::TopLeft, 330.0), 2.0, 10.0);
            b.label(panel, "THE MAYFLY  //  SALVAGE CUTTER", TextStyle::Body);
            let status = match game.profile.orbiting {
                _ if game.profile.docked => format!(
                    "Docked: Kestrel Exchange  ·  {} credits",
                    game.profile.credits
                ),
                Some(p) => format!("In orbit: {} ({})", p.info().name, p.info().kind),
                None => "Adrift in deep space".into(),
            };
            b.label(panel, status, TextStyle::Small);
            let hold = b.card(root, pinned(Anchor::TopRight, 360.0), 2.0, 10.0);
            b.label(hold, "CARGO HOLD", TextStyle::Body);
            b.label(
                hold,
                format_materials(&game.profile.materials),
                TextStyle::Small,
            );
            let stats = &game.profile.stats;
            b.label(
                hold,
                format!(
                    "Drops {}  ·  Extractions {}  ·  Kills {}",
                    stats.drops, stats.extractions, stats.kills
                ),
                TextStyle::Small,
            );
        }
        Phase::Surface | Phase::Lost { .. } | Phase::Ascent { .. } => {
            let Some(run) = &game.run else {
                return b.finish_laid_out(cx.size, cx.scale, cx.measure);
            };
            let info = run.planet.info();
            let secs = run.ticks / 60;
            let status = b.card(root, pinned(Anchor::TopLeft, 320.0), 4.0, 10.0);
            b.label(
                status,
                format!(
                    "{}  ·  {:02}:{:02}",
                    info.name.to_uppercase(),
                    secs / 60,
                    secs % 60
                ),
                TextStyle::Body,
            );
            let p = &run.player();
            b.label(
                status,
                format!("HULL {:.0} / {:.0}", p.health.max(0.0), p.max_health),
                TextStyle::Small,
            );
            b.progress(status, "", p.health.max(0.0), p.max_health);
            b.label(
                status,
                format!("LEVEL {}  ·  XP {} / {}", p.level, p.xp, p.xp_needed()),
                TextStyle::Small,
            );
            b.progress(status, "", p.xp as f32, p.xp_needed() as f32);
            let mut weapons = String::new();
            for wpn in &p.weapons {
                if !weapons.is_empty() {
                    weapons.push_str("  ·  ");
                }
                weapons.push_str(&format!("{} Lv{}", wpn.kind.name(), wpn.level()));
            }
            b.paragraph(status, &weapons, TextStyle::Small);
            // Class ability: ready prompt or recharge bar.
            let ability = &p.class.info().ability;
            let charges = if p.ability.max_charges > 1 {
                format!(" ×{}", p.ability.charges)
            } else {
                String::new()
            };
            let line = if p.ability.charges > 0 {
                text(&format!(
                    "{}  ·  {} READY{charges}  {{ability}}",
                    p.class.name().to_uppercase(),
                    ability.name.to_uppercase()
                ))
            } else {
                format!(
                    "{}  ·  {}  {}s",
                    p.class.name().to_uppercase(),
                    ability.name.to_uppercase(),
                    p.ability.cooldown.div_ceil(60)
                )
            };
            b.label(status, line, TextStyle::Small);
            let total = p.ability.cooldown_total.max(1);
            b.progress(
                status,
                "",
                (total - p.ability.cooldown.min(total)) as f32,
                total as f32,
            );
            // Torch heat.
            if let Some(w) = p
                .weapons
                .iter()
                .find(|w| w.kind.info().behavior == planetfall_sim::content::WeaponBehavior::Flamer)
            {
                let label = if w.overheated {
                    format!("{}  OVERHEATED", w.kind.name().to_uppercase())
                } else {
                    format!("{}  HEAT", w.kind.name().to_uppercase())
                };
                b.label(status, label, TextStyle::Small);
                b.progress(status, "", w.heat, 1.0);
            }
            // Thin air: the oxygen meter.
            if run.thin_air() {
                let max = planetfall_sim::content::content().tuning.hazards.oxygen_max;
                let line = if p.oxygen <= 0.0 {
                    "OXYGEN  EMPTY  ·  get to a vent or the pod".to_owned()
                } else {
                    format!("OXYGEN  {:.0}%", p.oxygen / max * 100.0)
                };
                b.label(status, line, TextStyle::Small);
                b.progress(status, "", p.oxygen, max);
            }
            // Statuses from hazards and disruptors.
            let mut effects = Vec::new();
            if p.jammed > 0 {
                effects.push("JAMMED (no ability, slow weapons)");
            }
            if p.chilled > 0 {
                effects.push("CHILLED");
            }
            if p.wet > 0 {
                effects.push("WET");
            }
            if !effects.is_empty() {
                b.label(status, effects.join("  ·  "), TextStyle::Small);
            }

            let cargo = b.card(root, pinned(Anchor::TopRight, 340.0), 3.0, 10.0);
            b.label(cargo, "CARGO (lost if you fall)", TextStyle::Small);
            for m in Material::ALL {
                let n = run.carried.get(m);
                if n > 0 || m != Material::Voidstone {
                    b.label(cargo, format!("{:<10} {}", m.name(), n), TextStyle::Body);
                }
            }
            let objective = if !run.objective_done() {
                format!("OBJECTIVE  {}", primary_status(run))
            } else {
                match run.extraction {
                    Extraction::Idle => text("OBJECTIVE COMPLETE  ·  {extract} call extraction"),
                    Extraction::Inbound { ticks, .. } => {
                        format!("EXTRACTION POD INBOUND  ·  {}s", ticks.div_ceil(60))
                    }
                    Extraction::Waiting { hold, .. } => {
                        format!("HOLD THE ZONE  ·  {}%", hold * 100 / extraction_hold())
                    }
                    Extraction::Departed { .. } => "EXTRACTED".into(),
                }
            };
            b.paragraph(cargo, &objective, TextStyle::Body);
            let m = &run.mission;
            if let (Some(secondary), Some(progress)) = (m.contract.secondary, m.secondary) {
                let line = if progress.done {
                    format!("OPTIONAL  {} ✓", secondary.name())
                } else {
                    format!(
                        "OPTIONAL  {}  {} / {}",
                        secondary.name(),
                        progress.count,
                        progress.need
                    )
                };
                b.label(cargo, line, TextStyle::Small);
            }
            if !run.tutorial {
                let mut tags = format!(
                    "Hazard {} {}  ·  {}",
                    m.hazard,
                    hazard_name(m.hazard),
                    m.contract.site.name()
                );
                if let Some(mutator) = m.contract.mutator {
                    tags.push_str(&format!("  ·  {}", mutator.name()));
                }
                b.label(cargo, tags, TextStyle::Small);
            }
            // A boss or hunted elite gets a health bar of its own.
            if let Some(e) = m
                .target
                .and_then(|id| run.enemies.iter().find(|e| e.id == id))
            {
                let bar = b.card(
                    root,
                    LayoutItem::size(Length::fixed(460.0), Length::Auto)
                        .with_anchor(Anchor::Bottom)
                        .with_offset(Vec2::new(0.0, -120.0)),
                    3.0,
                    8.0,
                );
                b.label(bar, e.kind.info().name.to_uppercase(), TextStyle::Small);
                b.progress(bar, "", e.health.max(0.0), e.max_health);
            }
            // Threat: the level, progress to the next, and surge warnings.
            if !run.tutorial {
                let level = run.threat_level();
                b.label(
                    cargo,
                    format!("THREAT {level}  ·  next in {}", threat_eta(run)),
                    TextStyle::Small,
                );
                b.progress(cargo, "", run.threat.fract(), 1.0);
                if run.surge.warning > 0 {
                    let warn = b.card(
                        root,
                        LayoutItem::size(Length::fixed(420.0), Length::Auto)
                            .with_anchor(Anchor::Top)
                            .with_offset(Vec2::new(0.0, 16.0)),
                        2.0,
                        10.0,
                    );
                    let _ = b.ui.set_style(
                        warn,
                        Some(
                            NodeStyle::filled(Color::rgba(60, 8, 8, 220))
                                .with_border(2.0, Color::rgba(255, 70, 50, 255))
                                .with_radius(3.0),
                        ),
                    );
                    b.label(
                        warn,
                        format!("SURGE INCOMING  ·  {}s", run.surge.warning.div_ceil(60)),
                        TextStyle::Body,
                    );
                    b.label(warn, "A wave is coming from every side.", TextStyle::Small);
                } else if run.surge.active > 0 {
                    let warn = b.card(
                        root,
                        LayoutItem::size(Length::fixed(300.0), Length::Auto)
                            .with_anchor(Anchor::Top)
                            .with_offset(Vec2::new(0.0, 16.0)),
                        2.0,
                        8.0,
                    );
                    b.label(warn, "SURGE", TextStyle::Body);
                }
                if run.quake.warning > 0 {
                    let warn = b.card(
                        root,
                        LayoutItem::size(Length::fixed(360.0), Length::Auto)
                            .with_anchor(Anchor::Top)
                            .with_offset(Vec2::new(0.0, 176.0)),
                        2.0,
                        8.0,
                    );
                    let _ = b.ui.set_style(
                        warn,
                        Some(
                            NodeStyle::filled(Color::rgba(40, 26, 8, 220))
                                .with_border(2.0, Color::rgba(255, 170, 40, 255))
                                .with_radius(3.0),
                        ),
                    );
                    b.label(
                        warn,
                        format!("QUAKE  ·  {}s", run.quake.warning.div_ceil(60)),
                        TextStyle::Body,
                    );
                    b.label(warn, "Rubble will fall nearby.", TextStyle::Small);
                }
                // The latest find, kept even if the drop goes wrong.
                if run.find_banner > 0
                    && let Some(find) = run.finds.last()
                {
                    let color = match find {
                        planetfall_sim::run::Find::Gear(g) => g.rarity,
                        planetfall_sim::run::Find::Weapon { .. } => {
                            planetfall_sim::content::Rarity::Legendary
                        }
                    };
                    let banner = b.card(
                        root,
                        LayoutItem::size(Length::fixed(420.0), Length::Auto)
                            .with_anchor(Anchor::Top)
                            .with_offset(Vec2::new(0.0, 96.0)),
                        2.0,
                        8.0,
                    );
                    let _ = b.ui.set_style(
                        banner,
                        Some(
                            NodeStyle::filled(Color::rgba(10, 14, 22, 220))
                                .with_border(2.0, crate::ui::rarity_color(color))
                                .with_radius(3.0),
                        ),
                    );
                    b.label(banner, format!("FOUND: {}", find.name()), TextStyle::Body);
                    b.label(
                        banner,
                        "Sent to the stash. Finds are kept even if you fall.",
                        TextStyle::Small,
                    );
                }
            }
            if matches!(game.phase, Phase::Lost { .. }) {
                let lost = b.card(
                    root,
                    LayoutItem::size(Length::fixed(440.0), Length::Auto)
                        .with_anchor(Anchor::Center)
                        .with_offset(Vec2::new(0.0, -60.0)),
                    4.0,
                    18.0,
                );
                b.label(lost, "SIGNAL LOST", TextStyle::Heading);
                b.label(
                    lost,
                    "Suit beacon recalled you. Cargo was not recovered.",
                    TextStyle::Small,
                );
            }
        }
        Phase::Ftl { to, .. } => {
            let panel = b.card(root, pinned(Anchor::Bottom, 400.0), 2.0, 12.0);
            b.label(panel, "FTL JUMP IN PROGRESS", TextStyle::Body);
            b.label(
                panel,
                match to {
                    Some(to) => format!("Destination: {} · {}", to.info().name, to.info().kind),
                    None => "Destination: Kestrel Exchange · trade station".to_owned(),
                },
                TextStyle::Small,
            );
        }
        Phase::Descent { .. } => {
            let Some(run) = &game.run else {
                return b.finish_laid_out(cx.size, cx.scale, cx.measure);
            };
            let panel = b.card(root, pinned(Anchor::Bottom, 400.0), 2.0, 12.0);
            b.label(panel, "DROP POD DESCENDING", TextStyle::Body);
            b.label(
                panel,
                format!(
                    "Target: {} · hazard x{:.1}",
                    run.planet.info().name,
                    run.planet.info().hazard
                ),
                TextStyle::Small,
            );
        }
    }

    // Bottom centre: tutorial prompt, notices and interaction hints, stacked
    // upward from the bottom edge; labels wrap at the card width.
    if !cx.menu_open {
        let mut blocks: Vec<(Option<String>, String)> = Vec::new();
        if let Some(prompt) = game.tutorial_prompt() {
            blocks.push((Some(prompt.title.to_string()), text(&prompt.body)));
        }
        if let Some(notice) = &game.notice {
            blocks.push((None, notice.text.clone()));
        }
        if let Some(hint) = game.interaction_hint() {
            blocks.push((None, text(&hint)));
        }
        if !blocks.is_empty() {
            let column = b.panel(root, pinned(Anchor::Bottom, 620.0), 8.0, 0.0);
            for (title, body) in blocks {
                let card = b.card(
                    column,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    1.0,
                    12.0,
                );
                if let Some(title) = title {
                    b.label(card, title, TextStyle::Small);
                }
                b.paragraph(card, &body, TextStyle::Body);
            }
        }
    }
    b.finish_laid_out(cx.size, cx.scale, cx.measure)
}

/// The primary objective's progress in words.
fn primary_status(run: &Run) -> String {
    let m = &run.mission;
    let name = m.contract.primary.name();
    let p = m.primary;
    match m.primary_kind() {
        ObjectiveKind::OreQuota => format!("Mine lumite {} / {}", p.count, p.need),
        ObjectiveKind::Boss | ObjectiveKind::Hunt => {
            match m
                .target
                .and_then(|id| run.enemies.iter().find(|e| e.id == id))
            {
                Some(e) => format!(
                    "{name}  ·  {:.0}%",
                    100.0 * e.health.max(0.0) / e.max_health.max(1.0)
                ),
                None => format!("{name}  ·  find it"),
            }
        }
        ObjectiveKind::Rescue => match m.survivor {
            Some(s) if s.down => "Survivor down! Stand close to revive".into(),
            Some(s) if s.following => "Escort the survivor to the pod".into(),
            _ => "Find the stranded survivor".into(),
        },
        ObjectiveKind::Defend => match m.rig {
            Some(r) if !r.active => "Stand at the drill rig to start it".into(),
            Some(r) if r.broken() => "The rig is down! Stand close to repair".into(),
            Some(r) => format!(
                "Drilling  {}%  ·  rig {:.0}%",
                r.drilled * 100 / r.need.max(1),
                100.0 * r.health / r.max_health.max(1.0)
            ),
            None => name.into(),
        },
        _ => format!("{name}  {} / {}", p.count, p.need),
    }
}

/// Time until the next threat level at the current rate ("38s").
fn threat_eta(run: &Run) -> String {
    let t = &content().tuning.threat;
    let rate = if matches!(run.extraction, Extraction::Idle) {
        t.per_minute
    } else {
        t.per_minute * t.extraction_rate
    };
    let seconds = (1.0 - run.threat.fract()) / rate.max(1e-3) * 60.0;
    format!("{}s", seconds.ceil() as u32)
}

pub fn roman(n: u8) -> &'static str {
    ["", "I", "II", "III", "IV", "V"]
        .get(n as usize)
        .copied()
        .unwrap_or("V+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use planetfall_sim::Profile;

    /// The HUD comes back ready to draw: the live game draws it without any
    /// further layout, so the tutorial prompt must already have a place on
    /// screen.
    #[test]
    fn hud_is_laid_out_with_the_tutorial_prompt() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let (measure, _text) = super::super::build_text(&root).unwrap();
        let game = Game::new(Profile::default(), 1);
        let prompt = game
            .tutorial_prompt()
            .expect("a new profile starts the tutorial");
        let layer = build(&HudContext {
            game: &game,
            measure: &measure,
            size: (1600.0, 900.0),
            scale: 1.0,
            prompts: &Prompts::default(),
            menu_open: false,
        });
        let entries = layer.ui.layout_entries();
        assert!(!entries.is_empty(), "HUD has no layout");
        let shown = entries.iter().any(|e| {
            e.rect.width > 0.0
                && e.rect.height > 0.0
                && e.rect.bottom() <= 900.0
                && layer.ui.node(e.id).is_some_and(|n| {
                    matches!(&n.widget, quasar_ui::Widget::Label { text, .. }
                        if layer.ui.resolve_text(text) == prompt.title)
                })
        });
        assert!(shown, "tutorial title is not on screen");
    }
}
