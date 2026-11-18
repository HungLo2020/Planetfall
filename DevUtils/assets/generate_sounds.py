#!/usr/bin/env python3
"""Synthesize Planetfall's sound effects, ambience and music as WAV files.

Everything is original and generated here: no samples, no downloads. Each
sound is designed in layers the way a sound designer builds them in a DAW:
a transient (the click or crack that reads first), a body (the tone or noise
that carries the character) and a tail (decay, debris, room). The toolkit
below provides band-limited oscillators, FM, coloured noise, resonant and
swept filters, saturation, delays and convolution reverb with generated
impulse responses (rooms, caves, plates).

Frequent sounds get several variants (`name.v2.wav`, ...); the game picks one
at random each time so repeats do not machine-gun. Music is stereo and
rendered so it loops seamlessly (three passes through the effects, keeping
the middle one, so reverb and delay tails wrap around the loop point).

Every random choice is seeded from the sound's name, so the output is
reproducible. Effects are 16-bit WAV, trimmed where the tail falls below
hearing and loudness-matched so the game's volumes stay meaningful. Music and
ambience loops are long, so they are encoded as Ogg Vorbis with ffmpeg
(libvorbis), which the game decodes natively.

The files in `assets/sounds` are the source of truth and may be replaced by
hand-made recordings or edits: this script only makes sounds that have no
file yet, unless it is asked for specific names (which it remakes, removing
their old files and variants) or `--force` (which remakes everything).

Usage:
    python3 DevUtils/assets/generate_sounds.py                 # add missing sounds
    python3 DevUtils/assets/generate_sounds.py blaster alarm   # remake these
    python3 DevUtils/assets/generate_sounds.py --force         # remake everything
"""

from __future__ import annotations

import argparse
import hashlib
import math
import shutil
import subprocess
import tempfile
import wave
from pathlib import Path
from typing import Callable

import numpy as np
from scipy import signal

REPO = Path(__file__).resolve().parents[2]
RATE = 44100
MUSIC_RATE = 32000
TAU = 2 * math.pi


# --- Core -------------------------------------------------------------------


class Ctx:
    """Per-sound state: sample rate and a seeded random generator."""

    def __init__(self, name: str, rate: int = RATE) -> None:
        self.rate = rate
        seed = int.from_bytes(hashlib.sha256(name.encode()).digest()[:8], "little")
        self.rng = np.random.default_rng(seed)

    def n(self, seconds: float) -> int:
        return max(1, int(round(seconds * self.rate)))

    def t(self, seconds: float) -> np.ndarray:
        return np.arange(self.n(seconds)) / self.rate

    def ramp(self, seconds: float, a: float, b: float, curve: float = 1.0) -> np.ndarray:
        x = np.linspace(0.0, 1.0, self.n(seconds))
        return a + (b - a) * x**curve

    def expo(self, seconds: float, a: float, b: float) -> np.ndarray:
        """Exponential glide (pitch sweeps sound natural this way)."""
        return a * (b / a) ** np.linspace(0.0, 1.0, self.n(seconds))

    # Oscillators take a frequency (scalar or per-sample array).
    def phase(self, freq, seconds: float) -> np.ndarray:
        f = np.broadcast_to(np.asarray(freq, dtype=float), (self.n(seconds),))
        return np.cumsum(f) / self.rate * TAU

    def sine(self, freq, seconds: float, phase0: float = 0.0) -> np.ndarray:
        return np.sin(self.phase(freq, seconds) + phase0)

    def tri(self, freq, seconds: float) -> np.ndarray:
        p = (self.phase(freq, seconds) / TAU) % 1.0
        return 4.0 * np.abs(p - 0.5) - 1.0

    def saw(self, freq, seconds: float) -> np.ndarray:
        """PolyBLEP saw (band-limited enough for sweeps and basses)."""
        f = np.broadcast_to(np.asarray(freq, dtype=float), (self.n(seconds),))
        dt = np.clip(f / self.rate, 1e-6, 0.5)
        p = (np.cumsum(dt)) % 1.0
        y = 2.0 * p - 1.0
        a = p < dt
        x = p[a] / dt[a]
        y[a] -= x + x - x * x - 1.0
        b = p > 1.0 - dt
        x = (p[b] - 1.0) / dt[b]
        y[b] -= x * x + x + x + 1.0
        return y

    def square(self, freq, seconds: float, width: float = 0.5) -> np.ndarray:
        f = np.broadcast_to(np.asarray(freq, dtype=float), (self.n(seconds),))
        a = self.saw(f, seconds)
        # Shifted copy: two saws make a band-limited pulse.
        dt = f / self.rate
        p = (np.cumsum(dt) + width) % 1.0
        b = 2.0 * p - 1.0
        return (a - b) * 0.5

    def fm(self, freq, seconds: float, ratio: float, index) -> np.ndarray:
        mod = self.sine(np.asarray(freq) * ratio, seconds) * index
        return np.sin(self.phase(freq, seconds) + mod)

    def noise(self, seconds: float, color: str = "white") -> np.ndarray:
        x = self.rng.standard_normal(self.n(seconds))
        if color == "white":
            return x / 3.0
        if color == "pink":
            b = [0.049922035, -0.095993537, 0.050612699, -0.004408786]
            a = [1, -2.494956002, 2.017265875, -0.522189400]
            return signal.lfilter(b, a, x) * 3.0
        if color == "brown":
            y = signal.lfilter([1.0], [1.0, -0.995], x)
            return hp(y, 20, self.rate) / max(1e-9, np.max(np.abs(y))) * 1.2
        raise ValueError(color)

    def crackle(self, seconds: float, density: float, decay: float = 0.002) -> np.ndarray:
        """Sparse random clicks (fire, electricity, debris)."""
        n = self.n(seconds)
        x = np.zeros(n)
        hits = self.rng.random(n) < density / self.rate
        x[hits] = self.rng.uniform(-1, 1, hits.sum())
        k = np.exp(-np.arange(self.n(decay * 6)) / (decay * self.rate))
        return np.convolve(x, k)[:n]


def env(c: Ctx, seconds: float, attack: float, decay: float, curve: float = 1.0) -> np.ndarray:
    """Attack ramp then exponential decay (decay = time to -60 dB)."""
    n = c.n(seconds)
    a = max(1, c.n(attack))
    out = np.empty(n)
    out[: min(a, n)] = np.linspace(0, 1, a)[: min(a, n)] ** curve
    if n > a:
        k = np.arange(n - a) / c.rate
        out[a:] = np.exp(-k * 6.9 / max(decay, 1e-4))
    return out


def adsr(c: Ctx, seconds: float, a: float, d: float, s: float, r: float) -> np.ndarray:
    n = c.n(seconds)
    out = np.full(n, s)
    na, nd, nr = c.n(a), c.n(d), c.n(r)
    out[: min(na, n)] = np.linspace(0, 1, na)[: min(na, n)]
    if na < n:
        seg = np.linspace(1, s, nd)[: max(0, n - na)]
        out[na : na + len(seg)] = seg
    if nr < n:
        out[n - nr :] *= np.linspace(1, 0, nr)
    return out


def biquad(kind: str, fc: float, q: float, rate: int):
    """RBJ cookbook biquad coefficients."""
    fc = min(max(fc, 10.0), rate * 0.45)
    w = TAU * fc / rate
    alpha = math.sin(w) / (2 * q)
    cw = math.cos(w)
    if kind == "low":
        b = [(1 - cw) / 2, 1 - cw, (1 - cw) / 2]
    elif kind == "high":
        b = [(1 + cw) / 2, -(1 + cw), (1 + cw) / 2]
    elif kind == "band":
        b = [alpha, 0, -alpha]
    else:
        raise ValueError(kind)
    a = [1 + alpha, -2 * cw, 1 - alpha]
    return np.array(b) / a[0], np.array(a) / a[0]


def lp(x, fc, rate=RATE, q=0.707):
    b, a = biquad("low", fc, q, rate)
    return signal.lfilter(b, a, x)


def hp(x, fc, rate=RATE, q=0.707):
    b, a = biquad("high", fc, q, rate)
    return signal.lfilter(b, a, x)


def bp(x, fc, q=2.0, rate=RATE):
    b, a = biquad("band", fc, q, rate)
    return signal.lfilter(b, a, x)


def sweep(x, kind: str, f0, f1=None, q: float = 0.9, rate: int = RATE, block: int = 64):
    """Time-varying filter: cutoff glides from f0 to f1 (or follows an array)."""
    n = len(x)
    if f1 is not None:
        f = f0 * (f1 / f0) ** np.linspace(0, 1, n)
    else:
        f = np.broadcast_to(np.asarray(f0, dtype=float), (n,))
    out = np.empty(n)
    zi = np.zeros(2)
    for i in range(0, n, block):
        b, a = biquad(kind, float(f[i]), q, rate)
        out[i : i + block], zi = signal.lfilter(b, a, x[i : i + block], zi=zi)
    return out


def drive(x, k: float):
    return np.tanh(x * k) / math.tanh(k)


def crush(x, bits: int):
    q = 2 ** (bits - 1)
    return np.round(x * q) / q


def pad(x, n: int):
    return x if len(x) >= n else np.concatenate([x, np.zeros(n - len(x))])


def mix(*tracks):
    n = max(len(t) for t in tracks)
    return sum(pad(np.asarray(t, dtype=float), n) for t in tracks)


def at(c: Ctx, seconds: float, x):
    return np.concatenate([np.zeros(c.n(seconds)), x])


def fade(x, rate=RATE, a=0.002, b=0.02):
    x = np.array(x, dtype=float)
    na, nb = int(a * rate), int(b * rate)
    if na:
        x[:na] *= np.linspace(0, 1, na)
    if nb and nb < len(x):
        x[-nb:] *= np.linspace(1, 0, nb)
    return x


def impulse(c: Ctx, seconds: float, damp: float, early: int = 6, bright: float = 9000) -> np.ndarray:
    """A room: early reflections then a darkening noise tail."""
    n = c.n(seconds)
    t = np.arange(n) / c.rate
    tail = c.rng.standard_normal(n) * np.exp(-t * 6.9 / seconds)
    # Higher frequencies die faster.
    tail = mix(lp(tail, bright, c.rate) * 0.6, lp(tail, damp, c.rate) * 0.8)
    for _ in range(early):
        k = int(c.rng.uniform(0.004, 0.035) * c.rate)
        if k < n:
            tail[k] += c.rng.uniform(0.4, 0.9) * c.rng.choice([-1, 1])
    return tail / np.sqrt(np.sum(tail**2) + 1e-9)


