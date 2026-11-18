//! Heads-up display: rebuilt from game state every frame (cheap for a small tree).
//!
//! On a drop: objectives, timer and threat top-centre; cargo top-right;
//! vitals (class emblem, hull, XP, oxygen, statuses) bottom-left; the
//! weapon and ability bar bottom-centre; alerts, the boss bar, finds and the
//! ship AI around them. On the ship: the ship card and the cargo hold.

use super::kit::{self, ACCENT, BAD, DIM, GOOD, INFO, WARN};
use super::{Builder, Layer};
use crate::bindings::Prompts;
use planetfall_sim::content::{AbilityKind, Material, ObjectiveKind, content};
use planetfall_sim::mission::hazard_name;
use planetfall_sim::run::{Extraction, Run, extraction_hold};
use planetfall_sim::{Game, Phase};
use quasar_ui::{
    Align, Anchor, Color, Edges, LayoutItem, Length, NodeId, TextMeasure, TextStyle, Vec2,
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

/// The icon of an objective kind.
pub fn objective_icon(kind: ObjectiveKind) -> &'static str {
    match kind {
        ObjectiveKind::OreQuota => "obj-ore_quota",
        ObjectiveKind::Boss => "obj-boss",
        ObjectiveKind::Rescue => "obj-rescue",
        ObjectiveKind::Defend => "obj-defend",
        ObjectiveKind::Salvage => "obj-salvage",
        ObjectiveKind::Hunt => "obj-hunt",
        ObjectiveKind::Glowcaps => "obj-glowcaps",
        ObjectiveKind::Cull => "obj-cull",
        ObjectiveKind::Scan => "obj-scan",
    }
}

pub fn ability_icon(kind: AbilityKind) -> &'static str {
    match kind {
        AbilityKind::Blink => "ability-blink",
        AbilityKind::Bore => "ability-bore",
        AbilityKind::Barrier => "ability-barrier",
        AbilityKind::Dome => "ability-dome",
    }
}

pub fn material_icon(m: Material) -> &'static str {
    match m {
        Material::Ferrite => "ferrite",
        Material::Lumite => "lumite",
        Material::Voidstone => "voidstone",
    }
}

/// Colour of a hazard level, from easy green to lethal violet.
pub fn hazard_color(level: u8) -> Color {
    match level {
        0 | 1 => GOOD,
        2 => WARN,
        3 => ACCENT,
        4 => BAD,
        _ => Color::rgb(200, 110, 255),
    }
}

pub fn build(cx: &HudContext) -> Layer {
    let mut b = Builder {
        family: cx.prompts.family,
        ..Builder::default()
    };
    let root = b.root(false);
    let game = cx.game;
    let text = |s: &str| cx.prompts.format(s);
    let mut bottom_offset = 0.0;
    match game.phase {
        Phase::Ship => ship(&mut b, root, game),
        Phase::Surface | Phase::Lost { .. } | Phase::Ascent { .. } => {
            let Some(run) = &game.run else {
                return b.finish_laid_out(cx.size, cx.scale, cx.measure);
            };
            top(&mut b, root, run);
            objectives(&mut b, root, run, &text);
            cargo(&mut b, root, run);
            loadout(&mut b, root, run, &text);
            bottom_offset = -6.0;
            alerts(&mut b, root, run);
            boss_bar(&mut b, root, run);
            find_banner(&mut b, root, run);
            if matches!(game.phase, Phase::Lost { .. }) {
                b.alert(
                    root,
                    LayoutItem::size(Length::fixed(480.0), Length::Auto)
                        .with_anchor(Anchor::Center)
                        .with_offset(Vec2::new(0.0, -60.0)),
                    BAD,
                    "threat",
                    "SIGNAL LOST",
                    "PENNY recalled your suit. The cargo stays down there.",
                );
            }
        }
        Phase::Ftl { to, .. } => {
            let (icon, line) = match to {
                Some(to) => (
                    format!("planet-{}", to.key()),
                    format!("{} · {}", to.info().name, to.info().kind),
                ),
                None => (
                    "planet-station".to_owned(),
                    "Kestrel Exchange · trade station".to_owned(),
                ),
            };
            transit(&mut b, root, &icon, "FTL JUMP · PLEASE HOLD", &line);
        }
        Phase::Descent { .. } => {
            if let Some(run) = &game.run {
                let info = run.planet.info();
                transit(
                    &mut b,
                    root,
                    "pod",
                    "DROP POD DESCENDING",
                    &format!(
                        "{} · hazard {} {}",
                        info.name,
                        run.mission.hazard,
                        hazard_name(run.mission.hazard)
                    ),
                );
            }
        }
    }

    // Bottom centre: the ship AI, notices and interaction hints, stacked
    // upward above the weapon bar; labels wrap at the card width.
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
            let column = b.panel(
                root,
                pinned(Anchor::Bottom, 540.0).with_offset(Vec2::new(0.0, bottom_offset)),
                8.0,
                0.0,
            );
            for (title, body) in blocks {
                let card = b.hrow(
                    column,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    12.0,
                    12.0,
                );
                b.style(card, Color::rgba(8, 13, 20, 190), None, 3.0);
                let tab = b.strut(
                    card,
                    LayoutItem::size(Length::fixed(3.0), Length::Fill),
                    3.0,
                    0.0,
                );
                b.style(tab, ACCENT, None, 0.0);
                let col = b.column(card, LayoutItem::size(Length::Fill, Length::Auto), 2.0, 0.0);
                if let Some(title) = title {
                    b.label(col, title, TextStyle::Small);
                }
                b.rich(col, &body, TextStyle::Body, 490.0);
            }
        }
    }
    b.finish_laid_out(cx.size, cx.scale, cx.measure)
}

