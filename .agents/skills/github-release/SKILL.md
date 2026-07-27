---
name: github-release
description: >-
  End-to-end GitHub release workflow for CodeTerm. Use when asked to release,
  ship, tag, publish, bump a version, trigger the self-hosted build machine,
  monitor release CI, produce signed/notarized macOS/Linux/Windows artifacts,
  update the public updater mirror, or debug the CodeTerm release pipeline.
---


# End-to-end CodeTerm release to GitHub

**Default now = one `release-X.Y.Z` tag on master; the self-hosted runners do the rest** (see
"Default release path" right below). The pipeline: bump version on a release branch/develop →
sync versions → merge to master by PR → tag the master commit with `release-X.Y.Z` → build
signed+notarized macOS (arm64+x86_64) + Linux (arm64+x86_64) + Windows (arm64+x86_64) → upload
to private repo (source) → mirror artifacts + signed `latest.json` to public repo (so the GUI
updater works) → (optional) update Homebrew cask. The manual per-platform / multi-machine steps
further down are the fallback for when the runners are down.

**Golden rule: all builds happen from the tagged commit on master. Never build release artifacts from develop.**

---

## ⚡ Default release path: single trigger tag (automated, self-hosted CI)

The whole pipeline runs itself on the **self-hosted runners**: Mac `codeterm-builder` via
`.github/workflows/release.yml`, Windows `windows-build` via `release-windows.yml`, and WSL2
Linux `linux-build` via `release-linux.yml`. **Self-hosted runner minutes are free** — this is
how we release without burning GitHub-hosted minutes. You do NOT build by hand on multiple
machines anymore (that's the fallback at the bottom).

**Normal change flow:** first prove `develop` already contains `origin/master`, merge `master` into
`develop` if needed, verify the UI from the main checkout on `develop`, clean old lane worktrees,
then bump `package.json`, run `npm run sync:versions`, verify `npm run check:versions`, open a PR
from `develop` into `master`, and merge it only after required checks are green. Releases are cut
from the resulting `master` commit.

**Hard preflight before version bump / master PR:**

```bash
git fetch origin --prune
git checkout develop
git pull --ff-only origin develop
git merge-base --is-ancestor origin/master HEAD || {
  git merge --no-ff origin/master --no-edit
  # Resolve conflicts, run the gates below, commit, and push develop before bumping the version.
}

git status --short                  # must be clean
ls -la ../codeterm-worktrees         # should be empty or only explicitly preserved lanes
curl -s http://127.0.0.1:7686/api/health
```

Start dev from this main checkout and visually inspect the release UI before opening the release
PR. Do not cut a release from a lane worktree, a stale dev build, or a `develop` that is missing
commits already present on `master`.

**To cut a release — after the version bump PR lands on `master`, push only the trigger tag:**

```bash
# On master, at the merge commit from the release PR:
git checkout master
git pull --ff-only origin master
git rev-parse --abbrev-ref HEAD   # must print master
git tag release-X.Y.Z && git push origin release-X.Y.Z
```

Do **not** create or push a separate `vX.Y.Z` tag for the normal path. The workflows build from
the `release-X.Y.Z` tag and publish/stamp/map the GitHub releases as `vX.Y.Z` themselves.

Pushing `release-X.Y.Z` triggers the runners, which **fully automatically**:
1. preflight (Developer ID, notary, minisign key, docker images, VM, gh) →
2. builds **all platforms** across three self-hosted workflows: Mac `codeterm-builder` builds
   macOS x64+arm64 plus arm64 Linux in Docker; ROG `windows-build` builds Windows x64+arm64;
   ROG WSL2 `linux-build` builds x86_64 Linux natively →
3. **signs** on each runner and **notarizes** the macOS DMGs on the Mac →
4. **publishes** to private `rollacode/codeterm` + public mirror `rollacode/codeterm-releases`
   with a signed `latest.json`, marks `--latest` →
5. verifies anon updater URLs return 200 and the manifest version is live.

End-to-end ≈ 25–30 min, hands-off. Watch it: `gh run watch --repo rollacode/codeterm`.

**Prereqs for the automated path** (one-time, see `docs/build_machine_setup.md`):
- self-hosted runner registered + running (`~/actions-runner`; `./svc.sh install` for always-on);
- Docker image baked for arm64 Linux: `codeterm-build-linux-arm64`;
- ROG self-hosted runners provisioned (Windows `windows-build`, x86_64 Linux `linux-build`) — they
  build + sign themselves via `TAURI_SIGNING_PRIVATE_KEY` / `PACKAGE_REPOS_TOKEN`, no SSH/QEMU;
- macOS signing/notary + `~/.tauri/codeterm.key` on the build machine (secrets table below).

If a stage fails before publishing: `gh run view <id> --log-failed`, fix the runner/build-farm issue,
and rerun the same workflow/tag only when the tag still points at the intended master SHA. Do not
move or overwrite a published release tag/artifact by default. If a public bad release was deleted
intentionally, cut the correction as a new patch version through the normal PR flow, then push only
the new `release-X.Y.Z` trigger tag.

> **Everything below this line is the MANUAL FALLBACK** — only for when the runner/CI is down or
> you're debugging a single platform. The full from-scratch build-machine setup (Docker arm64
> Linux, the ROG Windows + x86_64-Linux runners, signing) lives in **`docs/build_machine_setup.md`**.

---

## Two-repo layout (since 2026-05-03)

- **`rollacode/codeterm`** — private source repo. Releases here for archive only; the GUI updater can't reach private-repo assets anonymously.
- **`rollacode/codeterm-releases`** — public mirror, artifacts + `latest.json` only. The Tauri-plugin-updater endpoint baked into `tauri.conf.json` points here. Without this mirror, `update install --auto` from the GUI hangs/404s.