def reverb(c: Ctx, x, seconds=1.2, wet=0.25, damp=3500, bright=9000):
    ir = impulse(c, seconds, damp, bright=bright)
    y = signal.fftconvolve(x, ir)
    return mix(x * (1 - wet * 0.5), y * wet)


def echo(c: Ctx, x, time: float, feedback: float, wet: float, taps: int = 5, tone: float = 4000):
    out = np.array(x, dtype=float)
    d = c.n(time)
    tap = np.array(x, dtype=float)
    for k in range(1, taps + 1):
        tap = lp(tap, tone, c.rate) * feedback
        out = mix(out, np.concatenate([np.zeros(d * k), tap * wet]))
    return out


def normalize(x, peak_db: float = -1.0):
    m = np.max(np.abs(x))
    return x if m < 1e-9 else x / m * 10 ** (peak_db / 20)


# --- Output -------------------------------------------------------------------


def loudness(x, rms_db: float = -18.0, peak_db: float = -1.0):
    """Match the loudness of the audible part, never past the peak ceiling."""
    m = np.max(np.abs(x))
    if m < 1e-9:
        return x
    active = np.abs(x) > m * 0.05
    rms = math.sqrt(float(np.mean(x[active] ** 2)))
    gain = min(10 ** (rms_db / 20) / max(rms, 1e-9), 10 ** (peak_db / 20) / m)
    return x * gain


def trim(x, rate=RATE, floor_db: float = -54.0):
    """Cut the tail once it stays below `floor_db` of the peak, with a fade."""
    m = np.max(np.abs(x))
    loud = np.nonzero(np.abs(x) > m * 10 ** (floor_db / 20))[0]
    if len(loud) == 0:
        return x
    end = min(len(x), loud[-1] + int(0.02 * rate))
    return fade(x[:end], rate, a=0.0, b=min(0.05, end / rate * 0.2))


class Writer:
    def __init__(self, out: Path, only: set[str], force: bool) -> None:
        self.out = out
        self.only = only
        self.force = force
        self.written: list[str] = []
        self.kept = 0

    def files(self, name: str) -> list[Path]:
        """Every file of a sound: `name.wav`/`.ogg` and its `name.vN` variants."""
        return [
            f
            for f in [*self.out.glob(f"{name}.*"), *self.out.glob(f"{name}.v*.*")]
            if f.suffix in (".wav", ".ogg") and (f.stem == name or f.stem.startswith(f"{name}.v"))
        ]

    def wanted(self, name: str) -> bool:
        """Named sounds are always remade; otherwise only missing ones are,
        unless `--force`. Sounds being remade lose their old files first."""
        if self.only:
            write = name in self.only
        else:
            write = self.force or not self.files(name)
        if write:
            for old in set(self.files(name)):
                old.unlink()
        elif not self.only:
            self.kept += 1
        return write

    def wav(self, path: Path, data, rate: int):
        stereo = data.ndim == 2
        pcm = np.clip(np.round(data * 32767), -32768, 32767).astype("<i2")
        with wave.open(str(path), "wb") as w:
            w.setnchannels(2 if stereo else 1)
            w.setsampwidth(2)
            w.setframerate(rate)
            w.writeframes((pcm.T.reshape(-1) if stereo else pcm).tobytes())

    def save(self, file_stem: str, x, rate=RATE, peak_db=-1.0):
        data = normalize(np.asarray(x, dtype=float), peak_db)
        self.wav(self.out / f"{file_stem}.wav", data, rate)
        self.written.append(file_stem)

    def save_ogg(self, file_stem: str, x, rate: int, peak_db=-1.0, quality: int = 5):
        """Long loops as Ogg Vorbis (needs ffmpeg with libvorbis)."""
        if shutil.which("ffmpeg") is None:
            raise SystemExit("ffmpeg (with libvorbis) is needed to encode music and ambience")
        data = normalize(np.asarray(x, dtype=float), peak_db)
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "loop.wav"
            self.wav(src, data, rate)
            subprocess.run(
                ["ffmpeg", "-loglevel", "error", "-y", "-i", str(src), "-c:a", "libvorbis",
                 "-q:a", str(quality), "-fflags", "+bitexact", "-flags:a", "+bitexact",
                 str(self.out / f"{file_stem}.ogg")],
                check=True,
            )
        self.written.append(file_stem)

    def sound(self, name: str, make: Callable[[Ctx, int], np.ndarray], variants: int = 1, rms_db=-18.0):
        """One effect with variants: `name.wav`, `name.v2.wav`, ..."""
        if not self.wanted(name):
            return
        for v in range(variants):
            c = Ctx(f"{name}#{v}")
            # High-pass removes DC and sub-rumble no speaker plays.
            x = loudness(trim(hp(fade(make(c, v)), 28)), rms_db)
            stem = name if v == 0 else f"{name}.v{v + 1}"
            # Short transients stay WAV; long effects are Vorbis to keep the
            # download small.
            if len(x) > 0.8 * RATE:
                self.save_ogg(stem, x, RATE, peak_db=20 * math.log10(max(np.max(np.abs(x)), 1e-6)), quality=6)
            else:
                self.save(stem, x, peak_db=20 * math.log10(max(np.max(np.abs(x)), 1e-6)))


# --- Building blocks ------------------------------------------------------------


def thump(c: Ctx, f0=140.0, f1=45.0, seconds=0.25, decay=0.2):
    return c.sine(c.expo(seconds, f0, f1), seconds) * env(c, seconds, 0.001, decay)


def click(c: Ctx, seconds=0.012, tone=5000.0):
    return hp(c.noise(seconds), tone) * env(c, seconds, 0.0003, seconds)


def whoosh(c: Ctx, seconds, f0, f1, q=1.2, color="pink"):
    x = sweep(c.noise(seconds, color), "band", f0, f1, q)
    return x * np.sin(np.linspace(0, math.pi, len(x))) ** 1.5


def chime(c: Ctx, freq, seconds=0.8, ratio=3.5, index=2.0, decay=0.6):
    idx = index * env(c, seconds, 0.001, decay * 0.5)
    return c.fm(freq, seconds, ratio, idx) * env(c, seconds, 0.002, decay)


def notes_to_hz(midi):
    return 440.0 * 2 ** ((np.asarray(midi) - 69) / 12)


def arpeggio(c: Ctx, midis, step, length, voice):
    out = np.zeros(c.n(step * len(midis) + length))
    for i, m in enumerate(midis):
        x = voice(c, float(notes_to_hz(m)), length)
        s = c.n(step * i)
        out[s : s + len(x)] += x
    return out


def pluck(c: Ctx, freq, seconds):
    x = mix(c.saw(freq, seconds) * 0.6, c.square(freq * 2.0, seconds) * 0.2)
    return sweep(x, "low", 6000, 600, 1.4) * env(c, seconds, 0.001, seconds * 0.9)


def bell(c: Ctx, freq, seconds):
    return chime(c, freq, seconds, ratio=2.76, index=1.6, decay=seconds)


# --- Weapons --------------------------------------------------------------------------


def blaster(c, v):
    d = 0.22
    f = c.expo(d, 1900 * (1 + 0.06 * v), 380)
    body = c.fm(f, d, 1.5, 1.2 * env(c, d, 0.001, 0.1)) * env(c, d, 0.001, 0.16)
    sub = thump(c, 220, 80, 0.1, 0.07) * 0.6
    return reverb(c, mix(body * 0.8, click(c, 0.01) * 0.5, sub), 0.5, 0.18)


def cutter(c, v):
    d = 0.18
    buzz = c.saw(c.ramp(d, 240 + 15 * v, 200), d) * 0.5 + c.square(480, d, 0.3) * 0.25
    sizzle = hp(c.noise(d), 3500) * (0.6 + 0.4 * c.square(60, d)) * 0.4
    beam = sweep(mix(buzz, sizzle), "low", 7000, 1800, 2.5) * env(c, d, 0.003, 0.15)
    return reverb(c, mix(beam, click(c, 0.008, 7000) * 0.3), 0.4, 0.15)


def scatter(c, v):
    d = 0.7
    blast = lp(c.noise(0.25), 3800 - 300 * v) * env(c, 0.25, 0.0005, 0.18)
    boom = thump(c, 120, 42, 0.35, 0.28)
    crack = hp(c.noise(0.03), 2500) * env(c, 0.03, 0.0002, 0.03)
    # Pump action: two mechanical clacks.
    clack = mix(at(c, 0.36, click(c, 0.02, 1800) * 0.35), at(c, 0.47, click(c, 0.025, 1200) * 0.4))
    return reverb(c, mix(blast * 0.9, boom * 0.9, crack * 0.8, clack), 0.9, 0.22)[: c.n(d)]


def arc(c, v):
    d = 0.45
    gate = (c.rng.random(c.n(d)) < 0.07).astype(float)
    gate = lp(gate, 600) * 6
    zap = hp(c.noise(d), 1800) * np.clip(gate, 0, 1)
    chirp = c.sine(c.expo(d, 3200, 900 + 100 * v), d) * 0.25
    hum = c.square(120, d, 0.2) * 0.25
    x = mix(zap, chirp, hum, c.crackle(d, 900) * 0.8) * env(c, d, 0.002, 0.38)
    return reverb(c, drive(x, 1.8), 0.6, 0.2)


def mortar(c, v):
    d = 1.0
    launch = thump(c, 160, 48, 0.4, 0.3)
    tube = bp(c.noise(0.3), 420 + 40 * v, 4) * env(c, 0.3, 0.001, 0.25) * 2.5
    puff = whoosh(c, 0.35, 600, 2500) * 0.5
    return reverb(c, mix(launch, tube, puff, click(c, 0.01, 2500) * 0.4), 1.0, 0.25)[: c.n(d)]


def drone(c, v):
    d = 0.16
    f = c.ramp(d, 1300 + 120 * v, 1700) * (1 + 0.03 * c.sine(70, d))
    x = c.square(f, d, 0.35) * env(c, d, 0.002, 0.12)
    return reverb(c, lp(x, 5000) * 0.6, 0.3, 0.15)


def flamer(c, v):
    d = 0.45
    roar = sweep(c.noise(d, "pink"), "band", 700 + 80 * v, 1500, 0.7) * 1.2
    low = lp(c.noise(d, "brown"), 300) * 0.8
    crack = c.crackle(d, 160, 0.003) * 1.2
    x = mix(roar, low, crack) * adsr(c, d, 0.03, 0.1, 0.7, 0.2)
    return reverb(c, drive(x, 1.5), 0.6, 0.18)


