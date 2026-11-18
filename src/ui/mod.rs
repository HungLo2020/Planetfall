//! In-game UI built on `quasar-ui`: shaped fonts, the HUD and interactive menus.

pub mod console;
pub mod hud;
pub mod icons;
pub mod kit;
pub mod menu;
pub mod overlay;

use quasar_text::{FontData, FontStack};
use quasar_ui::{
    Anchor, Border, Color, Edges, Layer as UiLayer, LayoutItem, Length, NodeId, NodeStyle,
    PanelLayout, ShapedTextMeasure, StackDirection, TextMeasure, TextRenderer, TextRendererOptions,
    TextStyle, Theme, Ui, UiText, UiTextStyles, Vec2, Widget,
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
    // Buttons carry their own `NodeStyle` (see `kit::button_style`); the
    // theme only tints them on hover and press, over that style.
    t.button = Color::TRANSPARENT;
    t.button_hover = Color::rgba(255, 160, 80, 46);
    t.button_active = Color::rgba(255, 142, 48, 96);
    t.focus = Color::rgb(255, 176, 92);
    t.focus_width = 2.0;
    t.selection = Color::rgb(70, 42, 18);
    t.track = kit::TRACK;
    t.disabled = Color::rgb(96, 108, 120);
    t.overlay = Color::rgba(0, 4, 8, 165);
    t.font_size = 17.0;
    t.heading_size = 27.0;
    t.small_size = 13.0;
    // Buttons are 42 high; intrinsic sizing uses this as their minimum.
    t.row_height = 42.0;
    t
}

pub const DIM: Color = Color::rgba(0, 3, 8, 175);

/// Height of an item row and its icon.
pub const ITEM_ROW: f32 = 62.0;
pub const ITEM_ICON: f32 = 58.0;

/// Text drawn in its own colour (rarity names) over a placeholder node.
#[derive(Clone, Debug)]
pub struct Tint {
    pub node: NodeId,
    pub text: String,
    pub style: TextStyle,
    pub color: Color,
}

/// One UI tree drawn over the 3D scene, plus overlay trees that draw its
/// coloured text in place (`quasar-ui` labels take their colour from the
/// theme, so each colour gets a small tree of its own; see QUASAR-ISSUES).
pub struct Layer {
    pub ui: Ui,
    pub tints: Vec<Tint>,
    /// Nodes framed with corner brackets, and the bracket colour.
    pub decor: Vec<(NodeId, Color)>,
    pub overlays: Vec<Ui>,
}

impl Layer {
    /// Every tree to draw, in order.
    pub fn trees(&self) -> impl Iterator<Item = &Ui> {
        std::iter::once(&self.ui).chain(&self.overlays)
    }

