//! The Kestrel Exchange trade station: sell, buy, rework and trade
//! materials for credits.

use super::{Menu, MenuAction, materials_row};
use crate::ui::Builder;
use crate::ui::kit::{self, Kind};
use planetfall_sim::content::{Material, Rarity, content};
use planetfall_sim::{Command, Game};
use quasar_ui::{LayoutItem, Length, TextStyle};

impl Menu {
    /// The trade station: sell, buy, rework and trade materials.
    pub(super) fn trade(&mut self, b: &mut Builder, game: &Game) {
        let profile = &game.profile;
        let t = &content().tuning.trade;
        let w = Self::window(
            b,
            860.0,
            "trade",
            "Kestrel Exchange",
            "Trade station · credits for finds, gear for credits",
        );
        let card = w.body;
        let row = b.hrow(
            card,
            LayoutItem::size(Length::Fill, Length::Auto),
            18.0,
            0.0,
        );
        b.stat(
            row,
            "credits",
            profile.credits.to_string(),
            TextStyle::Body,
            Some(kit::WARN),
        );
        b.stat(
            row,
            "cargo",
            format!(
                "Stash {}/{}",
                profile.stash.len(),
                content().tuning.equipment.stash_size
            ),
            TextStyle::Body,
            None,
        );
        materials_row(b, row, &profile.materials, None);
        self.tabs(
            b,
            card,
            &[
                "SELL".to_owned(),
                "BUY".to_owned(),
                "WORKSHOP".to_owned(),
                "MATERIALS".to_owned(),
            ],
        );
        let item_row = |menu: &mut Self,
                        b: &mut Builder,
                        g: &planetfall_sim::gear::Gear,
                        text: String,
                        action: Command,
                        enabled: bool| {
            menu.item_row(
                b,
                card,
                &crate::ui::gear_view(g),
                text,
                MenuAction::Game(action),
                enabled,
            );
        };
        match self.view.tab {
            0 => {
                b.label(card, "Worn items cannot be sold.", TextStyle::Small);
                let mut items: Vec<_> = profile.stash.iter().collect();
                items.sort_by_key(|g| profile.wearer(g.id).is_some());
                if items.is_empty() {
                    b.label(card, "The stash is empty.", TextStyle::Small);
                }
                for g in self.page(&items).to_vec() {
                    let worn = profile.wearer(g.id);
                    let text = match worn {
                        Some(c) => format!("WORN · {}", c.name().to_uppercase()),
                        None => format!("SELL +{}", g.sell_value()),
                    };
                    item_row(self, b, g, text, Command::Sell(g.id), worn.is_none());
                }
                self.pager(b, card, items.len());
            }
            1 => {
                if profile.market.is_empty() {
                    b.label(card, "Sold out until the next visit.", TextStyle::Small);
                }
                for (i, g) in profile.market.iter().enumerate() {
                    let text = format!("BUY {}", g.price());
                    item_row(
                        self,
                        b,
                        g,
                        text,
                        Command::Buy(i),
                        g.price() <= profile.credits,
                    );
                }
            }
            2 => {
                b.section(card, "Fuse");
                for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
                    let have = profile.fusible(rarity).len();
                    let next = Rarity::ALL[rarity.index() + 1];
                    let cost = t.fuse_cost[rarity.index()];
                    let (_, right) = b.entry(
                        card,
                        "fabricator",
                        &format!("{} {} → 1 {}", t.fuse, rarity.name(), next.name()),
                        Some(crate::ui::rarity_text(next)),
                        &format!(
                            "{have} unworn {} items in the stash · {cost} credits",
                            rarity.name().to_lowercase()
                        ),
                    );
                    let ok = have >= t.fuse && profile.credits >= cost;
                    self.small_button(
                        b,
                        right,
                        "FUSE",
                        MenuAction::Game(Command::Fuse(rarity)),
                        ok,
                        if ok { Kind::Primary } else { Kind::Secondary },
                        120.0,
                    );
                }
                b.label(
                    card,
                    "Fusing uses the weakest unworn items. Rerolling keeps the base and rarity.",
                    TextStyle::Small,
                );
                b.section(card, "Reroll");
                let items: Vec<_> = profile.stash.iter().collect();
                for g in self.page(&items).to_vec() {
                    let cost = t.reroll[g.rarity.index()];
                    item_row(
                        self,
                        b,
                        g,
                        format!("REROLL {cost}"),
                        Command::Reroll(g.id),
                        profile.credits >= cost,
                    );
                }
                self.pager(b, card, items.len());
            }
            _ => {
                for (i, material) in Material::ALL.into_iter().enumerate() {
                    let (_, right) = b.entry(
                        card,
                        crate::ui::hud::material_icon(material),
                        material.name(),
                        None,
                        &format!(
                            "You hold {} · lots of {}",
                            profile.materials.get(material),
                            t.lot
                        ),
                    );
                    let sell = profile.materials.get(material) >= t.lot;
                    self.small_button(
                        b,
                        right,
                        format!("SELL +{}", t.material_sell[i]),
                        MenuAction::Game(Command::TradeMaterial {
                            material,
                            buy: false,
                        }),
                        sell,
                        Kind::Secondary,
                        130.0,
                    );
                    let buy = profile.credits >= t.material_buy[i];
                    self.small_button(
                        b,
                        right,
                        format!("BUY {}", t.material_buy[i]),
                        MenuAction::Game(Command::TradeMaterial {
                            material,
                            buy: true,
                        }),
                        buy,
                        Kind::Secondary,
                        130.0,
                    );
                }
            }
        }
        self.close(b, &w);
        self.footer(b, &w, "Prices in credits");
    }
}
