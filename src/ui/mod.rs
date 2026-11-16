//! In-game UI built on `quasar-ui`: shaped fonts, the HUD and interactive menus.

pub mod hud;
pub mod menu;
pub mod overlay;

use quasar_text::{FontData, FontStack};
use quasar_ui::{
    Color, Edges, Layer as UiLayer, LayoutItem, Length, NodeId, PanelLayout, ShapedTextMeasure,
    StackDirection, TextMeasure, TextRenderer, TextRendererOptions, TextStyle, Theme, Ui, UiText,
    UiTextStyles, Widget,
};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// Load the bundled DejaVu fonts into a shaped text engine.
pub fn build_text(asset_root: &Path) -> Result<(ShapedTextMeasure, TextRenderer), String> {
    let fonts = asset_root.join("fonts");
    let mut stack = FontStack::new();
    for name in ["DejaVuSans.ttf", "DejaVuSans-Bold.ttf"] {
        let path = fonts.join(name);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        stack.add_font(
            FontData::new(name, bytes)
                .with_license("Bitstream Vera / DejaVu (see LICENSE-DejaVu.txt)"),
        );
    }
    stack.with_default_family("DejaVu Sans");
    let engine = stack
        .build("en-US")
        .map_err(|e| format!("font system: {e}"))?;
    let renderer = TextRenderer::new(
        Rc::new(RefCell::new(engine)),
        UiTextStyles::new("DejaVu Sans"),
        TextRendererOptions::default(),
    )
    .map_err(|e| format!("text renderer: {e}"))?;
    Ok((renderer.measure(), renderer))
}

/// Planetfall's sci-fi palette over Quasar's modern theme.
pub fn theme() -> Theme {
    let mut t = Theme::modern();
    t.name = "planetfall".into();
    t.background = Color::rgba(6, 10, 16, 215);
    // Panel backgrounds are painted by the renderer from `Layer::boxes`.
    t.panel = Color::TRANSPARENT;
    t.panel_alt = Color::TRANSPARENT;
    t.text = Color::rgb(226, 236, 240);
    t.dim_text = Color::rgb(140, 168, 180);
    t.accent = Color::rgb(255, 142, 48);
    t.fill = Color::rgb(255, 142, 48);
    t.button = Color::rgb(22, 36, 50);
    t.button_hover = Color::rgb(36, 58, 78);
    t.button_active = Color::rgb(170, 88, 28);
    t.selection = Color::rgb(46, 80, 104);
    t.track = Color::rgb(8, 14, 22);
    t.overlay = Color::rgba(0, 4, 8, 165);
    t.font_size = 17.0;
    t.heading_size = 28.0;
    t.small_size = 13.0;
    t.row_height = 34.0;
    t
}

/// Replace input placeholders with labels for the active device.
pub fn device_text(text: &str, gamepad: bool) -> String {
    let labels: [(&str, &str, &str); 4] = [
        ("{move}", "WASD", "the left stick"),
        ("{interact}", "[E]", "(A)"),
        ("{dash}", "[Space]", "(B)"),
        ("{extract}", "[F]", "(Y)"),
    ];
    let mut out = text.to_owned();
    for (key, kb, pad) in labels {
        out = out.replace(key, if gamepad { pad } else { kb });
    }
    out
}

/// Greedy word wrap against the real shaped font metrics.
pub fn wrap(
    measure: &dyn TextMeasure,
    text: &str,
    style: TextStyle,
    size: f32,
    width: f32,
) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && measure.measure_style(&candidate, style, size).x > width {
                lines.push(std::mem::take(&mut line));
                line = word.to_owned();
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// A painted rectangle behind a laid-out UI node.
#[derive(Clone, Copy, Debug)]
pub struct UiBox {
    pub node: NodeId,
    pub fill: [u8; 4],
    pub border: Option<[u8; 4]>,
}

pub const CARD: [u8; 4] = [10, 18, 28, 218];
pub const CARD_BORDER: [u8; 4] = [255, 142, 48, 150];
pub const DIM: [u8; 4] = [0, 3, 8, 170];

/// One UI tree plus the panel backgrounds to paint under it.
pub struct Layer {
    pub ui: Ui,
    pub boxes: Vec<UiBox>,
}

/// Small builder helpers shared by the HUD and menus.
pub struct Builder<'m> {
    pub ui: Ui,
    pub boxes: Vec<UiBox>,
    pub measure: &'m dyn TextMeasure,
    pub scale: f32,
}

impl<'m> Builder<'m> {
    pub fn new(measure: &'m dyn TextMeasure, scale: f32) -> Self {
        Self {
            ui: Ui::new(theme()),
            boxes: Vec::new(),
            measure,
            scale,
        }
    }

