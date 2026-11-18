//! In-world terminals: level-up choices, the navigation console, mission
//! board, fabricator and the drop debrief.

use super::{Menu, MenuAction, materials_row};
use crate::ui::Builder;
use crate::ui::hud::{hazard_color, objective_icon, roman};
use crate::ui::kit::{self, Kind};
use planetfall_sim::content::{Material, PlanetId, Upgrade, UpgradeId, content};
use planetfall_sim::game::RunSummary;
use planetfall_sim::mission::{MAX_HAZARD, hazard_name};
use planetfall_sim::run::OfferSource;
use planetfall_sim::run::RunOutcome;
use planetfall_sim::{Command, Game, Terminal};
use quasar_ui::{Align, Color, LayoutItem, Length, TextStyle};

impl Menu {
    pub(super) fn level_up(&mut self, b: &mut Builder, game: &Game) {
        let run = game.run.as_ref();
        let level = run.map_or(1, |r| r.player().level);
        let source = run.and_then(|r| r.offer_source());
        let (icon, title, hint) = match source {
            Some(OfferSource::WeaponPick) => (
                "level",
                format!("Level {level} · new weapon"),
                "Choose a weapon for a free slot. Familiar weapons turn up more often.",
            ),
            Some(OfferSource::Cache) => (
                "cargo",
                "Supply cache".to_owned(),
                "Choose one rare or better upgrade.",
            ),
            _ => (
                "level",
                format!("Level {level} · upgrade"),
                "Choose one. Upgrades last until this drop ends.",
            ),
        };
        let w = Self::window(b, 760.0, icon, &title, hint);
        let choices = run
            .and_then(|r| r.level_up_choices())
            .unwrap_or(&[])
            .to_vec();
        for (i, offer) in choices.iter().enumerate() {
            let (icon, kind) = match offer.upgrade {
                Upgrade::NewWeapon(w) => (w.key(), "new weapon"),
                Upgrade::WeaponMod(w, _) => (w.key(), "weapon mod"),
                Upgrade::AbilityMod(_) => ("upgrade", "ability mod"),
                Upgrade::Field(_) => ("upgrade", "perk"),
            };
            let name = match offer.upgrade {
                Upgrade::NewWeapon(w) => w.info().name.clone(),
                _ => offer.title(),
            };
            let view = crate::ui::ItemView {
                icon,
                rarity: offer.rarity,
                tag: if source == Some(OfferSource::WeaponPick) {
                    kind.to_owned()
                } else {
                    format!("{} {kind}", offer.rarity.name())
                },
                name,
                detail: offer.description(),
            };
            self.item_row(
                b,
                w.body,
                &view,
                "TAKE",
                MenuAction::Game(Command::ChoosePerk(i)),
                true,
            );
        }
        self.footer(b, &w, "");
    }

    pub(super) fn terminal(&mut self, b: &mut Builder, game: &Game) {
        match &game.terminal {
            Some(Terminal::Navigation) => self.navigation(b, game),
            Some(Terminal::Fabricator) => self.fabricator(b, game),
            Some(Terminal::Locker) => self.locker(b, game),
            Some(Terminal::MissionBoard) => self.mission_board(b, game),
            Some(Terminal::Trade) => self.trade(b, game),
            Some(Terminal::Debrief(summary)) => self.debrief(b, game, summary),
            Some(Terminal::Module(upgrade)) => {
                let level = game.profile.level(*upgrade);
                let w = Self::window(
                    b,
                    560.0,
                    &format!("module-{}", upgrade.key()),
                    upgrade.name(),
                    &format!("Level {level} of {}", upgrade.max_level()),
                );
                b.pips(
                    w.body,
                    u32::from(level),
                    u32::from(upgrade.max_level()),
                    kit::ACCENT,
                    12.0,
                );
                b.paragraph(w.body, upgrade.info().description.as_str(), TextStyle::Body);
                b.label(
                    w.body,
                    "Upgrade this module at the Fabricator.",
                    TextStyle::Small,
                );
                self.close(b, &w);
                self.footer(b, &w, "");
            }
            None => {}
        }
    }