fn ship(b: &mut Builder, root: NodeId, game: &Game) {
    let card = b.card(root, pinned(Anchor::TopLeft, 360.0), 4.0, 12.0);
    let head = b.hrow(
        card,
        LayoutItem::size(Length::Fill, Length::Auto),
        10.0,
        0.0,
    );
    b.glyph(head, "pod", 40.0);
    let col = b.column(head, LayoutItem::size(Length::Fill, Length::Auto), 0.0, 0.0);
    b.tinted(col, "THE MAYFLY", TextStyle::Body, ACCENT);
    b.label(col, "Salvage cutter", TextStyle::Small);
    b.rule(card);
    let (icon, status) = match game.profile.orbiting {
        _ if game.profile.docked => (
            "planet-station".to_owned(),
            "Docked at the Kestrel Exchange".to_owned(),
        ),
        Some(p) => (
            format!("planet-{}", p.key()),
            format!("Orbiting {} · {}", p.info().name, p.info().kind),
        ),
        None => ("star-map".to_owned(), "Adrift in deep space".to_owned()),
    };
    let row = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
    b.glyph(row, &icon, 22.0);
    b.label(row, status, TextStyle::Small);

    let hold = b.card(root, pinned(Anchor::TopRight, 330.0), 6.0, 12.0);
    b.section(hold, "Cargo hold");
    let row = b.hrow(
        hold,
        LayoutItem::size(Length::Fill, Length::Auto),
        14.0,
        0.0,
    );
    for m in Material::ALL {
        b.stat(
            row,
            material_icon(m),
            game.profile.materials.get(m).to_string(),
            TextStyle::Body,
            None,
        );
    }
    b.stat(
        row,
        "credits",
        game.profile.credits.to_string(),
        TextStyle::Body,
        Some(WARN),
    );
    let stats = &game.profile.stats;
    b.label(
        hold,
        format!(
            "{} drops  ·  {} extractions  ·  {} kills",
            stats.drops, stats.extractions, stats.kills
        ),
        TextStyle::Small,
    );
}

/// Soft dark backing for frameless HUD text.
const BACKING: Color = Color::rgba(2, 5, 10, 120);

