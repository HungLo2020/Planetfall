#!/usr/bin/env python3
"""Build a Basalt-standard release archive.

Produces dist/Planetfall-<version>-linux-x64.zip whose root contains only
app/ (see Basalt STANDARDS.md section 3):

    app/bin/planetfall          release executable (mode 755)
    app/assets/...              content data, models, sounds, fonts and licenses
    app/basalt/cover.png        3:4 box art
    app/basalt/icon.svg         square emblem
    app/README.txt              runtime notes

The script never publishes anything. Publishing is the manual Release
workflow (.github/workflows/release.yml), which runs DevUtils/release.py.

Usage:
    python3 DevUtils/package.py [--binary PATH] [--skip-build] [--out dist]
    python3 DevUtils/package.py --check dist/Planetfall-<version>-linux-x64.zip
"""

from __future__ import annotations

import argparse
import re
import shutil
import stat
import subprocess
import sys
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
ASSET_DIRS = ["data", "models", "kenney", "sounds", "fonts"]
MAX_COVER_BYTES = 2 * 1024 * 1024

RUNTIME_NOTES = """Planetfall {version}

Run app/bin/planetfall. No arguments, network access or terminal input are
needed. Requires a Vulkan 1.3 capable GPU driver.

Data lives beside this app/ directory (or wherever Basalt's BASALT_* variables
point):
  data/save/slot-1.json ... slot-5.json  save slots (synced by Basalt)
  data/config/shared/settings.toml        volumes and options (synced)
  data/config/display.toml                window and graphics (this machine)

Controls: WASD/left stick move, Space/(B) dash, E/(A) use, F/(Y) call
extraction, Esc/Start pause. Weapons aim and fire automatically.
Content and tuning are TOML in app/assets/data/.

Run `planetfall --help` for headless, screenshot and autopilot options.
Third-party asset licenses are in app/assets/kenney/*/License.txt and
app/assets/fonts/LICENSE-DejaVu.txt.
"""


def version() -> str:
    text = (REPO / "Cargo.toml").read_text()
    match = re.search(r'^\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"', text, re.M | re.S)
    if not match:
        sys.exit("could not read [workspace.package].version from Cargo.toml")
    return match.group(1)


def build() -> Path:
    print("building release binary...", flush=True)
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "planetfall"], cwd=REPO, check=True)
    return REPO / "target" / "release" / "planetfall"


def archive_errors(archive: Path, ver: str) -> list[str]:
    """Return every way `archive` breaks the Basalt release layout (empty when valid)."""
    errors: list[str] = []
    if archive.name != f"Planetfall-{ver}-linux-x64.zip":
        errors.append(f"unexpected asset name {archive.name}")
    with zipfile.ZipFile(archive) as z:
        if z.testzip() is not None:
            errors.append("archive is corrupt")
        names = z.namelist()
        roots = {n.split("/", 1)[0] for n in names}
        if roots != {"app"}:
            errors.append(f"archive root must contain only app/, found {sorted(roots)}")
        for name in names:
            parts = Path(name).parts
            if name.startswith("/") or ".." in parts:
                errors.append(f"unsafe path {name}")
            if "data" in parts[:2] or ".basalt" in parts[:2]:
                errors.append(f"user data in archive: {name}")
        for required in ("app/bin/planetfall", "app/basalt/cover.png", "app/basalt/icon.svg", "app/README.txt"):
            if required not in names:
                errors.append(f"missing {required}")
        if "app/bin/planetfall" in names:
            info = z.getinfo("app/bin/planetfall")
            if not (info.external_attr >> 16) & stat.S_IXUSR:
                errors.append("app/bin/planetfall is missing its execute bit")
        if "app/basalt/cover.png" in names and z.getinfo("app/basalt/cover.png").file_size > MAX_COVER_BYTES:
            errors.append("cover.png exceeds 2 MB")
    return errors


def validate(archive: Path, ver: str) -> None:
    errors = archive_errors(archive, ver)
    if errors:
        sys.exit("invalid release archive: " + "; ".join(errors))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", type=Path, help="use an existing executable instead of building")
    parser.add_argument("--skip-build", action="store_true", help="use target/release/planetfall as-is")
    parser.add_argument("--out", type=Path, default=REPO / "dist")
    parser.add_argument("--check", type=Path, metavar="ZIP", help="only validate an existing archive")
    args = parser.parse_args()

    ver = version()
    if args.check:
        validate(args.check, ver)
        print(f"{args.check.name}: valid Basalt release archive")
        return
    if args.binary:
        binary = args.binary
    elif args.skip_build:
        binary = REPO / "target" / "release" / "planetfall"
    else:
        binary = build()
    if not binary.is_file():
        sys.exit(f"missing executable {binary}")

    cover = REPO / "assets" / "basalt" / "cover.png"
    if not cover.is_file():
        sys.exit("missing assets/basalt/cover.png (render it with `planetfall --cover assets/basalt/cover.png`)")

    stage = args.out / f"Planetfall-{ver}-linux-x64"
    shutil.rmtree(stage, ignore_errors=True)
    app = stage / "app"
    (app / "bin").mkdir(parents=True)
    shutil.copy2(binary, app / "bin" / "planetfall")
    (app / "bin" / "planetfall").chmod(0o755)
    for name in ASSET_DIRS:
        shutil.copytree(REPO / "assets" / name, app / "assets" / name)
    shutil.copytree(REPO / "assets" / "basalt", app / "basalt")
    (app / "README.txt").write_text(RUNTIME_NOTES.format(version=ver))

    archive = args.out / f"Planetfall-{ver}-linux-x64.zip"
    archive.unlink(missing_ok=True)
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for path in sorted(stage.rglob("*")):
            rel = path.relative_to(stage).as_posix()
            if path.is_dir():
                info = zipfile.ZipInfo(rel + "/")
                info.external_attr = (stat.S_IFDIR | 0o755) << 16
                z.writestr(info, b"")
                continue
            info = zipfile.ZipInfo.from_file(path, rel)
            mode = 0o755 if rel == "app/bin/planetfall" else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, path.read_bytes())
    validate(archive, ver)
    size = archive.stat().st_size / (1024 * 1024)
    print(f"wrote {archive.relative_to(REPO) if archive.is_relative_to(REPO) else archive} ({size:.1f} MiB)")


if __name__ == "__main__":
    main()