def ricochet(c, v):
    d = 0.5
    ring = chime(c, 1800 + 160 * v, d, ratio=1.41, index=3.0, decay=0.35) * 0.5
    zing = c.sine(c.expo(0.25, 2600, 4200), 0.25) * env(c, 0.25, 0.002, 0.2) * 0.3
    swish = whoosh(c, 0.2, 1500, 6000) * 0.5
    return reverb(c, mix(swish, ring, zing, click(c, 0.008, 6000) * 0.4), 0.7, 0.22)


def mine(c, v):
    clank = mix(bp(c.noise(0.08), 900, 5) * 3, bp(c.noise(0.08), 2300, 6) * 2) * env(c, 0.08, 0.0005, 0.07)
    beep = at(c, 0.12, c.square(1800, 0.06, 0.5) * env(c, 0.06, 0.001, 0.06) * 0.3)
    beep2 = at(c, 0.22, c.square(1800, 0.06, 0.5) * env(c, 0.06, 0.001, 0.06) * 0.3)
    return reverb(c, mix(clank, thump(c, 200, 90, 0.1, 0.08) * 0.6, beep, beep2), 0.5, 0.18)


def swing(c, v):
    d = 0.42
    air = whoosh(c, d, 250 + 40 * v, 1400, 0.9) * 1.4
    servo = c.saw(c.expo(d, 90, 160), d) * env(c, d, 0.01, 0.3) * 0.15
    impact = at(c, 0.16, thump(c, 110, 50, 0.2, 0.15) * 0.8)
    return reverb(c, mix(air, lp(servo, 900), impact), 0.6, 0.2)


def seeker(c, v):
    d = 0.9
    ignite = mix(c.crackle(0.15, 900, 0.002), hp(c.noise(0.15), 2000) * 0.5) * env(c, 0.15, 0.001, 0.12)
    rise = c.sine(c.expo(0.6, 300 + 30 * v, 1400), 0.6) * env(c, 0.6, 0.02, 0.5) * 0.3
    jet = sweep(c.noise(d, "pink"), "band", 900, 3500, 0.8) * env(c, d, 0.02, 0.7)
    return reverb(c, mix(ignite, at(c, 0.02, rise), jet * 0.8, thump(c, 150, 60, 0.15, 0.1) * 0.5), 0.9, 0.22)


def gravity(c, v):
    d = 1.4
    swell = c.sine(c.expo(d, 220, 45), d) * c.ramp(d, 0.1, 1.0, 2.0)
    warble = c.fm(c.ramp(d, 70, 40), d, 0.5, 3.0) * 0.5
    suck = whoosh(c, d, 4000, 300, 1.5)[::-1] * 0.4
    x = mix(swell, warble, suck) * adsr(c, d, 0.05, 0.4, 0.8, 0.4)
    return reverb(c, drive(x, 1.4), 1.6, 0.35, damp=1800)


# --- Impacts and enemies ---------------------------------------------------------------


def enemy_hit(c, v):
    d = 0.14
    smack = lp(c.noise(d), 1800 + 400 * v) * env(c, d, 0.0005, 0.07)
    body = thump(c, 260 - 25 * v, 110, d, 0.08) * 0.9
    squelch = bp(c.noise(d), 700, 3) * env(c, d, 0.005, 0.1) * 1.4
    return mix(smack, body, squelch)


def explosion(c, v):
    d = 2.2
    crack = hp(c.noise(0.05), 1200) * env(c, 0.05, 0.0002, 0.05) * 1.2
    boom = thump(c, 90 + 8 * v, 26, 1.2, 1.1) * 1.4
    body = sweep(c.noise(1.4, "brown"), "low", 3500, 160, 0.9) * env(c, 1.4, 0.002, 1.2) * 1.5
    debris = mix(c.crackle(d, 60, 0.004) * 0.6, lp(c.crackle(d, 25, 0.01), 1500)) * env(c, d, 0.05, 1.6)
    x = drive(mix(crack, boom, body, debris), 2.0)
    return reverb(c, x, 2.4, 0.3, damp=1500)[: c.n(d)]


def husk_death(c, v):
    d = 0.8
    groan = sweep(c.saw(c.expo(d, 160 - 10 * v, 70), d), "band", 900, 350, 2.5) * env(c, d, 0.02, 0.6) * 1.2
    wet = bp(c.noise(0.3), 500, 2) * env(c, 0.3, 0.002, 0.25) * 1.5
    thud = at(c, 0.35, thump(c, 120, 50, 0.2, 0.15) * 0.6)
    return reverb(c, mix(groan, wet, thud), 0.6, 0.15)


def skitter_death(c, v):
    d = 0.35
    chitter = c.crackle(d, 500, 0.0015) * env(c, d, 0.001, 0.25) * 1.5
    squeal = c.sine(c.expo(0.2, 2400 + 200 * v, 900), 0.2) * env(c, 0.2, 0.001, 0.15) * 0.4
    crunch = hp(c.noise(0.1), 1500) * env(c, 0.1, 0.0005, 0.08)
    return reverb(c, mix(chitter, squeal, crunch), 0.4, 0.15)


def bloater_death(c, v):
    d = 0.9
    pop = mix(thump(c, 300, 60, 0.3, 0.2), lp(c.noise(0.2), 900) * env(c, 0.2, 0.001, 0.15))
    spray = sweep(c.noise(0.7, "pink"), "band", 2000, 500, 1) * env(c, 0.7, 0.01, 0.6) * 0.8
    gurgle = bp(c.noise(0.6), 300, 6) * (0.5 + 0.5 * c.sine(18, 0.6)) * env(c, 0.6, 0.02, 0.5) * 2
    return reverb(c, mix(pop * 1.2, spray, gurgle), 0.8, 0.2)


def brute_death(c, v):
    d = 1.6
    roar = sweep(drive(c.saw(c.expo(d, 110, 45), d), 2.5), "low", 1400, 300, 1.5) * env(c, d, 0.05, 1.2)
    fall = at(c, 0.7, mix(thump(c, 80, 30, 0.6, 0.5) * 1.3, lp(c.noise(0.4), 600) * env(c, 0.4, 0.001, 0.3)))
    return reverb(c, mix(roar, fall), 1.6, 0.25, damp=2000)


def spitter_death(c, v):
    d = 0.6
    gurgle = sweep(c.noise(d, "pink"), "band", 1200, 300, 4) * env(c, d, 0.005, 0.45) * 1.5
    pop = thump(c, 400, 120, 0.12, 0.08)
    return reverb(c, mix(gurgle, pop), 0.5, 0.15)


def flyer_death(c, v):
    d = 0.4
    buzz = c.saw(c.expo(d, 240 + 30 * v, 80), d) * env(c, d, 0.001, 0.3) * 0.5
    snap = hp(c.noise(0.05), 3000) * env(c, 0.05, 0.0003, 0.04)
    return reverb(c, mix(lp(buzz, 3000), snap), 0.4, 0.12)


def disruptor_death(c, v):
    d = 0.9
    fizz = hp(c.noise(d), 2500) * (c.rng.random(c.n(d)) < 0.3) * env(c, d, 0.001, 0.7)
    down = c.square(c.expo(d, 800, 60), d, 0.3) * env(c, d, 0.001, 0.8) * 0.4
    pop = thump(c, 200, 60, 0.2, 0.15)
    return reverb(c, mix(fizz, lp(down, 2500), pop), 0.8, 0.2)


def burrower_death(c, v):
    d = 1.0
    crumble = lp(c.crackle(d, 120, 0.006), 1800) * env(c, d, 0.01, 0.8) * 1.2
    groan = lp(c.saw(c.expo(d, 90, 40), d), 500) * env(c, d, 0.02, 0.7)
    return reverb(c, mix(crumble, groan, thump(c, 90, 35, 0.4, 0.3)), 1.0, 0.2)


def boss_death(c, v):
    d = 3.5
    roar = sweep(drive(mix(c.saw(c.expo(d, 90, 30), d), c.saw(c.expo(d, 92, 31), d)), 3), "low", 2000, 200, 1.2)
    roar *= env(c, d, 0.05, 2.5)
    crash = at(c, 1.2, explosion(c, 0))
    return reverb(c, mix(roar, crash * 0.8), 2.5, 0.3, damp=1600)[: c.n(d)]


def husk_groan(c, v):
    d = 1.1
    f = c.expo(d, 120 + 12 * v, 95) * (1 + 0.04 * c.sine(5 + v, d))
    voice = drive(c.saw(f, d), 1.5)
    # Two formants make it sound throaty.
    x = mix(bp(voice, 600 + 50 * v, 5) * 2, bp(voice, 1100, 6) * 1.2) * adsr(c, d, 0.15, 0.3, 0.6, 0.4)
    return reverb(c, x, 1.0, 0.25, damp=2500)


def brute_roar(c, v):
    d = 1.4
    f = c.expo(d, 85 + 10 * v, 60) * (1 + 0.06 * c.sine(9, d))
    voice = drive(mix(c.saw(f, d), c.saw(f * 1.01, d), lp(c.noise(d), 900) * 0.6), 3.0)
    x = mix(bp(voice, 420, 3) * 2.2, bp(voice, 900, 4) * 1.2, lp(voice, 250)) * adsr(c, d, 0.08, 0.3, 0.7, 0.5)
    return reverb(c, x, 1.4, 0.3, damp=2000)


def boss_roar(c, v):
    d = 2.4
    f = c.expo(d, 70 - 6 * v, 42) * (1 + 0.08 * c.sine(6, d))
    voice = drive(mix(c.saw(f, d), c.saw(f * 1.5, d) * 0.5, c.saw(f * 0.5, d), lp(c.noise(d), 1200)), 3.5)
    x = mix(bp(voice, 350, 2.5) * 2.4, bp(voice, 800, 3) * 1.4, lp(voice, 180) * 1.3) * adsr(c, d, 0.15, 0.6, 0.75, 0.8)
    return reverb(c, x, 2.4, 0.35, damp=1600)


def boss_slam(c, v):
    d = 1.8
    slam = mix(thump(c, 70, 25, 1.2, 1.0) * 1.5, lp(c.noise(0.4, "brown"), 800) * env(c, 0.4, 0.001, 0.35) * 1.5)
    rumble = lp(c.crackle(d, 80, 0.008), 900) * env(c, d, 0.02, 1.4)
    return reverb(c, drive(mix(slam, rumble), 2), 2.0, 0.3, damp=1500)