    pub(super) fn debrief(&mut self, b: &mut Builder, game: &Game, summary: &RunSummary) {
        let (icon, title, tone) = match summary.outcome {
            RunOutcome::Extracted => (
                "check",
                "Extraction successful",
                "Cargo transferred to the hold.",
            ),
            RunOutcome::Died => (
                "threat",
                "Signal lost",
                "Recovered by suit beacon. All cargo was lost.",
            ),
            RunOutcome::Abandoned => (
                "back",
                "Drop aborted",
                "Emergency recall. All cargo was lost.",
            ),
        };
        let w = Self::window(
            b,
            680.0,
            icon,
            title,
            &format!("{} · {tone}", summary.planet.info().name),
        );
        let body = w.body;
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            22.0,
            0.0,
        );
        b.stat(
            row,
            "timer",
            format!("{:02}:{:02}", summary.seconds / 60, summary.seconds % 60),
            TextStyle::Body,
            None,
        );
        b.stat(
            row,
            "threat",
            format!("Threat {}", summary.threat),
            TextStyle::Body,
            None,
        );
        b.stat(
            row,
            "obj-cull",
            format!("{} kills", summary.kills),
            TextStyle::Body,
            None,
        );
        b.stat(
            row,
            "level",
            format!("Level {}", summary.level),
            TextStyle::Body,
            None,
        );
        let loadout = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
        for (weapon, level) in &summary.weapons {
            b.icon(loadout, weapon.key(), 44.0, kit::ACCENT_SOFT);
            b.label_fit(loadout, roman(*level), TextStyle::Small);
        }

        b.section(body, "Contract");
        let tick = |done: bool| {
            if done {
                ("check", kit::GOOD)
            } else {
                ("quit", kit::BAD)
            }
        };
        let primary = summary.contract.primary;
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        let (mark, color) = tick(summary.primary_done);
        b.glyph(row, mark, 24.0);
        b.glyph(row, objective_icon(primary.info().kind), 30.0);
        b.tinted_fit(row, primary.name(), TextStyle::Body, color);
        b.chip(
            row,
            &format!(
                "Hazard {} · {}",
                summary.hazard,
                hazard_name(summary.hazard)
            ),
            hazard_color(summary.hazard),
        );
        if let (Some(secondary), Some(done)) = (summary.contract.secondary, summary.secondary_done)
        {
            let row = b.hrow(
                body,
                LayoutItem::size(Length::Fill, Length::Auto),
                10.0,
                0.0,
            );
            let (mark, color) = tick(done);
            b.glyph(row, mark, 20.0);
            b.glyph(row, objective_icon(secondary.info().kind), 24.0);
            b.tinted_fit(
                row,
                format!("Optional · {}", secondary.name()),
                TextStyle::Small,
                color,
            );
        }
        if summary.bonus.total() > 0 {
            let row = b.hrow(
                body,
                LayoutItem::size(Length::Fill, Length::Auto),
                12.0,
                0.0,
            );
            b.label_fit(row, "Contract bonus", TextStyle::Small);
            materials_row(b, row, &summary.bonus, None);
        }
        if let Some(level) = summary.unlocked {
            let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
            b.glyph(row, "hazard", 26.0);
            b.tinted_fit(
                row,
                format!(
                    "HAZARD {level} UNLOCKED · {}",
                    hazard_name(level).to_uppercase()
                ),
                TextStyle::Body,
                hazard_color(level),
            );
        }

        b.section(body, "Class");
        let (before, after) = summary.class_levels;
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        b.glyph(row, &format!("class-{}", summary.class.key()), 36.0);
        b.label_fit(
            row,
            format!("{}  +{} class XP", summary.class.name(), summary.class_xp),
            TextStyle::Body,
        );
        if after > before {
            b.chip(row, &format!("Level up {before} → {after}"), kit::GOOD);
        } else {
            b.chip(row, &format!("Level {after}"), kit::DIM);
        }

