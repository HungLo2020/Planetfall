//! Planetfall's interface kit: the "industrial salvage" look shared by the
//! HUD and every menu. Deep navy glass panels, a warm orange accent, corner
//! brackets, header strips with an icon, section labels with an accent tab,
//! styled buttons (primary, secondary, danger), stat chips with icons and
//! coloured bars. Everything is built from `quasar-ui` panels, labels,
//! buttons and atlas images; corner brackets and coloured text are drawn by
//! the layer's overlay trees (see [`super::Layer::refresh_overlays`]).

use super::{Builder, icons};
use crate::bindings;
use quasar_ui::{
    ActionId, Align, Anchor, Border, Color, Edges, LayoutItem, Length, NodeId, NodeStyle,
    PanelLayout, StackDirection, TextStyle, UiText, Vec2, Widget,
};

pub const ACCENT: Color = Color::rgba(255, 142, 48, 255);
pub const ACCENT_SOFT: Color = Color::rgba(255, 142, 48, 120);
pub const ACCENT_DEEP: Color = Color::rgba(196, 92, 22, 245);
pub const PANEL: Color = Color::rgba(10, 16, 25, 236);
pub const PANEL_HEAD: Color = Color::rgba(20, 30, 44, 250);
pub const PANEL_INSET: Color = Color::rgba(5, 9, 15, 200);
pub const PANEL_ROW: Color = Color::rgba(18, 27, 40, 230);
pub const LINE: Color = Color::rgba(130, 156, 182, 70);
pub const TEXT: Color = Color::rgb(230, 238, 243);
pub const DIM: Color = Color::rgb(140, 164, 182);
pub const GOOD: Color = Color::rgb(110, 225, 120);
pub const BAD: Color = Color::rgb(255, 92, 72);
pub const WARN: Color = Color::rgb(255, 196, 64);
pub const INFO: Color = Color::rgb(90, 200, 255);
pub const TRACK: Color = Color::rgba(3, 6, 10, 225);

/// Button looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The main action of a screen: filled accent.
    Primary,
    /// Everything else: dark with a hairline.
    Secondary,
    /// Destructive actions: deep red.
    Danger,
    /// A selected tab or option.
    Selected,
    /// Quiet, borderless (close, back).
    Ghost,
}

pub fn button_style(kind: Kind, enabled: bool) -> NodeStyle {
    let (fill, border) = match (kind, enabled) {
        (_, false) => (Color::rgba(12, 18, 26, 200), Color::rgba(90, 100, 112, 60)),
        (Kind::Primary, _) => (ACCENT_DEEP, Color::rgba(255, 186, 110, 255)),
        (Kind::Secondary, _) => (Color::rgba(24, 36, 52, 245), Color::rgba(140, 165, 190, 90)),
        (Kind::Danger, _) => (
            Color::rgba(110, 24, 20, 245),
            Color::rgba(255, 110, 90, 220),
        ),
        (Kind::Selected, _) => (Color::rgba(70, 42, 18, 245), ACCENT),
        (Kind::Ghost, _) => (Color::rgba(14, 22, 32, 160), Color::rgba(120, 140, 160, 50)),
    };
    NodeStyle {
        background: Some(fill),
        border: Some(Border {
            width: 1.0,
            color: border,
        }),
        radius: 3.0,
    }
}

/// Approximate rendered width of `text` (layout placeholders for coloured
/// text that must not stretch).
pub fn text_width(text: &str, style: TextStyle) -> f32 {
    let size = super::theme().size(style);
    let wide = text
        .chars()
        .filter(|c| c.is_uppercase() || c.is_ascii_digit())
        .count() as f32;
    let narrow = text.chars().count() as f32 - wide;
    (wide * 0.7 + narrow * 0.56) * size + 4.0
}

/// A framed window: header strip, accent rule, body and optional footer.
pub struct Window {
    pub frame: NodeId,
    pub body: NodeId,
}

impl Builder {
    fn stack(
        &mut self,
        parent: NodeId,
        item: LayoutItem,
        direction: StackDirection,
        gap: f32,
        padding: Edges,
    ) -> NodeId {
        self.ui
            .add_child(
                parent,
                Widget::Panel {
                    layout: PanelLayout::Stack { direction, gap },
                    padding,
                    clip: false,
                    scrollable: false,
                },
                item,
            )
            .expect("valid parent")
    }

