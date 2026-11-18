"""Remastered sound effects: designed sounds that replace drafts from
`generate_sounds.py`.

Each recipe layers synthesis with CC0 recordings from Kenney's Impact Sounds
and Sci-fi Sounds packs (kept in `sources/kenney/`), then saturates,
compresses, EQs and loudness-matches every variant to the level of the draft
it replaced, so the game's mix stays balanced. `REMASTERED` lists every
remastered sound; `docs/assets.md` tracks them too.

    python3 DevUtils/assets/remaster_sounds.py                  # list them
    python3 DevUtils/assets/remaster_sounds.py blaster          # rewrite one
    python3 DevUtils/assets/remaster_sounds.py --all --out DIR  # preview all

Output is deterministic (each variant seeds its own generator). Needs numpy,
scipy and ffmpeg.
"""

import argparse
import subprocess
import wave
from pathlib import Path

import numpy as np
from scipy import signal

RATE = 44100
ROOT = Path(__file__).resolve().parents[2]
SOURCES = Path(__file__).resolve().parent / "sources" / "kenney"
SOUNDS = ROOT / "assets" / "sounds"
VARIANTS = 4


# --- I/O -------------------------------------------------------------------------------


def load(path: Path) -> np.ndarray:
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-ac", "1", "-ar", str(RATE), "-f", "f32le", "-"],
        capture_output=True,
        check=True,
    ).stdout
    x = np.frombuffer(raw, np.float32).astype(np.float64)
    return x / (np.abs(x).max() + 1e-12)


def save(path: Path, x: np.ndarray) -> None:
    pcm = (np.clip(x, -1, 1) * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())


# --- DSP -------------------------------------------------------------------------------


def n(seconds: float) -> int:
    return int(round(seconds * RATE))


def t(seconds: float) -> np.ndarray:
    return np.arange(n(seconds)) / RATE


def fit(x: np.ndarray, length: int) -> np.ndarray:
    return x[:length] if len(x) >= length else np.pad(x, (0, length - len(x)))


def place(length: int, x: np.ndarray, at: float = 0.0) -> np.ndarray:
    out = np.zeros(length)
    i = n(at)
    seg = x[: max(0, length - i)]
    out[i : i + len(seg)] = seg
    return out


def sos(kind: str, freq, order: int = 2):
    return signal.butter(order, freq, btype=kind, fs=RATE, output="sos")


def hp(x, f, order=2):
    return signal.sosfilt(sos("highpass", f, order), x)


def lp(x, f, order=2):
    return signal.sosfilt(sos("lowpass", f, order), x)


def bp(x, lo, hi, order=2):
    return signal.sosfilt(sos("bandpass", [lo, hi], order), x)


def peak_eq(x, f, gain_db, q=1.0):
    """RBJ peaking EQ."""
    a = 10 ** (gain_db / 40)
    w = 2 * np.pi * f / RATE
    alpha = np.sin(w) / (2 * q)
    b = [1 + alpha * a, -2 * np.cos(w), 1 - alpha * a]
    den = [1 + alpha / a, -2 * np.cos(w), 1 - alpha / a]
    return signal.lfilter(np.array(b) / den[0], np.array(den) / den[0], x)


