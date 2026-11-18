# DevUtils

Development scripts. All are Python 3 using only the standard library. Run them from the repository root.

| Script | Purpose |
|---|---|
| `verify.py` | `cargo fmt --check`, clippy (`-D warnings`), all tests, the offline release-tooling tests, a no-graphics build and a headless autopilot soak. `--gpu` also renders screenshots of each phase (ship, FTL, descent, surface, debrief, module construction, build mode) and the cover into `dist/screenshots/`. |
| `package.py` | Builds the Basalt release zip `dist/Planetfall-<version>-linux-x64.zip` and validates its layout. `--skip-build` reuses `target/release/planetfall`; `--check ZIP` only validates. |
| `release.py` | Run by the Release workflow: checks CI for the commit and replaces the zip on the rolling `Latest` GitHub Release. Needs `GH_TOKEN`; refuses to run off `main` or on a dirty checkout. |
| `test_release.py` | Offline tests for `release.py` against a fake GitHub API (`python3 -m unittest DevUtils.test_release`). |
| `assets/generate_models.py` | Regenerates every GLB in `assets/models/` (deterministic). |
| `assets/generate_sounds.py` | Regenerates every WAV in `assets/sounds/` (deterministic). |
| `assets/remaster_sounds.py` | Designed replacements for drafted sounds, built from synthesis, the CC0 recordings in `assets/sources/kenney/` and Sonniss GDC recordings (`--fetch-sources` downloads those into the git-ignored `assets/sources/sonniss/`); lists the remastered sounds, or rewrites the named ones (needs numpy, scipy and ffmpeg). |

The cover art is rendered by the game itself:
`cargo run --release -- --cover assets/basalt/cover.png`.
