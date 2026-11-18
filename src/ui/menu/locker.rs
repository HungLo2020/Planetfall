//! The locker room: classes, kit, progress and the gear tab (with the
//! tab and pager widgets the terminals share).

use super::{Menu, MenuAction, PAGE, View};
use crate::ui::Builder;
use crate::ui::hud::roman;
use crate::ui::kit::{self, Kind};
use planetfall_sim::content::{ClassId, GearSlot, class_xp_to_next, content};
use planetfall_sim::{Command, Game};
use quasar_ui::{Align, LayoutItem, Length, NodeId, TextStyle, Widget};

impl Menu {
    /// Locker room: pick a class, see its kit and progress, choose the
    /// starting weapon. Every class keeps its own progress.
    pub(super) fn locker(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let w = Self::window(
            b,
            820.0,
            &format!("class-{}", profile.class.key()),
            "Locker room",
            "Suit up as any class. Each keeps its own level, loadout and gear.",
        );
        let worn = profile.equipped(profile.class).len();
        self.tabs(
            b,
            w.body,
            &[
                "CREW".to_owned(),
                format!("GEAR {worn}/4  ·  STASH {}", profile.stash.len()),
            ],
        );
        if self.view.tab == 1 {
            self.gear(b, w.body, game);
            self.close(b, &w);
            self.footer(b, &w, "");
            return;
        }
        let grid =
            b.ui.add_child(
                w.body,
                Widget::Panel {
                    layout: quasar_ui::PanelLayout::Grid {
                        columns: 2,
                        gap: 8.0,
                    },
                    padding: quasar_ui::Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                LayoutItem::size(Length::Fill, Length::Auto),
            )
            .expect("valid parent");
        for class in ClassId::all() {
            let (level, into) = profile.class_level(class);
            let info = class.info();
            let active = profile.class == class;
            let card = b.hrow(
                grid,
                LayoutItem::size(Length::Fill, Length::Auto),
                10.0,
                8.0,
            );
            b.style(
                card,
                kit::PANEL_ROW,
                Some((
                    if active { 2.0 } else { 1.0 },
                    if active { kit::ACCENT } else { kit::LINE },
                )),
                3.0,
            );
            b.glyph(card, &format!("class-{}", class.key()), 48.0);
            let text = b.column(
                card,
                LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
                3.0,
                0.0,
            );
            b.tinted(
                text,
                info.name.to_uppercase(),
                TextStyle::Body,
                if active { kit::ACCENT } else { kit::TEXT },
            );
            b.label(text, info.role.as_str(), TextStyle::Small);
            let max = content().tuning.class.max_level;
            let row = b.hrow(text, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
            b.label_fit(row, format!("Lv {level}"), TextStyle::Small);
            let f = if level >= max {
                1.0
            } else {
                into as f32 / class_xp_to_next(level).max(1) as f32
            };
            b.bar(row, Length::Fill, 6.0, f, kit::INFO);
            if active {
                b.chip(card, "Active", kit::ACCENT);
            } else {
                self.small_button(
                    b,
                    card,
                    "SELECT",
                    MenuAction::Game(Command::SelectClass(class)),
                    true,
                    Kind::Secondary,
                    96.0,
                );
            }
        }
        let class = profile.class;
        let info = class.info();
        let (level, _) = profile.class_level(class);
        b.section(w.body, &format!("{} kit", info.name));
        b.paragraph(w.body, &info.description, TextStyle::Small);
        let (_, right) = b.entry(
            w.body,
            crate::ui::hud::ability_icon(info.ability.kind),
            &format!("{} {}", info.ability.name, roman(class.ability_tier(level))),
            Some(kit::INFO),
            &info.ability.description,
        );
        b.chip(right, "Ability", kit::INFO);
        let (_, right) = b.entry(
            w.body,
            "level",
            &format!("{} {}", info.passive.name, roman(class.passive_tier(level))),
            Some(kit::GOOD),
            &info.passive.description,
        );
        b.chip(right, "Passive", kit::GOOD);

        b.section(w.body, "Starting weapon");
        let options = profile.start_options(class);
        let current = profile.start_weapon(class);
        let weapons: Vec<_> = class
            .familiar()
            .chain(std::iter::once(class.signature()))
            .collect();
        for weapon in weapons.into_iter().rev() {
            let unlocked = options.contains(&weapon);
            let tag = if weapon == class.signature() {
                "Signature weapon"
            } else {
                "Familiar weapon"
            };
            let (_, right) = b.entry(
                w.body,
                weapon.key(),
                weapon.name(),
                Some(if unlocked { kit::TEXT } else { kit::DIM }),
                &format!("{tag} · {}", weapon.info().description),
            );
            if weapon == current {
                b.chip(right, "Equipped", kit::GOOD);
            } else if unlocked {
                self.small_button(
                    b,
                    right,
                    "EQUIP",
                    MenuAction::Game(Command::SetStartWeapon(weapon)),
                    true,
                    Kind::Secondary,
                    100.0,
                );
            } else {
                let at = info
                    .unlocks
                    .iter()
                    .find(|u| u.weapon == weapon.key())
                    .map_or("Find on a dive".to_owned(), |u| {
                        format!("Class level {}", u.level)
                    });
                b.glyph(right, "lock", 20.0);
                b.chip(right, &at, kit::DIM);
            }
        }
        self.close(b, &w);
        self.footer(b, &w, "");
    }

    /// A row of tab buttons; the current tab is highlighted.
    pub(super) fn tabs(&mut self, b: &mut Builder, card: NodeId, labels: &[String]) {
        let row = b.hrow(card, LayoutItem::size(Length::Fill, Length::Auto), 6.0, 0.0);
        for (i, label) in labels.iter().enumerate() {
            let current = usize::from(self.view.tab) == i;
            let action = MenuAction::SetView(View {
                tab: i as u8,
                ..View::default()
            });
            self.button_as(
                b,
                row,
                label.clone(),
                action,
                true,
                if current {
                    Kind::Selected
                } else {
                    Kind::Secondary
                },
            );
        }
    }

    /// Previous/next page buttons for a list of `len` rows.
    pub(super) fn pager(&mut self, b: &mut Builder, card: NodeId, len: usize) {
        let pages = len.div_ceil(PAGE).max(1);
        if pages <= 1 {
            return;
        }
        let page = self.view.page.min(pages - 1);
        let prev = View {
            page: page.saturating_sub(1),
            ..self.view
        };
        let next = View {
            page: (page + 1).min(pages - 1),
            ..self.view
        };
        let row = b.hrow(
            card,
            LayoutItem::size(Length::Fill, Length::Auto),
            10.0,
            0.0,
        );
        self.button_as(
            b,
            row,
            "◀ PREVIOUS",
            MenuAction::SetView(prev),
            page > 0,
            Kind::Secondary,
        );
        let label = b.column(
            row,
            LayoutItem::size(Length::fixed(140.0), Length::Auto).with_align(Align::Center),
            0.0,
            0.0,
        );
        b.label(
            label,
            format!("Page {} of {pages}", page + 1),
            TextStyle::Small,
        );
        self.button_as(
            b,
            row,
            "NEXT ▶",
            MenuAction::SetView(next),
            page + 1 < pages,
            Kind::Secondary,
        );
    }

    /// The rows of the current page.
    pub(super) fn page<'a, T>(&self, items: &'a [T]) -> &'a [T] {
        let pages = items.len().div_ceil(PAGE).max(1);
        let start = self.view.page.min(pages - 1) * PAGE;
        &items[start.min(items.len())..(start + PAGE).min(items.len())]
    }

