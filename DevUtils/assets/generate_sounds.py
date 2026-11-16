#!/usr/bin/env python3
"""Synthesize Planetfall's original sound effects and loops as 16-bit WAV files.

Everything is generated from oscillators, filtered noise and envelopes with a
fixed random seed, so the output is reproducible and needs no third-party
samples or licenses. Loops are built from whole oscillator cycles or are
crossfaded into themselves so they repeat without clicks.

Usage:
    python3 DevUtils/assets/generate_sounds.py [--out assets/sounds]
"""

from __future__ import annotations

import argparse
import math
import random
import struct
import wave
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
RATE = 44100
TAU = 2 * math.pi


def silence(seconds: float) -> list[float]:
    return [0.0] * int(seconds * RATE)


def mix(*tracks: list[float]) -> list[float]:
    length = max(len(t) for t in tracks)
    out = [0.0] * length
    for track in tracks:
        for i, v in enumerate(track):
            out[i] += v
    return out


def gain(track: list[float], amount: float) -> list[float]:
    return [v * amount for v in track]


def offset(track: list[float], seconds: float) -> list[float]:
    return silence(seconds) + track


def env(n: int, attack: float, decay: float) -> list[float]:
    """Linear attack then exponential decay (decay is the e-folding time in s)."""
    a = max(1, int(attack * RATE))
    out = []
    for i in range(n):
        if i < a:
            out.append(i / a)
        else:
            out.append(math.exp(-(i - a) / (decay * RATE)))
    return out


def sweep(seconds: float, f0: float, f1: float, shape: str = "sine", curve: float = 1.0) -> list[float]:
    n = int(seconds * RATE)
    phase = 0.0
    out = []
    for i in range(n):
        t = (i / n) ** curve
        f = f0 + (f1 - f0) * t
        phase += f / RATE
        p = phase % 1.0
        if shape == "sine":
            v = math.sin(TAU * p)
        elif shape == "square":
            v = 1.0 if p < 0.5 else -1.0
        elif shape == "saw":
            v = 2 * p - 1
        else:  # triangle
            v = 4 * abs(p - 0.5) - 1
        out.append(v)
    return out


def noise(seconds: float, rng: random.Random) -> list[float]:
    return [rng.uniform(-1, 1) for _ in range(int(seconds * RATE))]


def lowpass(track: list[float], cutoff: float | list[float]) -> list[float]:
    out, y = [], 0.0
    for i, v in enumerate(track):
        c = cutoff[i] if isinstance(cutoff, list) else cutoff
        alpha = 1 - math.exp(-TAU * c / RATE)
        y += alpha * (v - y)
        out.append(y)
    return out


def highpass(track: list[float], cutoff: float) -> list[float]:
    low = lowpass(track, cutoff)
    return [v - l for v, l in zip(track, low)]


def shaped(track: list[float], envelope: list[float]) -> list[float]:
    return [v * e for v, e in zip(track, envelope)]


def drive(track: list[float], amount: float) -> list[float]:
    return [math.tanh(v * amount) / math.tanh(amount) for v in track]


def tone(freq: float, seconds: float, attack=0.005, decay=0.2, shape="sine") -> list[float]:
    n = int(seconds * RATE)
    return shaped(sweep(seconds, freq, freq, shape), env(n, attack, decay))


def note(midi: int) -> float:
    return 440.0 * 2 ** ((midi - 69) / 12)


def normalize(track: list[float], peak: float = 0.85) -> list[float]:
    top = max((abs(v) for v in track), default=0.0) or 1.0
    return [v * peak / top for v in track]


def fade_edges(track: list[float], seconds: float = 0.004) -> list[float]:
    n = min(len(track) // 2, int(seconds * RATE))
    out = list(track)
    for i in range(n):
        out[i] *= i / n
        out[-1 - i] *= i / n
    return out


def seamless(track: list[float], crossfade: float) -> list[float]:
    """Fold the tail over the head so the loop point is continuous."""
    n = int(crossfade * RATE)
    body = track[:-n]
    tail = track[-n:]
    for i in range(n):
        w = i / n
        body[i] = body[i] * w + tail[i] * (1 - w)
    return body


def write(path: Path, track: list[float], peak: float = 0.85, fade: bool = True) -> None:
    data = normalize(fade_edges(track) if fade else track, peak)
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, v)) * 32767)) for v in data))