## When to use

User says: «сделай релиз», «выпусти vX.Y.Z», «ship it», «bump version + release», «assemble the binaries and put on github».

For just **building locally** (no release), use `prod-build`. For **just** the macOS x86_64 cross-compile gotchas, read `references/macos-x86-64.md`. This skill ties all three targets + upload + brew in one flow.

## Agent report contract — machine-token first line

Every worker that finishes a step reports back via `codeterm agent report`. The orchestrator parses the FIRST LINE of the report as a structured event, never regex-greps the prose. Sourced from `scripts/lib/report.sh` which both mac and win helpers `source`:

| Outcome  | First line                                     |
| -------- | ---------------------------------------------- |
| Ready    | `WINDOWS_READY=<ver> sig=<base64>` (or similar key=val pairs) |
| Blocked  | `BLOCKED step=<name> err=<short>`              |

A blank line separates the token from free-form prose. Helpers:

- `report_ready KEY=VAL ...` — emits the kv line + `\n\n` + stdin prose
- `report_blocked STEP MSG` — emits `BLOCKED step=… err=…` + `\n\n` + stdin prose
- `validate_b64_sig RAW` — echoes the trimmed sig if it looks valid (≥200 chars, only b64 alphabet), exits non-zero otherwise. Wrap the output of any `*-bundle.sh` before reporting it.

Reason: v1.3.2 release wasted a round-trip because BLACKSTAR reported "sig: \<base64\>" inside prose and the mac orchestrator couldn't extract it without human relay. Machine token on line 1 fixes that class permanently.

## Release-pipeline secrets backup

Stored in Bitwarden vault (retrieve via `codeterm mem secret get --name <slug>` on any machine with BW unlocked):

| Bitwarden slug | Restore to | Notes |
|---|---|---|
| `codeterm-apple-notary-env` | `~/.claude/.secrets/apple-notary.env` (`chmod 600`) | Then `source <file>; xcrun notarytool store-credentials codeterm --apple-id $APPLE_ID --team-id $APPLE_TEAM_ID --password $APPLE_APP_SPECIFIC_PASSWORD` |
| `codeterm-tauri-minisign-private-key` | `~/.tauri/codeterm.key` (`chmod 600`) | Empty password. Used by `tauri signer sign` for updater artifacts |
| `codeterm-tauri-minisign-public-key` | `~/.tauri/codeterm.key.pub` (`chmod 644`) | Same value already baked into `src-tauri/tauri.conf.json::plugins.updater.pubkey` |

**NOT in the vault** (live only in macOS Login Keychain):
- `Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)` certificate + private key. Back up via: Keychain Access → find by name → File → Export as `.p12` with passphrase → save `.p12` + passphrase to mem secret as `codeterm-apple-developer-id-p12`.

`codeterm mem secret status` must show `unlocked`; `codeterm mem secret list` shows everything.

## Prereqs (verify once per machine)

- Apple signing identity `Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)` in login keychain. Check: `security find-identity -v -p codesigning | grep ANDREY`.
- Notary keychain profile `codeterm`. If stale (`xcrun notarytool history --keychain-profile codeterm` → HTTP 401), re-store from the env file:
  ```bash
  source ~/.claude/.secrets/apple-notary.env
  xcrun notarytool store-credentials codeterm \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD"
  ```
  Note the env var is `APPLE_APP_SPECIFIC_PASSWORD`, NOT `APPLE_PASSWORD`.
- `gh` logged in as `rollacode`: `gh auth status`.
- Windows builder mesh peer paired and online: `codeterm mesh peers`. Default = **BLACKSTAR** (Windows 11, has its own minisign key copy since 2026-05-18, workspace at `C:\Developer\codeterm`). Fallback = **ROG** (primary workstation, Windows 11, `100.125.250.76`, codeterm workspace at `C:\Users\amigo\Developer\codeterm`, workspace id `28fa3d8d`).
- **Windows builder prereqs** (verify on the chosen peer before invoking `win-release.sh`):
  - `gh auth status` shows `rollacode (keyring)` as **valid**, not just `r0llac0de (work)` as the active account. The work account has happened to retain push access to `rollacode/codeterm` (so uploads can succeed even when rollacode keyring is stale), but this is fragile — refresh via `gh auth login` if rollacode is invalid. *(v1.2.1 caught: rollacode keyring expired silently; r0llac0de was the actual writer.)*
  - `node --version` ≥ 22.12 (Vite 8 hard requirement). If lower, `nvm use 22.22.0` (or whichever 22.12+ is installed). nvm-windows shim is usually not on PATH by default — prepend the version dir manually if `nvm use` doesn't work.
  - Windows SDK matches what `build.sh` expects (default `SDK_VER=10.0.22621.0`). Check `ls "/c/Program Files (x86)/Windows Kits/10/Include"`. On ROG only `10.0.26100.0` is installed → build.sh has a local stash with the override; restore via `git stash pop` after `git checkout v$VER`. A clean ROG without that stash dead-ends on `sqlite-sys: malloc.h not found`.

## Steps

### Agent model

When delegating this skill to an agent (release coordinator or ROG worker), **spawn with `--model haiku`**. The pipeline is mostly orchestration + bash + git/gh — Haiku is fast and cheap enough; Sonnet/Opus is overkill and doubles wall time on thinking phases.

Spawn with `codeterm agent spawn claude --task "..." --model haiku`. Parent is inferred from `$CODETERM_PANE_ID`; reports route back to you automatically (Orchestrator + child-tab model, no `--role` flag needed).

### 0. Kill any running dev

Rust builds share the target dir — concurrent `tauri build` + `tauri dev` fight and one silently fails:

