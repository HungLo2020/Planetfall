---
name: commit-and-release
description: Commit and push Planetfall's local changes to main, wait for CI on that commit, then dispatch the manual Release workflow and verify the rolling Latest GitHub Release holds the new Basalt zip. Use only when the user explicitly asks to commit/push and release, publish, or ship Planetfall.
compatibility: Requires git, cargo, python3 and an authenticated GitHub CLI (gh) with repo and workflow scope.
metadata:
  author: Planetfall
  version: "1.0"
---

# Commit, push and release

Ships the working tree as the Basalt build players install. Keep the order:
**verify → commit → push → CI green → dispatch Release → Release green →
verify what Basalt downloads**. Stop where the user asked you to stop (for
example "commit and push but don't release" ends after step 5).

Invoking this skill authorizes one normal commit on `main`, a normal push,
dispatching `release.yml`, and the rolling `Latest` release/tag moving to that
commit. It does not authorize force-pushes, amending pushed commits, version
tags, deleting other releases or tags, or touching any other repository.

How publishing works here (same model as the other Basalt games, e.g.
ClassicClassics): `planetfall.json` uses a `github_release` source, and Basalt
installs from `releases/latest`. There is exactly one release, tagged and named
`Latest`, which `DevUtils/release.py` replaces in place. The Release workflow is
`workflow_dispatch` only; it refuses commits that are not the current `main`
head or have no successful **Check** (`ci.yml`) run.

```text
- [ ] 1. Preflight
- [ ] 2. Verify locally
- [ ] 3. Version (optional)
- [ ] 4. Commit
- [ ] 5. Push
- [ ] 6. Wait for CI on the commit
- [ ] 7. Dispatch Release and watch it
- [ ] 8. Verify the published release
- [ ] 9. Report
```

## 1. Preflight

```sh
gh auth status
git status --short --branch
git fetch origin
git rev-list --left-right --count origin/main...HEAD   # behind / ahead
git diff --check
git log -5 --oneline
```

- The branch must be `main`. If it is behind `origin/main`, `git pull --ff-only`
  (stash nothing, discard nothing). If it diverged, stop and ask.
- Review every changed and untracked path. Never commit `target/`, `dist/`,
  `data/`, screenshots, logs, credentials, or anything outside the repo
  (`.gitignore` covers the first three; check anyway). If a change looks like
  unrelated user work, ask before including it.
- If there is nothing to commit and `HEAD == origin/main`, skip to step 6 and
  release the current head.

## 2. Verify locally

```sh
python3 DevUtils/verify.py --gpu      # fmt, clippy, tests, release tooling, soak, screenshots
python3 DevUtils/package.py           # builds and validates dist/Planetfall-<version>-linux-x64.zip
```

