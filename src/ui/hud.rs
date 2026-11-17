//! Heads-up display: rebuilt from game state every frame (cheap for a small tree).

use super::Layer;
use super::{Builder, format_materials};
use crate::bindings::Prompts;
use planetfall_sim::content::Material;
use planetfall_sim::run::{Extraction, extraction_hold};
use planetfall_sim::{Game, Phase};
use quasar_ui::{Anchor, Edges, LayoutItem, Length, TextMeasure, TextStyle, Vec2};

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

            let cargo = b.card(root, pinned(Anchor::TopRight, 340.0), 3.0, 10.0);
            b.label(cargo, "CARGO (lost if you fall)", TextStyle::Small);
            for m in Material::ALL {
                let n = run.carried.get(m);
                if n > 0 || m != Material::Voidstone {
                    b.label(cargo, format!("{:<10} {}", m.name(), n), TextStyle::Body);
                }
            }
            let objective = if !run.objective_done {
                format!(
                    "OBJECTIVE  Mine lumite {} / {}",
                    run.carried.lumite.min(run.objective),
                    run.objective
                )
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
            b.label(cargo, objective, TextStyle::Body);
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
                format!("Destination: {} · {}", to.info().name, to.info().kind),
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