def skitter_alert(c, v):
    d = 0.5
    chitter = (c.rng.random(c.n(d)) < 0.02).astype(float)
    chitter = bp(np.convolve(chitter, np.hanning(60), "same"), 3200 + 300 * v, 4) * 6
    return reverb(c, chitter * env(c, d, 0.01, 0.45), 0.4, 0.15)


def spit(c, v):
    d = 0.35
    hock = bp(c.noise(0.15), 900 + 100 * v, 3) * env(c, 0.15, 0.002, 0.12) * 2
    launch = c.sine(c.expo(0.25, 300, 900), 0.25) * env(c, 0.25, 0.005, 0.2) * 0.4
    splat = at(c, 0.05, lp(c.noise(0.2), 1500) * env(c, 0.2, 0.001, 0.15))
    return reverb(c, mix(hock, launch, splat * 0.6), 0.4, 0.15)


def jam(c, v):
    d = 0.7
    static = hp(c.noise(d), 900) * (0.5 + 0.5 * c.square(23 + 4 * v, d))
    tone = c.square(c.ramp(d, 600, 300), d, 0.2) * 0.3
    return reverb(c, crush(mix(static, tone) * env(c, d, 0.005, 0.6), 4), 0.5, 0.15)


def burrow(c, v):
    d = 1.0
    dig = lp(c.crackle(d, 200, 0.005), 1200) * 1.5
    rumble = lp(c.noise(d, "brown"), 200) * 1.5
    return reverb(c, mix(dig, rumble) * adsr(c, d, 0.1, 0.2, 0.8, 0.4), 0.8, 0.2)


def split(c, v):
    d = 0.4
    tear = sweep(c.noise(d), "band", 400, 1800, 3) * env(c, d, 0.005, 0.3) * 1.5
    squish = bp(c.noise(d), 350, 3) * env(c, d, 0.01, 0.3)
    return reverb(c, mix(tear, squish), 0.4, 0.15)


# --- Player -------------------------------------------------------------------------------


def player_hurt(c, v):
    d = 0.4
    clank = mix(bp(c.noise(0.12), 1300 + 200 * v, 6) * 2, bp(c.noise(0.12), 2900, 8)) * env(c, 0.12, 0.0005, 0.1)
    thud = thump(c, 180, 70, 0.2, 0.15)
    warn = at(c, 0.08, c.square(950, 0.12, 0.5) * env(c, 0.12, 0.002, 0.1) * 0.15)
    return reverb(c, mix(clank, thud, warn), 0.4, 0.15)


def player_die(c, v):
    d = 2.5
    crash = mix(thump(c, 120, 30, 0.8, 0.6), lp(c.noise(0.6), 1200) * env(c, 0.6, 0.001, 0.5))
    flat = at(c, 0.5, c.sine(880, 1.8) * adsr(c, 1.8, 0.01, 0.1, 0.6, 0.8) * 0.3)
    alarm = mix(*[at(c, 0.1 + 0.15 * k, c.square(1200 - 150 * k, 0.1, 0.5) * env(c, 0.1, 0.002, 0.09) * 0.2) for k in range(3)])
    return reverb(c, mix(crash, flat, alarm), 1.5, 0.3)


def dash(c, v):
    d = 0.32
    air = whoosh(c, d, 500 + 60 * v, 4500, 0.8) * 1.4
    jet = hp(c.noise(d), 3000) * env(c, d, 0.005, 0.2) * 0.4
    return reverb(c, mix(air, jet, thump(c, 200, 120, 0.06, 0.05) * 0.3), 0.4, 0.12)


def step_rock(c, v):
    d = 0.12
    scuff = lp(c.noise(d), 2200 + 300 * (v % 3)) * env(c, d, 0.002, 0.07)
    grit = c.crackle(d, 400, 0.0015) * env(c, d, 0.002, 0.08) * 0.5
    heel = thump(c, 130 + 10 * v, 60, 0.08, 0.05) * 0.6
    return mix(scuff, grit, heel)


def step_metal(c, v):
    d = 0.2
    ring = mix(bp(c.noise(d), 700 + 60 * v, 12) * 3, bp(c.noise(d), 1900 + 90 * v, 14) * 1.5) * env(c, d, 0.001, 0.15)
    heel = thump(c, 150, 80, 0.06, 0.04) * 0.7
    return reverb(c, mix(ring * 0.6, heel, click(c, 0.006, 3000) * 0.3), 0.35, 0.18)


# --- Mining ---------------------------------------------------------------------------------


def mine_hit(c, v):
    d = 0.2
    grind = lp(c.saw(c.ramp(d, 140, 120), d) * 0.4 + c.noise(d) * 0.8, 3000) * env(c, d, 0.002, 0.15)
    chip = c.crackle(d, 300, 0.002) * env(c, d, 0.001, 0.12)
    return reverb(c, mix(grind, chip, thump(c, 160, 90, 0.06, 0.05) * 0.5), 0.4, 0.15)


def mine_hit_ore(c, v):
    base = mine_hit(c, v)
    ping = chime(c, 2200 + 300 * v, 0.4, ratio=2.3, index=1.2, decay=0.3) * 0.35
    return mix(base, ping)


def block_break(c, v):
    d = 0.9
    crack = hp(c.noise(0.04), 900) * env(c, 0.04, 0.0003, 0.04)
    crumble = lp(c.crackle(d, 140 + 20 * v, 0.006), 2400) * env(c, d, 0.003, 0.6) * 1.5
    body = lp(c.noise(0.4, "brown"), 500) * env(c, 0.4, 0.002, 0.3) * 1.2
    return reverb(c, mix(crack, crumble, body, thump(c, 120, 45, 0.25, 0.2) * 0.8), 0.9, 0.2)


def ore_break(c, v):
    base = block_break(c, v) * 0.8
    shatter = mix(*[at(c, 0.01 * k, chime(c, 1800 + 450 * k + 100 * v, 0.6, 2.9, 1.5, 0.4) * 0.25) for k in range(4)])
    return mix(base, shatter)


# --- Pickups and loot ---------------------------------------------------------------------------


def pickup_xp(c, v):
    d = 0.18
    f = c.expo(d, 1300 + 110 * v, 2100 + 110 * v)
    return reverb(c, c.sine(f, d) * env(c, d, 0.002, 0.13) * 0.6, 0.4, 0.2)


def pickup_ferrite(c, v):
    clink = mix(bp(c.noise(0.15), 1500 + 150 * v, 10) * 3, bp(c.noise(0.15), 3700, 14) * 2) * env(c, 0.15, 0.0005, 0.12)
    return reverb(c, mix(clink, thump(c, 300, 150, 0.05, 0.04) * 0.4), 0.4, 0.2)


def pickup_lumite(c, v):
    root = 76 + (2 if v else 0)
    x = arpeggio(c, [root, root + 7, root + 12], 0.045, 0.5, bell)
    return reverb(c, x * 0.5, 0.8, 0.3, damp=6000)


def pickup_voidstone(c, v):
    x = arpeggio(c, [62, 61, 74], 0.06, 0.8, lambda c, f, s: chime(c, f, s, 1.414, 2.5, s))
    return reverb(c, drive(x, 1.2) * 0.6, 1.2, 0.35, damp=3000)


def loot(rarity: int):
    """Rising stingers: rarer is longer, brighter and wider."""

    def make(c, v):
        scales = [[72, 76, 79], [72, 76, 79, 84], [69, 72, 76, 79, 84, 88], [67, 71, 74, 79, 83, 86, 91, 95]]
        notes = scales[rarity]
        step = [0.07, 0.065, 0.06, 0.055][rarity]
        x = arpeggio(c, notes, step, 0.9 + 0.3 * rarity, bell)
        shine = hp(c.noise(1.2 + 0.4 * rarity), 6000) * env(c, 1.2 + 0.4 * rarity, 0.3, 1.0) * 0.08 * rarity
        if rarity == 3:
            pad_ = mix(*[c.saw(float(notes_to_hz(m)), 2.0) for m in (55, 62, 67, 71)]) * adsr(c, 2.0, 0.05, 0.5, 0.5, 1.0)
            x = mix(x, lp(pad_, 2500) * 0.25, thump(c, 120, 50, 0.4, 0.3) * 0.6)
        return reverb(c, mix(x, shine), 1.2 + 0.4 * rarity, 0.3, damp=6000)

    return make


def cache_open(c, v):
    hiss = whoosh(c, 0.4, 3000, 800) * 0.6
    latch = mix(click(c, 0.02, 1500), at(c, 0.05, click(c, 0.02, 2500)))
    lift = at(c, 0.1, c.sine(c.expo(0.4, 200, 400), 0.4) * env(c, 0.4, 0.05, 0.3) * 0.3)
    return reverb(c, mix(hiss, latch, lift, at(c, 0.3, loot(1)(c, v) * 0.6)), 0.8, 0.2)


def level_up(c, v):
    x = arpeggio(c, [67, 71, 74, 79, 83], 0.07, 1.2, pluck)
    up = c.sine(c.expo(0.6, 400, 1600), 0.6) * env(c, 0.6, 0.3, 0.3) * 0.15
    return reverb(c, mix(x * 0.7, up, at(c, 0.3, bell(c, float(notes_to_hz(91)), 1.0) * 0.4)), 1.4, 0.3)


def class_level_up(c, v):
    chord = [55, 59, 62, 67, 71, 74]
    stab = mix(*[c.saw(float(notes_to_hz(m)), 2.2) for m in chord]) * adsr(c, 2.2, 0.01, 0.4, 0.45, 1.2)
    stab = sweep(stab, "low", 600, 5000, 1.0)
    x = mix(stab * 0.35, level_up(c, v) * 0.8, thump(c, 100, 40, 0.5, 0.4) * 0.7)
    return reverb(c, x, 1.8, 0.3)


# --- Hazards and combos ---------------------------------------------------------------------------


def vent_heat(c, v):
    d = 1.2
    steam = sweep(c.noise(d), "band", 2500, 900, 0.7) * adsr(c, d, 0.02, 0.2, 0.6, 0.6) * 1.2
    roar = lp(c.noise(d, "brown"), 300) * env(c, d, 0.05, 0.9)
    return reverb(c, mix(steam, roar, c.crackle(d, 60) * 0.5), 1.0, 0.25)


