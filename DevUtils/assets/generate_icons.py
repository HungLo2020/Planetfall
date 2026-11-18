#!/usr/bin/env python3
"""Draw Planetfall's icons and logo: items, interface glyphs, objectives,
classes, abilities, planets and ship modules.

Every icon is original flat vector art in one style: chunky dark outlines,
two-tone cel shading with a top-left highlight and a coloured glow for
energy parts, drawn at 4x and downsampled for clean edges. The icons are
packed into one atlas (`assets/ui/icons.png`) with a manifest of names in
atlas order (`assets/ui/icons.toml`) that the game reads. The PLANETFALL
wordmark is drawn to `assets/ui/logo.png` from the bundled DejaVu font.

Usage:
    python3 DevUtils/assets/generate_icons.py [--out assets/ui]
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

REPO = Path(__file__).resolve().parents[2]
SIZE = 128
SS = 4
W = SIZE * SS
COLUMNS = 8
OUTLINE = (14, 18, 26, 255)
LINE = 11


def rgb(hex_color: str, alpha: int = 255) -> tuple[int, int, int, int]:
    h = hex_color.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), alpha)


def shade(color: tuple, k: float) -> tuple:
    """Lighten (k > 1) or darken (k < 1) a colour."""
    r, g, b, a = color
    if k >= 1:
        t = k - 1
        return (int(r + (255 - r) * t), int(g + (255 - g) * t), int(b + (255 - b) * t), a)
    return (int(r * k), int(g * k), int(b * k), a)


class Icon:
    """A 4x canvas with outlined, shaded primitives (coordinates 0..128)."""

    def __init__(self) -> None:
        self.image = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        self.glow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        self.draw = ImageDraw.Draw(self.image)
        self.glow_draw = ImageDraw.Draw(self.glow)

    @staticmethod
    def s(points):
        return [(x * SS, y * SS) for x, y in points]

    def poly(self, points, color, light=True, line=LINE):
        pts = self.s(points)
        self.draw.polygon(pts, fill=color, outline=OUTLINE, width=line)
        if light and len(points) >= 3:
            # Cel highlight: the shape shrunk toward its top-left corner.
            cx = sum(p[0] for p in points) / len(points)
            cy = sum(p[1] for p in points) / len(points)
            inner = [(cx + (x - cx) * 0.62 - 2.2, cy + (y - cy) * 0.62 - 2.2) for x, y in points]
            hi = Image.new("RGBA", (W, W), (0, 0, 0, 0))
            ImageDraw.Draw(hi).polygon(self.s(inner), fill=shade(color, 1.28))
            mask = Image.new("L", (W, W), 0)
            ImageDraw.Draw(mask).polygon(pts, fill=255)
            mask = mask.filter(ImageFilter.MinFilter(line * 2 + 1))
            self.image.paste(hi, (0, 0), Image.composite(hi, Image.new("RGBA", (W, W)), mask).split()[3])
            self.draw.polygon(pts, outline=OUTLINE, width=line)

    def rect(self, x0, y0, x1, y1, color, radius=4, light=True, line=LINE):
        box = [x0 * SS, y0 * SS, x1 * SS, y1 * SS]
        self.draw.rounded_rectangle(box, radius * SS, fill=color, outline=OUTLINE, width=line)
        if light:
            inset = 3.2
            hi = [
                (x0 + inset) * SS,
                (y0 + inset) * SS,
                (x1 - inset) * SS,
                (y0 + inset + max(2.0, (y1 - y0) * 0.28)) * SS,
            ]
            if hi[3] > hi[1] and hi[2] > hi[0]:
                self.draw.rounded_rectangle(hi, max(1, radius - 2) * SS, fill=shade(color, 1.25))
            lo = [
                (x0 + inset) * SS,
                (y1 - inset - max(1.5, (y1 - y0) * 0.16)) * SS,
                (x1 - inset) * SS,
                (y1 - inset) * SS,
            ]
            if lo[3] > lo[1] and lo[2] > lo[0] and y1 - y0 > 12:
                self.draw.rounded_rectangle(lo, max(1, radius - 2) * SS, fill=shade(color, 0.78))

    def circle(self, cx, cy, r, color, light=True, line=LINE):
        box = [(cx - r) * SS, (cy - r) * SS, (cx + r) * SS, (cy + r) * SS]
        self.draw.ellipse(box, fill=color, outline=OUTLINE, width=line)
        if light and r > 5:
            hr = r * 0.45
            hx, hy = cx - r * 0.3, cy - r * 0.3
            self.draw.ellipse(
                [(hx - hr) * SS, (hy - hr) * SS, (hx + hr) * SS, (hy + hr) * SS],
                fill=shade(color, 1.3),
            )

    def ring(self, cx, cy, r, color, width=3.0):
        box = [(cx - r) * SS, (cy - r) * SS, (cx + r) * SS, (cy + r) * SS]
        self.draw.ellipse(box, outline=OUTLINE, width=int((width + 2.6) * SS))
        self.draw.ellipse(box, outline=color, width=int(width * SS))

    def line(self, points, color, width=3.0, outline=True):
        pts = self.s(points)
        if outline:
            self.draw.line(pts, fill=OUTLINE, width=int((width + 2.6) * SS), joint="curve")
        self.draw.line(pts, fill=color, width=int(width * SS), joint="curve")

    def glow_at(self, cx, cy, r, color):
        """A soft light blob behind energy parts."""
        box = [(cx - r) * SS, (cy - r) * SS, (cx + r) * SS, (cy + r) * SS]
        self.glow_draw.ellipse(box, fill=color[:3] + (150,))

    def finish(self) -> Image.Image:
        glow = self.glow.filter(ImageFilter.GaussianBlur(9 * SS))
        out = Image.alpha_composite(glow, self.image)
        # Drop shadow under the whole icon.
        shadow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        alpha = out.split()[3].point(lambda a: 120 if a > 20 else 0)
        shadow.putalpha(alpha)
        shadow = shadow.filter(ImageFilter.GaussianBlur(2 * SS))
        base = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        base.paste(shadow, (SS * 2, SS * 3))
        out = Image.alpha_composite(base, out)
        return out.resize((SIZE, SIZE), Image.LANCZOS)


def rotate(points, angle, cx=64, cy=64):
    a = math.radians(angle)
    c, s = math.cos(a), math.sin(a)
    return [(cx + (x - cx) * c - (y - cy) * s, cy + (x - cx) * s + (y - cy) * c) for x, y in points]


def box(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


# Palette.
STEEL = rgb("#7c8a99")
DARK = rgb("#3b4552")
GUNMETAL = rgb("#4f5c6b")
ORANGE = rgb("#ff8e30")
CYAN = rgb("#46e3ff")
GREEN = rgb("#5be36a")
RED = rgb("#ff4a3d")
YELLOW = rgb("#ffd23f")
PURPLE = rgb("#a45cff")
RUST = rgb("#b5643a")
TAN = rgb("#c9a774")
WHITE = rgb("#e8eef2")


# --- Weapons -----------------------------------------------------------------


def pulse_blaster(i: Icon):
    i.glow_at(98, 50, 18, CYAN)
    i.poly(rotate(box(30, 60, 70, 96), -12), GUNMETAL)  # grip
    i.rect(18, 36, 96, 64, STEEL, 8)
    i.rect(92, 42, 112, 58, DARK, 4)
    i.rect(40, 42, 70, 56, CYAN, 3)
    i.circle(110, 50, 6, CYAN)


def scattergun(i: Icon):
    i.glow_at(112, 54, 12, ORANGE)
    i.poly([(8, 66), (40, 54), (46, 80), (14, 92)], RUST)  # stock
    i.rect(36, 46, 82, 76, GUNMETAL, 6)
    i.rect(78, 44, 118, 56, STEEL, 3)
    i.rect(78, 58, 118, 70, STEEL, 3)
    for x in (48, 58, 68):
        i.rect(x, 66, x + 7, 84, ORANGE, 2)


def arc_caster(i: Icon):
    i.glow_at(98, 50, 26, YELLOW)
    i.rect(14, 52, 64, 82, GUNMETAL, 8)
    i.poly([(60, 54), (100, 30), (106, 38), (68, 62)], STEEL)
    i.poly([(60, 74), (100, 96), (106, 88), (68, 68)], STEEL)
    i.line([(102, 36), (92, 52), (106, 58), (96, 74), (104, 90)], YELLOW, 4)
    i.circle(36, 67, 9, CYAN)


def plasma_mortar(i: Icon):
    i.glow_at(98, 30, 20, GREEN)
    i.poly(rotate(box(30, 50, 104, 76), -30), STEEL)
    i.poly(rotate(box(92, 46, 108, 80), -30), DARK)
    i.rect(16, 86, 70, 104, GUNMETAL, 5)
    i.circle(100, 28, 12, GREEN)


def orbit_drones(i: Icon):
    i.glow_at(64, 64, 20, CYAN)
    i.ring(64, 64, 40, rgb("#2d8aa0"), 3)
    i.circle(64, 64, 14, CYAN)
    for a in (20, 140, 260):
        x = 64 + 40 * math.cos(math.radians(a))
        y = 64 + 40 * math.sin(math.radians(a))
        i.poly([(x - 11, y), (x, y - 11), (x + 11, y), (x, y + 11)], STEEL)
        i.circle(x, y, 4, CYAN, light=False, line=6)


def seam_cutter(i: Icon):
    i.glow_at(108, 56, 16, RED)
    i.line([(96, 56), (122, 56)], RED, 5, outline=False)
    i.rect(10, 48, 92, 66, TAN, 6)
    i.rect(24, 62, 46, 92, GUNMETAL, 4)
    i.rect(84, 44, 100, 70, DARK, 3)
    i.rect(40, 40, 70, 50, STEEL, 3)


def torch(i: Icon):
    i.glow_at(104, 46, 22, ORANGE)
    i.poly([(92, 52), (122, 30), (116, 48), (126, 56), (114, 62), (120, 78), (92, 62)], ORANGE)
    i.poly([(96, 54), (112, 44), (110, 52), (116, 57), (96, 60)], YELLOW, light=False, line=5)
    i.rect(28, 46, 96, 64, STEEL, 5)
    i.rect(8, 58, 44, 104, RED, 12)
    i.rect(48, 60, 64, 90, GUNMETAL, 4)


def ripper(i: Icon):
    teeth = []
    for k in range(16):
        a = math.radians(k * 22.5)
        r = 48 if k % 2 == 0 else 38
        teeth.append((64 + r * math.cos(a), 64 + r * math.sin(a)))
    i.poly(teeth, STEEL)
    i.circle(64, 64, 22, GUNMETAL)
    i.circle(64, 64, 8, ORANGE)


def mine_layer(i: Icon):
    i.glow_at(64, 52, 14, RED)
    i.poly([(14, 80), (34, 60), (94, 60), (114, 80), (94, 96), (34, 96)], GUNMETAL)
    i.rect(40, 48, 88, 66, STEEL, 6)
    i.circle(64, 50, 9, RED)
    for x in (26, 102):
        i.rect(x - 4, 78, x + 4, 86, YELLOW, 1, light=False, line=5)


def breaker(i: Icon):
    i.poly(rotate(box(58, 40, 70, 120), 35), TAN)
    i.poly(rotate(box(26, 14, 92, 46), 35), STEEL)
    i.poly(rotate(box(30, 18, 44, 42), 35), ORANGE, light=False)


def seeker(i: Icon):
    i.glow_at(20, 70, 16, ORANGE)
    for y, c in ((44, RED), (74, RED)):
        i.poly([(30, y - 8), (96, y - 8), (116, y), (96, y + 8), (30, y + 8)], WHITE)
        i.poly([(30, y - 8), (40, y - 8), (40, y + 8), (30, y + 8)], c, light=False)
        i.poly([(36, y - 8), (26, y - 18), (46, y - 8)], c, light=False, line=6)
    i.rect(10, 86, 70, 108, GUNMETAL, 5)


def singularity(i: Icon):
    i.glow_at(64, 64, 34, PURPLE)
    i.circle(64, 64, 26, rgb("#1a0f2e"))
    for k, r in enumerate((34, 42)):
        box_ = [(64 - r) * SS, (64 - r * 0.45) * SS, (64 + r) * SS, (64 + r * 0.45) * SS]
        img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        d.ellipse(box_, outline=OUTLINE, width=int(6.5 * SS))
        d.ellipse(box_, outline=shade(PURPLE, 1.2 if k else 1.0), width=int(3.5 * SS))
        img = img.rotate(25 + k * 50, center=(W / 2, W / 2), resample=Image.BICUBIC)
        i.image.alpha_composite(img)
    i.circle(64, 64, 9, rgb("#e2c8ff"), light=False)


# --- Gear ----------------------------------------------------------------------


def helmet(i: Icon, shell, visor, extra=None):
    i.poly([(20, 70), (24, 38), (44, 18), (84, 18), (104, 38), (108, 70), (100, 104), (28, 104)], shell)
    i.rect(14, 64, 30, 92, DARK, 5)
    i.rect(98, 64, 114, 92, DARK, 5)
    if extra:
        extra(i)
    i.glow_at(64, 62, 20, visor)
    i.poly([(30, 50), (98, 50), (94, 80), (34, 80)], visor)


def scout_visor(i: Icon):
    helmet(i, STEEL, CYAN, lambda i: i.line([(84, 20), (96, 4)], STEEL, 3))


def miner_lamp(i: Icon):
    def lamp(i):
        i.glow_at(64, 14, 18, YELLOW)
        i.rect(48, 10, 80, 30, DARK, 5)
        i.circle(64, 20, 8, YELLOW, light=False)

    helmet(i, ORANGE, rgb("#3fb7d6"), lamp)


def targeting_helm(i: Icon):
    helmet(i, GUNMETAL, rgb("#2a3442"))
    i.glow_at(80, 64, 14, RED)
    i.ring(80, 64, 9, RED, 2.5)
    i.line([(80, 50), (80, 78)], RED, 2, outline=False)
    i.line([(66, 64), (94, 64)], RED, 2, outline=False)


def torso(i: Icon, color, trim):
    i.poly([(30, 22), (98, 22), (114, 44), (104, 64), (96, 60), (92, 112), (36, 112), (32, 60), (24, 64), (14, 44)], color)
    i.rect(50, 14, 78, 28, trim, 5)


def plated_suit(i: Icon):
    torso(i, GUNMETAL, DARK)
    i.rect(40, 34, 88, 62, STEEL, 6)
    i.rect(44, 66, 84, 84, STEEL, 5)
    i.rect(44, 88, 84, 106, STEEL, 5)


def medic_suit(i: Icon):
    torso(i, WHITE, rgb("#9aa6b1"))
    i.glow_at(64, 62, 18, GREEN)
    i.poly([(56, 40), (72, 40), (72, 54), (86, 54), (86, 70), (72, 70), (72, 84), (56, 84), (56, 70), (42, 70), (42, 54), (56, 54)], GREEN)


def rig_harness(i: Icon):
    torso(i, rgb("#56614a"), DARK)
    i.line([(38, 26), (86, 108)], TAN, 6)
    i.line([(90, 26), (42, 108)], TAN, 6)
    i.rect(26, 74, 48, 100, RUST, 4)
    i.rect(80, 74, 102, 100, RUST, 4)
    i.circle(64, 66, 8, ORANGE)


def boot(i: Icon, color, sole):
    i.poly([(40, 14), (78, 14), (80, 72), (116, 84), (116, 104), (24, 104), (28, 70)], color)
    i.rect(20, 98, 120, 114, sole, 4)
    i.rect(38, 40, 80, 50, shade(color, 0.7), 2, light=False)


def runner_boots(i: Icon):
    for k, y in enumerate((44, 62, 80)):
        i.line([(4 + k * 4, y), (20, y)], CYAN, 3, outline=False)
    boot(i, rgb("#3a8fd6"), DARK)
    i.poly([(56, 60), (78, 58), (96, 82), (70, 84)], CYAN, light=False, line=6)


def jump_boots(i: Icon):
    i.glow_at(60, 120, 18, ORANGE)
    i.poly([(46, 112), (74, 112), (66, 128), (54, 128)], ORANGE, light=False)
    boot(i, STEEL, GUNMETAL)
    i.rect(44, 102, 76, 116, DARK, 3)


def anchor_boots(i: Icon):
    boot(i, GUNMETAL, rgb("#252b33"))
    i.poly([(46, 54), (66, 54), (66, 74), (82, 74), (82, 88), (30, 88), (30, 74), (46, 74)], RED)


def salvage_scanner(i: Icon):
    i.rect(30, 14, 98, 112, GUNMETAL, 10)
    i.glow_at(64, 52, 22, GREEN)
    i.rect(38, 24, 90, 76, rgb("#12301c"), 6, light=False)
    i.ring(64, 50, 18, GREEN, 2)
    i.line([(64, 50), (78, 38)], GREEN, 2.5, outline=False)
    i.circle(72, 60, 3, YELLOW, light=False, line=4)
    for x in (44, 64, 84):
        i.circle(x, 94, 6, ORANGE if x == 64 else STEEL)


def ore_sifter(i: Icon):
    i.poly([(14, 30), (114, 30), (78, 76), (50, 76)], STEEL)
    i.rect(50, 74, 78, 96, GUNMETAL, 4)
    for x in range(30, 100, 14):
        i.line([(x, 34), (x + 6, 50)], DARK, 1.8, outline=False)
    i.glow_at(64, 108, 14, CYAN)
    crystal(i, 52, 108, 8, CYAN)
    crystal(i, 76, 110, 7, RUST)


def capacitor(i: Icon):
    i.glow_at(64, 64, 26, YELLOW)
    i.rect(32, 22, 96, 112, GUNMETAL, 10)
    i.rect(48, 10, 80, 24, STEEL, 4)
    i.rect(40, 32, 88, 102, rgb("#2b2f1a"), 6, light=False)
    i.poly([(70, 36), (48, 70), (62, 70), (54, 98), (82, 60), (68, 60), (78, 36)], YELLOW)


def neural_siphon(i: Icon):
    i.glow_at(64, 52, 26, PURPLE)
    for a in (200, 250, 290, 340):
        x = 64 + 50 * math.cos(math.radians(a + 180))
        y = 70 + 40 * math.sin(math.radians(a + 180))
        i.line([(64, 70), ((64 + x) / 2 + 6, (70 + y) / 2 + 8), (x, y)], rgb("#c58bff"), 3)
    i.circle(64, 50, 32, rgb("#e2a7c7"))
    i.line([(64, 22), (64, 78)], rgb("#a96b8c"), 2.5, outline=False)
    i.line([(42, 40), (54, 50), (44, 62)], rgb("#a96b8c"), 2.5, outline=False)
    i.line([(86, 40), (74, 50), (84, 62)], rgb("#a96b8c"), 2.5, outline=False)
    i.rect(50, 80, 78, 96, STEEL, 4)


def hunter_module(i: Icon):
    i.poly([(64, 10), (112, 38), (112, 90), (64, 118), (16, 90), (16, 38)], GUNMETAL)
    i.glow_at(64, 64, 20, RED)
    i.circle(64, 64, 22, rgb("#2a1414"), light=False)
    i.circle(64, 64, 11, RED)
    for a in (0, 90, 180, 270):
        x = 64 + 34 * math.cos(math.radians(a))
        y = 64 + 34 * math.sin(math.radians(a))
        i.circle(x, y, 4, ORANGE, light=False, line=5)


# --- Materials and upgrades -------------------------------------------------------


def crystal(i: Icon, cx, cy, r, color):
    i.poly([(cx, cy - r * 1.7), (cx + r, cy - r * 0.4), (cx + r * 0.6, cy + r), (cx - r * 0.6, cy + r), (cx - r, cy - r * 0.4)], color)


def ferrite(i: Icon):
    i.poly([(20, 70), (36, 34), (70, 22), (104, 40), (112, 78), (86, 104), (40, 106)], rgb("#6f5a4c"))
    for x, y, r in ((50, 56, 10), (80, 74, 12), (62, 88, 7)):
        i.poly([(x - r, y), (x - r * 0.3, y - r), (x + r, y - r * 0.5), (x + r * 0.6, y + r * 0.7), (x - r * 0.5, y + r * 0.8)], RUST)


def lumite(i: Icon):
    i.glow_at(64, 64, 34, CYAN)
    crystal(i, 64, 76, 20, CYAN)
    crystal(i, 36, 92, 11, rgb("#2fb4d6"))
    crystal(i, 94, 94, 13, rgb("#7ef0ff"))


def voidstone(i: Icon):
    i.glow_at(64, 64, 30, PURPLE)
    i.poly([(64, 12), (100, 50), (88, 108), (40, 108), (28, 50)], rgb("#3d1f66"))
    i.poly([(64, 26), (84, 54), (64, 92), (46, 54)], PURPLE)


def upgrade(i: Icon):
    i.glow_at(64, 64, 30, ORANGE)
    i.poly([(64, 12), (110, 58), (84, 58), (84, 112), (44, 112), (44, 58), (18, 58)], ORANGE)


# --- Interface glyphs ----------------------------------------------------------------


def star(cx, cy, r_out, r_in, points=5, rot=-90):
    pts = []
    for k in range(points * 2):
        r = r_out if k % 2 == 0 else r_in
        a = math.radians(rot + k * 180 / points)
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def ngon(cx, cy, r, n, rot=-90):
    return [(cx + r * math.cos(math.radians(rot + k * 360 / n)), cy + r * math.sin(math.radians(rot + k * 360 / n))) for k in range(n)]


def skull(i: Icon, cx=64, cy=60, s=1.0, color=WHITE):
    i.poly([(cx - 34 * s, cy - 6 * s), (cx - 30 * s, cy - 30 * s), (cx, cy - 44 * s), (cx + 30 * s, cy - 30 * s), (cx + 34 * s, cy - 6 * s),
            (cx + 24 * s, cy + 18 * s), (cx + 20 * s, cy + 40 * s), (cx - 20 * s, cy + 40 * s), (cx - 24 * s, cy + 18 * s)], color)
    i.poly([(cx - 24 * s, cy - 6 * s), (cx - 6 * s, cy - 2 * s), (cx - 10 * s, cy + 14 * s), (cx - 22 * s, cy + 10 * s)], rgb("#1b1f27"), light=False, line=6)
    i.poly([(cx + 24 * s, cy - 6 * s), (cx + 6 * s, cy - 2 * s), (cx + 10 * s, cy + 14 * s), (cx + 22 * s, cy + 10 * s)], rgb("#1b1f27"), light=False, line=6)
    i.poly([(cx, cy + 16 * s), (cx + 6 * s, cy + 26 * s), (cx - 6 * s, cy + 26 * s)], rgb("#1b1f27"), light=False, line=5)


def g_health(i):
    i.glow_at(64, 64, 26, RED)
    i.poly([(64, 12), (106, 26), (102, 74), (64, 116), (26, 74), (22, 26)], rgb("#c92f2a"))
    i.poly([(56, 38), (72, 38), (72, 56), (90, 56), (90, 72), (72, 72), (72, 90), (56, 90), (56, 72), (38, 72), (38, 56), (56, 56)], WHITE, light=False, line=7)


def g_xp(i):
    i.glow_at(64, 64, 30, CYAN)
    i.poly(star(64, 66, 52, 22, 4, -90), CYAN)
    i.poly(star(64, 66, 20, 8, 4, -45), WHITE, light=False, line=6)


def g_oxygen(i):
    i.glow_at(64, 64, 24, rgb("#7fd4ff"))
    i.rect(38, 26, 90, 114, rgb("#4aa8e0"), 22)
    i.rect(52, 10, 76, 28, STEEL, 4)
    i.circle(64, 72, 16, WHITE, line=7)
    i.circle(64, 72, 7, rgb("#4aa8e0"), light=False, line=5)


def g_threat(i):
    i.glow_at(64, 64, 26, RED)
    skull(i, 64, 62, 1.15, rgb("#e9e2d0"))


def g_boss(i):
    i.glow_at(64, 70, 28, PURPLE)
    i.poly([(30, 34), (40, 10), (52, 26), (64, 6), (76, 26), (88, 10), (98, 34)], YELLOW)
    skull(i, 64, 78, 0.95, rgb("#e9e2d0"))


def g_surge(i):
    i.glow_at(64, 64, 28, ORANGE)
    i.poly([(64, 10), (120, 108), (8, 108)], ORANGE)
    i.rect(56, 40, 72, 80, rgb("#1b1f27"), 4, light=False, line=5)
    i.rect(56, 86, 72, 100, rgb("#1b1f27"), 4, light=False, line=5)


def g_hazard(i):
    i.poly([(64, 10), (120, 108), (8, 108)], YELLOW)
    for k in range(3):
        x = 40 + k * 22
        i.poly([(x, 104), (x + 10, 104), (x + 22, 80), (x + 12, 80)], rgb("#1b1f27"), light=False, line=4)


def g_credits(i):
    i.glow_at(64, 64, 20, YELLOW)
    i.poly(ngon(64, 64, 50, 6, 0), rgb("#e0a822"))
    i.poly(ngon(64, 64, 32, 6, 0), rgb("#f6cf52"))
    i.rect(58, 40, 70, 88, rgb("#8a5a10"), 3, light=False, line=5)


def g_timer(i):
    i.rect(54, 8, 74, 20, STEEL, 3)
    i.circle(64, 70, 46, WHITE)
    i.circle(64, 70, 36, rgb("#232a35"), light=False)
    i.line([(64, 70), (64, 44)], ORANGE, 5, outline=False)
    i.line([(64, 70), (84, 80)], WHITE, 4, outline=False)


def g_cargo(i):
    i.rect(16, 30, 112, 108, rgb("#a8743c"), 6)
    i.rect(16, 30, 112, 50, rgb("#c48a4a"), 6)
    i.line([(28, 58), (100, 100)], rgb("#6e4a24"), 4, outline=False)
    i.line([(100, 58), (28, 100)], rgb("#6e4a24"), 4, outline=False)


def g_lock(i):
    i.ring(64, 44, 22, STEEL, 9)
    i.rect(26, 52, 102, 114, rgb("#d6a23a"), 10)
    i.circle(64, 78, 9, rgb("#1b1f27"), light=False, line=5)
    i.rect(60, 80, 68, 98, rgb("#1b1f27"), 2, light=False, line=4)


def g_check(i):
    i.glow_at(64, 64, 24, GREEN)
    i.poly([(14, 64), (34, 44), (54, 64), (96, 18), (116, 38), (54, 104)], GREEN)


def g_play(i):
    i.glow_at(64, 64, 26, ORANGE)
    i.poly([(30, 14), (114, 64), (30, 114)], ORANGE)


def g_new(i):
    i.glow_at(64, 64, 26, ORANGE)
    i.poly([(52, 12), (76, 12), (76, 52), (116, 52), (116, 76), (76, 76), (76, 116), (52, 116), (52, 76), (12, 76), (12, 52), (52, 52)], ORANGE)


def g_load(i):
    i.poly([(10, 30), (50, 30), (58, 42), (118, 42), (118, 110), (10, 110)], rgb("#d29a3a"))
    i.poly([(10, 54), (118, 54), (118, 110), (10, 110)], rgb("#efb54d"))


def g_settings(i):
    teeth = []
    for k in range(16):
        r = 54 if k % 2 == 0 else 42
        a = math.radians(k * 22.5)
        teeth.append((64 + r * math.cos(a), 64 + r * math.sin(a)))
    i.poly(teeth, STEEL)
    i.circle(64, 64, 18, rgb("#1b1f27"), light=False)


def g_quit(i):
    i.glow_at(64, 64, 24, RED)
    i.ring(64, 68, 40, RED, 10)
    i.rect(56, 6, 72, 64, RED, 6)


def g_save(i):
    i.rect(16, 14, 112, 114, rgb("#3f6fb3"), 10)
    i.rect(34, 14, 92, 46, WHITE, 4)
    i.rect(32, 70, 96, 114, rgb("#dfe6ee"), 4)
    i.rect(76, 20, 86, 40, rgb("#3f6fb3"), 2, light=False, line=4)


def g_trash(i):
    i.rect(24, 22, 104, 36, STEEL, 4)
    i.rect(52, 10, 76, 24, STEEL, 3)
    i.poly([(30, 40), (98, 40), (90, 116), (38, 116)], RED)
    for x in (50, 64, 78):
        i.line([(x, 54), (x, 102)], rgb("#7a1a14"), 3, outline=False)


def g_controller(i):
    i.poly([(14, 52), (30, 30), (98, 30), (114, 52), (118, 92), (102, 104), (84, 82), (44, 82), (26, 104), (10, 92)], GUNMETAL)
    i.rect(30, 50, 54, 58, WHITE, 2, light=False, line=4)
    i.rect(38, 42, 46, 66, WHITE, 2, light=False, line=4)
    for (x, y, c) in ((88, 44, GREEN), (100, 54, RED), (76, 54, CYAN), (88, 64, YELLOW)):
        i.circle(x, y, 6, c, light=False, line=4)


def g_keyboard(i):
    i.rect(8, 34, 120, 98, GUNMETAL, 8)
    for row in range(3):
        for col in range(6 - (row == 2) * 4):
            x = 18 + col * 16 + (row == 2) * 0
            w = 12 if row < 2 else 76
            i.rect(x + (row == 2) * 8, 42 + row * 18, x + w + (row == 2) * 8, 54 + row * 18, rgb("#cfd6de"), 2, light=False, line=4)


def g_pause(i):
    i.rect(26, 16, 54, 112, ORANGE, 5)
    i.rect(74, 16, 102, 112, ORANGE, 5)


def g_back(i):
    i.poly([(10, 64), (60, 18), (60, 44), (118, 44), (118, 84), (60, 84), (60, 110)], STEEL)


def g_sound(i):
    i.poly([(14, 46), (40, 46), (70, 18), (70, 110), (40, 82), (14, 82)], STEEL)
    for r in (22, 40):
        img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        box = [(70 - r) * SS, (64 - r) * SS, (70 + r) * SS, (64 + r) * SS]
        d.arc(box, -50, 50, fill=OUTLINE, width=int(9 * SS))
        d.arc(box, -50, 50, fill=CYAN, width=int(5 * SS))
        i.image.alpha_composite(img)


def g_display(i):
    i.rect(8, 16, 120, 92, GUNMETAL, 6)
    i.rect(18, 26, 110, 82, rgb("#2a5f8a"), 3, light=False)
    i.poly([(18, 82), (60, 40), (80, 60), (92, 48), (110, 66), (110, 82)], rgb("#4b8fc4"), light=False, line=4)
    i.rect(48, 92, 80, 106, STEEL, 2)
    i.rect(30, 104, 98, 114, STEEL, 3)


def g_starmap(i):
    i.glow_at(64, 64, 30, CYAN)
    i.circle(64, 64, 28, rgb("#3f7fd6"))
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    box = [10 * SS, 46 * SS, 118 * SS, 82 * SS]
    d.ellipse(box, outline=OUTLINE, width=int(9 * SS))
    d.ellipse(box, outline=rgb("#ffd56b"), width=int(5 * SS))
    img = img.rotate(-18, center=(W / 2, W / 2), resample=Image.BICUBIC)
    i.image.alpha_composite(img)
    for x, y in ((18, 20), (108, 22), (104, 108), (24, 104)):
        i.poly(star(x, y, 7, 3, 4), WHITE, light=False, line=3)


def g_fabricator(i):
    i.poly(rotate(box(54, 8, 74, 92), 45), STEEL)
    i.poly(rotate([(40, 6), (88, 6), (88, 34), (74, 34), (74, 22), (54, 22), (54, 34), (40, 34)], 45), STEEL)
    i.poly(rotate(box(54, 88, 74, 122), 45), ORANGE)


def g_board(i):
    i.rect(18, 18, 110, 118, rgb("#c99a5a"), 8)
    i.rect(28, 30, 100, 110, rgb("#eef1f4"), 4)
    i.rect(44, 8, 84, 28, STEEL, 4)
    for y in (48, 66, 84):
        i.rect(38, y, 48, y + 8, ORANGE, 2, light=False, line=3)
        i.rect(54, y + 1, 92, y + 7, rgb("#7c8a99"), 2, light=False, line=3)


def g_trade(i):
    i.glow_at(64, 64, 20, YELLOW)
    g_credits_small = [(40, 70, 26), (84, 54, 26)]
    for x, y, r in g_credits_small:
        i.poly(ngon(x, y, r, 6, 0), rgb("#e0a822"))
        i.poly(ngon(x, y, r * 0.62, 6, 0), rgb("#f6cf52"), light=False, line=5)
    i.poly([(22, 104), (60, 96), (106, 104), (106, 116), (22, 116)], STEEL)


def g_pod(i):
    i.glow_at(64, 112, 20, ORANGE)
    i.poly([(64, 8), (98, 40), (104, 92), (24, 92), (30, 40)], WHITE)
    i.rect(44, 34, 84, 60, rgb("#3fb7d6"), 6)
    i.poly([(24, 92), (104, 92), (92, 108), (36, 108)], GUNMETAL)
    i.poly([(50, 108), (78, 108), (64, 124)], ORANGE, light=False)


def g_level(i):
    i.glow_at(64, 64, 22, ORANGE)
    for k, y in enumerate((70, 40)):
        i.poly([(64, y - 30), (110, y + 12), (110, y + 34), (64, y - 8), (18, y + 34), (18, y + 12)], ORANGE if k else YELLOW)


def g_site(i):
    i.poly([(4, 112), (40, 36), (60, 66), (80, 22), (124, 112)], rgb("#7d6a5a"))
    i.poly([(70, 112), (70, 88), (90, 88), (90, 112)], rgb("#1b1f27"), light=False, line=5)


def g_mutator(i):
    i.glow_at(64, 64, 22, GREEN)
    for a in (-90, 30, 150):
        x = 64 + 26 * math.cos(math.radians(a))
        y = 66 + 26 * math.sin(math.radians(a))
        i.circle(x, y, 22, GREEN)
    i.circle(64, 66, 12, rgb("#1b1f27"), light=False)


# Objectives (by kind).


def o_quota(i):
    lumite(i)
    i.poly(rotate(box(80, 70, 88, 122), 40), TAN)
    i.poly(rotate([(62, 64), (106, 64), (100, 74), (68, 74)], 40), STEEL)


def o_rescue(i):
    i.glow_at(64, 50, 26, GREEN)
    i.circle(64, 36, 18, rgb("#f0c9a2"))
    i.poly([(30, 116), (34, 64), (94, 64), (98, 116)], ORANGE)
    i.poly([(56, 74), (72, 74), (72, 84), (82, 84), (82, 96), (72, 96), (72, 106), (56, 106), (56, 96), (46, 96), (46, 84), (56, 84)], WHITE, light=False, line=5)


def o_defend(i):
    i.poly([(20, 116), (44, 40), (84, 40), (108, 116)], STEEL)
    i.rect(40, 30, 88, 46, ORANGE, 4)
    i.poly([(54, 46), (74, 46), (64, 76)], rgb("#cfd6de"))
    i.line([(30, 100), (98, 100)], GUNMETAL, 4)


def o_salvage(i):
    i.rect(14, 46, 106, 112, rgb("#56614a"), 6)
    teeth = []
    for k in range(12):
        r = 30 if k % 2 == 0 else 22
        a = math.radians(k * 30)
        teeth.append((86 + r * math.cos(a), 44 + r * math.sin(a)))
    i.poly(teeth, STEEL)
    i.circle(86, 44, 9, rgb("#1b1f27"), light=False)


def o_hunt(i):
    i.glow_at(64, 64, 24, RED)
    i.ring(64, 64, 40, RED, 6)
    i.ring(64, 64, 18, RED, 5)
    for a, b in (((64, 6), (64, 34)), ((64, 94), (64, 122)), ((6, 64), (34, 64)), ((94, 64), (122, 64))):
        i.line([a, b], RED, 6)


def o_glowcaps(i):
    i.glow_at(64, 50, 30, rgb("#c58bff"))
    i.rect(52, 60, 76, 116, rgb("#e8dccb"), 8)
    i.poly([(14, 66), (24, 36), (64, 18), (104, 36), (114, 66)], rgb("#a45cff"))
    for x, y in ((40, 46), (64, 34), (88, 48)):
        i.circle(x, y, 6, rgb("#f2d8ff"), light=False, line=4)


def o_cull(i):
    i.circle(64, 72, 30, rgb("#6b7f3a"))
    i.circle(64, 36, 16, rgb("#7f9a44"))
    for s in (-1, 1):
        for dy in (-10, 6, 22):
            i.line([(64 + s * 26, 70 + dy), (64 + s * 52, 58 + dy * 1.4)], rgb("#3a4620"), 4)


def o_scan(i):
    i.glow_at(64, 30, 20, CYAN)
    i.poly([(40, 120), (60, 40), (68, 40), (88, 120)], STEEL)
    i.circle(64, 32, 12, CYAN)
    for r in (24, 38):
        img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        box_ = [(64 - r) * SS, (32 - r) * SS, (64 + r) * SS, (32 + r) * SS]
        for a0 in (-60, 120):
            d.arc(box_, a0, a0 + 120 - 120 + 60, fill=OUTLINE, width=int(8 * SS))
            d.arc(box_, a0, a0 + 60, fill=CYAN, width=int(4 * SS))
        i.image.alpha_composite(img)


# Classes and abilities.


def emblem(i: Icon, color, inner):
    i.poly([(64, 6), (114, 26), (110, 82), (64, 122), (18, 82), (14, 26)], color)
    i.poly([(64, 18), (102, 34), (99, 78), (64, 108), (29, 78), (26, 34)], shade(color, 0.55), light=False, line=5)
    inner(i)


def c_striker(i):
    emblem(i, rgb("#e0552c"), lambda i: i.poly([(72, 26), (40, 70), (60, 70), (52, 104), (88, 54), (68, 54), (80, 26)], YELLOW))


def c_prospector(i):
    def inner(i):
        i.poly(rotate(box(60, 36, 68, 104), 35), TAN)
        i.poly(rotate([(34, 40), (64, 30), (94, 40), (90, 48), (64, 42), (38, 48)], 35), STEEL)

    emblem(i, rgb("#d6a23a"), inner)


def c_warden(i):
    emblem(i, rgb("#3a8fd6"), lambda i: i.poly([(64, 32), (90, 42), (88, 72), (64, 96), (40, 72), (38, 42)], WHITE))


def c_engineer(i):
    def inner(i):
        i.poly(rotate(box(58, 36, 70, 100), -40), STEEL)
        i.ring(46, 50, 12, STEEL, 7)
        i.circle(84, 82, 9, GREEN)

    emblem(i, rgb("#4caf62"), inner)


def a_blink(i):
    i.glow_at(80, 64, 26, CYAN)
    for k, x in enumerate((26, 46, 66)):
        i.poly([(x, 30), (x + 22, 64), (x, 98), (x + 10, 64)], shade(CYAN, 0.5 + 0.25 * k), light=False, line=5)
    i.poly([(86, 24), (120, 64), (86, 104), (98, 64)], WHITE)


def a_bore(i):
    i.glow_at(98, 64, 18, ORANGE)
    i.poly([(14, 40), (70, 40), (118, 64), (70, 88), (14, 88)], STEEL)
    for x in (36, 56, 76):
        i.line([(x, 44), (x + 10, 84)], GUNMETAL, 3, outline=False)
    i.rect(6, 50, 22, 78, ORANGE, 3)


def a_barrier(i):
    i.glow_at(64, 64, 30, rgb("#6fb8ff"))
    for x in (26, 64, 102):
        i.rect(x - 8, 30, x + 8, 112, STEEL, 3)
        i.circle(x, 26, 9, rgb("#9fd6ff"), line=5)
    i.line([(26, 52), (64, 70), (102, 52)], rgb("#9fd6ff"), 4)
    i.line([(26, 82), (64, 98), (102, 82)], rgb("#9fd6ff"), 4)


def a_turret(i):
    i.glow_at(100, 44, 14, RED)
    i.poly([(28, 118), (44, 78), (84, 78), (100, 118)], GUNMETAL)
    i.rect(36, 48, 92, 82, rgb("#4caf62"), 8)
    i.rect(84, 56, 120, 68, STEEL, 3)
    i.circle(56, 64, 8, RED, light=False, line=5)


# Planets and the station.


def globe(i: Icon, base, band, extra=None, glow=None):
    if glow:
        i.glow_at(64, 64, 40, glow)
    i.circle(64, 64, 52, base)
    clip = Image.new("L", (W, W), 0)
    ImageDraw.Draw(clip).ellipse([16 * SS, 16 * SS, 112 * SS, 112 * SS], fill=255)
    layer = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    for y, h in ((38, 10), (60, 14), (86, 9)):
        d.ellipse([0, (y - h / 2) * SS, W, (y + h / 2) * SS], fill=band)
    if extra:
        extra(d)
    i.image.paste(layer, (0, 0), Image.composite(layer, Image.new("RGBA", (W, W)), clip).split()[3])
    # Terminator shadow.
    shadow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).ellipse([40 * SS, 30 * SS, 150 * SS, 140 * SS], fill=(8, 10, 20, 120))
    i.image.paste(shadow, (0, 0), Image.composite(shadow, Image.new("RGBA", (W, W)), clip).split()[3])
    box_ = [12 * SS, 12 * SS, 116 * SS, 116 * SS]
    ImageDraw.Draw(i.image).ellipse(box_, outline=OUTLINE, width=LINE)


def p_verdant(i):
    globe(i, rgb("#5c9a46"), rgb("#7cc25a"), lambda d: d.ellipse([60 * SS, 50 * SS, 90 * SS, 70 * SS], fill=rgb("#3a7a40")), rgb("#8fe07a"))


def p_cinder(i):
    globe(i, rgb("#5a2a1e"), rgb("#ff7a2a"), lambda d: d.line([(30 * SS, 70 * SS), (70 * SS, 50 * SS), (100 * SS, 80 * SS)], fill=rgb("#ffcf4a"), width=4 * SS), ORANGE)


def p_hollow(i):
    globe(i, rgb("#9fc6e0"), rgb("#e6f4ff"), None, rgb("#bfe6ff"))


def p_tidewell(i):
    globe(i, rgb("#2a6b7a"), rgb("#3f9aa0"), lambda d: d.ellipse([30 * SS, 30 * SS, 60 * SS, 48 * SS], fill=rgb("#5a8a3a")), rgb("#5fd0d0"))


def p_kharon(i):
    globe(i, rgb("#5a4a6a"), rgb("#7a6a8a"), lambda d: [d.ellipse([(x - r) * SS, (y - r) * SS, (x + r) * SS, (y + r) * SS], fill=rgb("#3e3250")) for x, y, r in ((44, 50, 8), (76, 76, 11), (82, 40, 6))], PURPLE)


def p_station(i):
    i.glow_at(64, 64, 26, YELLOW)
    i.ring(64, 64, 44, STEEL, 9)
    i.rect(56, 14, 72, 114, GUNMETAL, 4)
    i.rect(14, 56, 114, 72, GUNMETAL, 4)
    i.circle(64, 64, 18, rgb("#e0a822"))


# Ship modules.


def m_med(i):
    i.rect(14, 14, 114, 114, WHITE, 16)
    i.poly([(52, 28), (76, 28), (76, 52), (100, 52), (100, 76), (76, 76), (76, 100), (52, 100), (52, 76), (28, 76), (28, 52), (52, 52)], RED)


def m_armory(i):
    i.rect(10, 92, 118, 112, GUNMETAL, 4)
    for k, x in enumerate((26, 54, 82)):
        i.poly(rotate(box(x, 22, x + 16, 96), 0), STEEL if k != 1 else ORANGE)
        i.rect(x - 2, 30, x + 18, 40, DARK, 2, light=False, line=4)


def m_drill(i):
    i.glow_at(64, 110, 16, ORANGE)
    i.rect(38, 10, 90, 40, STEEL, 6)
    i.poly([(40, 40), (88, 40), (64, 118)], rgb("#cfd6de"))
    for y in (56, 74, 92):
        i.line([(50 - (y - 40) * 0.1, y), (78 + (y - 40) * 0.1, y - 8)], GUNMETAL, 3, outline=False)


def m_tractor(i):
    i.glow_at(64, 100, 24, CYAN)
    i.poly([(14, 16), (50, 16), (50, 66), (64, 80), (78, 66), (78, 16), (114, 16), (114, 72), (64, 116), (14, 72)], RED)
    i.rect(14, 16, 50, 34, STEEL, 2)
    i.rect(78, 16, 114, 34, STEEL, 2)


def m_reactor(i):
    i.glow_at(64, 64, 34, CYAN)
    for a in (0, 60, 120):
        img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        box_ = [12 * SS, 46 * SS, 116 * SS, 82 * SS]
        d.ellipse(box_, outline=OUTLINE, width=int(9 * SS))
        d.ellipse(box_, outline=CYAN, width=int(5 * SS))
        img = img.rotate(a, center=(W / 2, W / 2), resample=Image.BICUBIC)
        i.image.alpha_composite(img)
    i.circle(64, 64, 14, WHITE)


UI_ICONS = {
    "health": g_health,
    "xp": g_xp,
    "oxygen": g_oxygen,
    "threat": g_threat,
    "boss": g_boss,
    "surge": g_surge,
    "hazard": g_hazard,
    "credits": g_credits,
    "timer": g_timer,
    "cargo": g_cargo,
    "lock": g_lock,
    "check": g_check,
    "play": g_play,
    "new": g_new,
    "load": g_load,
    "settings": g_settings,
    "quit": g_quit,
    "save": g_save,
    "trash": g_trash,
    "controller": g_controller,
    "keyboard": g_keyboard,
    "pause": g_pause,
    "back": g_back,
    "sound": g_sound,
    "display": g_display,
    "star-map": g_starmap,
    "fabricator": g_fabricator,
    "board": g_board,
    "trade": g_trade,
    "pod": g_pod,
    "level": g_level,
    "site": g_site,
    "mutator": g_mutator,
    # Objectives by kind.
    "obj-ore_quota": o_quota,
    "obj-boss": g_boss,
    "obj-rescue": o_rescue,
    "obj-defend": o_defend,
    "obj-salvage": o_salvage,
    "obj-hunt": o_hunt,
    "obj-glowcaps": o_glowcaps,
    "obj-cull": o_cull,
    "obj-scan": o_scan,
    # Classes and abilities.
    "class-striker": c_striker,
    "class-prospector": c_prospector,
    "class-warden": c_warden,
    "class-engineer": c_engineer,
    "ability-blink": a_blink,
    "ability-bore": a_bore,
    "ability-barrier": a_barrier,
    "ability-turret": a_turret,
    # Planets (ids from planets.toml) and the trade station.
    "planet-verdant": p_verdant,
    "planet-cinder": p_cinder,
    "planet-hollow": p_hollow,
    "planet-tidewell": p_tidewell,
    "planet-kharon": p_kharon,
    "planet-station": p_station,
    # Ship modules (ids from upgrades.toml).
    "module-med-bay": m_med,
    "module-armory": m_armory,
    "module-drill-workshop": m_drill,
    "module-tractor-array": m_tractor,
    "module-reactor": m_reactor,
}


def logo(fonts: Path) -> Image.Image:
    """The PLANETFALL wordmark: a ringed planet behind heavy, outlined
    letters with a warm vertical gradient, and a small tagline."""
    from PIL import ImageFont

    width, height = 1400, 360
    k = 2
    img = Image.new("RGBA", (width * k, height * k), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Planet and ring behind the word.
    cx, cy, r = 1130 * k, 150 * k, 120 * k
    glow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([cx - r - 40 * k, cy - r - 40 * k, cx + r + 40 * k, cy + r + 40 * k], fill=(255, 142, 48, 90))
    img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(30 * k)))
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(46, 58, 84, 255), outline=OUTLINE, width=8 * k)
    d.chord([cx - r, cy - r, cx + r, cy + r], 200, 340, fill=(70, 88, 124, 255))
    ring = Image.new("RGBA", img.size, (0, 0, 0, 0))
    rd = ImageDraw.Draw(ring)
    rd.ellipse([cx - 230 * k, cy - 36 * k, cx + 230 * k, cy + 36 * k], outline=OUTLINE, width=16 * k)
    rd.ellipse([cx - 230 * k, cy - 36 * k, cx + 230 * k, cy + 36 * k], outline=(255, 170, 70, 255), width=8 * k)
    img.alpha_composite(ring.rotate(12, center=(cx, cy), resample=Image.BICUBIC))
    # The word: letter-spaced, gradient-filled, thick outline, drop shadow.
    font = ImageFont.truetype(str(fonts / "DejaVuSans-Bold.ttf"), 150 * k)
    text = "PLANETFALL"
    x, y = 40 * k, 60 * k
    spacing = 10 * k
    mask = Image.new("L", img.size, 0)
    md = ImageDraw.Draw(mask)
    cursor = x
    for ch in text:
        md.text((cursor, y), ch, font=font, fill=255)
        cursor += font.getlength(ch) + spacing
    outline = mask.filter(ImageFilter.MaxFilter(17 * k // 2 * 2 + 1))
    shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    shadow.putalpha(outline.point(lambda a: int(a * 0.6)))
    shadow = shadow.filter(ImageFilter.GaussianBlur(8 * k))
    img.alpha_composite(shadow, (6 * k, 10 * k))
    dark = Image.new("RGBA", img.size, OUTLINE)
    dark.putalpha(outline)
    img.alpha_composite(dark)
    grad = Image.new("RGBA", img.size, (0, 0, 0, 0))
    gd = ImageDraw.Draw(grad)
    top, bottom = y + 30 * k, y + 170 * k
    for row in range(top, bottom):
        t = (row - top) / (bottom - top)
        col = (255, int(214 - 90 * t), int(110 - 80 * t), 255)
        gd.line([(0, row), (img.size[0], row)], fill=col)
    letters = Image.new("RGBA", img.size, (0, 0, 0, 0))
    letters.paste(grad, (0, 0), mask)
    img.alpha_composite(letters)
    # Hard highlight on the upper half of the letters.
    hi = Image.new("RGBA", img.size, (255, 246, 220, 0))
    band = Image.new("L", img.size, 0)
    ImageDraw.Draw(band).rectangle([0, top, img.size[0], top + 46 * k], fill=90)
    hi.putalpha(Image.composite(band, Image.new("L", img.size, 0), mask))
    img.alpha_composite(hi)
    # Tagline with hazard ticks.
    small = ImageFont.truetype(str(fonts / "DejaVuSans-Bold.ttf"), 34 * k)
    tag = "SALVAGE  ·  SURVIVE  ·  EXTRACT"
    tx, ty = 52 * k, 262 * k
    d.rectangle([tx, ty + 12 * k, tx + 24 * k, ty + 30 * k], fill=(255, 142, 48, 255))
    cursor = tx + 44 * k
    for ch in tag:
        d.text((cursor, ty), ch, font=small, fill=(200, 214, 226, 255))
        cursor += small.getlength(ch) + 5 * k
    d.rectangle([cursor + 18 * k, ty + 12 * k, cursor + 260 * k, ty + 30 * k], fill=(255, 142, 48, 255))
    return img.resize((width, height), Image.LANCZOS)


# --- Controller and mouse glyphs (button prompts) -------------------------------


def letter(i: Icon, text: str, cx: float, cy: float, size: float, color):
    """A centred bold letter with a dark outline."""
    from PIL import ImageFont

    font = ImageFont.truetype(str(REPO / "assets" / "fonts" / "DejaVuSans-Bold.ttf"), int(size * SS))
    d = ImageDraw.Draw(i.image)
    d.text((cx * SS, cy * SS), text, font=font, fill=color, anchor="mm", stroke_width=int(2.5 * SS), stroke_fill=OUTLINE)


def face(color, text=None, symbol=None, dark=False):
    def paint(i: Icon):
        base = rgb("#20262f") if dark else color
        i.circle(64, 64, 54, base)
        if text:
            letter(i, text, 64, 66, 64, WHITE if not dark else color)
        if symbol:
            symbol(i)
    return paint


def ps_cross(i):
    i.line([(42, 42), (86, 86)], rgb("#7fb2ff"), 7, outline=False)
    i.line([(86, 42), (42, 86)], rgb("#7fb2ff"), 7, outline=False)


def ps_circle(i):
    i.ring(64, 64, 22, rgb("#ff6b6b"), 7)


def ps_square(i):
    d = ImageDraw.Draw(i.image)
    d.rectangle([42 * SS, 42 * SS, 86 * SS, 86 * SS], outline=rgb("#ff8de0"), width=int(7 * SS))


def ps_triangle(i):
    d = ImageDraw.Draw(i.image)
    d.polygon([(64 * SS, 38 * SS), (90 * SS, 84 * SS), (38 * SS, 84 * SS)], outline=rgb("#5fe0c0"), width=int(7 * SS))


def shoulder(text, trigger=False):
    def paint(i: Icon):
        if trigger:
            i.poly([(26, 20), (102, 20), (110, 60), (100, 110), (28, 110), (18, 60)], rgb("#3a424e"))
        else:
            i.rect(10, 36, 118, 92, rgb("#3a424e"), 26)
        letter(i, text, 64, 66 if trigger else 64, 46, WHITE)
    return paint


def stick(text):
    def paint(i: Icon):
        i.circle(64, 64, 54, rgb("#2a3038"))
        i.circle(64, 64, 34, rgb("#4a5260"))
        letter(i, text, 64, 66, 36, WHITE)
    return paint


def pad_dpad(i):
    i.poly([(46, 10), (82, 10), (82, 46), (118, 46), (118, 82), (82, 82), (82, 118), (46, 118), (46, 82), (10, 82), (10, 46), (46, 46)], rgb("#3a424e"))
    i.circle(64, 64, 10, rgb("#22272f"), light=False, line=4)


def pad_start(i):
    i.rect(14, 34, 114, 94, rgb("#3a424e"), 30)
    for y in (52, 64, 76):
        i.line([(46, y), (82, y)], WHITE, 4, outline=False)


def pad_select(i):
    i.rect(14, 34, 114, 94, rgb("#3a424e"), 30)
    i.rect(40, 48, 70, 72, WHITE, 3, light=False, line=4)
    i.rect(56, 58, 86, 82, rgb("#9aa6b1"), 3, light=False, line=4)


def mouse(button):
    def paint(i: Icon):
        i.poly([(28, 40), (40, 14), (88, 14), (100, 40), (100, 92), (84, 118), (44, 118), (28, 92)], rgb("#dfe6ee"))
        i.line([(64, 14), (64, 56)], rgb("#6a7480"), 3, outline=False)
        i.line([(28, 56), (100, 56)], rgb("#6a7480"), 3, outline=False)
        lit = {"left": [(30, 40), (41, 16), (62, 16), (62, 54), (30, 54)], "right": [(66, 16), (87, 16), (98, 40), (98, 54), (66, 54)]}
        if button in lit:
            i.poly(lit[button], ACCENT, light=False, line=5)
        else:
            i.rect(56, 22, 72, 48, ACCENT, 6, light=False, line=5)
    return paint


ACCENT = rgb("#ff8e30")

PAD_ICONS = {
    "xbox-a": face(rgb("#3fa34d"), "A"),
    "xbox-b": face(rgb("#d1382b"), "B"),
    "xbox-x": face(rgb("#2f6fd1"), "X"),
    "xbox-y": face(rgb("#e0b52a"), "Y"),
    "nin-a": face(rgb("#4a5260"), "A"),
    "nin-b": face(rgb("#4a5260"), "B"),
    "nin-x": face(rgb("#4a5260"), "X"),
    "nin-y": face(rgb("#4a5260"), "Y"),
    "ps-cross": face(None, symbol=ps_cross, dark=True),
    "ps-circle": face(None, symbol=ps_circle, dark=True),
    "ps-square": face(None, symbol=ps_square, dark=True),
    "ps-triangle": face(None, symbol=ps_triangle, dark=True),
    "pad-lb": shoulder("L"),
    "pad-rb": shoulder("R"),
    "pad-lt": shoulder("LT", True),
    "pad-rt": shoulder("RT", True),
    "pad-ls": stick("L"),
    "pad-rs": stick("R"),
    "pad-dpad": pad_dpad,
    "pad-start": pad_start,
    "pad-select": pad_select,
    "mouse-left": mouse("left"),
    "mouse-right": mouse("right"),
    "mouse-middle": mouse("middle"),
}


ICONS = {
    # Weapons (ids from weapons.toml).
    "pulse-blaster": pulse_blaster,
    "scattergun": scattergun,
    "arc-caster": arc_caster,
    "plasma-mortar": plasma_mortar,
    "orbit-drones": orbit_drones,
    "seam-cutter": seam_cutter,
    "torch": torch,
    "ripper": ripper,
    "mine-layer": mine_layer,
    "breaker": breaker,
    "seeker": seeker,
    "singularity": singularity,
    # Gear bases (ids from gear.toml).
    "scout-visor": scout_visor,
    "miner-lamp": miner_lamp,
    "targeting-helm": targeting_helm,
    "plated-suit": plated_suit,
    "medic-suit": medic_suit,
    "rig-harness": rig_harness,
    "runner-boots": runner_boots,
    "jump-boots": jump_boots,
    "anchor-boots": anchor_boots,
    "salvage-scanner": salvage_scanner,
    "ore-sifter": ore_sifter,
    "capacitor": capacitor,
    "neural-siphon": neural_siphon,
    "hunter-module": hunter_module,
    # Materials and generic upgrades.
    "ferrite": ferrite,
    "lumite": lumite,
    "voidstone": voidstone,
    "upgrade": upgrade,
    **UI_ICONS,
    **PAD_ICONS,
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=REPO / "assets" / "ui")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    rows = (len(ICONS) + COLUMNS - 1) // COLUMNS
    atlas = Image.new("RGBA", (COLUMNS * SIZE, rows * SIZE), (0, 0, 0, 0))
    for index, (name, paint) in enumerate(ICONS.items()):
        icon = Icon()
        paint(icon)
        atlas.paste(icon.finish(), ((index % COLUMNS) * SIZE, (index // COLUMNS) * SIZE))
    atlas.save(args.out / "icons.png", optimize=True)
    manifest = [
        "# Generated by DevUtils/assets/generate_icons.py: atlas cells in order.",
        f"size = {SIZE}",
        f"columns = {COLUMNS}",
        "names = [",
        *[f'    "{name}",' for name in ICONS],
        "]",
        "",
    ]
    (args.out / "icons.toml").write_text("\n".join(manifest))
    logo(REPO / "assets" / "fonts").save(args.out / "logo.png", optimize=True)
    print(f"wrote {len(ICONS)} icons to {args.out}")


if __name__ == "__main__":
    main()
