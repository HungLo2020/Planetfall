//! In-game UI built on `quasar-ui`: shaped fonts, the HUD and interactive menus.

pub mod hud;
pub mod menu;
pub mod overlay;

use quasar_text::{FontData, FontStack};
use quasar_ui::{
    Color, Edges, Layer as UiLayer, LayoutItem, Length, NodeId, NodeStyle, PanelLayout,
    ShapedTextMeasure, StackDirection, TextMeasure, TextRenderer, TextRendererOptions, TextStyle,
    Theme, Ui, UiText, UiTextStyles, Widget,
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
    // Structural panels are invisible; cards and dimmers set a `NodeStyle`.
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

pub const CARD: Color = Color::rgba(10, 18, 28, 218);
pub const CARD_BORDER: Color = Color::rgba(255, 142, 48, 150);
pub const DIM: Color = Color::rgba(0, 3, 8, 170);

/// One UI tree drawn over the 3D scene.
pub struct Layer {
    pub ui: Ui,
}

/// Small builder helpers shared by the HUD and menus.
pub struct Builder {
    pub ui: Ui,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            ui: Ui::new(theme()),
        }
    }
}

impl Builder {
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
            let _ = self.ui.set_style(id, Some(NodeStyle::filled(DIM)));
        }
        id
    }

    /// A card panel with the standard background and accent edge.
    pub fn card(&mut self, parent: NodeId, item: LayoutItem, gap: f32, padding: f32) -> NodeId {
        let id = self.panel(parent, item, gap, padding);
        let style = NodeStyle::filled(CARD)
            .with_border(1.5, CARD_BORDER)
            .with_radius(3.0);
        let _ = self.ui.set_style(id, Some(style));
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
        self.add_label(parent, Widget::label(UiText::literal(text.into()), style))
    }

    /// A label that word-wraps at its parent's width.
    pub fn paragraph(&mut self, parent: NodeId, text: &str, style: TextStyle) -> NodeId {
        self.add_label(
            parent,
            Widget::wrapped_label(UiText::literal(text), style, None),
        )
    }

    fn add_label(&mut self, parent: NodeId, widget: Widget) -> NodeId {
        self.ui
            .add_child(parent, widget, LayoutItem::size(Length::Fill, Length::Auto))
            .expect("valid parent")
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
        Layer { ui: self.ui }
    }

    /// Finish and lay the tree out for a `size` viewport (physical pixels),
    /// ready to draw. Layers that are not driven by `Ui::update` every frame
    /// (the HUD, offscreen title cards) must be laid out this way, or they
    /// have no layout entries and draw nothing.
    pub fn finish_laid_out(self, size: (f32, f32), scale: f32, measure: &dyn TextMeasure) -> Layer {
        let mut layer = self.finish();
        let viewport = quasar_ui::Rect::new(0.0, 0.0, size.0, size.1);
        layer.ui.layout(viewport, scale, Edges::ZERO, measure);
        layer
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