    /// An empty box that measures as `w`×`h` (Quasar's intrinsic sizing
    /// counts content, not a child's fixed size, so empty panels and spacers
    /// would otherwise collapse inside auto-sized rows).
    pub fn strut(&mut self, parent: NodeId, item: LayoutItem, w: f32, h: f32) -> NodeId {
        self.ui
            .add_child(
                parent,
                Widget::Panel {
                    layout: PanelLayout::Absolute,
                    padding: Edges::new(w * 0.5, h * 0.5, w * 0.5, h * 0.5),
                    clip: false,
                    scrollable: false,
                },
                item,
            )
            .expect("valid parent")
    }

    /// A filled box of a fixed size.
    pub fn block(
        &mut self,
        parent: NodeId,
        w: f32,
        h: f32,
        fill: Color,
        border: Option<(f32, Color)>,
        radius: f32,
    ) -> NodeId {
        let id = self.strut(
            parent,
            LayoutItem::size(Length::fixed(w), Length::fixed(h)).with_align(Align::Center),
            w,
            h,
        );
        self.style(id, fill, border, radius);
        id
    }

    /// A horizontal row with padding.
    pub fn hrow(&mut self, parent: NodeId, item: LayoutItem, gap: f32, pad: f32) -> NodeId {
        self.stack(
            parent,
            item,
            StackDirection::Horizontal,
            gap,
            Edges::all(pad),
        )
    }

    /// A vertical column with padding.
    pub fn column(&mut self, parent: NodeId, item: LayoutItem, gap: f32, pad: f32) -> NodeId {
        self.stack(parent, item, StackDirection::Vertical, gap, Edges::all(pad))
    }

    /// An absolute-layout box (children place themselves with anchors).
    pub fn layered(&mut self, parent: NodeId, item: LayoutItem) -> NodeId {
        self.ui
            .add_child(
                parent,
                Widget::Panel {
                    layout: PanelLayout::Absolute,
                    padding: Edges::ZERO,
                    clip: false,
                    scrollable: false,
                },
                item,
            )
            .expect("valid parent")
    }

    pub fn style(&mut self, node: NodeId, fill: Color, border: Option<(f32, Color)>, radius: f32) {
        let _ = self.ui.set_style(
            node,
            Some(NodeStyle {
                background: Some(fill),
                border: border.map(|(width, color)| Border { width, color }),
                radius,
            }),
        );
    }

    /// Corner brackets around `node` (drawn by the overlay).
    pub fn brackets(&mut self, node: NodeId, color: Color) {
        self.decor.push((node, color));
    }

    /// A glass panel with hairline border.
    pub fn glass(&mut self, parent: NodeId, item: LayoutItem, gap: f32, pad: f32) -> NodeId {
        let id = self.column(parent, item, gap, pad);
        self.style(id, PANEL, Some((1.0, LINE)), 3.0);
        id
    }

    /// An atlas icon without a frame.
    pub fn glyph(&mut self, parent: NodeId, name: &str, size: f32) -> NodeId {
        let item =
            LayoutItem::size(Length::fixed(size), Length::fixed(size)).with_align(Align::Center);
        match icons::source(name) {
            Some(source) => self
                .ui
                .add_child(
                    parent,
                    Widget::Image {
                        image: icons::ATLAS,
                        source: Some(source),
                        size: Vec2::new(size, size),
                    },
                    item,
                )
                .expect("valid parent"),
            None => self.strut(parent, item, size, size),
        }
    }

    /// The game logo image at `width` (aspect 1400:360).
    pub fn logo(&mut self, parent: NodeId, width: f32) -> NodeId {
        let height = width * 360.0 / 1400.0;
        self.ui
            .add_child(
                parent,
                Widget::Image {
                    image: icons::LOGO,
                    source: None,
                    size: Vec2::new(width, height),
                },
                LayoutItem::size(Length::fixed(width), Length::fixed(height)),
            )
            .expect("valid parent")
    }

    /// A fixed-width coloured label (for rows that must not stretch).
    pub fn tinted_fit(
        &mut self,
        parent: NodeId,
        text: impl Into<String>,
        style: TextStyle,
        color: Color,
    ) -> NodeId {
        let text = text.into();
        let width = text_width(&text, style);
        let size = super::theme().size(style);
        let height = (size * 1.3).ceil();
        let node = self.strut(
            parent,
            LayoutItem::size(Length::fixed(width), Length::fixed(height)).with_align(Align::Center),
            width,
            height,
        );
        self.tints.push(super::Tint {
            node,
            text,
            style,
            color,
        });
        node
    }