def vent_cold(c, v):
    d = 1.3
    hiss = hp(c.noise(d), 4000) * adsr(c, d, 0.02, 0.2, 0.5, 0.7)
    tinkle = mix(*[at(c, 0.1 + 0.08 * k, chime(c, 2500 + 400 * k, 0.5, 3.1, 1.0, 0.3) * 0.15) for k in range(5)])
    return reverb(c, mix(hiss, tinkle), 1.3, 0.3, damp=7000)


def spore_blast(c, v):
    d = 1.6
    whump = mix(thump(c, 110, 40, 0.6, 0.45) * 1.3, lp(c.noise(0.5, "pink"), 1200) * env(c, 0.5, 0.005, 0.4))
    fire = sweep(c.noise(d, "pink"), "band", 600, 2200, 0.7) * env(c, d, 0.05, 1.2)
    crackle = c.crackle(d, 140, 0.003) * env(c, d, 0.05, 1.3)
    return reverb(c, drive(mix(whump, fire, crackle), 1.8), 1.6, 0.3, damp=2200)


def oxygen_low(c, v):
    beeps = mix(*[at(c, 0.22 * k, c.square(1400, 0.12, 0.5) * env(c, 0.12, 0.002, 0.11) * 0.4) for k in range(3)])
    gasp = at(c, 0.7, whoosh(c, 0.4, 2000, 500) * 0.4)
    return reverb(c, mix(lp(beeps, 5000), gasp), 0.6, 0.2)


def quake(c, v):
    d = 3.0
    rumble = lp(c.noise(d, "brown"), 120) * adsr(c, d, 0.4, 0.5, 0.8, 1.4) * 2.0
    crack = at(c, 0.3, hp(c.noise(0.1), 800) * env(c, 0.1, 0.001, 0.08))
    debris = lp(c.crackle(d, 50, 0.01), 2000) * env(c, d, 0.5, 2.2)
    return reverb(c, drive(mix(rumble, crack, debris), 1.5), 2.2, 0.3, damp=1200)


def quake_warning(c, v):
    rumble = lp(c.noise(1.6, "brown"), 90) * adsr(c, 1.6, 0.5, 0.3, 0.7, 0.6) * 1.5
    klaxon = mix(*[at(c, 0.45 * k, c.square(c.ramp(0.35, 420, 520), 0.35, 0.4) * env(c, 0.35, 0.01, 0.33) * 0.3) for k in range(3)])
    return reverb(c, mix(rumble, lp(klaxon, 3500)), 1.0, 0.25)


def rubble(c, v):
    d = 1.0
    fall = lp(c.crackle(d, 90, 0.008), 2000) * env(c, d, 0.02, 0.7) * 1.4
    thud = mix(*[at(c, 0.05 + 0.13 * k, thump(c, 140 - 15 * k, 50, 0.2, 0.15) * (0.9 - 0.2 * k)) for k in range(3)])
    return reverb(c, mix(fall, thud), 1.0, 0.22)


def shatter(c, v):
    d = 0.9
    glass = mix(*[at(c, c.rng.uniform(0, 0.08), chime(c, c.rng.uniform(2500, 6500), 0.7, 2.7, 1.8, c.rng.uniform(0.2, 0.5)) * 0.2) for _ in range(10)])
    crack = hp(c.noise(0.06), 2500) * env(c, 0.06, 0.0003, 0.05)
    return reverb(c, mix(glass, crack), 0.9, 0.25, damp=8000)


def thermal(c, v):
    d = 0.9
    hiss = sweep(c.noise(d), "high", 6000, 2000, 0.7) * env(c, d, 0.005, 0.7) * 1.2
    pop = thump(c, 260, 90, 0.12, 0.09) * 0.7
    return reverb(c, mix(hiss, pop, c.crackle(d, 200) * 0.4), 0.8, 0.2)


def conductive(c, v):
    d = 0.6
    zap = arc(c, v)[: c.n(d)]
    sizzle = hp(c.noise(d), 5000) * env(c, d, 0.01, 0.5) * 0.4
    return mix(zap, sizzle)


# --- Missions, ship and UI ----------------------------------------------------------------------------


def alarm(c, v):
    x = mix(*[at(c, 0.5 * k, c.square(c.ramp(0.45, 520, 780), 0.45, 0.45) * adsr(c, 0.45, 0.02, 0.1, 0.8, 0.1) * 0.35) for k in range(2)])
    return reverb(c, lp(x, 4000), 0.9, 0.25)


def beep(c, v):
    f = [1760, 2093][v]
    return reverb(c, c.sine(f, 0.09) * env(c, 0.09, 0.002, 0.08) * 0.6, 0.4, 0.15)


def objective(c, v):
    x = arpeggio(c, [72, 79, 84], 0.09, 1.0, pluck)
    return reverb(c, mix(x * 0.7, at(c, 0.18, bell(c, 1046.5, 1.2) * 0.35)), 1.2, 0.3)


def progress(c, v):
    return reverb(c, mix(pluck(c, 1046.5, 0.25) * 0.6, bell(c, 2093, 0.3) * 0.2), 0.5, 0.2)


def surge_warning(c, v):
    d = 2.6
    horn = mix(c.saw(c.expo(d, 110, 98), d), c.saw(c.expo(d, 110.6, 98.5), d), c.saw(c.expo(d, 165, 147), d) * 0.6)
    horn = sweep(horn, "low", 300, 1600, 1.4) * adsr(c, d, 0.6, 0.4, 0.8, 0.8)
    return reverb(c, drive(horn, 1.5), 2.4, 0.35, damp=2000)


def surge_start(c, v):
    d = 2.4
    voices = mix(*[husk_groan(c, k % 3) * 0.4 for k in range(5)])
    stomp = mix(*[at(c, 0.25 * k, thump(c, 90, 40, 0.3, 0.25) * 0.5) for k in range(8)])
    return reverb(c, mix(voices[: c.n(d)], stomp[: c.n(d)], lp(c.noise(d, "brown"), 200) * env(c, d, 0.4, 2.0)), 1.8, 0.3)


def threat_rose(c, v):
    d = 1.6
    tone = mix(c.sine(65.4, d), c.sine(69.3, d) * 0.7, c.saw(130.8, d) * 0.15)
    return reverb(c, lp(tone, 600) * adsr(c, d, 0.3, 0.3, 0.6, 0.9), 1.5, 0.3)


def boss_awaken(c, v):
    return mix(boss_roar(c, v), boss_slam(c, v) * 0.9)


def survivor(c, v):
    x = arpeggio(c, [67, 72, 76], 0.08, 0.6, bell)
    return reverb(c, x * 0.6, 0.9, 0.3)


def rig_start(c, v):
    d = 2.0
    motor = c.saw(c.expo(d, 40, 160), d) * 0.4 + c.square(c.expo(d, 80, 320), d, 0.3) * 0.2
    clank = mix(click(c, 0.03, 900), at(c, 0.15, click(c, 0.03, 1400)))
    return reverb(c, mix(lp(motor, 1600) * adsr(c, d, 0.3, 0.3, 0.8, 0.5), clank), 1.0, 0.25)


def pod_impact(c, v):
    d = 2.0
    slam = mix(thump(c, 80, 28, 1.0, 0.8) * 1.6, lp(c.noise(0.3), 1200) * env(c, 0.3, 0.001, 0.25) * 1.3)
    metal = bp(c.noise(1.2), 600, 8) * env(c, 1.2, 0.002, 1.0) * 2.0
    hiss = at(c, 0.4, whoosh(c, 1.2, 4000, 1200) * 0.4)
    return reverb(c, drive(mix(slam, metal, hiss), 1.6), 1.8, 0.25)


def pod_descent(c, v):
    d = 3.0
    burn = lp(c.noise(d, "brown"), 700) * adsr(c, d, 0.5, 0.5, 0.9, 0.5) * 1.6
    rattle = bp(c.noise(d), 1400, 4) * (0.5 + 0.5 * c.sine(13, d)) * 0.4
    wind = sweep(c.noise(d, "pink"), "band", 600, 2600, 0.6) * c.ramp(d, 0.2, 1.0) * 0.6
    return reverb(c, mix(burn, rattle, wind), 1.2, 0.2)


def liftoff(c, v):
    d = 3.0
    ignite = thump(c, 90, 35, 0.6, 0.5)
    roar = sweep(c.noise(d, "brown"), "low", 300, 1800, 0.8) * adsr(c, d, 0.3, 0.3, 0.9, 0.9) * 1.5
    return reverb(c, mix(ignite, roar, whoosh(c, d, 300, 3000) * 0.5), 1.5, 0.2)


def ftl(c, v):
    d = 4.0
    spool = c.saw(c.expo(2.4, 60, 900), 2.4) * env(c, 2.4, 2.2, 0.3) * 0.3
    spool2 = c.saw(c.expo(2.4, 60.5, 905), 2.4) * env(c, 2.4, 2.2, 0.3) * 0.3
    jump = at(c, 2.3, mix(thump(c, 160, 30, 1.2, 1.0) * 1.2, whoosh(c, 1.6, 8000, 300) * 1.2))
    return reverb(c, mix(lp(spool + spool2, 3000), jump), 2.4, 0.3)


def door(c, v):
    d = 1.0
    hiss = whoosh(c, 0.5, 4000, 1500) * 0.6
    motor = lp(c.square(c.ramp(0.7, 90, 70), 0.7, 0.3), 800) * adsr(c, 0.7, 0.05, 0.1, 0.7, 0.2) * 0.4
    clunk = at(c, 0.7, thump(c, 160, 70, 0.2, 0.15))
    return reverb(c, mix(hiss, motor, clunk), 0.6, 0.25)


def construct(c, v):
    d = 1.4
    hits = mix(*[at(c, 0.16 * k, mix(click(c, 0.03, 1200), thump(c, 200, 90, 0.1, 0.08) * 0.6, bp(c.noise(0.15), 1100 + 150 * k, 8) * env(c, 0.15, 0.001, 0.12) * 2)) for k in range(5)])
    whirr = at(c, 0.8, lp(c.saw(c.expo(0.5, 200, 600), 0.5), 2000) * env(c, 0.5, 0.05, 0.4) * 0.3)
    return reverb(c, mix(hits, whirr), 0.7, 0.25)


def build_place(c, v):
    return reverb(c, mix(thump(c, 220, 90, 0.12, 0.09), click(c, 0.02, 1600) * 0.7, bp(c.noise(0.12), 1500, 8) * env(c, 0.12, 0.001, 0.1) * 1.5), 0.5, 0.2)


