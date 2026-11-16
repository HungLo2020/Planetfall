# Basalt integration

Planetfall follows [Basalt Game Standard v1](https://github.com/HungLo2020/Basalt/blob/main/STANDARDS.md)
with no layout or action overrides, so it needs only the minimal catalog entry.

## Catalog entry

[`planetfall.json`](../planetfall.json) at the repository root is the entry to
copy into `Basalt/resources/games/`. Try it locally first by copying it to
`~/.config/basalt/games/` (see the standard's section 10). It parses and
validates with Basalt's own `basalt-core::GameDefinition`.

```json
{
  "schema_version": 2,
  "standard_version": 1,
  "id": "Planetfall",
  "name": "Planetfall",
  "source": { "kind": "github_release", "repository": "HungLo2020/Planetfall",
              "asset_prefix": "Planetfall-", "asset_suffix": ".zip", "asset_contains": ["linux-x64"] },
  "launch": { "executable": "app/bin/planetfall" },
  "artwork": { "cover_url": "https://raw.githubusercontent.com/HungLo2020/Planetfall/main/assets/basalt/cover.png" }
}
```

## Install layout

```
<library>/Planetfall/
├── app/                          from the release zip; never written at runtime
│   ├── bin/planetfall
│   ├── assets/{models,kenney,sounds,fonts}
│   ├── basalt/{cover.png,icon.svg}
│   └── README.txt
└── data/
    ├── save/profile.json             progress            (synced, backed up)
    ├── config/shared/preferences.json volumes, shake     (synced, backed up)
    ├── config/display.json            window mode/size   (backed up)
    └── cache/                         currently unused   (disposable)
```

`src/storage.rs` resolves these through `quasar_storage::GamePaths`:
`BASALT_*` variables win; otherwise paths are relative to the install root
found from `app/bin/planetfall`; a source checkout uses `data/` beside
`Cargo.toml` (git-ignored). Directories are created on demand, so an empty or
missing `data/` starts cleanly. Defaults live in code, so nothing is copied
from `app/`.

Save rules:

- Writes are atomic (temp file + rename, via `quasar_storage::write_json`).
- The profile is saved whenever it changes (purchase, extraction, death,
  travel, tutorial completion) and on exit. Mid-drop state is never banked.
- `Profile.version` gates migrations; a newer save is never overwritten and the
  session runs without saving. An unreadable save is copied aside as
  `profile.corrupt-<time>.json` before starting fresh.
- Saved files contain no absolute paths or machine identifiers.

## Release packaging and publishing

`python3 DevUtils/package.py` builds `dist/Planetfall-<version>-linux-x64.zip`
containing only `app/` (no wrapper folder, so the catalog needs no
`archive_root`), with the executable bit set, and validates both
(`package.py --check ZIP` validates an existing zip).

Publishing follows the same rolling model as the other Basalt games. Basalt
installs from the repository's `releases/latest`, so there is one GitHub
Release, tagged and named `Latest`, whose single zip is replaced in place:

1. Push to `main`; the **Check** workflow (`ci.yml`) must pass for that commit.
2. Dispatch **Release** (`release.yml`, manual only) with that full SHA:
   `gh workflow run release.yml --ref main -f commit=<sha>`.
3. The workflow refuses a SHA that is not the current `main` head or lacks a
   green Check run, runs the tests, packages, uploads a workflow artifact, and
   runs `DevUtils/release.py`. That script uploads the new zip under a temporary
   name, checks its size and SHA-256 digest, only then deletes the old zip,
   moves only the `Latest` tag to the commit and marks the release latest.

The `commit-and-release` skill (`.agents/skills/`) walks an agent through the
whole sequence and the post-release download check. `DevUtils/test_release.py`
exercises `release.py` against an in-memory GitHub API (no network); it runs in
`verify.py` and CI.

The binary links only libc, libm, libgcc_s, libudev (controllers) and libasound
(audio) and loads the Vulkan loader at runtime, all part of a standard desktop
Linux install. There is no built-in updater.

## Compliance checklist

**Layout and data**
- [x] The release archive contains only `app/` at its root.
- [x] The game never writes to `app/` at runtime.
- [x] Progress is stored in `data/save/` (`$BASALT_SAVE_DIR`).
- [x] Machine-specific settings in `data/config/`; follow-the-player settings in `data/config/shared/`.
- [x] Rebuildable files would go in `data/cache/` (none yet).
- [x] Nothing is stored outside the install root.
- [x] Starts correctly with an empty or missing `data/`.

**Robustness**
- [x] Save files are written atomically.
- [x] Synced data has no absolute paths, machine IDs or lock files.
- [x] Saves from older versions load (versioned `Profile::migrate`).
- [x] Works from any install path (`BASALT_*` with relative fallbacks).

**Packaging and launch**
- [x] GitHub Release `.zip` assets with predictable, platform-tagged names (rolling `Latest` release).
- [x] Self-contained; execute bit set.
- [x] No built-in updater.
- [x] Starts without arguments, terminal input or network access.
- [x] Exits with 0 on a normal quit (non-zero with a message on errors).
- [x] Controller support (gilrs through `quasar-input`).

**Artwork**
- [x] `app/basalt/cover.png`: 3:4, 900x1200, under 2 MB.
- [x] `app/basalt/icon.svg`.
- [x] `artwork.cover_url` points at the same cover.

**Catalog**
- [x] Entry with `schema_version: 2` and `standard_version: 1`.
- [ ] Tried locally with `basalt install` before adding to Basalt (needs a published release).
- [x] No `layout` or `actions` overrides.