    /// A plain label sized to its text (inside rows).
    pub fn label_fit(
        &mut self,
        parent: NodeId,
        text: impl Into<String>,
        style: TextStyle,
    ) -> NodeId {
        let text = text.into();
        let width = text_width(&text, style);
        self.ui
            .add_child(
                parent,
                Widget::label(UiText::literal(text), style),
                LayoutItem::size(Length::fixed(width), Length::Auto).with_align(Align::Center),
            )
            .expect("valid parent")
    }

    /// A framed, centred window with a header (icon, title, subtitle).
    pub fn window(
        &mut self,
        root: NodeId,
        width: f32,
        icon: &str,
        title: &str,
        subtitle: &str,
    ) -> Window {
        let frame = self.column(
            root,
            LayoutItem::size(Length::fixed(width), Length::Auto).with_align(Align::Center),
            0.0,
            0.0,
        );
        self.style(
            frame,
            PANEL,
            Some((1.0, Color::rgba(255, 142, 48, 70))),
            3.0,
        );
        self.brackets(frame, ACCENT);
        let head = self.hrow(
            frame,
            LayoutItem::size(Length::Fill, Length::Auto),
            14.0,
            14.0,
        );
        self.style(head, PANEL_HEAD, None, 3.0);
        self.glyph(head, icon, 46.0);
        let text = self.column(head, LayoutItem::size(Length::Fill, Length::Auto), 0.0, 0.0);
        self.label(text, title.to_uppercase(), TextStyle::Heading);
        if !subtitle.is_empty() {
            self.label(text, subtitle, TextStyle::Small);
        }
        self.accent_rule(frame);
        let body = self.column(
            frame,
            LayoutItem::size(Length::Fill, Length::Auto),
            8.0,
            18.0,
        );
        Window { frame, body }
    }

    /// A 2 px accent line.
    pub fn accent_rule(&mut self, parent: NodeId) -> NodeId {
        let rule = self.strut(
            parent,
            LayoutItem::size(Length::Fill, Length::fixed(2.0)),
            0.0,
            2.0,
        );
        self.style(rule, ACCENT, None, 0.0);
        rule
    }

    /// A 1 px hairline.
    pub fn rule(&mut self, parent: NodeId) -> NodeId {
        let rule = self.strut(
            parent,
            LayoutItem::size(Length::Fill, Length::fixed(1.0)),
            0.0,
            1.0,
        );
        self.style(rule, LINE, None, 0.0);
        rule
    }

    /// The window footer: a dark strip of hints (button prompts).
    pub fn footer(&mut self, window: &Window, text: impl Into<String>) {
        let strip = self.hrow(
            window.frame,
            LayoutItem::size(Length::Fill, Length::Auto),
            8.0,
            10.0,
        );
        self.style(strip, PANEL_INSET, None, 3.0);
        self.rich(strip, &text.into(), TextStyle::Small, 2000.0);
    }

    /// A section label: an accent tab and the title in accent capitals.
    pub fn section(&mut self, parent: NodeId, title: &str) -> NodeId {
        let row = self.stack(
            parent,
            LayoutItem::size(Length::Fill, Length::Auto),
            StackDirection::Horizontal,
            8.0,
            Edges::new(0.0, 5.0, 0.0, 2.0),
        );
        self.block(row, 4.0, 14.0, ACCENT, None, 0.0);
        self.tinted_fit(row, title.to_uppercase(), TextStyle::Small, ACCENT);
        row
    }

    /// A styled button.
    pub fn button_kind(
        &mut self,
        parent: NodeId,
        text: impl Into<String>,
        action: u64,
        enabled: bool,
        kind: Kind,
        item: LayoutItem,
    ) -> NodeId {
        let id = self
            .ui
            .add_child(
                parent,
                Widget::Button {
                    text: UiText::literal(text.into()),
                    action: Some(ActionId::new(action)),
                },
                item,
            )
            .expect("valid parent");
        let _ = self.ui.set_style(id, Some(button_style(kind, enabled)));
        if !enabled {
            let _ = self.ui.set_enabled(id, false);
        }
        id
    }

    /// A full-width button with an icon on its left edge.
    pub fn icon_button(
        &mut self,
        parent: NodeId,
        icon: &str,
        text: impl Into<String>,
        action: u64,
        enabled: bool,
        kind: Kind,
    ) -> NodeId {
        let holder = self.layered(parent, LayoutItem::size(Length::Fill, Length::fixed(46.0)));
        self.strut(
            holder,
            LayoutItem::size(Length::Fill, Length::fixed(46.0)),
            0.0,
            46.0,
        );
        let button = self.button_kind(
            holder,
            text,
            action,
            enabled,
            kind,
            LayoutItem::fill().with_anchor(Anchor::Stretch),
        );
        let icon_box = self.layered(
            holder,
            LayoutItem::size(Length::fixed(34.0), Length::fixed(34.0))
                .with_anchor(Anchor::Left)
                .with_offset(Vec2::new(8.0, 0.0)),
        );
        self.glyph(icon_box, icon, 34.0);
        button
    }

