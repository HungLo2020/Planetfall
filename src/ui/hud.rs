//! Heads-up display: rebuilt from game state every frame (cheap for a small tree).

use super::Layer;
use super::{Builder, device_text, format_materials};
use planetfall_sim::content::Material;
use planetfall_sim::run::{EXTRACTION_HOLD, Extraction};
use planetfall_sim::{Game, Phase};
use quasar_ui::{LayoutItem, Length, TextMeasure, TextStyle, Vec2};

pub struct HudContext<'a> {
    pub game: &'a Game,
    pub measure: &'a dyn TextMeasure,
    /// Physical viewport size.
    pub size: (f32, f32),
    pub scale: f32,
    pub gamepad: bool,
    /// Set while a terminal/menu covers the centre of the screen.
    pub menu_open: bool,
}

fn at(x: f32, y: f32, width: f32) -> LayoutItem {
    LayoutItem::size(Length::fixed(width), Length::Auto).with_offset(Vec2::new(x, y))
}

pub fn build(cx: &HudContext) -> Layer {
    let mut b = Builder::new(cx.measure, cx.scale);
    let root = b.root(false);
    let (w, h) = (cx.size.0 / cx.scale, cx.size.1 / cx.scale);
    let game = cx.game;
    let text = |s: &str| device_text(s, cx.gamepad);
    match game.phase {
        Phase::Ship => {
            let panel = b.card(root, at(18.0, 16.0, 330.0), 2.0, 10.0);
            b.label(panel, "THE MAYFLY  //  SALVAGE CUTTER", TextStyle::Body);
            let status = match game.profile.orbiting {
                Some(p) => format!("In orbit: {} ({})", p.info().name, p.info().kind),
                None => "Adrift in deep space".into(),
            };
            b.label(panel, status, TextStyle::Small);
            let hold = b.card(root, at(w - 18.0 - 360.0, 16.0, 360.0), 2.0, 10.0);
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
                return b.finish();
            };
            let info = run.planet.info();
            let secs = run.ticks / 60;
            let status = b.card(root, at(18.0, 16.0, 320.0), 4.0, 10.0);
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
            let p = &run.player;
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
            for wpn in &run.weapons {
                if !weapons.is_empty() {
                    weapons.push_str("  ·  ");
                }
                weapons.push_str(&format!("{} {}", wpn.kind.name(), roman(wpn.level)));
            }
            b.label(status, weapons, TextStyle::Small);

            let cargo = b.card(root, at(w - 18.0 - 340.0, 16.0, 340.0), 3.0, 10.0);
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
                        format!("HOLD THE ZONE  ·  {}%", hold * 100 / EXTRACTION_HOLD)
                    }
                    Extraction::Departed { .. } => "EXTRACTED".into(),
                }
            };
            b.label(cargo, objective, TextStyle::Body);
            if matches!(game.phase, Phase::Lost { .. }) {
                let lost = b.card(root, at(w * 0.5 - 220.0, h * 0.36, 440.0), 4.0, 18.0);
                b.label(lost, "SIGNAL LOST", TextStyle::Heading);
                b.label(
                    lost,
                    "Suit beacon recalled you. Cargo was not recovered.",
                    TextStyle::Small,
                );
            }
        }
        Phase::Ftl { to, .. } => {
            let panel = b.card(root, at(w * 0.5 - 200.0, h - 120.0, 400.0), 2.0, 12.0);
            b.label(panel, "FTL JUMP IN PROGRESS", TextStyle::Body);
            b.label(
                panel,
                format!("Destination: {} · {}", to.info().name, to.info().kind),
                TextStyle::Small,
            );
        }
        Phase::Descent { .. } => {
            let Some(run) = &game.run else {
                return b.finish();
            };
            let panel = b.card(root, at(w * 0.5 - 200.0, h - 120.0, 400.0), 2.0, 12.0);
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

    // Bottom centre: tutorial prompt, notices and interaction hints.
    if !cx.menu_open {
        let width = 620.0;
        let mut y = h - 24.0;
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
        // Lay blocks out bottom-up using measured line counts.
        for (title, body) in blocks.into_iter().rev() {
            let size = b.ui.theme().size(TextStyle::Body) * cx.scale;
            let lines = super::wrap(
                cx.measure,
                &body,
                TextStyle::Body,
                size,
                (width - 28.0) * cx.scale,
            )
            .len();
            let height = 24.0 + lines as f32 * 21.0 + if title.is_some() { 22.0 } else { 0.0 };
            y -= height + 8.0;
            let panel = b.card(root, at(w * 0.5 - width * 0.5, y, width), 1.0, 12.0);
            if let Some(title) = title {
                b.label(panel, title, TextStyle::Small);
            }
            b.paragraph(panel, &body, TextStyle::Body, width - 28.0);
        }
    }
    b.finish()
}

pub fn roman(n: u8) -> &'static str {
    ["", "I", "II", "III", "IV", "V"]
        .get(n as usize)
        .copied()
        .unwrap_or("V+")
}