/// Top edge: a thin full-width XP bar with the level, and under it one
/// centred line with the drop timer and the threat level.
fn top(b: &mut Builder, root: NodeId, run: &Run) {
    let p = run.player();
    let column = b.column(
        root,
        LayoutItem::size(Length::Fill, Length::Auto).with_anchor(Anchor::Top),
        4.0,
        0.0,
    );
    let xp = b.hrow(
        column,
        LayoutItem::size(Length::Fill, Length::Auto),
        8.0,
        4.0,
    );
    b.style(xp, BACKING, None, 0.0);
    b.tinted_fit(xp, format!("LV {}", p.level), TextStyle::Small, INFO);
    b.bar(
        xp,
        Length::Fill,
        7.0,
        p.xp as f32 / p.xp_needed().max(1) as f32,
        INFO,
    );
    let line = b.hrow(
        column,
        LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
        14.0,
        5.0,
    );
    b.style(line, BACKING, None, 3.0);
    let secs = run.ticks / 60;
    b.stat(
        line,
        "timer",
        format!("{:02}:{:02}", secs / 60, secs % 60),
        TextStyle::Body,
        None,
    );
    if !run.tutorial {
        let level = run.threat_level();
        let color = if level >= 6 {
            BAD
        } else if level >= 3 {
            WARN
        } else {
            kit::TEXT
        };
        b.stat(
            line,
            "threat",
            level.to_string(),
            TextStyle::Body,
            Some(color),
        );
        b.bar(line, Length::fixed(56.0), 4.0, run.threat.fract(), BAD);
    }
}

/// One frameless HUD line with a soft backing, aligned left or right.
fn line(b: &mut Builder, column: NodeId, align: Align) -> NodeId {
    let row = b.hrow(
        column,
        LayoutItem::size(Length::Auto, Length::Auto).with_align(align),
        7.0,
        4.0,
    );
    b.style(row, BACKING, None, 3.0);
    row
}

/// Top left: the objectives, one quiet line each.
fn objectives(b: &mut Builder, root: NodeId, run: &Run, text: &dyn Fn(&str) -> String) {
    let column = b.column(
        root,
        LayoutItem::size(Length::Auto, Length::Auto)
            .with_anchor(Anchor::TopLeft)
            .with_margin(Edges::new(16.0, 30.0, 0.0, 0.0)),
        4.0,
        0.0,
    );
    let m = &run.mission;
    let start = Align::Start;
    if !run.objective_done() {
        let row = line(b, column, start);
        b.glyph(row, objective_icon(m.primary_kind()), 26.0);
        b.label_fit(row, primary_status(run), TextStyle::Body);
    } else {
        match run.extraction {
            Extraction::Idle => {
                let row = line(b, column, start);
                b.glyph(row, "check", 22.0);
                b.tinted_fit(row, "Objective complete", TextStyle::Body, GOOD);
                let row = line(b, column, start);
                b.rich(
                    row,
                    &text("{extract} call extraction"),
                    TextStyle::Small,
                    260.0,
                );
            }
            Extraction::Inbound { ticks, .. } => {
                let row = line(b, column, start);
                b.glyph(row, "pod", 24.0);
                b.tinted_fit(
                    row,
                    format!("Pod inbound · {}s", ticks.div_ceil(60)),
                    TextStyle::Body,
                    WARN,
                );
            }
            Extraction::Waiting { hold, .. } => {
                let row = line(b, column, start);
                b.glyph(row, "pod", 24.0);
                b.tinted_fit(row, "Hold the zone", TextStyle::Body, WARN);
                b.bar(
                    row,
                    Length::fixed(90.0),
                    6.0,
                    hold as f32 / extraction_hold().max(1) as f32,
                    WARN,
                );
            }
            Extraction::Departed { .. } => {
                let row = line(b, column, start);
                b.glyph(row, "check", 22.0);
                b.tinted_fit(row, "Extracted", TextStyle::Body, GOOD);
            }
        }
    }
    if let (Some(secondary), Some(progress)) = (m.contract.secondary, m.secondary) {
        let row = line(b, column, start);
        b.glyph(row, objective_icon(secondary.info().kind), 18.0);
        if progress.done {
            b.tinted_fit(
                row,
                secondary.name(),
                TextStyle::Small,
                Color::rgba(110, 225, 120, 150),
            );
            b.glyph(row, "check", 16.0);
        } else {
            b.tinted_fit(
                row,
                format!(
                    "{}  {} / {}",
                    secondary.name(),
                    progress.count,
                    progress.need
                ),
                TextStyle::Small,
                DIM,
            );
        }
    }
}

/// Top right: what you carry (lost if you fall).
fn cargo(b: &mut Builder, root: NodeId, run: &Run) {
    let column = b.column(
        root,
        LayoutItem::size(Length::Auto, Length::Auto)
            .with_anchor(Anchor::TopRight)
            .with_margin(Edges::new(0.0, 30.0, 16.0, 0.0)),
        4.0,
        0.0,
    );
    let row = line(b, column, Align::End);
    for m in Material::ALL {
        let n = run.carried.get(m);
        if n > 0 || m != Material::Voidstone {
            b.stat(row, material_icon(m), n.to_string(), TextStyle::Body, None);
        }
    }
}