    /// An icon and a value, sized to fit (stat chips in rows).
    pub fn stat(
        &mut self,
        parent: NodeId,
        icon: &str,
        text: impl Into<String>,
        style: TextStyle,
        color: Option<Color>,
    ) -> NodeId {
        let size = if style == TextStyle::Heading {
            36.0
        } else if style == TextStyle::Small {
            18.0
        } else {
            22.0
        };
        let row = self.hrow(
            parent,
            LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
            5.0,
            0.0,
        );
        self.glyph(row, icon, size);
        match color {
            Some(color) => self.tinted_fit(row, text, style, color),
            None => self.label_fit(row, text, style),
        };
        row
    }

    /// A coloured meter bar.
    pub fn bar(
        &mut self,
        parent: NodeId,
        width: Length,
        height: f32,
        fraction: f32,
        color: Color,
    ) -> NodeId {
        let track = self.layered(
            parent,
            LayoutItem::size(width, Length::fixed(height)).with_align(Align::Center),
        );
        self.style(
            track,
            TRACK,
            Some((1.0, Color::rgba(255, 255, 255, 28))),
            2.0,
        );
        let w = if let Length::Fixed(w) = width { w } else { 0.0 };
        self.strut(
            track,
            LayoutItem::size(Length::fixed(w), Length::fixed(height)),
            w,
            height,
        );
        let f = fraction.clamp(0.0, 1.0);
        if f > 0.0 {
            let fill = self.layered(
                track,
                LayoutItem::size(Length::percent(f), Length::Fill).with_anchor(Anchor::Left),
            );
            self.style(fill, color, None, 2.0);
        }
        track
    }

    /// `total` small squares, the first `filled` lit.
    pub fn pips(
        &mut self,
        parent: NodeId,
        filled: u32,
        total: u32,
        color: Color,
        size: f32,
    ) -> NodeId {
        let row = self.hrow(
            parent,
            LayoutItem::size(Length::Auto, Length::fixed(size)).with_align(Align::Center),
            3.0,
            0.0,
        );
        for k in 0..total {
            if k < filled {
                self.block(row, size, size, color, None, 1.0);
            } else {
                self.block(
                    row,
                    size,
                    size,
                    TRACK,
                    Some((1.0, Color::rgba(255, 255, 255, 40))),
                    1.0,
                );
            }
        }
        row
    }

    /// A small coloured tag ("HAZARD 3", "LOCKED").
    pub fn chip(&mut self, parent: NodeId, text: &str, color: Color) -> NodeId {
        let chip = self.hrow(
            parent,
            LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
            0.0,
            4.0,
        );
        let fill = Color::rgba(color.r / 4, color.g / 4, color.b / 4, 230);
        self.style(chip, fill, Some((1.0, color)), 2.0);
        self.tinted_fit(chip, text.to_uppercase(), TextStyle::Small, color);
        chip
    }

    /// A list row: icon, title (optionally coloured), subtitle and a right
    /// column for extras. Returns (row, right column).
    pub fn entry(
        &mut self,
        parent: NodeId,
        icon: &str,
        title: &str,
        title_color: Option<Color>,
        subtitle: &str,
    ) -> (NodeId, NodeId) {
        let row = self.hrow(
            parent,
            LayoutItem::size(Length::Fill, Length::Auto),
            12.0,
            8.0,
        );
        self.style(row, PANEL_ROW, Some((1.0, LINE)), 3.0);
        self.glyph(row, icon, 44.0);
        let text = self.column(
            row,
            LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
            1.0,
            0.0,
        );
        match title_color {
            Some(c) => {
                self.tinted(text, title, TextStyle::Body, c);
            }
            None => {
                self.label(text, title, TextStyle::Body);
            }
        }
        if !subtitle.is_empty() {
            self.paragraph(text, subtitle, TextStyle::Small);
        }
        let right = self.hrow(
            row,
            LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
            8.0,
            0.0,
        );
        (row, right)
    }