```bash
./build.sh kill
```

### 1. Bump version on develop/release branch (one commit)

```bash
git checkout develop
git fetch origin --prune
git pull --ff-only origin develop
git merge-base --is-ancestor origin/master HEAD

# If the merge-base check fails, merge origin/master into develop first and stop here until
# develop is pushed, green, and visually checked from the main checkout.

VER=1.2.0  # new
npm run sync:versions -- "$VER"
./build.sh check                      # refreshes Cargo.lock
npm run check:versions

# check:versions covers root package.json, src-tauri/Cargo.toml,
# cli/Cargo.toml, src-tauri/tauri.conf.json, provider plugin manifests,
# plugin package.json files, plugins/channel.json, and shared packages.
# Do not hand-edit just one version file.

git add package.json src-tauri/Cargo.toml src-tauri/Cargo.lock \
  src-tauri/tauri.conf.json cli/Cargo.toml plugins packages scripts
git commit -m "release: v$VER"
git push origin develop
```

Run these on the exact release head before opening the master PR:

```bash
pnpm build
CODETERM_API_PORT=7686 ./build.sh check
bash scripts/check-file-sizes.sh --fail
git diff --check
```

Then start dev from the main checkout and visually confirm the app:

```bash
./build.sh kill
codeterm tab new --cmd "cd $(pwd) && ./build.sh dev"
curl -s http://127.0.0.1:7686/api/health
```

Clean old orchestration state before the PR: close finished worker tabs, remove merged lane
worktrees under `../codeterm-worktrees/`, and leave only documented stashes/lane directories.
Run `scripts/merge-stack-audit.sh --target <release-head> <accepted-lane-ref>...`
before final QA; stale patch-landed refs or unlanded accepted lanes must be
merged, explicitly recorded as superseded, or deferred before the master PR.
Patch-equivalent stale refs are not ancestry proof and must not be silently
deleted without that superseded record.

### 2. Merge to master by PR + push the release trigger tag

**Always via PR on GitHub** — don't direct-push develop to master. This avoids merge-commit
collisions when multiple machines operate on master. The normal trigger is a single
`release-$VER` tag on the master merge commit.

```bash
# On GitHub: open PR develop → master, watch checks, and merge via "Create a merge commit".
gh pr checks <pr-number> --watch
gh pr view <pr-number> --json mergeable,mergeStateStatus,statusCheckRollup,headRefOid

# Then locally, tag the exact merged master commit:
git checkout master
git pull origin master --no-rebase   # get the merge commit GitHub created
INTENDED_MASTER_SHA=$(git rev-parse HEAD)
git ls-remote --tags origin "refs/tags/release-$VER" "refs/tags/v$VER"  # must be empty
test "$(git rev-parse origin/master)" = "$INTENDED_MASTER_SHA"
git tag "release-$VER" "$INTENDED_MASTER_SHA"
git push origin "release-$VER"
```

Do **not** create or push `v$VER` here. The release workflows create/update the private and
public GitHub releases under `v$VER` and map the updater metadata there.

### 2.1. Release closeout cleanup

After the workflows publish successfully and before archiving the plan:

```bash
git worktree list
find ../codeterm-worktrees -mindepth 1 -maxdepth 1 -type d -print
cargo sweep --time 2
```

Remove merged lane worktrees and delete their merged `lane/*` branches. Close finished worker tabs
and stop lane-specific dev daemons. Keep only documented stashes or explicitly preserved scratch
checkouts. Do not leave `src-tauri/target` or lane `target/` directories to grow silently between
releases; `cargo sweep --time 2` is the default cleanup because active build artifacts are touched
daily and stale layers otherwise accumulate into tens of GB.

If you need to merge post-release develop commits to master (docs, skills, etc.) after the tag:
```bash
git checkout master && git pull origin master --no-rebase
git merge --no-ff origin/develop --no-edit
git push origin master
```

If push to master is rejected: always `git pull origin master --no-rebase --no-edit` first — origin/master may already have a merge commit from a GitHub PR that your local doesn't have yet.

### 3. Manual fallback only: create GitHub release (draft)

Skip this in the normal automated path. The `release-$VER` workflows create/update `v$VER`.

```bash
gh release create v$VER --title "v$VER" --draft --notes "$(cat <<'NOTES'
## Changes
- ...
NOTES
)"
```

Create as draft first. Publish after all assets are uploaded.

### 4. Manual fallback only: checkout tag for building

**Critical: build from the tag, not from a branch.**

```bash
git checkout "release-$VER"
```

You're now in detached HEAD on the exact tagged commit. All builds below use this tree.

### 5+6. Build + notarize + pack updater bundles (both archs)

```bash
WORK=/tmp/codeterm-${VER}-mirror
mkdir -p "$WORK"
HELPERS=.agents/skills/github-release/scripts
ARM_SIG=$("$HELPERS/mac-bundle.sh" "$VER" arm64   "$WORK")
X64_SIG=$("$HELPERS/mac-bundle.sh" "$VER" x86_64 "$WORK")
```