/// Bottom left: weapon slots with levels, the class ability and any
/// statuses, in one compact bar.
fn loadout(b: &mut Builder, root: NodeId, run: &Run, text: &dyn Fn(&str) -> String) {
    let p = run.player();
    let column = b.column(
        root,
        LayoutItem::size(Length::Auto, Length::Auto)
            .with_anchor(Anchor::BottomLeft)
            .with_margin(Edges::new(16.0, 0.0, 0.0, 16.0)),
        6.0,
        0.0,
    );
    let mut effects = Vec::new();
    if p.jammed > 0 {
        effects.push(("Jammed", Color::rgb(200, 120, 255)));
    }
    if p.chilled > 0 {
        effects.push(("Chilled", INFO));
    }
    if p.wet > 0 {
        effects.push(("Wet", Color::rgb(90, 150, 255)));
    }
    if !effects.is_empty() {
        let row = b.hrow(
            column,
            LayoutItem::size(Length::Auto, Length::Auto),
            6.0,
            0.0,
        );
        for (name, color) in effects {
            b.chip(row, name, color);
        }
    }
    let bar = b.hrow(
        column,
        LayoutItem::size(Length::Auto, Length::Auto),
        8.0,
        8.0,
    );
    b.style(bar, BACKING, None, 3.0);
    let slots = content().tuning.weapons.max_carried;
    for i in 0..slots {
        let slot = b.column(
            bar,
            LayoutItem::size(Length::fixed(50.0), Length::Auto),
            3.0,
            0.0,
        );
        match p.weapons.get(i) {
            Some(w) => {
                let flamer =
                    w.kind.info().behavior == planetfall_sim::content::WeaponBehavior::Flamer;
                let frame = if flamer && w.overheated {
                    BAD
                } else {
                    kit::ACCENT_SOFT
                };
                b.icon(slot, w.kind.key(), 50.0, frame);
                if flamer {
                    b.bar(
                        slot,
                        Length::Fill,
                        4.0,
                        w.heat,
                        if w.overheated { BAD } else { WARN },
                    );
                } else {
                    b.pips(slot, (w.level() as u32).min(8), 8, ACCENT, 4.0);
                }
            }
            None => {
                b.block(
                    slot,
                    50.0,
                    50.0,
                    kit::PANEL_INSET,
                    Some((1.0, kit::LINE)),
                    4.0,
                );
                b.strut(
                    slot,
                    LayoutItem::size(Length::Fill, Length::fixed(4.0)),
                    0.0,
                    4.0,
                );
            }
        }
    }
    b.block(bar, 1.0, 56.0, kit::LINE, None, 0.0);
    let ability = &p.class.info().ability;
    let slot = b.column(
        bar,
        LayoutItem::size(Length::fixed(50.0), Length::Auto),
        3.0,
        0.0,
    );
    let ready = p.ability.charges > 0;
    b.icon(
        slot,
        ability_icon(ability.kind),
        50.0,
        if ready { GOOD } else { kit::LINE },
    );
    let total = p.ability.cooldown_total.max(1);
    b.bar(
        slot,
        Length::Fill,
        4.0,
        (total - p.ability.cooldown.min(total)) as f32 / total as f32,
        if ready { GOOD } else { INFO },
    );
    let info = b.column(
        bar,
        LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
        2.0,
        0.0,
    );
    if ready {
        let charges = if p.ability.max_charges > 1 {
            format!(" ×{}", p.ability.charges)
        } else {
            String::new()
        };
        b.rich(
            info,
            &text(&format!("{{ability}} READY{charges}")),
            TextStyle::Small,
            140.0,
        );
    } else {
        b.label_fit(
            info,
            format!("{}s", p.ability.cooldown.div_ceil(60)),
            TextStyle::Small,
        );
    }
}