    /// Full-screen root; `dim` darkens the game behind a menu.
    pub fn root(&mut self, modal: bool) -> NodeId {
        let id = self.ui.add_root(
            Widget::Panel {
                layout: PanelLayout::Absolute,
                padding: Edges::ZERO,
                clip: false,
                scrollable: false,
            },
            LayoutItem::fill(),
            UiLayer::Normal,
            modal,
        );
        if modal {
            self.boxes.push(UiBox {
                node: id,
                fill: DIM,
                border: None,
            });
        }
        id
    }

    /// A card panel with the standard background and accent edge.
    pub fn card(&mut self, parent: NodeId, item: LayoutItem, gap: f32, padding: f32) -> NodeId {
        let id = self.panel(parent, item, gap, padding);
        self.boxes.push(UiBox {
            node: id,
            fill: CARD,
            border: Some(CARD_BORDER),
        });
        id
    }

    pub fn panel(&mut self, parent: NodeId, item: LayoutItem, gap: f32, padding: f32) -> NodeId {
        self.ui
            .add_child(
                parent,
                Widget::Panel {
                    layout: PanelLayout::Stack {
                        direction: StackDirection::Vertical,
                        gap,
                    },
                    padding: Edges::all(padding),
                    clip: false,
                    scrollable: false,
                },
                item,
            )
            .expect("valid parent")
    }

    pub fn label(&mut self, parent: NodeId, text: impl Into<String>, style: TextStyle) -> NodeId {
        self.ui
            .add_child(
                parent,
                Widget::Label {
                    text: UiText::literal(text.into()),
                    style,
                },
                LayoutItem::size(Length::Fill, Length::Auto),
            )
            .expect("valid parent")
    }

    /// Wrapped paragraph as one label per line.
    pub fn paragraph(&mut self, parent: NodeId, text: &str, style: TextStyle, width: f32) {
        let size = self.ui.theme().size(style) * self.scale;
        for line in wrap(self.measure, text, style, size, width * self.scale) {
            self.label(parent, line, style);
        }
    }

    pub fn progress(
        &mut self,
        parent: NodeId,
        label: impl Into<String>,
        value: f32,
        max: f32,
    ) -> NodeId {
        self.ui
            .add_child(
                parent,
                Widget::Progress {
                    label: UiText::literal(label.into()),
                    value,
                    max: max.max(1e-3),
                },
                LayoutItem::size(Length::Fill, Length::fixed(12.0)),
            )
            .expect("valid parent")
    }

    pub fn button(
        &mut self,
        parent: NodeId,
        text: impl Into<String>,
        action: u64,
        enabled: bool,
    ) -> NodeId {
        let id = self
            .ui
            .add_child(
                parent,
                Widget::Button {
                    text: UiText::literal(text.into()),
                    action: Some(quasar_ui::ActionId::new(action)),
                },
                LayoutItem::size(Length::Fill, Length::fixed(38.0)),
            )
            .expect("valid parent");
        if !enabled {
            let _ = self.ui.set_enabled(id, false);
        }
        id
    }

    pub fn finish(self) -> Layer {
        Layer {
            ui: self.ui,
            boxes: self.boxes,
        }
    }
}

pub fn format_materials(m: &planetfall_sim::content::Materials) -> String {
    format!(
        "Ferrite {}   Lumite {}   Voidstone {}",
        m.ferrite, m.lumite, m.voidstone
    )
}

pub fn format_cost(m: &planetfall_sim::content::Materials) -> String {
    let mut parts = Vec::new();
    if m.ferrite > 0 {
        parts.push(format!("{} Ferrite", m.ferrite));
    }
    if m.lumite > 0 {
        parts.push(format!("{} Lumite", m.lumite));
    }
    if m.voidstone > 0 {
        parts.push(format!("{} Voidstone", m.voidstone));
    }
    if parts.is_empty() {
        "free".into()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mono;
    impl TextMeasure for Mono {
        fn measure(&self, text: &str, size: f32) -> quasar_ui::Vec2 {
            quasar_ui::Vec2::new(text.chars().count() as f32 * size * 0.5, size)
        }
    }

    #[test]
    fn wraps_to_width() {
        let lines = wrap(
            &Mono,
            "one two three four five six",
            TextStyle::Body,
            10.0,
            60.0,
        );
        assert!(lines.len() > 1);
        assert!(
            lines
                .iter()
                .all(|l| l.chars().count() as f32 * 5.0 <= 60.0 || !l.contains(' '))
        );
    }

    #[test]
    fn device_labels() {
        assert_eq!(device_text("press {interact}", false), "press [E]");
        assert_eq!(device_text("press {interact}", true), "press (A)");
    }
}