def build(rng: random.Random) -> dict[str, tuple[list[float], float, bool]]:
    s: dict[str, tuple[list[float], float, bool]] = {}

    def add(name, track, peak=0.85, fade=True):
        s[name] = (track, peak, fade)

    # --- weapons ---------------------------------------------------------
    n = int(0.14 * RATE)
    add("blaster", shaped(mix(sweep(0.14, 1500, 260, "square", 0.6), gain(sweep(0.14, 750, 130, "sine", 0.6), 1.2)), env(n, 0.002, 0.05)), 0.55)
    n = int(0.22 * RATE)
    burst = lowpass(noise(0.22, rng), 3200)
    add("scatter", mix(shaped(burst, env(n, 0.001, 0.05)), shaped(sweep(0.22, 140, 45), env(n, 0.002, 0.08))), 0.7)
    n = int(0.3 * RATE)
    crackle = [v if rng.random() < 0.35 else 0.0 for v in highpass(noise(0.3, rng), 1800)]
    add("arc", mix(shaped(crackle, env(n, 0.002, 0.12)), shaped(sweep(0.3, 2400, 1800, "saw"), gain(env(n, 0.002, 0.06), 0.3))), 0.6)
    n = int(0.3 * RATE)
    add("mortar", mix(shaped(sweep(0.3, 220, 70), env(n, 0.003, 0.1)), shaped(lowpass(noise(0.3, rng), 900), gain(env(n, 0.001, 0.05), 0.6))), 0.7)
    n = int(0.9 * RATE)
    boom = lowpass(noise(0.9, rng), [2500 * math.exp(-i / (0.15 * RATE)) + 120 for i in range(n)])
    add("explosion", drive(mix(shaped(boom, env(n, 0.002, 0.25)), shaped(sweep(0.9, 110, 30), env(n, 0.002, 0.2))), 2.0), 0.9)
    n = int(0.18 * RATE)
    add("drone", shaped(mix(sweep(0.18, 900, 1300, "triangle"), gain(lowpass(noise(0.18, rng), 4000), 0.4)), env(n, 0.01, 0.06)), 0.35)

    # --- enemies -----------------------------------------------------------
    n = int(0.08 * RATE)
    add("enemy_hit", shaped(mix(lowpass(noise(0.08, rng), 1500), sweep(0.08, 300, 120)), env(n, 0.001, 0.025)), 0.5)
    n = int(0.42 * RATE)
    add("enemy_die", mix(shaped(sweep(0.42, 420, 70, "saw", 0.5), env(n, 0.004, 0.12)), shaped(lowpass(noise(0.42, rng), 1200), gain(env(n, 0.002, 0.09), 0.7))), 0.65)
    n = int(0.25 * RATE)
    add("skitter_die", shaped(mix(sweep(0.25, 2200, 600, "square", 0.4), gain(highpass(noise(0.25, rng), 2000), 0.5)), env(n, 0.001, 0.05)), 0.45)
    n = int(0.7 * RATE)
    add("brute_roar", drive(shaped(mix(sweep(0.7, 95, 70, "saw"), sweep(0.7, 143, 100, "saw"), gain(lowpass(noise(0.7, rng), 600), 0.8)), env(n, 0.08, 0.3)), 3.0), 0.75)
    n = int(0.5 * RATE)
    add("husk_groan", shaped(lowpass(mix(sweep(0.5, 160, 120, "saw"), sweep(0.5, 163, 118, "saw")), 900), env(n, 0.06, 0.2)), 0.45)

    # --- player ------------------------------------------------------------
    n = int(0.28 * RATE)
    add("player_hurt", drive(shaped(mix(sweep(0.28, 220, 90, "square"), gain(lowpass(noise(0.28, rng), 1600), 0.6)), env(n, 0.002, 0.09)), 2.5), 0.65)
    n = int(0.22 * RATE)
    add("dash", shaped(lowpass(noise(0.22, rng), [400 + 5000 * math.sin(math.pi * i / n) for i in range(n)]), env(n, 0.03, 0.08)), 0.5)
    n = int(1.6 * RATE)
    add("player_die", mix(shaped(sweep(1.6, 400, 40, "saw", 0.5), env(n, 0.01, 0.5)), shaped(lowpass(noise(1.6, rng), 500), gain(env(n, 0.01, 0.4), 0.5))), 0.75)

    # --- mining & pickups ---------------------------------------------------
    n = int(0.16 * RATE)
    clink = mix(tone(1870, 0.16, 0.001, 0.04), gain(tone(2950, 0.16, 0.001, 0.03), 0.6), gain(tone(620, 0.16, 0.001, 0.05), 0.5))
    add("mine_hit", mix(clink, shaped(lowpass(noise(0.16, rng), 3000), gain(env(n, 0.0005, 0.015), 0.8))), 0.5)
    n = int(0.45 * RATE)
    rubble = [v * (0.4 + 0.6 * (rng.random() < 0.2)) for v in lowpass(noise(0.45, rng), 1100)]
    add("block_break", mix(shaped(rubble, env(n, 0.002, 0.13)), shaped(sweep(0.45, 160, 50), gain(env(n, 0.002, 0.07), 0.8))), 0.7)
    add("pickup_xp", mix(tone(1320, 0.09, 0.001, 0.03), offset(tone(1760, 0.07, 0.001, 0.03), 0.03)), 0.3)
    add("pickup_material", mix(tone(880, 0.25, 0.002, 0.08, "triangle"), offset(tone(1318.5, 0.2, 0.002, 0.08, "triangle"), 0.06)), 0.45)
    lvl = []
    for i, m in enumerate((72, 76, 79, 84)):
        lvl.append(offset(mix(tone(note(m), 0.5, 0.003, 0.18, "square"), gain(tone(note(m) * 2, 0.4, 0.003, 0.1), 0.4)), i * 0.09))
    add("level_up", lowpass(mix(*lvl), 5000), 0.55)
    chord = mix(*[gain(sweep(1.0, note(m) * 0.5, note(m), "saw", 0.3), 0.3) for m in (60, 67, 72)])
    add("weapon_found", lowpass(shaped(chord, env(int(1.0 * RATE), 0.01, 0.35)), 3500), 0.6)

    # --- interface ----------------------------------------------------------
    add("ui_move", tone(1600, 0.035, 0.001, 0.01, "triangle"), 0.25)
    add("ui_confirm", mix(tone(990, 0.08, 0.001, 0.03, "square"), offset(tone(1480, 0.1, 0.001, 0.04, "square"), 0.06)), 0.3)
    add("ui_deny", shaped(sweep(0.22, 180, 150, "square"), env(int(0.22 * RATE), 0.002, 0.08)), 0.35)
    beeps = [offset(tone(f, 0.07, 0.001, 0.03, "square"), i * 0.075) for i, f in enumerate((1200, 900, 1500, 1100))]
    add("console", lowpass(mix(*beeps), 4000), 0.3)
    n = int(0.9 * RATE)
    clank = mix(shaped(lowpass(noise(0.9, rng), 2500), env(n, 0.001, 0.05)), tone(320, 0.9, 0.001, 0.15, "square"), offset(tone(240, 0.6, 0.001, 0.1, "square"), 0.2))
    add("construct", mix(lowpass(clank, 2200), offset(mix(tone(note(79), 0.5, 0.003, 0.2), offset(tone(note(84), 0.5, 0.003, 0.2), 0.08)), 0.35)), 0.6)
    tut = mix(tone(note(81), 0.18, 0.002, 0.08, "triangle"), offset(tone(note(88), 0.25, 0.002, 0.1, "triangle"), 0.1))
    add("tutorial", tut, 0.35)

    # --- travel -------------------------------------------------------------
    n = int(3.2 * RATE)
    rise = lowpass(noise(3.2, rng), [200 + 7000 * (i / n) ** 2 for i in range(n)])
    whine = sweep(3.2, 80, 1200, "saw", 2.0)
    swell = [min(1.0, (i / n) * 1.3) for i in range(n)]
    boom = offset(shaped(mix(sweep(1.0, 120, 30), lowpass(noise(1.0, rng), 800)), env(int(1.0 * RATE), 0.002, 0.25)), 3.0)
    add("ftl", mix(shaped(mix(rise, gain(whine, 0.3)), swell), gain(boom, 1.4)), 0.85)
    n = int(3.0 * RATE)
    roar = lowpass(noise(3.0, rng), [300 + 1500 * (i / n) for i in range(n)])
    add("pod_descent", shaped(mix(roar, gain(sweep(3.0, 60, 90, "saw"), 0.4)), [min(1.0, i / (0.4 * RATE)) for i in range(n)]), 0.7)
    n = int(1.4 * RATE)
    thud = mix(shaped(sweep(1.4, 90, 25), env(n, 0.001, 0.3)), shaped(lowpass(noise(1.4, rng), 1400), env(n, 0.001, 0.25)))
    add("pod_impact", drive(thud, 2.5), 0.95)
    n = int(1.6 * RATE)
    siren = [math.sin(TAU * (700 + 250 * math.sin(TAU * 2.5 * i / RATE)) * i / RATE) for i in range(n)]
    add("alarm", shaped(siren, [1.0 if i < n - 2000 else (n - i) / 2000 for i in range(n)]), 0.4)
    n = int(3.0 * RATE)
    lift = lowpass(noise(3.0, rng), [3000 - 2400 * (i / n) for i in range(n)])
    add("liftoff", shaped(mix(lift, gain(sweep(3.0, 70, 160, "saw"), 0.35)), [min(1.0, i / (0.15 * RATE)) * (1 - 0.6 * i / n) for i in range(n)]), 0.8)
    add("door", mix(shaped(sweep(0.5, 300, 180, "saw"), env(int(0.5 * RATE), 0.02, 0.2)), shaped(lowpass(noise(0.5, rng), 900), env(int(0.5 * RATE), 0.01, 0.1))), 0.4)

    # --- loops (whole cycles or crossfaded, played with LoopMode::Whole) -----
    seconds = 4.0
    n = int(seconds * RATE)
    hum = [0.0] * n
    for f, a in ((55, 0.5), (110, 0.25), (165, 0.12), (220.0, 0.06)):
        for i in range(n):
            hum[i] += a * math.sin(TAU * f * i / RATE)
    wobble = [1 + 0.15 * math.sin(TAU * 0.5 * i / RATE) for i in range(n)]
    add("ship_hum", [h * w for h, w in zip(hum, wobble)], 0.3, False)
    wind = lowpass(noise(7.0, rng), [500 + 350 * math.sin(TAU * i / (3.5 * RATE)) for i in range(int(7.0 * RATE))])
    add("planet_wind", seamless(wind, 1.0), 0.3, False)

    def music(bpm: float, bars: int, bass: list[int], pad: list[list[int]], hats: bool) -> list[float]:
        beat = 60 / bpm
        total = int(bars * 4 * beat * RATE)
        out = [0.0] * total
        for bar in range(bars):
            chord = pad[bar % len(pad)]
            start = int(bar * 4 * beat * RATE)
            length = int(4 * beat * RATE)
            for m in chord:
                f = note(m)
                for i in range(length):
                    t = i / RATE
                    e = min(1.0, i / (0.3 * RATE)) * min(1.0, (length - i) / (0.3 * RATE))
                    out[start + i] += 0.08 * e * (math.sin(TAU * f * t) + 0.3 * math.sin(TAU * f * 2.003 * t))
            for step in range(8):
                m = bass[(bar * 8 + step) % len(bass)]
                if m < 0:
                    continue
                f = note(m)
                s0 = start + int(step * beat / 2 * RATE)
                ln = int(beat / 2 * RATE)
                for i in range(ln):
                    if s0 + i >= total:
                        break
                    t = i / RATE
                    p = (f * t) % 1.0
                    out[s0 + i] += 0.22 * math.exp(-t / 0.18) * (2 * p - 1)
                if hats:
                    for i in range(int(0.03 * RATE)):
                        if s0 + i < total:
                            out[s0 + i] += 0.05 * math.exp(-i / (0.008 * RATE)) * rng.uniform(-1, 1)
        return lowpass(out, 6000)

    add("music_surface", music(124, 8, [40, -1, 40, 43, 40, -1, 47, 45, 38, -1, 38, 41, 38, -1, 45, 43], [[52, 55, 59], [50, 53, 57], [48, 52, 55], [50, 54, 57]], True), 0.45, False)
    add("music_ship", music(76, 4, [-1] * 16, [[57, 60, 64, 67], [53, 57, 60, 64], [55, 59, 62, 66], [52, 55, 59, 62]], False), 0.4, False)
    return s


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=REPO / "assets" / "sounds")
    args = parser.parse_args()
    rng = random.Random(0x504C4E54)
    for name, (track, peak, fade) in build(rng).items():
        path = args.out / f"{name}.wav"
        write(path, track, peak, fade)
        print(f"wrote {path.relative_to(REPO) if path.is_relative_to(REPO) else path} ({len(track) / RATE:.2f}s)")


if __name__ == "__main__":
    main()