def sweep_lp(x, f0, f1, q=0.9, block=32):
    """Resonant low-pass whose cutoff glides exponentially from f0 to f1."""
    out = np.zeros_like(x)
    zi = None
    steps = max(1, len(x) // block)
    for k in range(steps + 1):
        a, b = k * block, min(len(x), (k + 1) * block)
        if a >= b:
            break
        f = f0 * (f1 / f0) ** (a / max(1, len(x)))
        w = 2 * np.pi * min(f, RATE * 0.45) / RATE
        alpha = np.sin(w) / (2 * q)
        cb = np.array([(1 - np.cos(w)) / 2, 1 - np.cos(w), (1 - np.cos(w)) / 2])
        ca = np.array([1 + alpha, -2 * np.cos(w), 1 - alpha])
        cb, ca = cb / ca[0], ca / ca[0]
        if zi is None:
            zi = signal.lfilter_zi(cb, ca) * 0
        out[a:b], zi = signal.lfilter(cb, ca, x[a:b], zi=zi)
    return out


def env(length: int, attack: float, decay: float, curve: float = 1.0) -> np.ndarray:
    """Linear attack, then exponential decay (`decay` = seconds to -60 dB)."""
    s = np.arange(length) / RATE
    a = np.clip(s / max(attack, 1e-5), 0, 1)
    d = np.exp(-6.9 * np.clip(s - attack, 0, None) / decay) ** curve
    return a * d


def pitch(x, ratio: float) -> np.ndarray:
    """Resample to shift pitch (and length) by `ratio`."""
    return signal.resample(x, max(8, int(len(x) / ratio)))


def gate(x, keep: float, fade: float) -> np.ndarray:
    """Keep the first `keep` seconds, fading out over `fade`."""
    out = fit(x, n(keep + fade)).copy()
    out[n(keep) :] *= np.linspace(1, 0, len(out) - n(keep)) ** 2
    return out


def saturate(x, drive: float) -> np.ndarray:
    return np.tanh(x * drive) / np.tanh(drive)


def compress(x, threshold_db=-18.0, ratio=4.0, attack=0.002, release=0.06, makeup_db=0.0):
    """Feed-forward peak compressor."""
    level = np.abs(x)
    ga, gr = np.exp(-1 / (attack * RATE)), np.exp(-1 / (release * RATE))
    follow = np.zeros_like(x)
    e = 0.0
    for i, v in enumerate(level):
        g = ga if v > e else gr
        e = g * e + (1 - g) * v
        follow[i] = e
    db = 20 * np.log10(follow + 1e-9)
    over = np.clip(db - threshold_db, 0, None)
    gain = 10 ** ((-over * (1 - 1 / ratio) + makeup_db) / 20)
    return x * gain


def room(rng, seconds: float, damp: float, density: float = 1.0) -> np.ndarray:
    """A small-room impulse response: sparse early taps, then a darkening tail."""
    length = n(seconds)
    ir = np.zeros(length)
    for _ in range(int(14 * density)):
        i = rng.integers(n(0.003), n(0.03))
        ir[i] += rng.uniform(-0.6, 0.6)
    tail = rng.standard_normal(length) * env(length, 0.004, seconds * 0.8)
    tail = lp(tail, damp)
    ir += tail * 0.35
    ir[0] = 1.0
    return ir


def reverb(x, ir, wet: float) -> np.ndarray:
    y = signal.fftconvolve(x, ir)[: len(x)]
    y *= np.abs(x).max() / (np.abs(y).max() + 1e-12)
    return x * (1 - wet) + y * wet


def finish(x, rms_db: float, peak_db: float = -1.0, tail_floor_db: float = -60.0) -> np.ndarray:
    """Trim the silent tail, fade the edges and match loudness under a peak ceiling."""
    peak = np.abs(x).max() + 1e-12
    loud = np.abs(x) > peak * 10 ** (tail_floor_db / 20)
    end = len(x) - np.argmax(loud[::-1])
    x = x[: min(len(x), end + n(0.01))].copy()
    x[: n(0.0005)] *= np.linspace(0, 1, n(0.0005))
    x[-n(0.008) :] *= np.linspace(1, 0, n(0.008))
    above = np.abs(x) > np.abs(x).max() * 0.01
    rms = np.sqrt(np.mean(x[above] ** 2)) + 1e-12
    x *= 10 ** (rms_db / 20) / rms
    ceiling = 10 ** (peak_db / 20)
    if np.abs(x).max() > ceiling:
        x = np.tanh(x / ceiling) * ceiling  # soft limit the few hot peaks
    return x


# --- Balanced mixing -------------------------------------------------------------------

BANDS = [(20, 120), (120, 500), (500, 2000), (2000, 6000), (6000, 20000)]


def band_energy(x) -> np.ndarray:
    spec = np.abs(np.fft.rfft(x)) ** 2
    f = np.fft.rfftfreq(len(x), 1 / RATE)
    return np.array([spec[(f >= a) & (f < b)].sum() for a, b in BANDS])


def balance(layers, target, bounds):
    """Mix `layers` (equal length) with gains chosen so the sum's energy per
    band approaches `target` shares. Layers are roughly uncorrelated, so band
    energies add: solve for each layer's power share within `bounds`."""
    from scipy.optimize import lsq_linear

    energies = np.array([band_energy(x) for x in layers])  # layer x band
    energies /= energies.sum(axis=1, keepdims=True)  # each layer: unit total
    target = np.asarray(target, float) / np.sum(target)
    # Rows: bands (weighted) plus one row pinning the total share to 1.
    a = np.vstack([energies.T * 4.0, np.ones(len(layers))])
    b = np.concatenate([target * 4.0, [1.0]])
    lo, hi = zip(*bounds)
    share = lsq_linear(a, b, bounds=(lo, hi)).x
    out = np.zeros(len(layers[0]))
    for x, w in zip(layers, share):
        e = np.sqrt(np.sum(x**2)) + 1e-12
        out += x / e * np.sqrt(w)
    return out, share


def level_lows(x, share: float, freq: float = 75.0) -> np.ndarray:
    """Saturation thickens some takes' lows: trim each variant to the same
    sub-120 Hz share so all of them sit the same in the mix."""
    for _ in range(6):
        e = band_energy(x)
        if e[0] / e.sum() <= share:
            break
        x = peak_eq(x, freq, -2.0, 0.7)
    return x


def level_band(x, band: int, share: float, freq: float, tolerance: float = 0.04) -> np.ndarray:
    """Nudge one band of `BANDS` toward `share` of the energy with a broad
    EQ at `freq`, so variants built from different takes match in tone."""
    for _ in range(8):
        e = band_energy(x)
        now = e[band] / e.sum()
        if abs(now - share) <= tolerance:
            break
        x = peak_eq(x, freq, 1.5 if now < share else -1.5, 0.8)
    return x


# --- Oscillators ------------------------------------------------------------------------


def osc(freq: np.ndarray, shape: str, phase0: float = 0.0) -> np.ndarray:
    ph = phase0 + np.cumsum(freq) / RATE
    frac = ph % 1.0
    if shape == "sine":
        return np.sin(2 * np.pi * ph)
    if shape == "saw":
        # PolyBLEP saw to keep the falling sweep free of aliasing hiss.
        dt = freq / RATE
        y = 2 * frac - 1
        m1 = frac < dt
        y[m1] -= (frac[m1] / dt[m1] - 1) ** 2
        m2 = frac > 1 - dt
        y[m2] += ((frac[m2] - 1) / dt[m2] + 1) ** 2
        return y
    if shape == "square":
        return np.sign(np.sin(2 * np.pi * ph))
    raise ValueError(shape)


def glide(length: int, f0: float, f1: float, seconds: float) -> np.ndarray:
    """Exponential pitch fall from f0 to f1 over `seconds`, then holding f1."""
    s = np.arange(length) / RATE
    k = np.clip(s / seconds, 0, 1)
    return f0 * (f1 / f0) ** (1 - (1 - k) ** 2.2)


# --- Sounds -----------------------------------------------------------------------------


def enemy_hit(lib: dict, v: int, rng) -> np.ndarray:
    """A bolt landing in chitin and flesh: a hard smack, a wet splat and a tiny
    energy fizz. Four variants differ in source takes and pitch."""
    length = n(0.2)
    tune = [1.0, 0.93, 1.07, 0.97][v]

    # Body: a recorded heavy punch (1 ms attack), tuned up and cut short.
    punch = lib["impactPunch_heavy"][[0, 2, 3, 4][v]]
    punch = hp(gate(pitch(punch, 1.25 * tune), 0.06, 0.04), 70)

    # Smack: a recorded soft impact adds the meaty low-mid.
    soft = lib["impactSoft_medium"][v]
    soft = gate(pitch(soft, 1.4 * tune), 0.05, 0.04)
    soft = peak_eq(hp(soft, 120), 400, 4, 1.0)

    # Crack: chitin breaking, a burst of band-passed noise with a fast decay.
    crack = bp(rng.standard_normal(length), 1400 * tune, 6500) * env(length, 0.0003, 0.025, 1.2)

    # Splat: the wet squelch of a slime recording, cut from its loudest moment.
    slime = lib["slime"][0]
    top = int(np.argmax(np.abs(slime)))
    start = max(0, top - n(0.02) + n(0.01) * v)
    splat = slime[start : start + n(0.09)]
    splat = bp(pitch(splat, 0.8 * tune), 700, 4500) * env(len(pitch(splat, 0.8 * tune)), 0.004, 0.08)

    # Fizz: the energy bolt discharging, a few crackles high up.
    sparks = (rng.random(length) < 0.02).astype(float) * rng.uniform(-1, 1, length)
    fizz = hp(sparks, 4000) * env(length, 0.002, 0.07)

    # A punchy low end, a meaty middle and a clear crack on top so a hit
    # reads through gunfire.
    x, _ = balance(
        [
            place(length, punch),
            place(length, soft),
            place(length, crack),
            place(length, splat, 0.006),
            place(length, fizz, 0.004),
        ],
        target=[22, 34, 26, 15, 3],
        bounds=[(0.12, 0.45), (0.08, 0.35), (0.04, 0.25), (0.05, 0.3), (0.005, 0.04)],
    )
    x = saturate(x / np.abs(x).max() * 1.2, 2.2)
    x = compress(x, -12, 4, 0.001, 0.05)
    x = peak_eq(x, 250, -2.5, 1.0)  # less mud than the old thud
    x = peak_eq(x, 2400, 2.0, 1.2)  # more presence so hits read in a crowd
    x = hp(x, 45)
    x = reverb(x, room(rng, 0.18, 3000, 0.6), 0.08)
    return finish(x, rms_db=-20.2, peak_db=-3.0)


def grains(rng, length: int, count: int, spread: float, lo: float, hi: float,
           size=(0.003, 0.012), decay: float = 0.25) -> np.ndarray:
    """Scattered rock chips: `count` tiny noise bursts, each band-passed around
    a random pitch between `lo` and `hi`, thinning out over `spread` seconds."""
    out = np.zeros(length)
    for _ in range(count):
        at = n(spread * rng.random() ** 1.8)
        dur = n(rng.uniform(*size))
        if at + dur >= length or dur < 8:
            continue
        centre = np.exp(rng.uniform(np.log(lo), np.log(hi)))
        burst = bp(rng.standard_normal(dur), centre * 0.7, min(centre * 1.4, RATE * 0.45))
        burst *= env(dur, 0.0003, dur / RATE) * np.exp(-at / RATE / decay)
        out[at : at + dur] += burst * rng.uniform(0.4, 1.0)
    return out


def block_break(lib: dict, v: int, rng) -> np.ndarray:
    """A block giving way: a heavy crunch, the slab cracking apart, then
    rubble tumbling and dust settling."""
    length = n(0.75)
    tune = [1.0, 0.94, 1.05, 0.97][v]

    # Crunch: a recorded crunch for the break itself.
    crunch = lib["explosionCrunch"][[0, 1, 3, 0][v]]
    crunch = hp(gate(pitch(crunch, 1.1 * tune), 0.22, 0.2), 45)

    # Weight: a pick-on-rock take pitched down under it.
    pick = lib["impactMining"][(v + 2) % 5]
    weight = gate(pitch(pick, 0.8 * tune), 0.15, 0.15)

    # Crack: the slab splitting, a sharp mid-high burst.
    crack = bp(rng.standard_normal(length), 900, 6000) * env(length, 0.0003, 0.04, 1.3)

    # Rubble: chunks then smaller bits, thinning out; a few low tumbles.
    rubble = grains(rng, length, 140, 0.55, 300, 5000, size=(0.004, 0.02), decay=0.25)
    tumbles = np.zeros(length)
    for k in range(4):
        at = n(0.08 + 0.1 * k + rng.uniform(0, 0.05))
        thud = osc(glide(n(0.08), 140, 60, 0.06), "sine") * env(n(0.08), 0.002, 0.07)
        tumbles += place(length, thud, at / RATE) * (0.8 ** k)

    # Dust: a soft hiss settling.
    dust = lp(rng.standard_normal(length), 2500) * env(length, 0.05, 0.6) * 0.6

    x, _ = balance(
        [place(length, l) for l in (crunch, weight, crack, rubble, tumbles, dust)],
        target=[24, 31, 25, 16, 4],
        bounds=[(0.25, 0.55), (0.08, 0.3), (0.02, 0.12), (0.06, 0.3), (0.02, 0.12), (0.01, 0.06)],
    )
    x = saturate(x / np.abs(x).max() * 1.15, 2.0)
    x = compress(x, -15, 3.5, 0.002, 0.1)
    x = peak_eq(x, 300, -2.0, 0.9)  # less boom, more crunch than the draft
    x = lp(hp(x, 38), 12000)
    x = reverb(x, room(rng, 0.7, 3000), 0.2)
    x = level_lows(x, 0.26, 80)
    return finish(x, rms_db=-21.6, peak_db=-2.0)


def formant_voice(rng, length: int, pitch_hz: np.ndarray, vowels, breath: float = 0.3) -> np.ndarray:
    """A creature voice: a jittery glottal pulse train plus breath noise
    through three formant resonances gliding between `vowels` (lists of
    (F1, F2, F3) in Hz, spread evenly over the sound)."""
    jitter = 1 + 0.04 * lp(rng.standard_normal(length), 40) / 0.1
    f = np.clip(pitch_hz * jitter, 30, 2000)
    phase = np.cumsum(f) / RATE
    pulse = (phase % 1.0) ** 6  # sharp glottal closure, rich in harmonics
    pulse = hp(pulse - pulse.mean(), 40)
    source = pulse / (np.abs(pulse).max() + 1e-9) + breath * rng.standard_normal(length) * 0.3
    out = np.zeros(length)
    block = 256
    shape = np.array(vowels, float)
    for i in range(3):
        track = np.interp(np.linspace(0, len(shape) - 1, length), np.arange(len(shape)), shape[:, i])
        y = np.zeros(length)
        zi = None
        for a in range(0, length, block):
            b = min(length, a + block)
            fc = track[a]
            bw = 80 + fc * 0.08
            cb, ca = signal.iirpeak(fc, fc / bw, fs=RATE)
            if zi is None:
                zi = np.zeros(2)
            y[a:b], zi = signal.lfilter(cb, ca, source[a:b], zi=zi)
        out += y * [1.0, 0.6, 0.3][i]
    return out


def enemy_die(lib: dict, v: int, rng) -> np.ndarray:
    """A husk going down: its shell cracking, a gurgling death groan, a wet
    splat and the body hitting the ground."""
    length = n(0.85)
    tune = [1.0, 0.92, 1.08, 0.96][v]

    # Crack: a short crunch as the shell gives.
    crunch = lib["explosionCrunch"][2]
    crack = hp(gate(pitch(crunch, 1.4 * tune), 0.05, 0.05), 200)

    # Groan: a voiced exhale falling in pitch, gurgling, vowel "uh" to "oh".
    groan_len = n(0.55)
    s = np.arange(groan_len) / RATE
    gurgle = 1 + 0.25 * np.sin(2 * np.pi * (22 + 6 * v) * s)
    hz = 150 * tune * (0.45 ** (s / 0.55)) * gurgle
    groan = formant_voice(rng, groan_len, hz, [(650, 1100, 2450), (560, 950, 2300), (430, 780, 2200)], 0.5)
    groan *= env(groan_len, 0.03, 0.5, 0.9)
    groan = saturate(groan / (np.abs(groan).max() + 1e-9), 2.5)

    # Splat: wet matter from a slime take (a different moment per variant).
    slime = lib["slime"][1]
    start = n(0.6 + 0.9 * v)
    splat = bp(pitch(slime[start : start + n(0.18)], 0.7 * tune), 300, 3500)
    splat *= env(len(splat), 0.005, 0.15)

    # Fall: the body hitting the ground a moment later.
    fall = lib["impactSoft_heavy"][v]
    fall = hp(gate(pitch(fall, 1.1 * tune), 0.18, 0.12), 45)
    debris = grains(rng, n(0.3), 30, 0.2, 400, 3000, decay=0.12)

    x, _ = balance(
        [
            place(length, crack),
            place(length, groan, 0.02),
            place(length, splat, 0.01),
            place(length, fall, 0.3 + 0.03 * v),
            place(length, debris, 0.31 + 0.03 * v),
        ],
        target=[22, 33, 28, 14, 3],
        bounds=[(0.04, 0.2), (0.2, 0.5), (0.05, 0.25), (0.12, 0.4), (0.01, 0.08)],
    )
    x = saturate(x / np.abs(x).max() * 1.1, 1.8)
    x = compress(x, -16, 3.0, 0.002, 0.1)
    x = lp(hp(x, 40), 11000)
    x = reverb(x, room(rng, 0.6, 3200), 0.16)
    x = level_lows(x, 0.25, 80)
    # The groan's formants crowd the mids: give the low-mids their body back.
    x = level_band(x, 2, 0.34, 1200)
    x = level_band(x, 1, 0.26, 320)
    return finish(x, rms_db=-22.6, peak_db=-2.0)


def step_rock(lib: dict, v: int, rng) -> np.ndarray:
    """A suited boot on planet rock (six variants, several a second at a
    run): the heel strike, gravel crunching under it and a faint armour tick."""
    length = n(0.14)
    tune = [1.0, 0.95, 1.05, 0.98, 1.03, 0.92][v]

    boot = lib["footstep_concrete"][v % 5]
    heel = hp(gate(pitch(boot, 1.05 * tune), 0.07, 0.04), 60)

    grit = grains(rng, length, 45, 0.06, 1500, 6500, size=(0.002, 0.006), decay=0.04)

    metal = lib["impactMetal_light"][v % 5]
    tick = hp(gate(pitch(metal, 1.3 * tune), 0.012, 0.02), 3000)

    thump = osc(glide(n(0.05), 120 * tune, 70, 0.04), "sine") * env(n(0.05), 0.001, 0.045)

    x, _ = balance(
        [place(length, heel), place(length, grit, 0.004), place(length, tick, 0.002), place(length, thump)],
        target=[18, 46, 21, 12, 3],
        bounds=[(0.45, 0.85), (0.04, 0.25), (0.01, 0.06), (0.02, 0.15)],
    )
    x = saturate(x / np.abs(x).max(), 1.5)
    x = compress(x, -14, 3.0, 0.001, 0.04)
    x = peak_eq(x, 3000, -2.5, 1.0)  # constant while running: keep it soft
    x = lp(hp(x, 50), 10000)
    x = reverb(x, room(rng, 0.2, 3000, 0.6), 0.1)
    x = level_lows(x, 0.2, 90)
    return finish(x, rms_db=-20.8, peak_db=-4.0)


def pickup_xp(lib: dict, v: int, rng) -> np.ndarray:
    """Collecting an XP shard (often several a second): a small crystal tink
    on a pentatonic note, so a stream of pickups sounds musical, with a
    glassy click and a faint sparkle."""
    length = n(0.22)
    # E6, G6, A6: neighbouring pickups land on consonant notes.
    f = [1318.5, 1568.0, 1760.0][v]

    glass = lib["impactGlass_light"][[0, 3, 4][v]]
    tink = hp(gate(pitch(glass, 1.7), 0.04, 0.05), 1500)

    # A crystal bar's inharmonic partials, the higher ones dying first.
    s = np.arange(length) / RATE
    chime = np.zeros(length)
    for ratio, amp, decay in [(1.0, 1.0, 0.16), (2.76, 0.45, 0.07), (5.40, 0.2, 0.035)]:
        chime += np.sin(2 * np.pi * f * ratio * s) * amp * env(length, 0.0008, decay)
    warmth = np.sin(2 * np.pi * f / 2 * s) * env(length, 0.002, 0.05)

    sparkle = grains(rng, length, 18, 0.1, 6000, 12000, size=(0.001, 0.003), decay=0.06)

    x, _ = balance(
        [place(length, l) for l in (chime, tink, warmth, sparkle)],
        target=[1, 6, 50, 36, 7],
        bounds=[(0.35, 0.75), (0.1, 0.4), (0.02, 0.12), (0.01, 0.06)],
    )
    x = saturate(x / np.abs(x).max(), 1.3)
    x = peak_eq(x, 4000, -2.0, 1.0)  # a stream of these must not sting
    x = lp(hp(x, 300), 13000)
    x = reverb(x, room(rng, 0.3, 6000, 0.6), 0.14)
    return finish(x, rms_db=-21.0, peak_db=-2.0)


def skitter_die(lib: dict, v: int, rng) -> np.ndarray:
    """A skitter popping: a light shell crunch, a short insect squeal, legs
    rattling, a small wet splat and the body ticking down."""
    length = n(0.32)
    tune = [1.0, 1.08, 0.93][v]

    crunch = lib["explosionCrunch"][2]
    crack = hp(gate(pitch(crunch, 1.9 * tune), 0.04, 0.04), 400)

    # Squeal: a high, fast voice falling from "ee" to "eh".
    squeal_len = n(0.16)
    s = np.arange(squeal_len) / RATE
    hz = 760 * tune * (0.55 ** (s / 0.16)) * (1 + 0.08 * np.sin(2 * np.pi * 38 * s))
    squeal = formant_voice(rng, squeal_len, hz, [(320, 2400, 3200), (520, 1850, 2700)], 0.35)
    squeal *= env(squeal_len, 0.004, 0.15, 0.9)

    legs = grains(rng, length, 40, 0.18, 1800, 7000, size=(0.001, 0.004), decay=0.08)

    slime = lib["slime"][1]
    start = n(1.4 + 0.7 * v)
    splat = bp(pitch(slime[start : start + n(0.1)], 1.2 * tune), 600, 5000)
    splat *= env(len(splat), 0.003, 0.08)

    body = lib["impactSoft_medium"][v + 1]
    tap = hp(gate(pitch(body, 1.8 * tune), 0.04, 0.04), 120)

    x, _ = balance(
        [
            place(length, crack),
            place(length, squeal, 0.006),
            place(length, legs, 0.01),
            place(length, splat, 0.004),
            place(length, tap, 0.07),
        ],
        target=[12, 26, 33, 23, 6],
        bounds=[(0.08, 0.3), (0.12, 0.4), (0.03, 0.15), (0.05, 0.25), (0.05, 0.25)],
    )
    x = saturate(x / np.abs(x).max() * 1.1, 1.8)
    x = compress(x, -14, 3.0, 0.001, 0.06)
    x = lp(hp(x, 70), 12000)
    x = reverb(x, room(rng, 0.35, 3800, 0.8), 0.12)
    x = level_lows(x, 0.14, 100)
    return finish(x, rms_db=-22.6, peak_db=-2.0)


# --- Sonniss recordings ----------------------------------------------------------------

# Recordings from the Sonniss #GameAudioGDC 2023 bundle (royalty-free for
# games; the raw files are not redistributed here). `--fetch-sources`
# downloads exactly these into `sources/sonniss/` (git-ignored) from the
# Internet Archive copy of the bundle, checking each file's MD5.
SONNISS_DIR = Path(__file__).resolve().parent / "sources" / "sonniss"
SONNISS_URL = "https://archive.org/download/gdc-2023-game-audio-bundle/"
SONNISS_BUNDLE = "Sonniss.com - GDC 2023 - Game Audio Bundle"
SONNISS = {
    "Heavyweapon12 Shot": ("David Dumais Audio - Sci-Fi Weapons Pack 1/SCIWeap_Heavyweapon12 Shot 04_DDUMAIS_NONE.flac", "ede737b9f2166fb03f6ce36070d19c33"),
    "Shooting BB Rifle": ("Sonic Bat - BB Rifle/SBbbr_Shooting BB Rifle 003.flac", "3827655e630fb2139414a8b4ed92c609"),
    "Industrial drill 01": ("Pole Position Production - The Electromagnetic Library/Electric motor - Industrial drill 01 - Drone - Priezor.flac", "1d75d5e0a2d5842cbf58572c0cfdd487"),
    "Hitting Chisel With Hammer": ("344 Audio - Nuts And Bolts/Hitting Chisel With Hammer Close.flac", "9b4005ecf390082ddd1f8b4552969ab4"),
    "Small Crushed Rock": ("Pole Position Production - The Stone & Sand Library/Small Crushed Rock - DROP - Thick - Stereo.flac", "3722306d74cd40d38d66a39e327d02d1"),
}
_recordings: dict = {}


def fetch_sources() -> None:
    import hashlib
    import urllib.parse
    import urllib.request

    SONNISS_DIR.mkdir(parents=True, exist_ok=True)
    for path, md5 in SONNISS.values():
        dest = SONNISS_DIR / Path(path).name
        if dest.exists() and hashlib.md5(dest.read_bytes()).hexdigest() == md5:
            continue
        url = SONNISS_URL + urllib.parse.quote(f"{SONNISS_BUNDLE}/{path}")
        print("fetching", dest.name)
        urllib.request.urlretrieve(url, dest)
        if hashlib.md5(dest.read_bytes()).hexdigest() != md5:
            dest.unlink()
            raise SystemExit(f"checksum mismatch for {dest.name}")


def rec(key: str) -> np.ndarray:
    """A Sonniss recording by its key in `SONNISS`."""
    if key not in _recordings:
        path = SONNISS_DIR / Path(SONNISS[key][0]).name
        if not path.exists():
            raise SystemExit(f"missing {path.name}: run with --fetch-sources first")
        _recordings[key] = load(path)
    return _recordings[key]


def slices(x: np.ndarray, count: int, length: float, min_gap: float = 0.08, pre: float = 0.002):
    """The `count` strongest separate hits in a long recording, each cut from
    just before its onset."""
    e = np.abs(x)
    smooth = np.convolve(e, np.ones(n(0.002)) / n(0.002), "same")
    slow = np.convolve(e, np.ones(n(0.05)) / n(0.05), "same")
    rising = (smooth > 3 * slow + 0.01) & (smooth > 0.05 * smooth.max())
    onsets, last = [], -n(1)
    for i in np.flatnonzero(rising):
        if i - last > n(min_gap):
            onsets.append(i)
        last = i
    scored = sorted(onsets, key=lambda i: -smooth[i : i + n(0.02)].max())
    chosen = []
    for i in scored:
        if all(abs(i - j) > n(length) for j in chosen):
            chosen.append(i)
        if len(chosen) == count:
            break
    out = []
    for i in sorted(chosen):
        seg = x[max(0, i - n(pre)) : i + n(length)].copy()
        seg /= np.abs(seg).max() + 1e-9
        out.append(seg)
    while len(out) < count:
        out.append(out[len(out) % max(1, len(out))])
    return out


def onset(x: np.ndarray, thresh: float = 0.1) -> np.ndarray:
    """Trim leading silence of a one-shot."""
    i = int(np.argmax(np.abs(x) > thresh * np.abs(x).max()))
    return x[max(0, i - n(0.002)) :]


def mixdown(length, layers):
    """Layers as (signal, start seconds, gain in dB relative to the first)."""
    out = np.zeros(length)
    for x, at, db in layers:
        y = place(length, x, at)
        rms = np.sqrt(np.mean(x[: n(0.1)] ** 2)) + 1e-9
        out += y / rms * 10 ** (db / 20)
    return out / (np.abs(out).max() + 1e-9)


CENTRES = [70, 260, 1000, 3500, 9000]


def tone(x, target, steps: int = 14, tolerance: float = 0.03):
    """Gentle match EQ: nudge each band's share of the energy toward `target`
    (percentages per band of `BANDS`) with broad peaking filters."""
    target = np.asarray(target, float) / np.sum(target)
    for _ in range(steps):
        e = band_energy(x)
        share = e / e.sum()
        worst = int(np.argmax(np.abs(share - target)))
        if abs(share[worst] - target[worst]) <= tolerance:
            break
        gain = float(np.clip(10 * np.log10((target[worst] + 1e-4) / (share[worst] + 1e-4)), -2.0, 2.0))
        x = peak_eq(x, CENTRES[worst], gain, 0.8)
    return x


def tight(x, thresh=0.25):
    """Start right at the transient (no slow lead-in)."""
    return onset(x, thresh)


def master(x, rng, drive=1.6, comp=(-14, 3.0), tail=(0.35, 3500, 0.12), hp_hz=40, lp_hz=12000):
    x = saturate(x * 1.05, drive)
    x = compress(x, comp[0], comp[1], 0.0015, 0.06)
    x = lp(hp(x, hp_hz), lp_hz)
    return reverb(x, room(rng, tail[0], tail[1]), tail[2])


# --- Sonniss-based recipes (chosen by ear) -----------------------------------------

def blaster(lib: dict, v: int, rng) -> np.ndarray:
    """Heavy pulse: a heavy energy weapon pitched up for rapid fire, with a
    BB rifle's air snap for a tight mechanical front."""
    length = n(0.42)
    tune = [1.0, 1.05, 0.96, 1.02][v]
    heavy = tight(rec("Heavyweapon12 Shot"))
    heavy = gate(pitch(heavy, 1.45 * tune), 0.24, 0.12)
    snap = onset(rec("Shooting BB Rifle"))
    snap = hp(gate(pitch(snap, 1.1), 0.05, 0.04), 300)
    x = mixdown(length, [(heavy, 0, 0), (snap, 0, -5)])
    x = peak_eq(master(x, rng, 1.7, (-15, 3.5)), 2800, -2.0, 1.0)
    x = tone(x, [18, 36, 22, 16, 8])
    return finish(level_lows(x, 0.2), rms_db=-23.0, peak_db=-2.0)


def mine_hit(lib: dict, v: int, rng) -> np.ndarray:
    """Drill bite: a burst of an industrial drill motor biting in, a chisel
    tick and crumbling grit."""
    length = n(0.24)
    motor = rec("Industrial drill 01")
    start = n(3.0 + 4.1 * v)
    burst = motor[start : start + n(0.2)].copy()
    burst = burst / (np.abs(burst).max() + 1e-9) * env(len(burst), 0.004, 0.18)
    strike = slices(rec("Hitting Chisel With Hammer"), 4, 0.08)[(v + 2) % 4]
    strike = hp(gate(strike, 0.04, 0.04), 800)
    grit = slices(rec("Small Crushed Rock"), 4, 0.15)[v]
    grit = hp(gate(grit, 0.1, 0.06), 400)
    x = mixdown(length, [(burst, 0, -2), (strike, 0, 0), (grit, 0.01, -5)])
    x = master(x, rng, 1.7, (-14, 3.0), (0.25, 3500, 0.1))
    x = tone(x, [10, 25, 40, 17, 8])
    return finish(level_lows(x, 0.2, 90), rms_db=-22.8, peak_db=-3.0)


# Remastered sounds, in the order they were redone. Each writes `name.wav`
# and `name.v2.wav` onward (four variants unless `VARIANT_COUNT` says).
REMASTERED = {
    "blaster": blaster,
    "enemy_hit": enemy_hit,
    "mine_hit": mine_hit,
    "block_break": block_break,
    "enemy_die": enemy_die,
    "step_rock": step_rock,
    "pickup_xp": pickup_xp,
    "skitter_die": skitter_die,
}

# Seeds of sounds picked from auditioned takes, so they render exactly as
# heard; other sounds seed from their name.
SEEDS = {
    "blaster": lambda v: 7000 + 100 * v + ord("C"),
    "mine_hit": lambda v: 7000 + 100 * v + ord("B"),
}

# Variants per sound (default `VARIANTS`).
VARIANT_COUNT = {"step_rock": 6, "pickup_xp": 3, "skitter_die": 3}

SOURCE_SETS = [
    "laserSmall",
    "impactPunch_heavy",
    "impactSoft_medium",
    "slime",
    "impactMining",
    "impactMetal_light",
    "explosionCrunch",
    "footstep_concrete",
    "impactSoft_heavy",
    "impactGlass_light",
]


def variant_path(out: Path, name: str, v: int) -> Path:
    return out / f"{name}{'' if v == 0 else f'.v{v + 1}'}.wav"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("names", nargs="*", help="remastered sounds to write")
    parser.add_argument("--all", action="store_true", help="write every remastered sound")
    parser.add_argument("--out", type=Path, default=SOUNDS, help="output directory")
    parser.add_argument("--fetch-sources", action="store_true", help="download the Sonniss recordings first")
    args = parser.parse_args()
    if args.fetch_sources:
        fetch_sources()
    names = list(REMASTERED) if args.all else args.names
    if not names:
        print("Remastered sounds:", ", ".join(REMASTERED))
        return
    unknown = [x for x in names if x not in REMASTERED]
    if unknown:
        parser.error(f"not remastered: {', '.join(unknown)}")
    lib = {s: [load(f) for f in sorted(SOURCES.rglob(f"{s}_*.ogg"))] for s in SOURCE_SETS}
    args.out.mkdir(parents=True, exist_ok=True)
    for name in names:
        for v in range(VARIANT_COUNT.get(name, VARIANTS)):
            seed = SEEDS.get(name, lambda v: 1000 * len(name) + v)(v)
            rng = np.random.default_rng(seed)
            path = variant_path(args.out, name, v)
            save(path, REMASTERED[name](lib, v, rng))
            # A draft variant in Ogg would shadow or duplicate the new WAV.
            path.with_suffix(".ogg").unlink(missing_ok=True)
            print("wrote", path.relative_to(ROOT) if path.is_relative_to(ROOT) else path)


if __name__ == "__main__":
    main()