    /// Locker GEAR tab: the active class's four slots; opening a slot lists
    /// the stash items that fit it.
    pub(super) fn gear(&mut self, b: &mut Builder, card: NodeId, game: &Game) {
        let profile = &game.profile;
        let class = profile.class;
        let worn = profile.equipped(class);
        let stats: Vec<String> = worn
            .iter()
            .flat_map(|g| g.stats())
            .map(|(e, a)| e.describe(a))
            .collect();
        b.section(card, &format!("{} wears", class.name()));
        b.paragraph(
            card,
            &if stats.is_empty() {
                "Nothing yet. Gear drops from caches, elites and bosses.".to_owned()
            } else {
                stats.join("  ·  ")
            },
            TextStyle::Small,
        );
        for slot in GearSlot::ALL {
            let current = worn.iter().find(|g| g.slot() == slot);
            let open = self.view.slot == Some(slot);
            let toggle = View {
                slot: (!open).then_some(slot),
                ..self.view
            };
            let group = b.column(
                card,
                LayoutItem::size(Length::Fill, Length::Auto),
                6.0,
                if open { 8.0 } else { 0.0 },
            );
            let toggle_text = if open { "CLOSE" } else { "CHANGE" };
            match current {
                Some(g) => {
                    self.item_row(
                        b,
                        group,
                        &crate::ui::gear_view(g),
                        toggle_text,
                        MenuAction::SetView(toggle),
                        true,
                    );
                }
                None => {
                    let (_, right) = b.entry(
                        group,
                        "lock",
                        &format!("{} · empty", slot.name()),
                        Some(kit::DIM),
                        "No item worn in this slot.",
                    );
                    self.small_button(
                        b,
                        right,
                        toggle_text,
                        MenuAction::SetView(toggle),
                        true,
                        Kind::Secondary,
                        160.0,
                    );
                }
            }
            if !open {
                continue;
            }
            b.style(group, kit::PANEL_INSET, Some((1.0, kit::ACCENT_SOFT)), 3.0);
            if current.is_some() {
                self.button_as(
                    b,
                    group,
                    "TAKE OFF",
                    MenuAction::Game(Command::Unequip(slot)),
                    true,
                    Kind::Ghost,
                );
            }
            let base = current.map_or(0.0, |g| g.score());
            let mut options: Vec<_> = profile
                .stash
                .iter()
                .filter(|g| g.slot() == slot && Some(g.id) != current.map(|c| c.id))
                .collect();
            options.sort_by(|a, b| b.score().total_cmp(&a.score()));
            if options.is_empty() {
                b.label(
                    group,
                    "Nothing in the stash for this slot.",
                    TextStyle::Small,
                );
            }
            for g in options.into_iter().take(PAGE) {
                let compare = if g.score() > base + 0.05 {
                    "EQUIP ▲"
                } else if g.score() < base - 0.05 {
                    "EQUIP ▼"
                } else {
                    "EQUIP ="
                };
                let mut view = crate::ui::gear_view(g);
                if let Some(c) = profile.wearer(g.id) {
                    view.tag = format!("{} · worn by {}", view.tag, c.name());
                }
                self.item_row(
                    b,
                    group,
                    &view,
                    compare,
                    MenuAction::Game(Command::Equip(g.id)),
                    true,
                );
            }
        }
    }
}
