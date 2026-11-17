#!/usr/bin/env python3
"""Run Planetfall's checks: formatting, lints, tests and a headless soak.

    python3 DevUtils/verify.py          # fmt, clippy, tests, release tooling, autopilot soak
    python3 DevUtils/verify.py --gpu    # also render offscreen screenshots (needs a GPU)

Screenshots land in dist/screenshots/. Nothing is published or installed.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
# Ticks chosen to land on distinct phases with the default headless seed.
# Tick at which the gear tab is shown (stash well stocked). Find ticks with
# `--headless --autoplay --trace`.
MENU_TICKS = 60540

SHOTS = {
    "ship": 150,
    "ftl": 300,
    "descent": 660,
    "surface": 1800,
    "construct": 2600,
    "board": 2664,
    "levelup": 3487,
    "surge": 25200,
    "debrief": 26271,
    "build": 26400,
    "locker": 26610,
    # Later worlds on seeds whose autopilot reaches them: (seed, ticks).
    "tidewell": (5, 109500),
    "kharon": (9, 147000),
}


def step(title: str, command: list[str]) -> str:
    print(f"==> {title}: {' '.join(command)}", flush=True)
    result = subprocess.run(command, cwd=REPO, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        sys.exit(f"FAILED: {title}")
    return result.stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--gpu", action="store_true", help="also render offscreen screenshots")
    parser.add_argument("--soak-ticks", type=int, default=60 * 60 * 30)
    args = parser.parse_args()

    step("format", ["cargo", "fmt", "--all", "--", "--check"])
    step("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"])
    step("tests", ["cargo", "test", "--workspace", "--locked", "--release"])
    step("release tooling (offline)", [sys.executable, "-m", "unittest", "-q", "DevUtils.test_release"])
    step("sim without graphics", ["cargo", "build", "--locked", "--no-default-features"])
    out = step(
        "headless soak",
        ["cargo", "run", "--release", "--locked", "--", "--headless", "--autoplay", "--ticks", str(args.soak_ticks)],
    )
    print(out.strip())
    extractions = int(re.search(r"extractions=(\d+)", out).group(1))
    if extractions < 2:
        sys.exit("FAILED: autopilot extracted fewer than 2 times during the soak")

    if args.gpu:
        shots = REPO / "dist" / "screenshots"
        shots.mkdir(parents=True, exist_ok=True)
        for name, at in SHOTS.items():
            seed, ticks = at if isinstance(at, tuple) else (1, at)
            step(
                f"screenshot {name}",
                ["cargo", "run", "--release", "--locked", "--", "--headless", "--autoplay", "--seed", str(seed),
                 "--ticks", str(ticks), "--screenshot", str(shots / f"{name}.png")],
            )
        for menu in ["main", "saves", "settings", "controls"]:
            step(
                f"screenshot menu-{menu}",
                ["cargo", "run", "--release", "--locked", "--", "--headless", "--ticks", "10", "--menu", menu,
                 "--screenshot", str(shots / f"menu-{menu}.png")],
            )
        # The locker's gear tab and the station market, opened on the ship
        # once the autopilot has found some gear.
        for menu in ["gear", "trade"]:
            step(
                f"screenshot {menu}",
                ["cargo", "run", "--release", "--locked", "--", "--headless", "--autoplay", "--ticks",
                 str(MENU_TICKS), "--menu", menu, "--screenshot", str(shots / f"{menu}.png")],
            )
        step("cover", ["cargo", "run", "--release", "--locked", "--", "--cover", str(shots / "cover.png")])
        print(f"screenshots in {shots.relative_to(REPO)}")
    print("all checks passed")


if __name__ == "__main__":
    main()