        if !summary.finds.is_empty() {
            b.section(body, "Finds · kept");
            let shown = summary.finds.len().min(4);
            for find in &summary.finds[..shown] {
                b.item_view(body, &crate::ui::find_view(find), true);
            }
            if summary.finds.len() > shown {
                b.label(
                    body,
                    format!("…and {} more in the stash", summary.finds.len() - shown),
                    TextStyle::Small,
                );
            }
        }
        if summary.salvaged > 0 {
            let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 8.0, 0.0);
            b.stat(
                row,
                "credits",
                format!(
                    "Stash full: finds salvaged for {} credits",
                    summary.salvaged
                ),
                TextStyle::Small,
                Some(kit::WARN),
            );
        }

        b.section(body, "Cargo");
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            0.0,
        );
        if summary.outcome == RunOutcome::Extracted {
            b.label_fit(row, "Banked", TextStyle::Body);
            materials_row(b, row, &summary.banked, Some(kit::GOOD));
            if summary.salvage.total() > 0 {
                let row = b.hrow(
                    body,
                    LayoutItem::size(Length::Fill, Length::Auto),
                    12.0,
                    0.0,
                );
                b.label_fit(row, "Ship AI salvage", TextStyle::Small);
                materials_row(b, row, &summary.salvage, None);
            }
        } else {
            b.label_fit(row, "Lost", TextStyle::Body);
            materials_row(b, row, &summary.lost, Some(kit::BAD));
        }
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            0.0,
        );
        b.label_fit(row, "Hold", TextStyle::Small);
        materials_row(b, row, &game.profile.materials, None);

        self.cancel = MenuAction::Game(Command::Close);
        self.button_as(
            b,
            body,
            "CONTINUE",
            MenuAction::Game(Command::Close),
            true,
            Kind::Primary,
        );
        self.footer(b, &w, "");
    }

    pub(super) fn navigation(&mut self, b: &mut Builder, game: &Game) {
        let w = Self::window(
            b,
            760.0,
            "star-map",
            "Navigation",
            "Plot an FTL jump. Dangerous worlds pay better.",
        );
        for planet in PlanetId::all() {
            let info = planet.info();
            let unlocked = game.profile.unlocked(planet);
            let here = game.profile.orbiting == Some(planet);
            let subtitle = if unlocked {
                format!("{} · {}", info.kind, info.description)
            } else {
                info.kind.clone()
            };
            let (_, right) = b.entry(
                w.body,
                &format!("planet-{}", planet.key()),
                &info.name.to_uppercase(),
                Some(if unlocked { kit::TEXT } else { kit::DIM }),
                &subtitle,
            );
            if !unlocked {
                b.glyph(right, "lock", 22.0);
                b.chip(right, &format!("Nav core {}", info.nav_level), kit::DIM);
            } else if here {
                b.chip(right, "In orbit", kit::GOOD);
            } else {
                b.chip(right, &format!("Pay ×{:.1}", info.hazard), kit::WARN);
                self.small_button(
                    b,
                    right,
                    "JUMP",
                    MenuAction::Game(Command::TravelTo(planet)),
                    true,
                    Kind::Primary,
                    110.0,
                );
            }
        }
        let docked = game.profile.docked;
        let (_, right) = b.entry(
            w.body,
            "planet-station",
            "KESTREL EXCHANGE",
            Some(kit::WARN),
            "Trade station · sell finds, buy gear, rework items and trade materials.",
        );
        if docked {
            b.chip(right, "Docked", kit::GOOD);
        } else {
            self.small_button(
                b,
                right,
                "DOCK",
                MenuAction::Game(Command::Dock),
                true,
                Kind::Primary,
                110.0,
            );
        }
        self.close(b, &w);
        self.footer(b, &w, "");
    }

    /// Mission board: contracts for the planet in orbit and the hazard
    /// level to take them at.
    pub(super) fn mission_board(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let unlocked = profile.hazard_unlocked;
        if self.hazard == 0 || self.hazard > unlocked {
            self.hazard = profile.hazard_choice.clamp(1, unlocked);
        }
        let hazard = self.hazard;
        let planet = profile
            .orbiting
            .map_or("Deep space".to_owned(), |p| p.name().to_owned());
        let w = Self::window(
            b,
            860.0,
            "board",
            "Mission board",
            &format!("Contracts on {planet}"),
        );
        let body = w.body;
        b.section(body, "Hazard level");
        let row = b.hrow(body, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
        for level in 1..=MAX_HAZARD {
            let open = level <= unlocked;
            let label = if open {
                format!("{level} · {}", hazard_name(level).to_uppercase())
            } else {
                format!("{level} · LOCKED")
            };
            let kind = if level == hazard {
                Kind::Selected
            } else {
                Kind::Secondary
            };
            let action = if open {
                MenuAction::SetHazard(level)
            } else {
                MenuAction::None
            };
            self.button_as(b, row, label, action, open, kind);
        }
        let h = &content().tuning.hazard;
        let i = usize::from(hazard - 1);
        let row = b.hrow(
            body,
            LayoutItem::size(Length::Fill, Length::Auto),
            18.0,
            0.0,
        );
        b.pips(
            row,
            u32::from(hazard),
            u32::from(MAX_HAZARD),
            hazard_color(hazard),
            10.0,
        );
        b.stat(
            row,
            "health",
            format!("Enemy health ×{:.2}", h.enemy_health[i]),
            TextStyle::Small,
            None,
        );
        b.stat(
            row,
            "threat",
            format!("Damage ×{:.2}", h.enemy_damage[i]),
            TextStyle::Small,
            None,
        );
        b.stat(
            row,
            "cargo",
            format!("Haul ×{:.2}", h.reward[i]),
            TextStyle::Small,
            Some(kit::GOOD),
        );
        b.stat(
            row,
            "xp",
            format!("Class XP ×{:.2}", h.class_xp[i]),
            TextStyle::Small,
            Some(kit::INFO),
        );
        if unlocked < MAX_HAZARD {
            b.label(
                body,
                format!(
                    "Complete a contract at hazard {unlocked} to unlock hazard {}.",
                    unlocked + 1
                ),
                TextStyle::Small,
            );
        }
        b.section(body, "Contracts");
        for (n, contract) in profile.board.iter().enumerate() {
            let card = b.column(
                body,
                LayoutItem::size(Length::Fill, Length::Auto),
                6.0,
                10.0,
            );
            b.style(card, kit::PANEL_ROW, Some((1.0, kit::LINE)), 3.0);
            let head = b.hrow(
                card,
                LayoutItem::size(Length::Fill, Length::Auto),
                12.0,
                0.0,
            );
            b.glyph(head, objective_icon(contract.primary.info().kind), 48.0);
            let text = b.column(
                head,
                LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
                2.0,
                0.0,
            );
            b.tinted(
                text,
                contract.primary.name().to_uppercase(),
                TextStyle::Body,
                kit::ACCENT,
            );
            b.paragraph(text, &contract.primary.describe(hazard), TextStyle::Small);
            self.small_button(
                b,
                head,
                "DEPLOY",
                MenuAction::Game(Command::Deploy {
                    contract: n,
                    hazard,
                }),
                true,
                Kind::Primary,
                130.0,
            );
            let tags = b.hrow(
                card,
                LayoutItem::size(Length::Fill, Length::Auto),
                14.0,
                0.0,
            );
            if let Some(boss) = contract.boss {
                b.stat(
                    tags,
                    "boss",
                    boss.name(),
                    TextStyle::Small,
                    Some(Color::rgb(220, 160, 255)),
                );
            }
            if let Some(secondary) = contract.secondary {
                b.stat(
                    tags,
                    objective_icon(secondary.info().kind),
                    format!("Optional · {}", secondary.name()),
                    TextStyle::Small,
                    Some(kit::GOOD),
                );
            }
            let site = contract.site.info();
            b.stat(
                tags,
                "site",
                site.name.as_str(),
                TextStyle::Small,
                Some(kit::DIM),
            );
            if let Some(mutator) = contract.mutator {
                b.stat(
                    tags,
                    "mutator",
                    mutator.name(),
                    TextStyle::Small,
                    Some(kit::WARN),
                );
            }
            let mut notes = vec![site.description.clone()];
            if let Some(mutator) = contract.mutator {
                notes.push(mutator.info().description.clone());
            }
            if let Some(secondary) = contract.secondary {
                notes.insert(0, format!("Optional: {}", secondary.describe(hazard)));
            }
            b.paragraph(card, &notes.join("  ·  "), TextStyle::Small);
        }
        self.close(b, &w);
        self.footer(b, &w, "");
    }

    pub(super) fn fabricator(&mut self, b: &mut Builder, game: &Game) {
        let w = Self::window(
            b,
            820.0,
            "fabricator",
            "Fabricator",
            "Build and upgrade ship modules. Upgrades are permanent.",
        );
        let row = b.hrow(
            w.body,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            0.0,
        );
        b.label_fit(row, "Hold", TextStyle::Small);
        materials_row(b, row, &game.profile.materials, None);
        for upgrade in UpgradeId::all() {
            let level = game.profile.level(upgrade);
            let card = b.hrow(
                w.body,
                LayoutItem::size(Length::Fill, Length::Auto),
                12.0,
                8.0,
            );
            b.style(card, kit::PANEL_ROW, Some((1.0, kit::LINE)), 3.0);
            b.glyph(card, &format!("module-{}", upgrade.key()), 52.0);
            let text = b.column(
                card,
                LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
                3.0,
                0.0,
            );
            let head = b.hrow(
                text,
                LayoutItem::size(Length::Fill, Length::Auto),
                10.0,
                0.0,
            );
            b.tinted_fit(
                head,
                upgrade.name().to_uppercase(),
                TextStyle::Body,
                if level > 0 { kit::ACCENT } else { kit::TEXT },
            );
            b.pips(
                head,
                u32::from(level),
                u32::from(upgrade.max_level()),
                kit::ACCENT,
                9.0,
            );
            b.paragraph(text, upgrade.info().description.as_str(), TextStyle::Small);
            let right = b.column(
                card,
                LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
                6.0,
                0.0,
            );
            match game.profile.next_cost(upgrade) {
                None => {
                    b.chip(right, "Complete", kit::GOOD);
                }
                Some(cost) => {
                    let affordable = game.profile.materials.covers(cost);
                    let costs = b.hrow(
                        right,
                        LayoutItem::size(Length::Auto, Length::Auto),
                        8.0,
                        0.0,
                    );
                    for m in Material::ALL {
                        let need = cost.get(m);
                        if need > 0 {
                            let color = if game.profile.materials.get(m) >= need {
                                kit::TEXT
                            } else {
                                kit::BAD
                            };
                            b.stat(
                                costs,
                                crate::ui::hud::material_icon(m),
                                need.to_string(),
                                TextStyle::Small,
                                Some(color),
                            );
                        }
                    }
                    let verb = if level == 0 { "BUILD" } else { "UPGRADE" };
                    self.small_button(
                        b,
                        right,
                        verb,
                        MenuAction::Game(Command::Purchase(upgrade)),
                        affordable,
                        if affordable {
                            Kind::Primary
                        } else {
                            Kind::Secondary
                        },
                        150.0,
                    );
                }
            }
        }
        self.close(b, &w);
        self.footer(b, &w, "");
    }
}