Look at `dist/screenshots/*.png` if anything visual changed. Without a Vulkan
GPU run `verify.py` without `--gpu` and say so in the report. Do not commit if a
check fails; fix it first. If visuals changed, regenerate the cover
(`cargo run --release -- --cover assets/basalt/cover.png`) so the committed
`assets/basalt/cover.png` (also the catalog's `cover_url`) matches the game.

## 3. Version (optional)

`[workspace.package].version` in `Cargo.toml` names the zip
(`Planetfall-<version>-linux-x64.zip`). Re-releasing the same version is
allowed (the asset is replaced). Bump it only when the user asks, or when they
ask for a "new version": third number by default, then refresh the lock with
`cargo check --workspace` and confirm only workspace versions changed in
`Cargo.lock`. If the save format changed, `PROFILE_VERSION` and
`Profile::migrate` must already be updated.

## 4. Commit

Stage reviewed paths explicitly (or `git add -A` after confirming
`git status --short` holds only intended files), then:

```sh
git diff --cached --stat
git commit -m "<type>(<scope>): <summary>"     # Conventional Commit, imperative
git show --stat --oneline HEAD
```

End the message with the attribution trailer the session specifies. Never amend
a commit that is already pushed; fixes are new commits.

## 5. Push

```sh
git push origin main
sha=$(git rev-parse HEAD)
git ls-remote origin refs/heads/main          # must print $sha
```

Never force-push. If the push is rejected because `main` moved, fetch, inspect
the incoming commits, `git pull --rebase` only if it applies cleanly, rerun
step 2 for the combined tree, and push again.

## 6. Wait for CI on the commit

Pushing to `main` starts **Check** (`.github/workflows/ci.yml`). Runs can take a
few seconds to appear:

```sh
gh run list --commit "$sha" --json databaseId,workflowName,status,conclusion,url
gh run watch <check-run-id> --exit-status
```

Watch in the background (Bash `run_in_background` or Monitor) rather than
polling by hand; CI takes several minutes. Every run for `$sha` must end
`completed` / `success`. On failure:

```sh
gh run view <run-id> --log-failed
```

Reproduce locally, fix the cause (never weaken or skip checks), then repeat from
step 2 with a new commit. A run that failed for infrastructure reasons (runner
outage, network) may be retried once with `gh run rerun <id> --failed`; say so.
If no Check run exists for a head you are re-releasing, dispatch one with
`gh workflow run ci.yml --ref main` and watch it.

## 7. Dispatch Release and watch it

Confirm `main` still points at `$sha`, note existing Release runs, then dispatch:

```sh
gh api repos/HungLo2020/Planetfall/branches/main --jq .commit.sha    # must equal $sha
gh workflow run release.yml --ref main -f commit="$sha"
gh run list --workflow release.yml --event workflow_dispatch --limit 5 \
  --json databaseId,displayTitle,status,conclusion,url
gh run watch <release-run-id> --exit-status
```

The new run's title is `Release Latest (<sha>)`. It rechecks CI, runs the tests,
builds and validates the zip, uploads a workflow artifact, then replaces the
`Latest` release asset (the old zip stays until the new one uploads and its
size and digest check out).

If it fails, read `gh run view <id> --log-failed`. Fix an understood cause
(code fixes go through steps 2 to 6 again with a new commit) and retry once.
A failed first publication can leave a draft `Latest`; the next run reuses it.
Stop and report an unresolved failure instead of repeatedly mutating releases.

## 8. Verify the published release

Success of the workflow alone is not proof. Check what Basalt will see:

```sh
gh api repos/HungLo2020/Planetfall/releases/latest \
  --jq '{tag: .tag_name, draft, prerelease, url: .html_url, assets: [.assets[] | {name, size, digest}]}'
gh api repos/HungLo2020/Planetfall/git/ref/tags/Latest --jq .object.sha      # must equal $sha
```

Require: tag `Latest`, not draft or prerelease, exactly one asset named
`Planetfall-<version>-linux-x64.zip` with nonzero size. Then download both
copies into fresh ignored directories and compare:

```sh
rm -rf target/release-check && mkdir -p target/release-check/{release,actions,install}
gh release download Latest --repo HungLo2020/Planetfall \
  --pattern 'Planetfall-*-linux-x64.zip' --dir target/release-check/release
gh run download <release-run-id> --repo HungLo2020/Planetfall \
  --name Planetfall-linux-x64 --dir target/release-check/actions
sha256sum target/release-check/{release,actions}/*.zip     # must match each other and the asset digest
python3 DevUtils/package.py --check target/release-check/release/Planetfall-*-linux-x64.zip
unzip -q target/release-check/release/*.zip -d target/release-check/install
(cd target/release-check/install && app/bin/planetfall --headless --autoplay --ticks 600)
ls target/release-check/install                            # app/ and data/ only
```

## 9. Report

- commit SHA, subject, and that `origin/main` points at it
- Check run and Release run: IDs, URLs, conclusions
- release URL, asset name, size and SHA-256, and the downloaded-asset checks
- anything skipped (for example `--gpu` without a GPU) and why

The catalog entry `planetfall.json` needs no change for a new build; Basalt's
maintainer copies it into `Basalt/resources/games/` once. Do not edit Basalt.
