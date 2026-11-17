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
SHOTS = {
    "ship": 150,
    "ftl": 300,
    "descent": 660,
    "surface": 2400,
    "debrief": 3060,
    "construct": 3165,
    "build": 5200,
    "locker": 5376,
    "levelup": 5714,
    "surge": 27560,
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
        for name, ticks in SHOTS.items():
            step(
                f"screenshot {name}",
                ["cargo", "run", "--release", "--locked", "--", "--headless", "--autoplay", "--ticks", str(ticks),
                 "--screenshot", str(shots / f"{name}.png")],
            )
        for menu in ["main", "saves", "settings", "controls"]:
            step(
                f"screenshot menu-{menu}",
                ["cargo", "run", "--release", "--locked", "--", "--headless", "--ticks", "10", "--menu", menu,
                 "--screenshot", str(shots / f"menu-{menu}.png")],
            )
        step("cover", ["cargo", "run", "--release", "--locked", "--", "--cover", str(shots / "cover.png")])
        print(f"screenshots in {shots.relative_to(REPO)}")
    print("all checks passed")


if __name__ == "__main__":
    main()