    /// Rebuild the coloured-text overlays over the placeholders' laid-out
    /// rectangles. Call after every layout of `ui`.
    pub fn refresh_overlays(
        &mut self,
        viewport: quasar_ui::Rect,
        scale: f32,
        measure: &dyn TextMeasure,
    ) {
        self.overlays.clear();
        let mut colors: Vec<Color> = Vec::new();
        for t in &self.tints {
            if !colors.contains(&t.color) {
                colors.push(t.color);
            }
        }
        let scale = scale.max(1e-3);
        // Corner brackets: two short bars at each corner of a framed node.
        if !self.decor.is_empty() {
            let mut ui = Ui::new(theme());
            let root = ui.add_root(
                Widget::Panel {
                    layout: PanelLayout::Absolute,
                    padding: Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                LayoutItem::fill(),
                UiLayer::Overlay,
                false,
            );
            for (node, color) in &self.decor {
                let Some(entry) = self.ui.layout_entries().iter().find(|e| e.id == *node) else {
                    continue;
                };
                let r = entry.rect;
                let (x0, y0) = (r.x / scale - 3.0, r.y / scale - 3.0);
                let (x1, y1) = (
                    (r.x + r.width) / scale + 3.0,
                    (r.y + r.height) / scale + 3.0,
                );
                let (long, thick) = (16.0, 3.0);
                for (x, y, w, h) in [
                    (x0, y0, long, thick),
                    (x0, y0, thick, long),
                    (x1 - long, y0, long, thick),
                    (x1 - thick, y0, thick, long),
                    (x0, y1 - thick, long, thick),
                    (x0, y1 - long, thick, long),
                    (x1 - long, y1 - thick, long, thick),
                    (x1 - thick, y1 - long, thick, long),
                ] {
                    if let Ok(bar) = ui.add_child(
                        root,
                        Widget::Panel {
                            layout: PanelLayout::Absolute,
                            padding: Edges::ZERO,
                            clip: false,
                            scrollable: false,
                        },
                        LayoutItem::size(Length::fixed(w), Length::fixed(h))
                            .with_anchor(Anchor::TopLeft)
                            .with_offset(Vec2::new(x, y)),
                    ) {
                        let _ = ui.set_style(bar, Some(NodeStyle::filled(*color)));
                    }
                }
            }
            ui.layout(viewport, scale, Edges::ZERO, measure);
            self.overlays.push(ui);
        }
        for color in colors {
            let mut t = theme();
            t.text = color;
            t.dim_text = color;
            let mut ui = Ui::new(t);
            let root = ui.add_root(
                Widget::Panel {
                    layout: PanelLayout::Absolute,
                    padding: Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                LayoutItem::fill(),
                UiLayer::Overlay,
                false,
            );
            for tint in self.tints.iter().filter(|t| t.color == color) {
                let Some(entry) = self.ui.layout_entries().iter().find(|e| e.id == tint.node)
                else {
                    continue;
                };
                let r = entry.rect;
                let item = LayoutItem::size(
                    Length::fixed(r.width / scale),
                    Length::fixed(r.height / scale),
                )
                .with_anchor(Anchor::TopLeft)
                .with_offset(Vec2::new(r.x / scale, r.y / scale));
                let _ = ui.add_child(
                    root,
                    Widget::label(UiText::literal(tint.text.clone()), tint.style),
                    item,
                );
            }
            ui.layout(viewport, scale, Edges::ZERO, measure);
            self.overlays.push(ui);
        }
    }
}

/// Small builder helpers shared by the HUD and menus.
pub struct Builder {
    pub ui: Ui,
    pub tints: Vec<Tint>,
    pub decor: Vec<(NodeId, Color)>,
    /// Device family whose button glyphs prompts show.
    pub family: quasar_input::DeviceFamily,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            ui: Ui::new(theme()),
            tints: Vec::new(),
            decor: Vec::new(),
            family: quasar_input::DeviceFamily::KeyboardMouse,
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

    /// A glass card with a hairline edge and corner brackets.
    pub fn card(&mut self, parent: NodeId, item: LayoutItem, gap: f32, padding: f32) -> NodeId {
        let id = self.glass(parent, item, gap, padding);
        self.brackets(id, kit::ACCENT_SOFT);
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

    /// A label drawn in `color` (one line, ellipsis-free: keep it short).
    pub fn tinted(
        &mut self,
        parent: NodeId,
        text: impl Into<String>,
        style: TextStyle,
        color: Color,
    ) -> NodeId {
        let size = theme().size(style);
        let height = (size * 1.3).ceil();
        let text: String = text.into();
        let width = kit::text_width(&text, style);
        let node = self.strut(
            parent,
            LayoutItem::size(Length::Fill, Length::fixed(height)),
            width,
            height,
        );
        self.tints.push(Tint {
            node,
            text,
            style,
            color,
        });
        node
    }

    /// A horizontal row.
    pub fn row(&mut self, parent: NodeId, item: LayoutItem, gap: f32) -> NodeId {
        self.ui
            .add_child(
                parent,
                Widget::Panel {
                    layout: PanelLayout::Stack {
                        direction: StackDirection::Horizontal,
                        gap,
                    },
                    padding: Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                item,
            )
            .expect("valid parent")
    }

    /// An item icon from the atlas in a square frame bordered by `frame`.
    pub fn icon(&mut self, parent: NodeId, name: &str, size: f32, frame: Color) -> NodeId {
        let tile = self
            .ui
            .add_child(
                parent,
                Widget::Panel {
                    layout: PanelLayout::Absolute,
                    padding: Edges::all(3.0),
                    clip: false,
                    scrollable: false,
                },
                LayoutItem::size(Length::fixed(size), Length::fixed(size)),
            )
            .expect("valid parent");
        let fill = Color::rgba(frame.r / 5, frame.g / 5, frame.b / 5, 235);
        let _ = self.ui.set_style(
            tile,
            Some(NodeStyle {
                background: Some(fill),
                border: Some(Border {
                    width: 2.0,
                    color: frame,
                }),
                radius: 4.0,
            }),
        );
        if let Some(source) = icons::source(name) {
            let inner = size - 6.0;
            let _ = self.ui.add_child(
                tile,
                Widget::Image {
                    image: icons::ATLAS,
                    source: Some(source),
                    size: Vec2::new(inner, inner),
                },
                LayoutItem::size(Length::fixed(inner), Length::fixed(inner))
                    .with_anchor(Anchor::Center),
            );
        }
        tile
    }

    /// An item block: icon, small rarity tag and the name in the rarity
    /// colour, then optional stat lines.
    pub fn item(
        &mut self,
        parent: NodeId,
        icon: &str,
        rarity: planetfall_sim::content::Rarity,
        tag: &str,
        name: &str,
        detail: Option<&str>,
    ) -> NodeId {
        let color = rarity_text(rarity);
        let row = self.row(
            parent,
            LayoutItem::size(Length::Fill, Length::fixed(ITEM_ROW)),
            10.0,
        );
        self.icon(row, icon, ITEM_ICON, rarity_color(rarity));
        let text = self.panel(row, LayoutItem::size(Length::Fill, Length::Fill), 0.0, 0.0);
        self.tinted(text, tag.to_uppercase(), TextStyle::Small, color);
        self.tinted(text, name, TextStyle::Body, color);
        if let Some(detail) = detail {
            self.label(text, detail, TextStyle::Small);
        }
        row
    }

    pub fn finish(self) -> Layer {
        Layer {
            ui: self.ui,
            tints: self.tints,
            decor: self.decor,
            overlays: Vec::new(),
        }
    }

    /// Finish and lay the tree out for a `size` viewport (physical pixels),
    /// ready to draw. Layers that are not driven by `Ui::update` every frame
    /// (the HUD, offscreen title cards) must be laid out this way, or they
    /// have no layout entries and draw nothing.
    pub fn finish_laid_out(self, size: (f32, f32), scale: f32, measure: &dyn TextMeasure) -> Layer {
        let mut layer = self.finish();
        let viewport = quasar_ui::Rect::new(0.0, 0.0, size.0, size.1);
        layer.ui.layout(viewport, scale, Edges::ZERO, measure);
        layer.refresh_overlays(viewport, scale, measure);
        layer
    }
}

/// Border colour of an upgrade or item by rarity.
pub(crate) fn rarity_color(rarity: planetfall_sim::content::Rarity) -> Color {
    use planetfall_sim::content::Rarity;
    match rarity {
        Rarity::Common => Color::rgba(140, 150, 160, 200),
        Rarity::Uncommon => Color::rgba(90, 215, 100, 230),
        Rarity::Rare => Color::rgba(80, 150, 255, 240),
        Rarity::Legendary => Color::rgba(255, 165, 40, 255),
    }
}

/// Text colour of an item name by rarity (brighter than the frame colour).
pub(crate) fn rarity_text(rarity: planetfall_sim::content::Rarity) -> Color {
    use planetfall_sim::content::Rarity;
    match rarity {
        Rarity::Common => Color::rgb(200, 208, 214),
        Rarity::Uncommon => Color::rgb(120, 235, 120),
        Rarity::Rare => Color::rgb(110, 175, 255),
        Rarity::Legendary => Color::rgb(255, 180, 60),
    }
}

/// How a find shows up: icon, colour, small tag, name and stat line.
pub(crate) struct ItemView {
    pub icon: &'static str,
    pub rarity: planetfall_sim::content::Rarity,
    pub tag: String,
    pub name: String,
    pub detail: String,
}

pub(crate) fn gear_view(g: &planetfall_sim::gear::Gear) -> ItemView {
    ItemView {
        icon: g.base.key(),
        rarity: g.rarity,
        tag: format!("{} {}", g.rarity.name(), g.slot().name()),
        name: g.name(),
        detail: g.summary(),
    }
}

pub(crate) fn find_view(find: &planetfall_sim::run::Find) -> ItemView {
    match find {
        planetfall_sim::run::Find::Gear(g) => gear_view(g),
        planetfall_sim::run::Find::Weapon { weapon, class } => ItemView {
            icon: weapon.key(),
            rarity: planetfall_sim::content::Rarity::Legendary,
            tag: "Weapon blueprint".into(),
            name: weapon.name().to_owned(),
            detail: format!("{} can now start a drop with it.", class.name()),
        },
    }
}

impl Builder {
    /// An item block for a view.
    pub(crate) fn item_view(&mut self, parent: NodeId, view: &ItemView, detail: bool) -> NodeId {
        self.item(
            parent,
            view.icon,
            view.rarity,
            &view.tag,
            &view.name,
            detail.then_some(view.detail.as_str()),
        )
    }
}