def build_remove(c, v):
    return reverb(c, mix(c.sine(c.expo(0.18, 900, 300), 0.18) * env(c, 0.18, 0.002, 0.15) * 0.4, click(c, 0.03, 1200) * 0.5), 0.5, 0.2)


def ui_move(c, v):
    return mix(lp(c.sine(2400 + 200 * v, 0.03) * env(c, 0.03, 0.001, 0.025), 6000) * 0.5, click(c, 0.004, 6000) * 0.2)


def ui_confirm(c, v):
    x = mix(pluck(c, 1046.5, 0.18) * 0.6, at(c, 0.05, pluck(c, 1568, 0.25) * 0.6))
    return reverb(c, x, 0.4, 0.2)


def ui_deny(c, v):
    x = mix(c.square(220, 0.09, 0.4), at(c, 0.1, c.square(185, 0.14, 0.4))) * 0.4
    return reverb(c, lp(x, 1800) * env(c, len(x) / c.rate, 0.002, 0.22)[: len(x)], 0.3, 0.15)


def console(c, v):
    x = mix(*[at(c, 0.045 * k, c.sine(float(notes_to_hz(m)), 0.08) * env(c, 0.08, 0.001, 0.07) * 0.4) for k, m in enumerate([84, 88, 91])])
    return reverb(c, mix(x, whoosh(c, 0.25, 2500, 6000) * 0.15), 0.5, 0.2)


def console_close(c, v):
    x = mix(*[at(c, 0.04 * k, c.sine(float(notes_to_hz(m)), 0.07) * env(c, 0.07, 0.001, 0.06) * 0.35) for k, m in enumerate([88, 81])])
    return reverb(c, x, 0.4, 0.2)


def trade(c, v):
    coins = mix(*[at(c, 0.05 * k, chime(c, 2600 + 350 * k, 0.35, 2.1, 1.0, 0.25) * 0.3) for k in range(4)])
    chunk = thump(c, 300, 140, 0.08, 0.06) * 0.5
    return reverb(c, mix(coins, chunk), 0.5, 0.25)


def equip(c, v):
    clank = mix(bp(c.noise(0.2), 900, 7) * 3, bp(c.noise(0.2), 2100, 9) * 2) * env(c, 0.2, 0.0005, 0.15)
    seal = at(c, 0.12, whoosh(c, 0.2, 5000, 2000) * 0.3)
    return reverb(c, mix(clank, seal, thump(c, 200, 90, 0.08, 0.06) * 0.5), 0.5, 0.2)


def class_select(c, v):
    return mix(equip(c, v) * 0.7, at(c, 0.15, ui_confirm(c, v)))


def tutorial(c, v):
    x = mix(c.sine(1318.5, 0.1), at(c, 0.11, c.sine(1975.5, 0.14))) * 0.3
    x *= mix(env(c, 0.1, 0.002, 0.09), at(c, 0.11, env(c, 0.14, 0.002, 0.13)))
    return reverb(c, x, 0.6, 0.25)


def ability_bore(c, v):
    d = 1.0
    drill = drive(c.saw(c.expo(d, 70, 140), d) + c.noise(d) * 0.6, 2.5)
    return reverb(c, mix(lp(drill, 2400) * adsr(c, d, 0.05, 0.2, 0.7, 0.4), block_break(c, 0) * 0.6), 0.8, 0.2)


def ability_blink(c, v):
    d = 0.6
    up = c.sine(c.expo(0.25, 400, 3000), 0.25) * env(c, 0.25, 0.01, 0.2)
    pop = at(c, 0.22, mix(whoosh(c, 0.2, 6000, 1500) * 0.6, c.sine(c.expo(0.25, 2600, 600), 0.25) * env(c, 0.25, 0.002, 0.2)))
    return reverb(c, mix(up * 0.5, pop * 0.6), 0.8, 0.3, damp=7000)


def ability_barrier(c, v):
    d = 1.4
    posts = mix(*[at(c, 0.07 * k, mix(thump(c, 200, 80, 0.15, 0.1) * 0.5, click(c, 0.02, 2000) * 0.4)) for k in range(5)])
    field = c.square(120, d, 0.5) * 0.15 + hp(c.noise(d), 3000) * 0.15
    return reverb(c, mix(posts, lp(field, 2500) * adsr(c, d, 0.3, 0.2, 0.6, 0.6)), 1.0, 0.25)


def ability_turret(c, v):
    d = 1.3
    unfold = mix(*[at(c, 0.12 * k, click(c, 0.03, 1500 + 400 * k) * 0.6) for k in range(4)])
    servo = at(c, 0.4, lp(c.saw(c.expo(0.6, 200, 500), 0.6), 1800) * env(c, 0.6, 0.05, 0.5) * 0.3)
    ready = at(c, 1.0, mix(c.sine(1568, 0.1), at(c, 0.1, c.sine(2093, 0.12))) * env(c, 0.22, 0.002, 0.2) * 0.3)
    return reverb(c, mix(unfold, servo, ready), 0.6, 0.2)


# --- Ambience loops --------------------------------------------------------------------------------


def seamless(c: Ctx, make: Callable[[float], np.ndarray], seconds: float, cross: float = 2.0):
    """Render `seconds + cross` and fold the overlap back into the start."""
    x = make(seconds + cross)
    n, k = c.n(seconds), c.n(cross)
    out = np.array(x[:n])
    w = np.linspace(0, 1, k)
    out[:k] = x[:k] * w + x[n : n + k] * (1 - w)
    return out


def scatter_events(c: Ctx, seconds: float, rate_hz: float, make) -> np.ndarray:
    out = np.zeros(c.n(seconds))
    t = 0.0
    while True:
        t += c.rng.exponential(1.0 / rate_hz)
        if t >= seconds:
            break
        e = make()
        s = c.n(t)
        out[s : s + len(e)] += e[: len(out) - s]
    return out


def ship_hum(c, seconds=20.0):
    def make(d):
        hum = c.sine(50, d) * 0.5 + c.sine(100, d) * 0.3 + c.sine(150.5, d) * 0.08
        air = lp(c.noise(d, "pink"), 900) * 0.25
        vent = bp(c.noise(d), 3000, 1.0) * (0.6 + 0.4 * c.sine(0.13, d)) * 0.05
        relays = scatter_events(c, d, 0.25, lambda: click(c, 0.01, 2500) * 0.15)
        return mix(hum, air, vent, relays)

    return seamless(c, lambda d: reverb(c, make(d), 0.6, 0.15)[: c.n(d)], seconds)


def wind(c: Ctx, d: float, f0: float, f1: float, q: float, slow: float = 0.07):
    lfo = (np.sin(c.t(d) * TAU * slow) + np.sin(c.t(d) * TAU * slow * 2.3 + 1)) * 0.25 + 0.5
    cutoff = f0 + (f1 - f0) * lfo
    return sweep(c.noise(d, "pink"), "band", cutoff, None, q) * (0.5 + 0.5 * lfo)


def amb_verdant(c, seconds=24.0):
    def make(d):
        breeze = wind(c, d, 300, 1200, 0.7) * 0.6
        insects = scatter_events(c, d, 1.2, lambda: c.fm(c.rng.uniform(3500, 5200), 0.25, 0.25, 4) * env(c, 0.25, 0.02, 0.2) * (0.5 + 0.5 * c.square(c.rng.uniform(25, 40), 0.25)) * 0.07)
        drips = scatter_events(c, d, 0.4, lambda: c.sine(c.expo(0.08, c.rng.uniform(1200, 2000), 700), 0.08) * env(c, 0.08, 0.001, 0.07) * 0.12)
        return mix(breeze, insects, drips)

    return seamless(c, lambda d: reverb(c, make(d), 1.6, 0.3)[: c.n(d)], seconds)


def amb_cinder(c, seconds=24.0):
    def make(d):
        roar = lp(c.noise(d, "brown"), 160) * 1.2
        hot = wind(c, d, 200, 600, 0.6) * 0.4
        crackle = lp(c.crackle(d, 25, 0.004), 3000) * 0.3
        blorps = scatter_events(c, d, 0.3, lambda: lp(c.sine(c.expo(0.3, 90, 50), 0.3), 300) * env(c, 0.3, 0.01, 0.25) * 0.5)
        return mix(roar, hot, crackle, blorps)

    return seamless(c, lambda d: reverb(c, make(d), 1.8, 0.3, damp=1800)[: c.n(d)], seconds)


def amb_hollow(c, seconds=24.0):
    def make(d):
        howl = wind(c, d, 600, 2200, 2.5, 0.05) * 0.5
        gust = wind(c, d, 200, 700, 0.6, 0.11) * 0.4
        creaks = scatter_events(c, d, 0.15, lambda: bp(c.saw(c.expo(0.6, 180, 120), 0.6), 700, 5) * env(c, 0.6, 0.1, 0.5) * 0.2)
        return mix(howl, gust, creaks)

    return seamless(c, lambda d: reverb(c, make(d), 2.2, 0.35, damp=5000)[: c.n(d)], seconds)


def amb_tidewell(c, seconds=24.0):
    def make(d):
        lap = lp(c.noise(d, "pink"), 700) * (0.6 + 0.4 * np.sin(c.t(d) * TAU * 0.3) ** 2) * 0.6
        drips = scatter_events(c, d, 1.5, lambda: c.sine(c.expo(0.06, c.rng.uniform(900, 1800), 500), 0.06) * env(c, 0.06, 0.001, 0.05) * 0.15)
        croaks = scatter_events(c, d, 0.25, lambda: bp(c.saw(c.rng.uniform(90, 130), 0.3) * (0.5 + 0.5 * c.square(30, 0.3)), 500, 3) * env(c, 0.3, 0.02, 0.25) * 0.25)
        return mix(lap, drips, croaks, wind(c, d, 300, 900, 0.7) * 0.25)

    return seamless(c, lambda d: reverb(c, make(d), 1.4, 0.3)[: c.n(d)], seconds)


def amb_kharon(c, seconds=24.0):
    def make(d):
        drone_ = mix(c.sine(55, d), c.sine(55.7, d) * 0.8, c.sine(82.4, d) * 0.3, c.saw(110.3, d) * 0.05)
        drone_ = lp(drone_, 400) * 0.5
        groans = scatter_events(c, d, 0.12, lambda: bp(c.saw(c.expo(2.0, 70, 52), 2.0), 400, 6) * env(c, 2.0, 0.6, 1.2) * 0.4)
        return mix(drone_, groans, wind(c, d, 250, 800, 0.8) * 0.3)

    return seamless(c, lambda d: reverb(c, make(d), 3.0, 0.4, damp=1500)[: c.n(d)], seconds)