fn alerts(b: &mut Builder, root: NodeId, run: &Run) {
    if run.tutorial {
        return;
    }
    let item = |y: f32| {
        LayoutItem::size(Length::fixed(420.0), Length::Auto)
            .with_anchor(Anchor::Top)
            .with_offset(Vec2::new(0.0, y))
    };
    let mut y = if run.mission.target.is_some() {
        132.0
    } else {
        74.0
    };
    if run.surge.warning > 0 {
        b.alert(
            root,
            item(y),
            BAD,
            "surge",
            &format!("SURGE INCOMING · {}s", run.surge.warning.div_ceil(60)),
            "A wave is coming from every side. Brace.",
        );
        y += 80.0;
    } else if run.surge.active > 0 {
        b.alert(
            root,
            item(y),
            BAD,
            "surge",
            "SURGE",
            "Hold on. This part is billable.",
        );
        y += 80.0;
    }
    if run.quake.warning > 0 {
        b.alert(
            root,
            item(y),
            WARN,
            "hazard",
            &format!("QUAKE · {}s", run.quake.warning.div_ceil(60)),
            "Rubble incoming. Hard hats recommended.",
        );
    }
}

/// A boss or hunted elite gets a health bar of its own, under the timer.
fn boss_bar(b: &mut Builder, root: NodeId, run: &Run) {
    let Some(e) = run
        .mission
        .target
        .and_then(|id| run.enemies.iter().find(|e| e.id == id))
    else {
        return;
    };
    let card = b.hrow(
        root,
        LayoutItem::size(Length::fixed(560.0), Length::Auto)
            .with_anchor(Anchor::Top)
            .with_offset(Vec2::new(0.0, 72.0)),
        10.0,
        6.0,
    );
    b.style(card, BACKING, None, 3.0);
    b.glyph(
        card,
        if e.kind.info().boss {
            "boss"
        } else {
            "obj-hunt"
        },
        30.0,
    );
    let col = b.column(
        card,
        LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
        3.0,
        0.0,
    );
    b.tinted(
        col,
        e.kind.info().name.to_uppercase(),
        TextStyle::Small,
        Color::rgb(220, 160, 255),
    );
    b.bar(
        col,
        Length::Fill,
        9.0,
        e.health.max(0.0) / e.max_health.max(1.0),
        BAD,
    );
}

/// The latest find, kept even if the drop goes wrong.
fn find_banner(b: &mut Builder, root: NodeId, run: &Run) {
    if run.find_banner == 0 {
        return;
    }
    let Some(find) = run.finds.last() else {
        return;
    };
    let view = super::find_view(find);
    let banner = b.column(
        root,
        LayoutItem::size(Length::fixed(380.0), Length::Auto)
            .with_anchor(Anchor::TopRight)
            .with_margin(Edges::new(0.0, 0.0, 16.0, 0.0))
            .with_offset(Vec2::new(0.0, 74.0)),
        4.0,
        8.0,
    );
    b.style(
        banner,
        kit::PANEL,
        Some((2.0, super::rarity_color(view.rarity))),
        3.0,
    );
    b.item_view(banner, &view, true);
    b.label(
        banner,
        "Sent to the stash · kept even if you aren't",
        TextStyle::Small,
    );
}

/// FTL and descent: a centred status card at the bottom.
fn transit(b: &mut Builder, root: NodeId, icon: &str, title: &str, line: &str) {
    let card = b.hrow(root, pinned(Anchor::Bottom, 460.0), 12.0, 12.0);
    b.style(card, kit::PANEL, Some((1.0, kit::ACCENT_SOFT)), 3.0);
    b.brackets(card, ACCENT);
    b.glyph(card, icon, 44.0);
    let col = b.column(
        card,
        LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
        0.0,
        0.0,
    );
    b.tinted(col, title, TextStyle::Body, ACCENT);
    b.label(col, line, TextStyle::Small);
}

/// The primary objective's progress in words.
fn primary_status(run: &Run) -> String {
    let m = &run.mission;
    let name = m.contract.primary.name();
    let p = m.primary;
    match m.primary_kind() {
        ObjectiveKind::OreQuota => format!("Mine lumite  {} / {}", p.count, p.need),
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
            Some(s) if s.following => "Escort the prospector to the pod".into(),
            _ => "Find the stranded prospector".into(),
        },
        ObjectiveKind::Defend => match m.rig {
            Some(r) if !r.active => "Stand at the drill rig to start it".into(),
            Some(r) if r.broken() => "The rig is down! Stand close to fix it".into(),
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
pub fn threat_eta(run: &Run) -> String {
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
