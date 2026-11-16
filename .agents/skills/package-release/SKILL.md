---
name: package-release
description: Verify Planetfall and build its Basalt release zip (dist/Planetfall-X.Y.Z-linux-x64.zip). Use when asked to package, cut a release, bump the version, or check Basalt compliance. Never publishes or pushes on its own (see commit-and-release).
compatibility: Requires cargo, python3 and a Vulkan-capable GPU for the cover/screenshot steps.
metadata:
  author: Planetfall
  version: "1.0"
---

# Package a release

Keep the steps in order. Stop where the user asked you to stop.

1. **Version.** Bump `[workspace.package].version` in the root `Cargo.toml`
   (third number by default). Crate manifests inherit it.
2. **Verify.** `python3 DevUtils/verify.py --gpu`. Inspect
   `dist/screenshots/*.png` for visual regressions.
3. **Cover.** If visuals changed, regenerate the cover and check it reads at
   tile size (3:4, 900x1200, under 2 MB):
   `cargo run --release -- --cover assets/basalt/cover.png`.
4. **Package.** `python3 DevUtils/package.py`. The script validates that the zip
   root holds only `app/`, that `app/bin/planetfall` is executable and that the
   cover is under 2 MB.
5. **Smoke-test the install layout.** Unzip into an empty directory and run
   `app/bin/planetfall --headless --autoplay --ticks 600` there, then a short
   windowed `--no-audio --frames 120` run. Confirm `data/` appears beside `app/`
   and nothing is written inside `app/`.
6. **Publish (only when asked).** Use the
   [commit-and-release](../commit-and-release/SKILL.md) skill: it commits,
   pushes, waits for CI, dispatches the manual Release workflow and verifies the
   rolling `Latest` release. The catalog entry is `planetfall.json`; Basalt's
   maintainer copies it into `Basalt/resources/games/`.

See [docs/basalt.md](../../../docs/basalt.md) for the full compliance checklist.
