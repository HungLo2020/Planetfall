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
        AbilityKind::Turret => "ability-turret",
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

/// Hull colour: green when healthy, amber when hurt, red when critical.
fn hull_color(fraction: f32) -> Color {
    if fraction > 0.6 {
        GOOD
    } else if fraction > 0.3 {
        WARN
    } else {
        BAD
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
            tracker(&mut b, root, run, &text);
            cargo(&mut b, root, run);
            vitals(&mut b, root, run);
            loadout(&mut b, root, run, &text);
            bottom_offset = -132.0;
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
                    "Suit beacon recalled you. Cargo was not recovered.",
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
            transit(&mut b, root, &icon, "FTL JUMP IN PROGRESS", &line);
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
                pinned(Anchor::Bottom, 640.0).with_offset(Vec2::new(0.0, bottom_offset)),
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
                b.style(card, kit::PANEL, Some((1.0, kit::ACCENT_SOFT)), 3.0);
                b.brackets(card, kit::ACCENT_SOFT);
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
                b.rich(col, &body, TextStyle::Body, 590.0);
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

/// Top centre: planet, hazard and timer; the objective and optional
/// objective; the threat meter.
fn tracker(b: &mut Builder, root: NodeId, run: &Run, text: &dyn Fn(&str) -> String) {
    let card = b.card(
        root,
        LayoutItem::size(Length::fixed(560.0), Length::Auto)
            .with_anchor(Anchor::Top)
            .with_margin(Edges::new(0.0, 14.0, 0.0, 0.0)),
        6.0,
        12.0,
    );
    let m = &run.mission;
    let info = run.planet.info();
    let head = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
    b.glyph(head, &format!("planet-{}", run.planet.key()), 26.0);
    b.tinted_fit(head, info.name.to_uppercase(), TextStyle::Body, kit::TEXT);
    if !run.tutorial {
        b.chip(
            head,
            &format!("Hazard {} · {}", m.hazard, hazard_name(m.hazard)),
            hazard_color(m.hazard),
        );
    }
    b.strut(
        head,
        LayoutItem::size(Length::Fill, Length::fixed(1.0)),
        0.0,
        1.0,
    );
    let secs = run.ticks / 60;
    b.stat(
        head,
        "timer",
        format!("{:02}:{:02}", secs / 60, secs % 60),
        TextStyle::Body,
        None,
    );
    b.rule(card);

    // Primary objective, or extraction once it is done.
    let mut prompt = None;
    let (icon, line, color, progress) = if !run.objective_done() {
        let p = m.primary;
        let progress = match m.primary_kind() {
            ObjectiveKind::Boss | ObjectiveKind::Hunt => m
                .target
                .and_then(|id| run.enemies.iter().find(|e| e.id == id))
                .map(|e| 1.0 - e.health.max(0.0) / e.max_health.max(1.0)),
            ObjectiveKind::Rescue => None,
            ObjectiveKind::Defend => m.rig.map(|r| r.drilled as f32 / r.need.max(1) as f32),
            _ => Some(p.count as f32 / p.need.max(1) as f32),
        };
        (
            objective_icon(m.primary_kind()),
            primary_status(run),
            kit::TEXT,
            progress,
        )
    } else {
        match run.extraction {
            Extraction::Idle => {
                prompt = Some(text("{extract} call the extraction pod"));
                ("check", "Objective complete".to_owned(), GOOD, None)
            }
            Extraction::Inbound { ticks, .. } => (
                "pod",
                format!("Extraction pod inbound · {}s", ticks.div_ceil(60)),
                WARN,
                None,
            ),
            Extraction::Waiting { hold, .. } => (
                "pod",
                "Hold the zone".to_owned(),
                WARN,
                Some(hold as f32 / extraction_hold().max(1) as f32),
            ),
            Extraction::Departed { .. } => ("check", "Extracted".to_owned(), GOOD, None),
        }
    };
    let row = b.hrow(
        card,
        LayoutItem::size(Length::Fill, Length::Auto),
        10.0,
        0.0,
    );
    b.glyph(row, icon, 34.0);
    let col = b.column(
        row,
        LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
        4.0,
        0.0,
    );
    b.tinted(col, &line, TextStyle::Body, color);
    if let Some(prompt) = prompt {
        b.rich(col, &prompt, TextStyle::Small, 460.0);
    }
    if let Some(f) = progress {
        b.bar(
            col,
            Length::Fill,
            6.0,
            f,
            if color == kit::TEXT { ACCENT } else { color },
        );
    }
    if let (Some(secondary), Some(progress)) = (m.contract.secondary, m.secondary) {
        let row = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
        b.glyph(row, objective_icon(secondary.info().kind), 22.0);
        if progress.done {
            b.tinted_fit(
                row,
                format!("Optional · {}", secondary.name()),
                TextStyle::Small,
                GOOD,
            );
            b.glyph(row, "check", 18.0);
        } else {
            b.label_fit(
                row,
                format!("Optional · {}", secondary.name()),
                TextStyle::Small,
            );
            b.label_fit(
                row,
                format!("{} / {}", progress.count, progress.need),
                TextStyle::Small,
            );
        }
    }
    if !run.tutorial {
        let level = run.threat_level();
        let row = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
        b.glyph(row, "threat", 22.0);
        b.tinted_fit(
            row,
            format!("THREAT {level}"),
            TextStyle::Small,
            if level >= 6 { BAD } else { WARN },
        );
        b.bar(row, Length::Fill, 6.0, run.threat.fract(), BAD);
        b.label_fit(row, format!("next {}", threat_eta(run)), TextStyle::Small);
        let tags = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
        b.stat(
            tags,
            "site",
            m.contract.site.name(),
            TextStyle::Small,
            Some(DIM),
        );
        if let Some(mutator) = m.contract.mutator {
            b.stat(
                tags,
                "mutator",
                mutator.name(),
                TextStyle::Small,
                Some(GOOD),
            );
        }
    }
}

fn cargo(b: &mut Builder, root: NodeId, run: &Run) {
    let card = b.card(root, pinned(Anchor::TopRight, 230.0), 6.0, 12.0);
    b.section(card, "Cargo");
    for m in Material::ALL {
        let n = run.carried.get(m);
        if n > 0 || m != Material::Voidstone {
            b.stat(
                card,
                material_icon(m),
                n.to_string(),
                TextStyle::Heading,
                None,
            );
        }
    }
    b.label(card, "Lost if you fall", TextStyle::Small);
}

/// Bottom left: class emblem, hull, XP, oxygen and statuses.
fn vitals(b: &mut Builder, root: NodeId, run: &Run) {
    let p = run.player();
    let card = b.card(root, pinned(Anchor::BottomLeft, 400.0), 6.0, 12.0);
    let head = b.hrow(
        card,
        LayoutItem::size(Length::Fill, Length::Auto),
        10.0,
        0.0,
    );
    b.glyph(head, &format!("class-{}", p.class.key()), 48.0);
    let col = b.column(
        head,
        LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
        0.0,
        0.0,
    );
    b.tinted(col, p.class.name().to_uppercase(), TextStyle::Body, ACCENT);
    b.label(col, format!("Level {}", p.level), TextStyle::Small);
    let meter = |b: &mut Builder, icon: &str, f: f32, color: Color, value: String, h: f32| {
        let row = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
        b.glyph(row, icon, 22.0);
        b.bar(row, Length::Fill, h, f, color);
        b.label_fit(row, value, TextStyle::Small);
    };
    let hull = p.health.max(0.0) / p.max_health.max(1.0);
    meter(
        b,
        "health",
        hull,
        hull_color(hull),
        format!("{:.0}/{:.0}", p.health.max(0.0), p.max_health),
        14.0,
    );
    meter(
        b,
        "xp",
        p.xp as f32 / p.xp_needed().max(1) as f32,
        INFO,
        format!("{}/{}", p.xp, p.xp_needed()),
        8.0,
    );
    if run.thin_air() {
        let max = content().tuning.hazards.oxygen_max;
        let f = p.oxygen / max;
        let color = if f > 0.25 {
            Color::rgb(120, 200, 255)
        } else {
            BAD
        };
        let value = if p.oxygen <= 0.0 {
            "EMPTY".to_owned()
        } else {
            format!("{:.0}%", f * 100.0)
        };
        meter(b, "oxygen", f, color, value, 8.0);
    }
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
        let row = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
        for (name, color) in effects {
            b.chip(row, name, color);
        }
    }
}

/// Bottom centre: weapon slots with levels and the class ability.
fn loadout(b: &mut Builder, root: NodeId, run: &Run, text: &dyn Fn(&str) -> String) {
    let p = run.player();
    let bar = b.hrow(
        root,
        LayoutItem::size(Length::Auto, Length::Auto)
            .with_anchor(Anchor::Bottom)
            .with_margin(Edges::new(0.0, 0.0, 0.0, 16.0)),
        10.0,
        10.0,
    );
    b.style(bar, kit::PANEL, Some((1.0, kit::LINE)), 3.0);
    b.brackets(bar, kit::ACCENT_SOFT);
    let slots = content().tuning.weapons.max_carried;
    for i in 0..slots {
        let slot = b.column(
            bar,
            LayoutItem::size(Length::fixed(64.0), Length::Auto),
            4.0,
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
                b.icon(slot, w.kind.key(), 64.0, frame);
                if flamer {
                    b.bar(
                        slot,
                        Length::Fill,
                        5.0,
                        w.heat,
                        if w.overheated { BAD } else { WARN },
                    );
                } else {
                    let level = (w.level() as u32).min(8);
                    b.pips(slot, level.min(8), 8, ACCENT, 5.0);
                }
            }
            None => {
                b.block(
                    slot,
                    64.0,
                    64.0,
                    kit::PANEL_INSET,
                    Some((1.0, kit::LINE)),
                    4.0,
                );
                b.strut(
                    slot,
                    LayoutItem::size(Length::Fill, Length::fixed(5.0)),
                    0.0,
                    5.0,
                );
            }
        }
    }
    b.block(bar, 1.0, 70.0, kit::LINE, None, 0.0);
    let ability = &p.class.info().ability;
    let slot = b.column(
        bar,
        LayoutItem::size(Length::fixed(170.0), Length::Auto),
        4.0,
        0.0,
    );
    b.strut(
        slot,
        LayoutItem::size(Length::fixed(170.0), Length::fixed(0.0)),
        170.0,
        0.0,
    );
    let row = b.hrow(slot, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
    let ready = p.ability.charges > 0;
    b.icon(
        row,
        ability_icon(ability.kind),
        56.0,
        if ready { GOOD } else { kit::LINE },
    );
    let col = b.column(
        row,
        LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
        2.0,
        0.0,
    );
    b.label(col, ability.name.to_uppercase(), TextStyle::Small);
    if ready {
        let charges = if p.ability.max_charges > 1 {
            format!(" ×{}", p.ability.charges)
        } else {
            String::new()
        };
        b.tinted(col, format!("READY{charges}"), TextStyle::Small, GOOD);
        b.rich(col, &text("{ability}"), TextStyle::Small, 100.0);
    } else {
        b.label(
            col,
            format!("{}s", p.ability.cooldown.div_ceil(60)),
            TextStyle::Small,
        );
    }
    let total = p.ability.cooldown_total.max(1);
    b.bar(
        slot,
        Length::Fill,
        5.0,
        (total - p.ability.cooldown.min(total)) as f32 / total as f32,
        if ready { GOOD } else { INFO },
    );
}

fn alerts(b: &mut Builder, root: NodeId, run: &Run) {
    if run.tutorial {
        return;
    }
    let item = |y: f32| {
        LayoutItem::size(Length::fixed(440.0), Length::Auto)
            .with_anchor(Anchor::Top)
            .with_offset(Vec2::new(0.0, y))
    };
    let mut y = 236.0;
    if run.surge.warning > 0 {
        b.alert(
            root,
            item(y),
            BAD,
            "surge",
            &format!("SURGE INCOMING · {}s", run.surge.warning.div_ceil(60)),
            "A wave is coming from every side.",
        );
        y += 84.0;
    } else if run.surge.active > 0 {
        b.alert(root, item(y), BAD, "surge", "SURGE", "Hold on.");
        y += 84.0;
    }
    if run.quake.warning > 0 {
        b.alert(
            root,
            item(y),
            WARN,
            "hazard",
            &format!("QUAKE · {}s", run.quake.warning.div_ceil(60)),
            "Rubble will fall nearby.",
        );
    }
}

/// A boss or hunted elite gets a health bar of its own.
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
        LayoutItem::size(Length::fixed(600.0), Length::Auto)
            .with_anchor(Anchor::Bottom)
            .with_offset(Vec2::new(0.0, -128.0)),
        12.0,
        10.0,
    );
    b.style(
        card,
        kit::PANEL,
        Some((1.0, Color::rgba(200, 110, 255, 160))),
        3.0,
    );
    b.brackets(card, Color::rgb(200, 110, 255));
    b.glyph(
        card,
        if e.kind.info().boss {
            "boss"
        } else {
            "obj-hunt"
        },
        40.0,
    );
    let col = b.column(
        card,
        LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
        4.0,
        0.0,
    );
    b.tinted(
        col,
        e.kind.info().name.to_uppercase(),
        TextStyle::Body,
        Color::rgb(220, 160, 255),
    );
    b.bar(
        col,
        Length::Fill,
        12.0,
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
        LayoutItem::size(Length::fixed(400.0), Length::Auto)
            .with_anchor(Anchor::TopRight)
            .with_margin(Edges::new(0.0, 0.0, 16.0, 0.0))
            .with_offset(Vec2::new(0.0, 214.0)),
        4.0,
        10.0,
    );
    b.style(
        banner,
        kit::PANEL,
        Some((2.0, super::rarity_color(view.rarity))),
        3.0,
    );
    b.brackets(banner, super::rarity_text(view.rarity));
    b.item_view(banner, &view, true);
    b.label(
        banner,
        "Sent to the stash · kept even if you fall",
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