`mac-bundle.sh` does the whole arch-pipeline per call:
1. `build-mac.sh` — sources `~/.claude/.secrets/apple-notary.env`, kills any running dev, runs the build, verifies the resulting `.app` was signed with `Developer ID Application` + `flags=0x10000(runtime)`. Failing this sanity check beats discovering an ad-hoc-signed binary half an hour into Apple's notarize queue. (Hard-learned 2026-05-17 v1.3.0.)
2. `notarize-dmg.sh` — submits, waits (quiet mode: dot-spam suppressed), staples, runs `spctl`. On rejection prints Apple's detailed issues to stderr.
3. `pack-mac-updater.sh` — `tar --no-mac-metadata` (required: without it Tauri's Rust tar lib materializes `._CodeTerm.app` AppleDouble shadows on extract and the updater rejects with "failed to unpack `._CodeTerm.app`"), Python-extract sanity check, minisign-sign. Echoes the base64 signature to stdout.

The captured `ARM_SIG` / `X64_SIG` go straight into latest.json below.

### Status dashboard (any time during the flow)

```bash
.agents/skills/github-release/scripts/release-status.sh "$VER"
```

Compact one-shot view of: private/public assets, latest.json manifest plat coverage, and per-platform CI run state. Replaces 3-4 separate `gh` queries.

Cross-compile detail (why this script exists): `references/macos-x86-64.md`.

### 7. Upload macOS DMGs to the private (source) repo

```bash
gh release upload v$VER "$ARM" "$X64" --clobber
```

### 7b. Build `latest.json` + mirror to public repo

The GUI updater fetches `latest.json` anonymously, so the manifest plus the asset copies live in `rollacode/codeterm-releases`. Same files, separate repo.

**⚠️ Format gotcha:** updater artifact names depend on `bundle.createUpdaterArtifacts`. CodeTerm currently uses Tauri v2 mode (`true`), not `"v1Compatible"`:

- macOS: `CodeTerm_aarch64.app.tar.gz` / `CodeTerm_x64.app.tar.gz` (the `.app` bundle wrapped in gzipped tar)
- Linux: `CodeTerm_<ver>_amd64.AppImage`
- Windows: `<name>_<ver>_x64-setup.exe` (the signed NSIS installer itself)

Pointing macOS `latest.json` at `.dmg` makes `codeterm update install --auto` fail with `invalid gzip header`. In Tauri v2 mode, Windows `.exe` and Linux `.AppImage` are valid updater URLs; `.nsis.zip` / `.AppImage.tar.gz` are the old v1-compatible artifact names.

**Preferred path:** tag-push CI builds each platform, uploads source artifacts to `rollacode/codeterm`, writes updater fragments whose URLs point at `rollacode/codeterm-releases`, then mirrors public assets and merged `latest.json`.

```bash
.agents/skills/github-release/scripts/release-status.sh "$VER"
```

**Manual mirror fallback:** use only when CI produced source assets but the public mirror failed (usually `PACKAGE_REPOS_TOKEN` 403 or a workflow script bug). This is what keeps the release shippable without rebuilding all platforms.

```bash
WORK=/tmp/codeterm-v${VER}-mirror
rm -rf "$WORK" && mkdir -p "$WORK"
gh release download "v$VER" --repo rollacode/codeterm --dir "$WORK" --clobber

ARM_SIG=$(tr -d '\n' < "$WORK/CodeTerm_aarch64.app.tar.gz.sig")
X64_SIG=$(tr -d '\n' < "$WORK/CodeTerm_x64.app.tar.gz.sig")
LINUX_SIG=$(tr -d '\n' < "$WORK/CodeTerm_${VER}_amd64.AppImage.sig")
WIN_SIG=$(tr -d '\n' < "$WORK/CodeTerm_${VER}_x64-setup.exe.sig")
PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
NOTES="See the assets to download this version and install."
cat > "$WORK/latest.json" <<JSON
{
  "version": "$VER",
  "notes": "$NOTES",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://github.com/rollacode/codeterm-releases/releases/latest/download/CodeTerm_aarch64.app.tar.gz",
      "signature": "$ARM_SIG"
    },
    "darwin-x86_64": {
      "url": "https://github.com/rollacode/codeterm-releases/releases/latest/download/CodeTerm_x64.app.tar.gz",
      "signature": "$X64_SIG"
    },
    "linux-x86_64": {
      "url": "https://github.com/rollacode/codeterm-releases/releases/latest/download/CodeTerm_${VER}_amd64.AppImage",
      "signature": "$LINUX_SIG"
    },
    "windows-x86_64": {
      "url": "https://github.com/rollacode/codeterm-releases/releases/latest/download/CodeTerm_${VER}_x64-setup.exe",
      "signature": "$WIN_SIG"
    }
  }
}
JSON
python3 -m json.tool "$WORK/latest.json" >/dev/null  # validate

if ! gh release view "v$VER" --repo rollacode/codeterm-releases >/dev/null 2>&1; then
  gh release create "v$VER" \
    --repo rollacode/codeterm-releases \
    --title "CodeTerm v$VER" \
    --notes "Auto-mirrored release artifacts for CodeTerm v$VER. Source: rollacode/codeterm v$VER." \
    --latest
fi

gh release upload "v$VER" --repo rollacode/codeterm-releases --clobber \
  "$WORK/CodeTerm_${VER}_aarch64.dmg" \
  "$WORK/CodeTerm_${VER}_x64.dmg" \
  "$WORK/CodeTerm_${VER}_amd64.AppImage" \
  "$WORK/CodeTerm_${VER}_amd64.AppImage.sig" \
  "$WORK/CodeTerm_${VER}_amd64.deb" \
  "$WORK/CodeTerm_${VER}_amd64.deb.sig" \
  "$WORK/CodeTerm_${VER}_x64-setup.exe" \
  "$WORK/CodeTerm_${VER}_x64-setup.exe.sig" \
  "$WORK/CodeTerm_aarch64.app.tar.gz" \
  "$WORK/CodeTerm_aarch64.app.tar.gz.sig" \
  "$WORK/CodeTerm_x64.app.tar.gz" \
  "$WORK/CodeTerm_x64.app.tar.gz.sig" \
  "$WORK/latest.json"

# Verify anon access and all updater URLs:
curl -sIL https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json \
  | grep -E '^HTTP' | tail -1     # -> HTTP/2 200
python3 - <<'PY'
import json, urllib.request
m=json.load(urllib.request.urlopen("https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json"))
for platform, entry in m["platforms"].items():
    req=urllib.request.Request(entry["url"], method="HEAD")
    print(platform, urllib.request.urlopen(req).status, entry["url"])
PY

# End-to-end smoke: a freshly-installed v$VER- daemon should pick this up.
# Run on the build host (after self-installing v$VER- via codeterm self-update or DMG):
codeterm update check       # → 'Update available!' OR 'Up to date' (latest manifest is current)
# On the previous-version host (still on v<previous>):
codeterm update install --auto     # → must NOT fail with 'invalid gzip header'.
                            # If it does, the manifest is still wrong — fix and re-publish before continuing.
```

If the Windows or Linux build is still pending, do not publish the source release yet. Wait for all four updater platforms to exist in public `latest.json`, or users on the missing platform will not auto-update.

### 8. Windows build

**Default path: dispatch a worker on a paired Windows mesh peer** running `win-release.sh`. The script is the single end-to-end Windows pipeline (checkout → install → bundle → sign → upload → machine-parseable agent report). BLACKSTAR is the primary builder; ROG is a fallback with the same flow.

```bash
WORKSPACE_ID=$(codeterm --mesh BLACKSTAR --json workspace list \
  | python3 -c "import json,sys; print(next(w['id'] for w in json.load(sys.stdin) if w['name']=='codeterm'))")

codeterm --mesh BLACKSTAR agent spawn claude \
  --workspace "$WORKSPACE_ID" \
  --title "v$VER windows release" \
  --role Orchestrator \
  --model haiku \
  --task "bash .agents/skills/github-release/scripts/win-release.sh $VER"
```

The agent's final `codeterm agent report` line is parseable:

```
WINDOWS_READY=1.3.3 sig=dW50cnVzdGVkIGNvbW1lbnQ6...

Windows release v1.3.3 built, signed, and uploaded to both repos.
```

Grep the first line for `WINDOWS_READY=` (success) or `BLOCKED step=` (failure) — the orchestrator does NOT need to read the prose.

What the script does internally (`scripts/win-bundle.sh` + `win-upload.sh`):

1. `git -c core.autocrlf=false checkout v$VER` (CRLF phantom-dirt workaround on Windows is built in).
2. `pnpm install --frozen-lockfile`.
3. **Export `TAURI_SIGNING_PRIVATE_KEY` BEFORE `./build.sh build`.** Without this env var, the Tauri bundler skips updater signatures.
4. `./build.sh build` (NSIS bundler emits `.exe` + `.exe.sig` in current Tauri v2 mode).
5. Idempotent `gh release view || gh release create --draft` for both repos, then `gh release upload --clobber` for `.exe` + `.exe.sig`.

Wall times (BLACKSTAR, cache-warm): ~3min build, +~30s install if `node_modules` cold, +~10s upload. Cold from a fresh `pnpm install`: ~5-7min total.

**CI path (`release-windows.yml`)**: still triggers automatically on tag push and uploads `.exe` + `.exe.sig` to the private repo. Treat CI artifacts as a back-up source for `gh release download` when the worker is offline.

Direct local invocation (on a Windows machine with a checkout, e.g. for testing the pipeline without dispatching an agent):

```bash
# In Git Bash on BLACKSTAR / ROG:
bash .agents/skills/github-release/scripts/win-release.sh $VER
# stdout is the machine-parseable agent report
```

ROG-specific gotchas (still relevant if you fall back to ROG):
- Local SDK pinning (`build.sh` SDK_VER + `@rolldown/binding-win32-x64-msvc` 1.0.0-rc.9). Stored in a local `git stash` on the machine; `win-release.sh` does not auto-pop it — manual `git stash pop` if you see "SDK not found".
- `nvm use 22.22.0` (or any 22.12+) BEFORE invocation — Vite 8 fails on lower Node.
- gh authed as `rollacode (keyring)` should be valid; `r0llac0de (work)` is the work fallback and has push access too.

Alternative paths (debugging tactics, not normal flow):
- Send to an existing General Agent tab via `codeterm --mesh BLACKSTAR send --tab <general> --from-file <msg.txt>` to bypass `agent spawn` argv-split bugs.
- Open a parallel shell tab on the builder with `codeterm tab new --workspace` for tactical interventions while the agent runs the long compile.

### 9. Publish release + verify

```bash
# Private source repo:
gh release edit v$VER --draft=false --latest
gh release view v$VER --json assets --jq '.assets[] | "\(.name)  \(.size)B"'
# Expect 4-5 assets on the source repo: aarch64.dmg, x64.dmg, x64-setup.exe + the
# Linux artifacts (`amd64.deb`, `amd64.AppImage`) when CI shipped them via release-linux.yml.
# The mirror (codeterm-releases) holds the same set plus `.app.tar.gz` and `latest.json`.

# Public mirror — once Windows .exe arrives, also update its sig in latest.json
# and re-upload --clobber. Verify GUI updater can resolve anonymously:
curl -sIL https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json | grep '^HTTP' | tail -1
```

### 10. Sync develop with master + return to develop

After the release commit + tag land on master, `develop` is behind by the release/merge commits (and any docs/skill tweaks the release agent committed directly on master during the run). Fast-forward develop and push so the next release doesn't start from a stale base.

```bash
git checkout develop
git pull --ff-only origin master   # fast-forward; master == develop + release commits
git push origin develop
```

If `--ff-only` refuses (someone committed to develop during the release window), `git merge --no-ff origin/master --no-edit && git push origin develop`. **Don't skip this** — v1.2.1 forgot it and the next release had to re-discover the missing commits manually.

### 11. Bump Homebrew cask (optional, repos still private)

```bash
gh api repos/rollacode/codeterm/releases/tags/v$VER --jq '.assets[] | select(.name|endswith(".dmg")) | "\(.name) \(.digest)"'
# → captures two sha256 values; drop them into the cask

gh repo clone rollacode/homebrew-codeterm /tmp/homebrew-codeterm
cd /tmp/homebrew-codeterm && git pull --ff-only
# Edit Casks/codeterm.rb:
#   - bump version string
#   - replace on_arm sha256 with aarch64 digest
#   - replace on_intel sha256 with x64 digest
git add Casks/codeterm.rb
git commit -m "codeterm: bump to $VER"
git push origin main
```

Both repos are currently PRIVATE — `brew install codeterm` still won't work for the public. Bumping the cask keeps the tap self-consistent for when repos go public.

### 12. Regenerate translated READMEs from README.md (agent pass) translations must not ship stale

## Retrigger in-place (post-publish hotfix)

When a critical bug surfaces shortly after publish — **before users have widely auto-updated** —
it's safe to force-move the release trigger tag and re-upload artifacts under the same version
rather than bumping. Used for v1.1.15 (2026-05-05).

```bash
# 1. Commit fix on develop, merge by PR to master:
git checkout develop && git push origin develop
# On GitHub: open PR develop -> master and merge it.
git checkout master && git pull origin master --no-rebase

# 2. Move the release trigger tag:
git tag -f "release-$VER"
git push origin "release-$VER" --force

# 3. Checkout the new trigger tag commit (now points at the merge):
git checkout "release-$VER"

# 4. Rebuild + notarize + staple all 3 platforms (steps 5–8 above).

# 5. Re-upload to private repo with --clobber:
gh release upload v$VER "$ARM" "$X64" "$WIN_EXE" --clobber

# 6. Fetch existing latest.json BEFORE patching, to preserve any platform's signature
#    you are NOT rebuilding (e.g. if you only re-ran macOS, keep the Windows sig):
curl -sLO https://github.com/rollacode/codeterm-releases/releases/download/v$VER/latest.json
#    ... patch only the rebuilt platforms ...
gh release upload v$VER --repo rollacode/codeterm-releases <new files + latest.json> --clobber

# 7. Bump brew cask sha256 (DMG bytes change with every rebuild even if content is equivalent).
```

**Constraint:** only safe while the audience is small/internal. Anyone who already auto-updated to the broken artifact keeps the broken binary. For external user-facing releases, always bump version instead.

Force-retag pre-publish is unilaterally allowed per `rule_blackstar_mac_release_sync_protocol`.

## Failure modes seen in the wild

| Symptom | Cause | Fix |
|---------|-------|-----|
| `tauri build` ends with no DMG, no error | `build.sh dev` still running, Cargo target dir contention | `./build.sh kill` first |
| DMG `spctl` → "rejected, Unnotarized Developer ID" | tauri signed the `.app` but left the DMG wrapper unsigned | Run `xcrun notarytool submit … --wait && xcrun stapler staple …` manually |
| `notarytool store-credentials` → HTTP 401 | keychain creds expired OR used `APPLE_PASSWORD` instead of `APPLE_APP_SPECIFIC_PASSWORD` | source the env file, use correct var name |
| `git push origin master` → non-fast-forward | master diverged (e.g. previous release merged differently) | `git pull origin master --no-rebase --no-edit` then push again |
| `codeterm update install --auto` → "invalid gzip header" | macOS `latest.json` URL points at raw `.dmg` instead of `.app.tar.gz` | rebuild manifest with `.app.tar.gz` URLs — see Step 7b |
| tauri-plugin-updater unpack on Windows → "Compression method not supported" | `.nsis.zip` built with 7z default (Deflate64); zip-rs only supports Deflate/Store | regenerate with `7z a -tzip -mm=Copy` (Store compression) |
| tauri-plugin-updater unpack on macOS → "failed to unpack ._CodeTerm.app" | macOS `tar` includes AppleDouble metadata files (`._*`) by default | `COPYFILE_DISABLE=1 tar -czf ...` |
| BLACKSTAR build: `UNKNOWN error opening node_modules/sigma/package.json` | Windows filesystem race on freshly added dep | `rm -rf node_modules && pnpm install` on BLACKSTAR |
| Windows: Vite 8 build fails / version error | system node < 22.12 | `nvm install 22.22.0 && nvm use 22.22.0` on builder |
| macOS CI: `codesign ... no identity found` for bundled CLI/sidecar | workflow passed a stale `APPLE_SIGNING_IDENTITY` secret that does not match the imported p12 certificate | after `apple-actions/import-codesign-certs`, resolve `Developer ID Application` with `security find-identity -v -p codesigning` and export that to `GITHUB_ENV`; do not override it with the secret |
| release workflow mirror step → `EXTRA_FLAGS[@]: unbound variable` | empty Bash array expansion under `set -u` on older Bash | avoid optional-flag arrays in release shell; branch explicitly for prerelease vs stable `gh release create` |
| macOS x86_64 release job stays queued on `macos-13` | GitHub retired the macOS 13 runner image in December 2025 | use `macos-15-intel` for `x86_64-apple-darwin` |
| release workflow mirror step → HTTP 403 `Resource not accessible by personal access token` | `PACKAGE_REPOS_TOKEN` lacks `contents:write` on `rollacode/codeterm-releases` | rotate/update the repo secret, or manually mirror with a local `gh` login that has admin/write on the public mirror before publishing |
| `latest.json` maps both macOS platforms to `CodeTerm.app.tar.gz` | macOS workflow used the local generic updater bundle basename for the public URL | copy/sign/upload arch-specific updater assets and build fragments from `CodeTerm_aarch64.app.tar.gz` / `CodeTerm_x64.app.tar.gz` |
| public mirror has assets but GUI updater still 404s | fragments were generated with `REPO=rollacode/codeterm`, so `latest.json` points at the private source repo | generate fragments with `REPO=rollacode/codeterm-releases`; source release may still store the fragment, but its URLs must be public |
| Windows workflow says `No NSIS signature` while `.exe.sig` exists | workflow still assumes `createUpdaterArtifacts="v1Compatible"` `.nsis.zip`; current Tauri v2 `true` emits `.exe.sig` | use `*-setup.exe` / `.exe.sig` for `windows-x86_64`, or switch config to `"v1Compatible"` intentionally |
| Windows: `sqlite-sys` cc-rs → "malloc.h not found" | `build.sh`'s `SDK_VER` (10.0.22621.0) ≠ installed Windows Kit | check `ls "/c/Program Files (x86)/Windows Kits/10/Include"` then `export SDK_VER=<found-version>` (e.g. 10.0.26100.0 on ROG) |
| `gh release upload ...` → "no matches found" on Windows | MSYS git-bash + stale cd + relative paths | `cd` back to repo root before upload, or use absolute paths |
| `git rev-parse v$VER` returns a SHA that doesn't appear in `git log` | annotated tag (`-a`) creates a tag object that points to the commit (one level of indirection) | use `git rev-parse v$VER^{commit}` for the commit SHA, or `gh release view v$VER --json targetCommitish` |
| `codeterm send --mesh PEER --tab ID "..."` does not get a reply | target tab is busy or the receiving agent has not processed the input yet | send is fire-and-forget by design; check output later with `codeterm tab get-text --mesh PEER --tab ID` |
| SSH to BLACKSTAR: `cd /c/…` fails | `bash -lc` runs a login shell that re-homes | use `bash -c` (non-login) |
| `scp User@blackstar:/c/…` fails | remote shell is cmd.exe, doesn't grok the path | stage the file to `C:\Users\User\` on BLACKSTAR first, then `scp User@blackstar:filename` |
| Public `curl` of release asset → 404 | repo private, CDN rejects unauthenticated | use `gh release download`, or make repo public |
| Tag pushed but version files still say OLD | step 1 sed didn't catch all 3 (quote style, OLD value wrong) | run the post-bump verification grep BEFORE pushing the tag |
| `codeterm update check` returns stale `Latest:` after a fresh release | daemon caches the manifest at startup; default TTL outlasts your release | `codeterm update check --force` bypasses cache. GUI path (tauri-plugin-updater via public mirror endpoint) is independent of daemon cache — that always sees fresh |

## Multi-machine operational protocol

Adopted v1.1.8 onward. Cross-machine release coordination between Mac-Air (macOS signing host), ROG / BLACKSTAR (Windows builders), and the rest of the mesh.

**Roles per release:**
- **macOS builder (Mac-Air)**: macOS arm64 + x86_64 builds, notarize+staple, minisign all 3 platforms, compose `latest.json`, publish public mirror.
- **Windows builder (ROG primary, BLACKSTAR fallback)**: Windows build, `.exe` + `.exe.sig` upload to private repo draft.
- **Either side** can make the final "mark Latest on private" call after public mirror is verified live.

**Communication contract:**
- Mesh messages limited to milestones and blockers — not ad-hoc "where are you" polls.
- Default for status checks: fetch from `gh` + `git` (release asset list, master HEAD, tag SHA) without pinging the other side. Ping only when those don't tell the story, or at a milestone gate.
- Milestone format: `v$VER milestone: $stage — $details`. Stages: `tagged`, `win-built`, `mac-built`, `signed`, `public-live`, `private-published`.
- Blocker format: `v$VER BLOCKER: $where — $reason`. Reply within ~5 min or the other side may proceed unilaterally on small calls.

**Unilateral permissions (no ack needed each time):**
- Either side: cfg-only patches to fix per-target build breaks, force-retag tags pre-publish, regenerate `latest.json`.

**Catastrophic-only escalations (always ack required from the other side):**
- master force-push that touches commits already in BOTH local copies.
- minisign or Apple Developer cert key rotation.
- public mirror destructive ops (delete release, change `tauri.conf.json` updater endpoint URL, alter pubkey).

**Tiebreaker:** Andrey (local user) decides anything contested.

**Why:** prior coordination ping-ponged every blocker which stalled both sides. Codifying default permissions and milestone-only comms reduces interrupts. Catastrophic-ack list contains ops that can lose work or brick existing installs.

## Cross-machine signing when Bitwarden is locked

If the Windows builder can't access the minisign key (Bitwarden locked, no `~/.tauri/codeterm.key` on disk), delegate signing to whichever peer DOES have BW unlocked.

```bash
# On the build host (ROG), send the file to the signer peer via Taildrop:
tailscale file cp ~/Downloads/CodeTerm_${VER}_x64-setup.exe BLACKSTAR:

# On the signer host (BLACKSTAR):
# 1. File lands in ~/Downloads via the Windows GUI Tailscale (NOT the CLI inbox —
#    `tailscale file get` returns 0/0 because GUI Tailscale handles incoming differently).
#    May need to `mv ~/Downloads/CodeTerm_*.exe ./` manually.

# 2. Verify hash matches what the builder reported:
sha256sum CodeTerm_${VER}_x64-setup.exe
# → compare against builder's number

# 3. Pull minisign key from Bitwarden (BW unlocked here):
codeterm mem secret get --name codeterm-tauri-minisign-private-key > ~/.tauri/codeterm.key

# 4. Sign — key is used AS-IS (single-line base64), don't base64-decode:
KEY=$(cat ~/.tauri/codeterm.key | tr -d '\n')
TAURI_SIGNING_PRIVATE_KEY=$KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
  pnpm --package=@tauri-apps/cli dlx tauri signer sign CodeTerm_${VER}_x64-setup.exe

# 5. Upload the .sig to the private repo:
gh release upload v$VER CodeTerm_${VER}_x64-setup.exe.sig --clobber --repo rollacode/codeterm
```

**Important:** if Mac-Air already published the public mirror with its own valid sig (signs everything from one place), the cross-machine `.sig` upload to the private repo is redundant but harmless. The public mirror's `latest.json` is what the auto-updater actually consumes.

## Local install paths — pre-release dev/test only

Three ways to get a freshly-built binary running on YOUR machine, in increasing order of "real" install:

### Path 1 — manual file swap (Windows, debug-only)

Use when iterating on a prod-mode-only bug and you need fresh `codeterm.exe` in the running install without touching the installer:

```bash
# 1. Kill running prod (port 7685)
powershell.exe -NoProfile -Command "Get-Process codeterm -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2"

# 2. Copy fresh binaries into the installed location
cp C:/Developer/codeterm/src-tauri/target/release/codeterm.exe \
   "$USERPROFILE/AppData/Local/CodeTerm/codeterm.exe"
cp C:/Developer/codeterm/src-tauri/target/release/codeterm-embed.exe \
   "$USERPROFILE/AppData/Local/CodeTerm/codeterm-embed.exe"   # only if you touched the sidecar

# 3. Restart
powershell.exe -NoProfile -Command "Start-Process -FilePath 'C:\Users\User\AppData\Local\CodeTerm\codeterm.exe'"

# 4. Verify
curl -s http://127.0.0.1:7685/api/health
```

If the window shows `ERR_CONNECTION_REFUSED` for localhost → your build was compiled in dev mode (devUrl = http://localhost:1420). Re-run `./build.sh build`, not raw `cargo build --release`.

Known gotcha — WIX MSI bundler may fail on Windows (`failed to run light.exe`) but the `.exe`s are already produced by then; NSIS / file-swap paths still work.

### Path 2 — self-install via CLI (Windows, programmatic)

`codeterm update install --from-file` runs the freshly-built NSIS installer silently in a detached helper, swaps the binary, and restarts:

```bash
codeterm update install --from-file 'C:\Developer\codeterm\src-tauri\target\release\bundle\nsis\CodeTerm_X.Y.Z_x64-setup.exe'

# With session continuation (resume the calling agent's session after restart):
codeterm update install \
  --from-file '...\CodeTerm_X.Y.Z_x64-setup.exe' \
  --continue-prompt "after restart, verify tab performance"
```

`--continue-tab` defaults to `$CODETERM_PANE_ID`. The helper:
1. Sleeps 2s, lets the calling CLI return.
2. `taskkill /F /IM codeterm.exe codeterm-embed.exe`.
3. Runs the NSIS installer with `/S` (silent — no UAC since `installMode=currentUser`).
4. Launches the freshly installed `codeterm.exe`.
5. If `--continue-prompt`: polls `/api/health` until 200, then `codeterm send --tab <id>` resumes the calling agent.
6. Logs to `%LocalAppData%\codeterm\self-install.log`.

Fallback when CLI itself is broken or version-mismatched — use the standalone PowerShell script:
```powershell
powershell -ExecutionPolicy Bypass `
  -File "$env:USERPROFILE\.codeterm\skills\self-install-local\scripts\self-install-local.ps1" `
  -Installer "C:\Developer\codeterm\src-tauri\target\release\bundle\nsis\CodeTerm_X.Y.Z_x64-setup.exe" `
  -Resume
```

The script lives in this skill's folder at `<repo>/.agents/skills/github-release/scripts/self-install-local.ps1`.

Gotchas:
- Don't pass `-Verb RunAs` to the installer. `installMode=currentUser` doesn't need elevation; RunAs triggers UAC and hangs on headless.
- Helper survives the daemon, not your terminal tab — if invoked from a CodeTerm-managed tab, the tab dies with the daemon. Tail progress from outside: `Get-Content -Wait "$env:LocalAppData\codeterm\self-install.log"`.
- Auto-resume of agent tabs is best-effort — `~/.codeterm/sessions/last.json` may be slightly behind if the daemon was killed abruptly.

### Path 3 — full release publish

The rest of this skill. Use when shipping to users. The local-install paths above are NOT a substitute; the public `latest.json` must be updated for end-users' auto-update to work.

## Why no specialized "devops" agent

As of 2026-05-12 this skill is the canonical release procedure. No `.claude/agents/devops.md` exists. When the user says «релиз», «выпусти vX.Y.Z», «ship it», any Lead or Sonnet worker can execute by fetching this skill from mem (`codeterm mem get skill_end_to_end_github_release_for_codeterm`) and following it end-to-end. Single source of truth, no drift between `.md` and `mem`.

## Known gaps (fix opportunistically)

- `./build.sh build` should staple the DMG wrapper automatically — currently manual step in §5. Worth patching `build.sh` to mirror what `scripts/release-macos-x86_64.sh` already does.
- Release workflows still need a green dry-run after `PACKAGE_REPOS_TOKEN` rotation; manual mirror fallback above is the escape hatch when source artifacts are valid but mirror upload fails.
- Brew tap bump is manual — a `scripts/release-bump-brew.sh` could read the release JSON and rewrite the cask in one shot.
- `tailscale file get` on Windows ignores GUI-Tailscale-received files (they land in `~/Downloads`, not the CLI inbox). Worth verifying / filing upstream.