# --- Music -------------------------------------------------------------------------------------


class Song:
    """A stereo multitrack: instruments render notes into panned buffers."""

    def __init__(self, name: str, bpm: float, bars: int, beats: int = 4) -> None:
        self.c = Ctx(name, MUSIC_RATE)
        self.bpm = bpm
        self.beat = 60.0 / bpm
        self.length = bars * beats * self.beat
        # Three passes so effects wrap around the loop point.
        self.n = self.c.n(self.length)
        self.left = np.zeros(self.n * 3 + self.c.n(6))
        self.right = np.zeros_like(self.left)
        self.send_l = np.zeros_like(self.left)
        self.send_r = np.zeros_like(self.left)

    def add(self, beat: float, x, pan: float = 0.0, send: float = 0.2, gain: float = 1.0):
        """Place `x` at a beat in every pass (pan -1..1)."""
        x = np.asarray(x) * gain
        l, r = math.cos((pan + 1) * math.pi / 4), math.sin((pan + 1) * math.pi / 4)
        for k in range(3):
            s = self.c.n(beat * self.beat) + self.n * k
            e = min(len(self.left), s + len(x))
            if s >= len(self.left):
                continue
            seg = x[: e - s]
            self.left[s:e] += seg * l
            self.right[s:e] += seg * r
            self.send_l[s:e] += seg * l * send
            self.send_r[s:e] += seg * r * send

    def render(self, room: float = 2.0, damp: float = 3000, echo_beats: float = 0.75, echo_mix: float = 0.0):
        c = self.c
        ir_l = impulse(c, room, damp)
        ir_r = impulse(c, room, damp)
        wet_l = signal.fftconvolve(self.send_l, ir_l)[: len(self.left)]
        wet_r = signal.fftconvolve(self.send_r, ir_r)[: len(self.left)]
        left, right = self.left + wet_l * 0.6, self.right + wet_r * 0.6
        if echo_mix > 0:
            d = c.n(echo_beats * self.beat)
            # Ping-pong delay.
            el, er = np.zeros_like(left), np.zeros_like(right)
            tap_l, tap_r = self.send_l.copy(), self.send_r.copy()
            for k in range(1, 6):
                tap_l, tap_r = lp(tap_r, 3500, c.rate) * 0.55, lp(tap_l, 3500, c.rate) * 0.55
                el[d * k :] += tap_l[: len(el) - d * k]
                er[d * k :] += tap_r[: len(er) - d * k]
            left, right = left + el * echo_mix, right + er * echo_mix
        mid = slice(self.n, self.n * 2)
        out = np.stack([left[mid], right[mid]])
        # Gentle glue: soft-clip the peaks, then normalize.
        out = out / max(1e-9, np.max(np.abs(out))) * 1.3
        return np.tanh(out) / math.tanh(1.3)

    # Instruments (mono, return a buffer for `add`).
    def kick(self, punch=1.0):
        c = self.c
        return mix(thump(c, 150, 42, 0.45, 0.32) * 1.2, click(c, 0.006, 3500) * 0.25 * punch)

    def snare(self):
        c = self.c
        tone = c.sine(c.expo(0.15, 230, 180), 0.15) * env(c, 0.15, 0.001, 0.1) * 0.6
        body = bp(c.noise(0.25), 2200, 0.8) * env(c, 0.25, 0.001, 0.18)
        return mix(tone, body * 1.2)

    def clap(self):
        c = self.c
        hits = mix(*[at(c, 0.011 * k, bp(c.noise(0.2), 1500, 1.2) * env(c, 0.2, 0.001, 0.12 if k == 3 else 0.02)) for k in range(4)])
        return hits * 1.4

    def hat(self, open_=False):
        c = self.c
        d = 0.3 if open_ else 0.05
        return hp(c.noise(d), 7500) * env(c, d, 0.001, d) * 0.5

    def tom(self, f=110):
        c = self.c
        return c.sine(c.expo(0.4, f * 1.6, f), 0.4) * env(c, 0.4, 0.001, 0.35) * 0.9

    def bass(self, midi, beats, cutoff=900, drive_=1.5, sub=0.6):
        c = self.c
        d = beats * self.beat
        f = float(notes_to_hz(midi))
        x = mix(c.saw(f, d) * 0.7, c.saw(f * 1.005, d) * 0.4, c.sine(f / 2, d) * sub)
        x = sweep(x, "low", cutoff * 2.5, cutoff, 1.2, rate=c.rate)
        return drive(x, drive_) * adsr(c, d, 0.005, 0.1, 0.8, min(0.08, d * 0.3))

    def pad(self, midis, beats, cutoff=1800, attack=0.6):
        c = self.c
        d = beats * self.beat
        voices = []
        for m in midis:
            f = float(notes_to_hz(m))
            for det in (-0.12, 0.0, 0.11):
                voices.append(c.saw(f * 2 ** (det / 12), d))
        x = lp(mix(*voices) / len(voices), cutoff, c.rate) * 2.0
        return x * adsr(c, d, attack, 0.5, 0.8, min(1.2, d * 0.4))

    def pluck(self, midi, beats=0.5, bright=5000):
        c = self.c
        d = max(0.25, beats * self.beat * 1.8)
        f = float(notes_to_hz(midi))
        x = mix(c.saw(f, d) * 0.5, c.square(f, d, 0.3) * 0.4)
        return sweep(x, "low", bright, 400, 1.6, rate=c.rate) * env(c, d, 0.002, d * 0.8)

    def bell(self, midi, beats=2.0):
        c = self.c
        d = beats * self.beat
        return chime(c, float(notes_to_hz(midi)), d, ratio=3.5, index=1.4, decay=d)

    def lead(self, midi, beats, vibrato=0.006, cutoff=3500):
        c = self.c
        d = beats * self.beat
        f = float(notes_to_hz(midi)) * (1 + vibrato * c.sine(5.5, d) * c.ramp(d, 0, 1))
        x = mix(c.saw(f, d) * 0.6, c.square(f * 0.5, d, 0.5) * 0.3)
        return lp(x, cutoff, c.rate) * adsr(c, d, 0.02, 0.2, 0.7, min(0.2, d * 0.3))

    def riser(self, beats):
        c = self.c
        d = beats * self.beat
        x = mix(sweep(c.noise(d), "band", 300, 6000, 1.5, rate=c.rate), c.saw(c.expo(d, 200, 1600), d) * 0.15)
        return x * c.ramp(d, 0, 1, 2)

    def drums(self, bars, kicks, snares, hats, open_hats=(), steps=16, swing=0.0, start=0, vel=None):
        """Patterns are step indices within a bar (16 steps = 16ths)."""
        per = 4.0 / steps
        k, s, h, o = self.kick(), self.snare(), self.hat(), self.hat(True)
        for bar in range(start, start + bars):
            base = bar * 4
            for i in kicks:
                self.add(base + i * per, k, 0, 0.05, 0.9)
            for i in snares:
                self.add(base + i * per, s, 0.05, 0.25, 0.7)
            for i in hats:
                sw = swing * per if i % 2 else 0.0
                self.add(base + i * per + sw, h, 0.35, 0.05, (0.35 if i % 4 else 0.5) * (vel or 1.0))
            for i in open_hats:
                self.add(base + i * per, o, -0.3, 0.1, 0.35)


def chord(root: int, kind: str) -> list[int]:
    shapes = {
        "min": [0, 3, 7],
        "maj": [0, 4, 7],
        "min7": [0, 3, 7, 10],
        "maj7": [0, 4, 7, 11],
        "sus2": [0, 2, 7],
        "sus4": [0, 5, 7],
        "min9": [0, 3, 7, 10, 14],
        "add9": [0, 4, 7, 14],
        "dim": [0, 3, 6],
    }
    return [root + i for i in shapes[kind]]