    /// An alert card (surges, quakes): coloured border, icon, title, line.
    pub fn alert(
        &mut self,
        root: NodeId,
        item: LayoutItem,
        color: Color,
        icon: &str,
        title: &str,
        line: &str,
    ) -> NodeId {
        let card = self.hrow(root, item, 12.0, 10.0);
        let fill = Color::rgba(color.r / 6, color.g / 6, color.b / 6, 235);
        self.style(card, fill, Some((2.0, color)), 3.0);
        self.brackets(card, color);
        self.glyph(card, icon, 40.0);
        let text = self.column(
            card,
            LayoutItem::size(Length::Fill, Length::Auto).with_align(Align::Center),
            0.0,
            0.0,
        );
        self.tinted(text, title, TextStyle::Body, color);
        if !line.is_empty() {
            self.label(text, line, TextStyle::Small);
        }
        card
    }

    /// A key cap for a keyboard key, or the device glyph for a controller
    /// button, stick or mouse button (`label` is a binding name).
    pub fn keycap(&mut self, parent: NodeId, label: &str, style: TextStyle) -> NodeId {
        let size = super::theme().size(style);
        if let Some(glyph) = bindings::glyph(label, self.family) {
            return self.glyph(parent, glyph, (size * 1.6).round());
        }
        let text = bindings::keycap_text(label);
        let cap = self.hrow(
            parent,
            LayoutItem::size(Length::Auto, Length::Auto).with_align(Align::Center),
            0.0,
            0.0,
        );
        let pad = (size * 0.35).round();
        let inner = self.stack(
            cap,
            LayoutItem::size(Length::Auto, Length::Auto),
            StackDirection::Horizontal,
            0.0,
            Edges::new(pad + 2.0, 2.0, pad + 2.0, 3.0),
        );
        self.style(
            cap,
            Color::rgba(46, 58, 76, 250),
            Some((1.0, Color::rgba(214, 224, 236, 230))),
            4.0,
        );
        self.ui
            .add_child(
                inner,
                Widget::label(UiText::literal(text), style),
                LayoutItem::auto(),
            )
            .expect("valid parent");
        cap
    }

    /// Text with inline key caps: every `[label]` token becomes a key cap or
    /// button glyph, words wrap at `width`, and `\n` breaks lines.
    pub fn rich(&mut self, parent: NodeId, text: &str, style: TextStyle, width: f32) -> NodeId {
        enum Piece {
            Word(String),
            Key(String),
            Break,
        }
        let mut pieces = Vec::new();
        for (n, line) in text.split('\n').enumerate() {
            if n > 0 {
                pieces.push(Piece::Break);
            }
            let mut rest = line;
            while !rest.is_empty() {
                match (rest.find('['), rest.find(']')) {
                    (Some(open), Some(close)) if close > open => {
                        pieces.extend(
                            rest[..open]
                                .split_whitespace()
                                .map(|w| Piece::Word(w.to_owned())),
                        );
                        pieces.push(Piece::Key(rest[open + 1..close].to_owned()));
                        rest = &rest[close + 1..];
                    }
                    _ => {
                        pieces.extend(rest.split_whitespace().map(|w| Piece::Word(w.to_owned())));
                        rest = "";
                    }
                }
            }
        }
        let size = super::theme().size(style);
        let block = self.column(
            parent,
            LayoutItem::size(Length::Fill, Length::Auto),
            3.0,
            0.0,
        );
        let new_line = |b: &mut Builder| {
            b.hrow(
                block,
                LayoutItem::size(Length::Fill, Length::Auto),
                5.0,
                0.0,
            )
        };
        let mut line = new_line(self);
        let mut x = 0.0;
        let mut words = String::new();
        let flush = |b: &mut Builder, line: NodeId, words: &mut String| {
            if !words.is_empty() {
                b.ui.add_child(
                    line,
                    Widget::label(UiText::literal(std::mem::take(words)), style),
                    LayoutItem::auto().with_align(Align::Center),
                )
                .expect("valid parent");
            }
        };
        for piece in pieces {
            let w = match &piece {
                Piece::Word(word) => text_width(word, style) + size * 0.3,
                Piece::Key(label) => match bindings::glyph(label, self.family) {
                    Some(_) => size * 1.6 + 5.0,
                    None => text_width(&bindings::keycap_text(label), style) + size * 0.7 + 5.0,
                },
                Piece::Break => 0.0,
            };
            if matches!(piece, Piece::Break) || (x > 0.0 && x + w > width) {
                flush(self, line, &mut words);
                line = new_line(self);
                x = 0.0;
            }
            match piece {
                Piece::Word(word) => {
                    if !words.is_empty() {
                        words.push(' ');
                    }
                    words.push_str(&word);
                }
                Piece::Key(label) => {
                    flush(self, line, &mut words);
                    self.keycap(line, &label, style);
                }
                Piece::Break => {}
            }
            x += w;
        }
        flush(self, line, &mut words);
        block
    }
}