def music_ship() -> np.ndarray:
    """Calm, warm and a little lonely: a salvage ship drifting in orbit."""
    s = Song("music_ship", 72, 16)
    prog = [(50, "min9"), (46, "maj7"), (41, "maj7"), (48, "add9")]  # Dm9 Bbmaj7 Fmaj7 Cadd9
    for bar in range(16):
        root, kind = prog[(bar // 2) % 4]
        if bar % 2 == 0:
            s.add(bar * 4, s.pad(chord(root + 12, kind), 8, cutoff=1400, attack=1.2), 0, 0.5, 0.2)
            s.add(bar * 4, s.bass(root - 12, 7.5, cutoff=300, drive_=1.0, sub=0.6), 0, 0.1, 0.28)
        # Arpeggio of 8ths, panned left and right in turn.
        notes = chord(root + 24, kind)
        for i in range(8):
            if (bar * 8 + i) % 3 == 2 and bar % 4 == 3:
                continue
            m = notes[(i * 2 + bar) % len(notes)]
            s.add(bar * 4 + i * 0.5, s.pluck(m, 0.5, 2600), -0.5 if i % 2 else 0.5, 0.45, 0.24)
    # A sparse bell melody in the second half.
    for beat, m in [(32, 81), (35, 79), (38, 77), (40, 76), (44, 74), (48, 81), (51, 84), (54, 83), (56, 79), (60, 77)]:
        s.add(beat, s.bell(m, 3), 0.2, 0.6, 0.32)
    # Soft shaker pulse.
    sh = s.hat()
    for b in range(64):
        s.add(b + 0.5, sh, 0.4, 0.2, 0.12)
    return s.render(room=3.0, damp=2500, echo_beats=0.75, echo_mix=0.35)


def music_surface() -> np.ndarray:
    """Tense and driving: the horde is coming and the clock is running."""
    s = Song("music_surface", 112, 24)
    prog = [(40, "min"), (36, "maj"), (38, "sus2"), (35, "min")]  # Em C D Bm (low)
    for bar in range(24):
        root, kind = prog[bar % 4]
        # 16th-note bass ostinato with octave jumps.
        for i in range(16):
            m = root - 12 + (12 if i in (3, 7, 11, 14) else 0)
            s.add(bar * 4 + i * 0.25, s.bass(m, 0.24, cutoff=700 + 300 * (bar % 4 == 3), drive_=2.0, sub=0.4), 0, 0.05, 0.35)
        if bar % 2 == 0:
            s.add(bar * 4, s.pad(chord(root + 12, kind), 8, cutoff=1200, attack=0.8), 0, 0.4, 0.22)
    # Drums enter after 4 bars; fills every 8.
    s.drums(4, [0], [], [2, 6, 10, 14], start=0, vel=0.7)
    s.drums(20, [0, 6, 8, 11], [4, 12], [0, 2, 4, 6, 8, 10, 12, 14], [14], start=4, swing=0.08)
    for fill in (11, 19, 23):
        for i, f in enumerate((180, 150, 120, 95)):
            s.add(fill * 4 + 2 + i * 0.5, s.tom(f), -0.4 + 0.25 * i, 0.2, 0.6)
    # Lead melody over the back half.
    melody = [(64, 1.5), (66, 0.5), (67, 1.0), (71, 1.0), (69, 2.0), (67, 1.0), (66, 1.0),
              (64, 1.5), (62, 0.5), (64, 1.0), (67, 1.0), (66, 3.0), (59, 1.0)]
    beat = 48.0
    for rep in range(2):
        for m, d in melody:
            s.add(beat, s.lead(m + 12, d * 0.95, cutoff=2800), 0.15, 0.35, 0.2)
            beat += d
    s.add(44, s.riser(4), 0, 0.3, 0.25)
    return s.render(room=1.8, damp=3000, echo_beats=0.75, echo_mix=0.25)


def music_extraction() -> np.ndarray:
    """Urgent: hold the zone until the pod arrives."""
    s = Song("music_extraction", 134, 16)
    prog = [(36, "min"), (32, "maj"), (34, "maj"), (31, "maj")]  # Cm Ab Bb G
    for bar in range(16):
        root, kind = prog[(bar // 2) % 4]
        for i in range(8):
            m = root - 12 + (12 if i % 2 else 0)
            s.add(bar * 4 + i * 0.5, s.bass(m, 0.45, cutoff=900, drive_=2.6, sub=0.5), 0, 0.05, 0.4)
        if bar % 2 == 0:
            s.add(bar * 4, s.pad(chord(root + 24, kind), 8, cutoff=2400, attack=0.1), 0, 0.3, 0.18)
        # Siren-like stabs.
        for i in (0, 3, 6):
            s.add(bar * 4 + i * 0.5, s.pluck(chord(root + 24, kind)[i % 3] + 12, 0.4, 6000), 0.4 if i % 2 else -0.4, 0.3, 0.14)
    s.drums(16, [0, 4, 8, 12], [4, 12], list(range(16)), [2, 6, 10, 14])
    clap = s.clap()
    for bar in range(16):
        s.add(bar * 4 + 1, clap, -0.1, 0.3, 0.35)
        s.add(bar * 4 + 3, clap, 0.1, 0.3, 0.35)
    s.add(56, s.riser(8), 0, 0.3, 0.3)
    return s.render(room=1.4, damp=3500, echo_beats=0.5, echo_mix=0.2)


def music_boss() -> np.ndarray:
    """Heavy and menacing: something enormous is awake."""
    s = Song("music_boss", 92, 16)
    prog = [(42, "min"), (42, "min"), (45, "maj"), (40, "maj"), (42, "min"), (38, "maj"), (40, "maj"), (37, "maj")]
    for bar in range(16):
        root, kind = prog[bar % 8]
        # Half-time distorted bass hits.
        for beat, d in ((0, 1.5), (1.5, 0.5), (2, 1.0), (3.25, 0.75)):
            s.add(bar * 4 + beat, s.bass(root - 12, d, cutoff=600, drive_=4.0, sub=0.9), 0, 0.05, 0.42)
        s.add(bar * 4, s.pad(chord(root + 12, kind) + [root + 24], 4, cutoff=1600, attack=0.3), 0, 0.5, 0.28)
        # Brass stab on the downbeat of odd bars.
        if bar % 2 == 1:
            stab = s.pad(chord(root + 24, kind), 0.6, cutoff=3500, attack=0.01)
            s.add(bar * 4, stab, 0, 0.4, 0.25)
    k, sn = s.kick(1.5), s.snare()
    for bar in range(16):
        for b in (0, 1.5, 2.75):
            s.add(bar * 4 + b, k, 0, 0.05, 1.0)
        s.add(bar * 4 + 2, sn, 0, 0.4, 0.9)
        for i in range(8):
            s.add(bar * 4 + i * 0.5, s.hat(), 0.3, 0.05, 0.25)
        if bar % 4 == 3:
            for i, f in enumerate((160, 130, 100, 80)):
                s.add(bar * 4 + 3 + i * 0.25, s.tom(f), -0.3 + 0.2 * i, 0.3, 0.7)
    # Low choir-like drone melody.
    for beat, m in [(0, 66), (8, 69), (16, 64), (24, 61), (32, 66), (40, 62), (48, 64), (56, 61)]:
        s.add(beat, s.lead(m, 7.5, vibrato=0.01, cutoff=1200), -0.2, 0.6, 0.18)
    return s.render(room=2.6, damp=2000)


# --- Catalogue ----------------------------------------------------------------------------------


EFFECTS: list[tuple[str, Callable, int]] = [
    # Weapons.
    ("blaster", blaster, 4),
    ("cutter", cutter, 3),
    ("scatter", scatter, 3),
    ("arc", arc, 3),
    ("mortar", mortar, 3),
    ("drone", drone, 3),
    ("flamer", flamer, 3),
    ("ricochet", ricochet, 3),
    ("mine", mine, 2),
    ("swing", swing, 3),
    ("seeker", seeker, 3),
    ("gravity", gravity, 2),
    # Combat.
    ("enemy_hit", enemy_hit, 4),
    ("explosion", explosion, 3),
    ("enemy_die", husk_death, 4),
    ("skitter_die", skitter_death, 3),
    ("bloater_die", bloater_death, 2),
    ("brute_die", brute_death, 2),
    ("spitter_die", spitter_death, 2),
    ("flyer_die", flyer_death, 3),
    ("disruptor_die", disruptor_death, 2),
    ("burrower_die", burrower_death, 2),
    ("boss_die", boss_death, 1),
    ("husk_groan", husk_groan, 3),
    ("skitter_alert", skitter_alert, 2),
    ("brute_roar", brute_roar, 2),
    ("boss_roar", boss_roar, 2),
    ("boss_slam", boss_slam, 2),
    ("spit", spit, 3),
    ("jam", jam, 2),
    ("burrow", burrow, 2),
    ("split", split, 2),
    # Player.
    ("player_hurt", player_hurt, 4),
    ("player_die", player_die, 1),
    ("dash", dash, 3),
    ("step_rock", step_rock, 6),
    ("step_metal", step_metal, 6),
    # Mining and pickups.
    ("mine_hit", mine_hit, 4),
    ("mine_hit_ore", mine_hit_ore, 3),
    ("block_break", block_break, 4),
    ("ore_break", ore_break, 3),
    ("pickup_xp", pickup_xp, 3),
    ("pickup_ferrite", pickup_ferrite, 2),
    ("pickup_lumite", pickup_lumite, 2),
    ("pickup_voidstone", pickup_voidstone, 2),
    ("loot_common", loot(0), 1),
    ("loot_uncommon", loot(1), 1),
    ("loot_rare", loot(2), 1),
    ("loot_legendary", loot(3), 1),
    ("cache_open", cache_open, 1),
    ("level_up", level_up, 1),
    ("class_level_up", class_level_up, 1),
    # Hazards and combos.
    ("vent_heat", vent_heat, 2),
    ("vent_cold", vent_cold, 2),
    ("spore_blast", spore_blast, 2),
    ("oxygen_low", oxygen_low, 1),
    ("quake", quake, 1),
    ("quake_warning", quake_warning, 1),
    ("rubble", rubble, 3),
    ("shatter", shatter, 2),
    ("thermal", thermal, 2),
    ("conductive", conductive, 2),
    # Missions, ship and UI.
    ("alarm", alarm, 1),
    ("beep", beep, 2),
    ("objective", objective, 1),
    ("progress", progress, 1),
    ("surge_warning", surge_warning, 1),
    ("surge_start", surge_start, 1),
    ("threat_rose", threat_rose, 1),
    ("boss_awaken", boss_awaken, 1),
    ("survivor", survivor, 1),
    ("rig_start", rig_start, 1),
    ("pod_impact", pod_impact, 1),
    ("pod_descent", pod_descent, 1),
    ("liftoff", liftoff, 1),
    ("ftl", ftl, 1),
    ("door", door, 1),
    ("construct", construct, 1),
    ("build_place", build_place, 2),
    ("build_remove", build_remove, 2),
    ("ui_move", ui_move, 2),
    ("ui_confirm", ui_confirm, 1),
    ("ui_deny", ui_deny, 1),
    ("console", console, 1),
    ("console_close", console_close, 1),
    ("trade", trade, 1),
    ("equip", equip, 2),
    ("class_select", class_select, 1),
    ("tutorial", tutorial, 1),
    ("ability_bore", ability_bore, 1),
    ("ability_blink", ability_blink, 1),
    ("ability_barrier", ability_barrier, 1),
    ("ability_turret", ability_turret, 1),
]

AMBIENCE = [
    ("ship_hum", ship_hum),
    ("amb_verdant", amb_verdant),
    ("amb_cinder", amb_cinder),
    ("amb_hollow", amb_hollow),
    ("amb_tidewell", amb_tidewell),
    ("amb_kharon", amb_kharon),
]

MUSIC = [
    ("music_ship", music_ship),
    ("music_surface", music_surface),
    ("music_extraction", music_extraction),
    ("music_boss", music_boss),
]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=REPO / "assets" / "sounds")
    parser.add_argument("names", nargs="*", help="remake only these sounds, replacing them")
    parser.add_argument("--force", action="store_true", help="remake every sound")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    known = {n for n, _, _ in EFFECTS} | {n for n, _ in AMBIENCE} | {n for n, _ in MUSIC}
    unknown = set(args.names) - known
    if unknown:
        raise SystemExit(f"unknown sounds: {', '.join(sorted(unknown))}")
    w = Writer(args.out, set(args.names), args.force)
    for name, make, variants in EFFECTS:
        w.sound(name, make, variants)
    for name, make in AMBIENCE:
        if w.wanted(name):
            c = Ctx(name)
            w.save_ogg(name, make(c), RATE, peak_db=-3.0, quality=3)
    for name, make in MUSIC:
        if w.wanted(name):
            w.save_ogg(name, make(), MUSIC_RATE, peak_db=-1.0, quality=5)
            print(f"  {name}", flush=True)
    print(f"wrote {len(w.written)} files to {args.out}, kept {w.kept} existing sounds")


if __name__ == "__main__":
    main()
