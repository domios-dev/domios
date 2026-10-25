---
name: github-release
description: >-
  Prepare, dry-run, monitor and publish Domios releases; debug release workflows,
  shared full/headless packaging, signing, artifact readback and the updater mirror.
---

# Domios release

## Keep this skill useful

Update this skill when release work reveals a verified, reusable constraint or a missing
procedure. Put the lesson in its existing section, replace superseded guidance and remove
duplicates in the same edit. Keep commands, prerequisites, failure boundaries and observable
pass conditions. Run IDs, candidate SHAs, timings, temporary workarounds and incident narratives
belong in the release handoff or plan evidence. Do not append a new troubleshooting story for
each failed run. An untested proposal must not become an established recipe.

## Authority and prerequisites

- Publication, tags, version bumps, installation and production restart require owner GO.
  Dry-run authorization does not authorize shipping. Never move a published tag, overwrite a
  published version with rebuilt bytes, force-push master or bypass a failed gate.
- Use the exact reviewed committed candidate. Real-merge lanes; never reconstruct their tree
  by copying patches. Exclude unrelated dirty files. Record workflow-ref SHA separately from
  application `source_ref` SHA when they differ.
- Use the authorized self-hosted fleet. Honor physical-machine exclusions for every job,
  including preparation/tests and that machine's WSL/VM runners. Verify eligible labels and
  actual job runner names through the runners/jobs REST APIs. Do not change labels, repository
  variables or shared routing to evade an exclusion. BLACKSTAR is excluded from current
  release builds and packaging.
- Read [gates](../gates/SKILL.md) before qualification, [build](../build/SKILL.md) for build and
  install verbs, [build-machine](../build-machine/SKILL.md) for runner/image recovery, and
  [release-signing](../release-signing/SKILL.md) before credential changes. A busy build gateway
  is a refusal unless intentional waiting was explicitly authorized.
- Keep PROD 7685 untouched. Native builds, suites and installs follow the assigned machine and
  checkout ownership. Do not stop another owner's process or replace installed files manually.

## Choose the release kind

| Kind | Procedure |
|---|---|
| Application | Rust/CLI/native changes require the application graph, including bundled frontend. |
| Frontend only | Follow [frontend release](../../../scripts/frontend-release/README.md); no native build. |
| Official plugin | Independent plugin version/tag and `plugin-release.yml`; no app version bump. |

Application and frontend versions share one increasing sequence. An incompatible frontend wire
change updates `API_VERSION`, the bundled API range and `frontend-release-contract.json` together.
For frontend-only releases, change the frontend contract version; do not change Rust or app version
files, which select an application release. Frontend discovery is separate from app `latest`.

For a plugin release, run `node scripts/release-plugin.mjs bump <id> <version>`, with explicit
`--min-codeterm` only when a new host capability is required. Verify `release-plugin.mjs check`
and `check-plugin.mjs <id>`, merge the checked PR, then with publication GO tag the verified master
commit `plugin-<id>-v<version>`. Verify the installed version through `domios plugin update-system`.
The tag's `publish_core_channel` job runs `scripts/publish-core-channel.mjs`, which uploads the whole
`plugins/` tree as `codeterm-core.tar.gz` (+ `.sha256`, `.source-commit`) to the public
`rollacode/codeterm-releases` release `codeterm-core-latest`, skips an older master commit, and
verifies the anonymous download. Every install refreshes `codeterm-core` from there.

## Application train

| Branch | Purpose |
|---|---|
| `develop` | Integration through reviewed real merges. |
| `release/X.Y.Z` | One version; owner pushes start the publishing release graph. |
| `master` | Released code, promoted by the successful release workflow. |
| `ci/*` | Reviewed non-publishing candidate refs for diagnostics. |

1. Record the candidate, included lanes, fixed/deferred issues and evidence in
   `docs/release/<version>-pr.md`. Review changed commands/docs and remove obsolete callers.
2. Before a release-branch push, qualify the exact committed release head in canonical using
   the gates skill. `build.sh check` is not release qualification. Required live evidence includes
   [core journeys J1–J4](../../../evals/core-journeys/README.md) and
   [upgrade restore](../../../evals/upgrade-restore/README.md), with isolated named instances,
   their documented commands, passing evidence contracts and real startup/restore results.
   Source fixtures or capture/import alone do not replace these gates.
3. With version-bump GO, create the release branch from reviewed develop. Edit package.json's
   version, run `npm run sync:versions`, refresh both workspace lockfiles offline, then run
   `npm run check:versions` and `node scripts/check-release-locks.mjs`. Commit the complete bump;
   plugin versions remain independent. Run `build.sh release-preflight` per the gates skill on
   that committed head and `git diff --check`.
   Before every push of `release/X.Y.Z` (the first one and each one carrying a fix or merge), the
   Orchestrator mirrors the CI fast tier on that exact candidate in canonical, so a CI round never
   stops on a one-minute defect. Rust jobs go through
   the gateway one after another; the frontend and guards run in parallel in a second utility tab:
   - Rust: `./build.sh fmt-check`, `./build.sh ci-clippy`, `./build.sh nextest cli 'all()'` (every
     CLI target, generated manifest, essence map), and `./build.sh ci-tests full` with `RUNNER_OS`
     set to the host OS (the whole workspace, one build).
   - Frontend: `pnpm exec tsc --noEmit`, `pnpm check:api-effects`, `npm run test:unit` (all TS unit
     files) and `pnpm exec vitest run` (all files). Never only the files that failed before.
   - Guards: `bash scripts/check-file-sizes.sh --fail`, `node scripts/check-provider-boundary.mjs`,
     `node scripts/check-installed-identity.mjs`, `node scripts/test-catalogue.mjs validate`,
     `node scripts/check-plugin-bundles.mjs` (a changed plugin UI source needs its rebuilt bundle).
   While iterating on a fix, `pnpm exec vitest related <files>` and the matching Rust filterset give
   a quick signal, but the push to the release branch still waits for the whole list above: the warm
   build keeps that rerun cheap.
   Before every push to `release/X.Y.Z`, including each merge or fix, the release manager runs the
   checks a CI round would otherwise find one at a time: `./build.sh fmt-check`,
   `./build.sh nextest cli --wait-build` (every CLI target: transport rows, help text, generated
   manifest, essence map), `node scripts/test-catalogue.mjs run --entry python-script-tests` (POSIX)
   and `node --test` over the changed `scripts/**/*.test.mjs`. The owner authorizes these for the
   release manager; a refused gate goes to the Orchestrator as the exact command to grant.
4. Verify runner availability with `bash scripts/check-release-runners.sh` and credential
   preflight through `release-credentials.yml`. Confirm the intended version is unpublished.
   With publication GO, push `release/X.Y.Z` and open its PR to master. Do not manually create
   `vX.Y.Z`; release.yml owns tag creation and promotion.
5. The final manifest job requires all builds and test gates, publishes and verifies served
   bytes. Promotion merges the release PR to master, merges master back to develop and deletes
   the release branch. A later correction takes a new version.

## Dry runs and narrow diagnostics

Inspect the actual workflow inputs and publication conditions at the pinned workflow ref before
any dispatch. `mirror=false` alone is not a dry run. For the full graph use:

```bash
gh workflow run release.yml --repo rollacode/codeterm --ref <reviewed-ref> \
  -f tag=<version-tag> -f dry_run=true -f mirror=false -f dry_run_tests=true
```

Until repository variable `RELEASE_PUBLISH_ENABLED` is `true`, a `release/X.Y.Z` push runs as a
full dry run of that branch (tests included, no drafts or uploads) in its own concurrency group, so
it never cancels another version's run; its branch must carry version X.Y.Z.

Verify the remote ref resolves to the reviewed SHA before dispatch. Capture the returned/new run
ID and its workflow/source identity. Do not identify a run merely as the latest result in a list.
`dry_run_tests=false` is an explicitly limited packaging diagnostic: skipped tests remain unverified.
Dry runs skip release creation/public uploads/publication, public served-byte verification and promotion;
they cannot prove write permissions or CDN behavior.

Dry-run platform jobs retain original updater bytes, signed fragments and digest/source receipts
in the existing private MinIO CI store under `<run-id>/release-dry-run/<platform>/`, with its
existing retention. `release-manifest-dry-run.yml` assembles all six platforms plus both Debian
entries and invokes the existing validator with `--local`. It refuses missing, changed or foreign
evidence; no public release, tag, channel asset or issue-closure boundary is created. Its direct
dispatch inputs pin the producing run ID, tag and source SHA for a follow-up within retention.
An older run that did not retain those inputs cannot be retroactively called validated.

Builds run on builder machines and start as soon as the frontend bundle exists. Rust tests run on
test machines and open through `tests-tier` (`scripts/release-tier-gate.mjs`) once every fast check is
claimed by a runner or finished, so a free test runner never takes a test before a fast check. A fast
check still unclaimed after 45 minutes opens the tests anyway with a `tier order fallback` warning, a
run-summary line and the receipt `<run-id>/release-tiers/fast-checks.json`; a red fast check still
cancels the run.

Publish a wholly green dry run with `release-publish.yml`, never by rebuilding. Its tooling comes from
the default branch, so fixing publication never invalidates a qualified run:

```bash
gh workflow run release-publish.yml --repo rollacode/codeterm --ref master \
  -f run_id=<dry-run run id> -f tag=vX.Y.Z -f rehearsal=true   # then rehearsal=false with publication GO
```

`scripts/release-publish.sh` refuses unless the run is a completed, successful release.yml dry run
on `release/X.Y.Z` whose platform, test and manifest jobs all succeeded. A rehearsal checks both
tokens' write access, downloads and re-validates every retained byte and signature, and changes
nothing. Publication creates both drafts, uploads the retained bytes (fragments only to the source
draft) with read-back against each receipt, merges and verifies `domios-latest.json`/`domios-latest-deb.json`,
tags `vX.Y.Z` on the run's SHA, publishes both releases and verifies the served manifests. With
repository variable `RELEASE_AUTO_PROMOTE=true`, every green dry run of a release branch publishes
itself through the same workflow; otherwise the run waits for this command. Issue closure and the
merge to master stay separate steps.

The store endpoint is defined once in `scripts/ci-store.sh`. When a CI job finds it unreachable,
`ci-artifact.sh put` spools the file with a warning and the job's `ci-artifact-spool` action uploads
it as a run artifact; `get`/`get-prefix` read `<run-id>/...` keys back from that run's artifacts.
`manifest-local-verification.json` records `retained_via` and a per-file `location`. Rust jobs then
compile with a job-local sccache that `build.sh cache-release` stops at job end. A reachable store
that refuses a request, and any digest or signature mismatch, still fail.

For one platform, direct-dispatch `release-linux.yml`, `release-windows.yml` or
`release-macos-build.yml` with explicit `source_ref`, tag, selected architecture, `dry_run=true`
and `mirror=false`, using that workflow's declared inputs. A `tests.yml` dispatch runs the fast tier only.
Direct platform dispatch cannot mutate release assets. Its guard distinguishes the direct workflow
name from a manually dispatched release.yml caller: reusable workflows inherit the caller's event.
Do not disable publication for a legitimate release caller merely because its event is workflow_dispatch.

The full graph separates fast guards/checks (`tests-fast.yml`) from full Rust and platform-specific
tests (`tests-rust.yml`); release.yml calls each tier directly. Tiers open on claim, not on completion:
the Rust tier starts once every fast check holds a runner, and the six builds start once every Rust
test job does (`release-tier-gate.mjs`, 45-minute visible fallback). Required
Rust receipts include nonzero executed counts for the full suite and each OS subset. A failed,
cancelled or skipped required gate cannot publish. Diagnose cancellation from run/job evidence
and workflow conditions; never call cancelled jobs passes or assume who cancelled them.

## One build, two distributions

A release run builds the platform-independent frontend once in its `frontend-dist` job; each
platform job fetches that archive, checks its SHA-256 and installs it as `dist` through
`CODETERM_FRONTEND_ARCHIVE`/`CODETERM_FRONTEND_SHA256` instead of building it. Direct platform
dispatches, without that digest, still build their own frontend.

Per target, build CLI and release server once, then native client once. Both distributions
consume the same final server/CLI/resources; packaging must not compile them again.

1. `release-inputs --for-bundle` builds inputs and stages the actual frontend at
   `target/domios-package/dist` before Tauri compilation. Compile-only resource stubs are marked;
   they never authorize packaging.
2. `tauri build --no-bundle` compiles the native client (`domios-app`, with .exe on Windows); the
   packaged CLI ships `domios` and `codeterm` from identical signed bytes. Linux's compile-only config empties resources/externalBin to
   avoid the raw server-file/resource-directory collision. Restore full platform maps for bundling.
   macOS also empties compile-only externalBin: tauri-build otherwise copies the sidecar stub
   over the already signed Cargo server when both share the target release directory.
3. `scripts/release-stage.mjs stage-full` replaces the compile stage with complete resources and
   manifests from final executable bytes. The stable Tauri source is `target/domios-package/`;
   the target receipt stage is `release-artifacts/common-stage/<rust-target>/`.
4. Use the pinned Tauri CLI from the frozen lockfile and `tauri bundle --no-binary-patching`.
   Earlier bundlers mutate the client after its manifest digest is frozen. Never repair a finished
   installer by replacing its embedded resource. Bundle-only must reuse the compiled client.
5. Finalize extracts the full package and compares manifest/server/CLI bytes to the stage, then
   produces the headless archive, checksum and metadata and independently reads them back.
   Missing resources, stubs, symlinks, path escapes, extras or changed bytes fail closed.
   On Windows use `build.ps1 release-finalize` so Cargo receipt paths remain accessible through
   the gateway's stable drive mapping; direct finalize after gateway exit loses that mapping.

The stage receipt schema1 inventories every regular payload file except itself as UTF-8-sorted,
unique `{path,sha256}` rows, with platform/target/tag/version/source_sha identity. Required payload:
server, CLI, frontend index/assets, skills, LICENSE/NOTICE and Windows target ConPTY pair.
Use the existing validators in `headless-release-package.mjs`; do not invent a parallel schema.

Component manifest schema2 uses exact final `server_sha256`, independent `server_build`, API and
bootstrap versions, plus platform-specific `client_identity`:

| Package | Client identity | Server path relative to manifest resource root |
|---|---|---|
| Windows/Linux full | `sha256` of final native client | `domios-server/domios-server[.exe]` |
| macOS full | `macos_designated_requirement` | exactly `../MacOS/domios-server` |
| Headless, every OS | `none` | `domios-server/domios-server[.exe]` |

Full and headless manifests differ; their server digest/build/protocol identity and payload bytes
must agree. Headless `none` never enrolls a native client. Windows/Linux resolve resources through
Tauri resource_dir, not hardcoded installation paths. Platform overlays provide one server location.

### macOS signing

Sign/freeze CLI and nested server first. Tauri's sidecar source needs a byte-identical
`domios-server-<rust-target>` alias; the installed server is `Contents/MacOS/domios-server`.
The manifest is sealed at `Contents/Resources/server-component.json`.
Its client identity contains team_id, identifier `com.domios.app`, and exactly:

```text
anchor apple generic and identifier "com.domios.app" and certificate leaf[subject.OU] = "<TEAMID>"
```

Derive TEAMID from the approved signing identity. Do not embed the final outer app cdhash inside
its own seal. Runtime verifies the authenticated process and containing app through Security.framework.
No ad-hoc signer, raw-SHA/debug exception or unsigned adjacent manifest is permitted.

`scripts/release-macos-bundle.mjs` assembles the app without re-signing frozen nested bytes,
signs only the outer app, verifies byte equality and strict signing, creates the standard app-only
DMG, mounts it read-only and verifies the payload. The existing notary helper submits/staples the
DMG; staple/assess the app before creating its updater archive. Unexpected helper executables or
Tauri re-signing fail loudly. Native execution must validate this path; injected fixtures are only
source-contract evidence. Standard DMG, .app.tar.gz updater and self-install remain the transports.

### Runtime and bundle proof

Release build jobs retain bundle contents, digest and signing checks. Server startup,
local API-token resolution, authenticated CLI identify and clean shutdown run in
CLI integration target isolated_server through the Rust test tier on every native test OS.
The nextest cli, test-one cli isolated_server and CI CLI runners prepare the debug server
and its exact bound CLI before the capped tests. This test creates disposable data/config
roots and uses an explicit private port; CODETERM_API_URL alone never selects that port.

Native-client attach, browser authentication and cold update/restore require their own
platform evidence. A CLI integration pass does not establish those surfaces.

Headless users launch from the extracted root with
`./domios-server/domios-server start --production --json` (Windows: the .exe equivalent).
The default resource root is executable parent-parent; `--resources` overrides it. Authenticate
through WebGate using protected local configuration. Never expose credentials in archive or output.

## Manual move from CodeTerm (1.15.2)

Publish [the exact owner install steps](../../../docs/release/1.15.2-manual-install.md)
in both release bodies; `sync-release-notes.mjs` appends them for v1.15.2.
Quit the old app and disable its autostart before installing Domios fresh:

- Windows: run the new setup into `%LOCALAPPDATA%\Domios`, then run the old
  uninstaller in `%LOCALAPPDATA%\CodeTerm` (normally `uninstall.exe`). Remove
  any remaining `CodeTerm-Autostart` task and old shortcuts/pins.
- macOS: install `/Applications/Domios.app` from its DMG, then delete
  `/Applications/CodeTerm.app`. Remove the old Login Item and unload/remove
  `~/Library/LaunchAgents/com.codeterm.app.plist` if present.
- Linux: install the Domios package, then remove `codeterm` with the same package
  manager (`sudo apt remove codeterm` or `sudo rpm -e codeterm`). Remove the old
  AppImage/desktop entry instead for an AppImage install. Disable/remove the old
  `codeterm.service` user unit. A package-file conflict requires removing the
  old package before retrying the new install; never force-overwrite it.

Launch Domios once to install/repair `~/.domios/bin/domios` and `codeterm`
(the same CLI, with `.exe` on Windows), then re-enable autostart if desired.
In a new terminal, `where.exe domios` / `where.exe codeterm` (Windows) or
`command -v domios` / `command -v codeterm` (Unix) must select those CLI entries
or their `~/.local/bin` links. Deb/RPM own `/usr/bin/domios` and `/usr/bin/codeterm`;
the GUI is `domios-app[.exe]`, so CLI PATH never chooses a GUI executable.
macOS and AppImage keep user CLI links; users control their shell PATH.
The app never edits shell dotfiles. Remove old PATH entries,
shortcuts, and URL/file handlers; verify `domios://` opens only Domios. Keep all
`~/.domios` data. No automatic data migration, install shim or copied OS grants.

macOS `com.domios.app` needs fresh TCC microphone/Accessibility grants; Keychain
items authorized to the old signed identity may prompt or require sign-in again.
Windows uses a new notification AUMID and preferences; allow Domios notifications.
Verify those prompts on owner installs; source/config checks are not permission proof.

The old 1.15.1 updater otherwise installs side by side on Windows, replaces the
bundle at its old path on macOS, and risks side-by-side/conflicting packages on
Linux (or an AppImage with the old filename). New clients exclusively use
`domios-latest*.json`. Both publication paths generate `latest*.json` refusals
for the old channels and Deb/RPM transports with manual notes and no platforms.
Old discovery reports incompatible with these notes; forced install fails before
artifact download or server shutdown. Metadata cannot add a custom old-binary dialog.
Verify the served legacy refusals as well as the new signed manifests before calling
publication complete. Never upload a Domios payload into a legacy manifest.

## Credentials, caches and runners

Use repository secrets and `scripts/release-signing-material.py`. macOS jobs import the approved
certificate and pinned Apple intermediates into a unique RUNNER_TEMP keychain, validate team/identity
and notary credentials, and clean up only their own keychain/files in always(). No persistent login
keychain changes or copied private keys are a release prerequisite. Updater keys use temporary
restricted files; Tauri needs `TAURI_SIGNING_PRIVATE_KEY` pointing to the prepared key file.
Off-CI recovery copies live in Bitwarden, read with `domios mem secret get --name <slug>` and
never printed: `codeterm-tauri-minisign-private-key` (updater signing, empty password; its public
key is `tauri.conf.json` `plugins.updater.pubkey`) and `codeterm-apple-notary-env`. The Developer ID
certificate is backed up only as the `APPLE_CERTIFICATE`/`APPLE_CERTIFICATE_PASSWORD` repo secrets.
Windows updater signatures are not Authenticode: no Authenticode signing is currently configured.

### Runner map

Workflows select runners by label, never by name. Each runner carries `host-<machine>`, which no
job routes on; `./build.sh runner-pause` keeps it and never lends it.

| Runner (planned name) | Machine, OS/arch | Routing labels | Role |
|---|---|---|---|
| `codeterm-builder-2` (`eight-macos-arm64`) | eight, macOS arm64 | `codeterm-builder`, `heavy`, `light`, `host-eight` | builds: macOS arm64, Linux arm64 Docker; quick checks |
| `build-mac-x64` | build-mac, macOS x64 (Intel) | `macos-x64-build`, `host-dev` | build: macOS x64, native (`RELEASE_MACOS_X64_LABEL=macos-x64-build`); also the private store host |
| `air-macos` (`air-macos-arm64`) | air, macOS arm64 | `macos-ci`, `rust-shard`, `light`, `host-air` | tests: macOS Rust platform; quick checks |
| `ROG-windows-build` (`rog-windows-x64`) | ROG, Windows x64 | `windows-build`, `windows-x64-build`, `medium`, `host-rog` | builds: Windows x64 and arm64 NSIS |
| `BLACKSTAR-windows-x64` (`blackstar-windows-x64`) | BLACKSTAR, Windows x64 | `windows-ci`, `rust-shard`, `heavy`, `host-blackstar` | tests: Windows Rust platform and Windows test jobs |
| `ubunta-linux-build` (`ubunta-linux-x64`) | ubunta, Linux x64 | `linux-build`, `host-ubunta` | builds: release prepare, frontend bundle, Linux x64, dry-run manifest, publication |
| `BLACKSTAR-wsl-ci` (`blackstar-wsl-x64`) | BLACKSTAR, WSL2 Linux x64 | `linux-ci`, `rust-tests`, `heavy`, `light`, `host-blackstar` | tests: Rust full suite, embeddings, Linux platform, clippy |
| `BLACKSTAR-wsl-light-1/2` (`blackstar-wsl-x64-light-1/2`) | BLACKSTAR, WSL2 Linux x64 | `linux-ci`, `rust-tests`, `light`, `host-blackstar` | tests: second and third Linux Rust test runners; quick checks |
| `ubunta-light-1/2` (`ubunta-linux-x64-light-1/2`) | ubunta, Linux x64 | `light`, `host-ubunta` | quick checks and tier gates |
| `green-windows-ci` (`green-windows-x64`) | green, Windows x64 | `windows-light`, `host-green` | Windows frontend quick check |

The BLACKSTAR WSL runners (distro `BLACKSTAR-CI`, user `ci`) share one toolchain: rustup with the
pinned channel from `rust-toolchain.toml` plus clippy/rustfmt in `~/.cargo`, `cargo-nextest`, and
sccache in `~/.local/bin` on one Unix-socket server. `~/start-runner.sh` and
`~/start-light-runner.sh <n>` export that PATH and cache environment before `run.sh`, so a light
runner takes Linux test jobs without further setup.

Release builds and Rust tests never share a machine: build labels live only on builders and test
labels only on test machines. Pausing a test machine (`./build.sh runner-pause <machine>`) lends its
unique test labels to an online runner of the same OS, the one documented exception.

Planned names follow `<machine>-<os>-<arch>[-role]`; renames re-register one machine at a time between
runs, passing the current labels plus `host-<machine>`. Before dispatching on a new or changed macOS
builder, run `gh workflow run release-credentials.yml -f macos_label=<label> -f notary_probe=true`:
it checks only that runner and notarizes a trivial app there in minutes.

Use the existing sccache policy and named cache-verify/cache-stats operations. Configured zero
counters do not prove a live cache or reuse. Container evidence comes from its own cache server,
not host statistics. Recovery/image baking follows the build-machine skill and assigned authority;
no automatic reboots, pruning, runner provisioning or global configuration changes.
Rust warning reports must be complete and zero-new-warning. Fix owning-platform warnings; never
increase baseline allowances or treat missing/cancelled reports as clean. Retrieve CI report evidence
through the existing ci-artifact tooling rather than assuming GitHub artifact storage.

## Monitoring and failure recovery

Read `domios docs BLUEPRINTS`. Use one named monitor per actual run ID; validate its probe once
and verify blueprint show names that run's real bucket. Handle success, failure, cancellation,
probe error and queued-too-long explicitly. Stop at terminal; never leave repeated failing probes,
sleep/poll loops or duplicate watchers. A launch acknowledgement is not a completed build receipt.

Fetch completed job logs while siblings run:

```bash
gh api repos/rollacode/codeterm/actions/runs/<run-id>/jobs --paginate
gh api repos/rollacode/codeterm/actions/jobs/<job-id>/logs
```

Classify the first error as pipeline, product or machine failure. Fix owned pipeline defects with
focused checks; route product defects with exact SHA/job/step/log. Collect remaining platform evidence
without blind reruns. A source fix needs a new SHA/run; do not combine artifacts across candidates.
For a proven environmental failure on unchanged source, rerun only the failed job after recovery.
After publication starts, recover the original manifest/promote job rather than rebuilding that version.

Manual fallback uses the same committed build/stage/readback helpers on an authorized peer through
the build gateway, isolated checkout and output directory. Inspect helpers before invoking: some
legacy wrappers upload or install. Missing signing/notary inputs remain an explicit verification gap;
unsigned diagnostics cannot be called signed releases. No file swaps or process-kill workaround.

## Publication, notes and issue closure

Private source: `rollacode/codeterm`. Public updater mirror: `rollacode/codeterm-releases`.
Manifest URLs must be anonymously reachable public assets. macOS updater uses arch-specific
.app.tar.gz, Linux AppImage, Windows NSIS .exe; a DMG is not a macOS updater archive.
Keep signatures/fragments paired to the exact bytes from their producing run.

The final job assembles all six updater keys (darwin/windows/linux × aarch64/x86_64), verifies
private draft bytes, publishes and verifies public served bytes with a cache-busted URL and required
version/platforms using `scripts/verify-updater-manifest.mjs`. HTTP 200 alone proves no installability.
A partial recovery cannot mint complete release evidence. Manual mirror repair needs owner GO and
must reuse original verified artifacts/fragments, not handwrite a reduced domios-latest.json.

Served bytes are not the whole proof: on a host still running the previous version,
`domios update install --auto` must install this release and come back on the new pair with
sessions restored. A published defect ships as the next patch version through this same train;
never move a tag or re-upload changed bytes under a published version.

`sync-release-notes.mjs` maintains one substantive notes body across both repositories. Historical
backfill mutates published notes and needs explicit authorization.

Declare fixed issues as exact metadata, one per line:

```text
Repository-Issue: rollacode/codeterm#<number>
Public-Issue: rollacode/codeterm-releases#<number>
```

Integration PRs use metadata; the final master PR gets generated closing keywords through
`release_issue_contract.py repository-closes`, `reconcile` and `audit-commits`. Record investigated
and deferred issues with reasons; do not invent fixes or handwrite a second closing set.
Public issues close only after this version's complete served-byte verification passes
`check-release-closure.mjs`. The closure boundary records validator run ID/repository/URL and source
SHA; readers confirm that successful run through Actions. Missing/unverifiable boundaries are
refused, never inferred from asset presence or release ownership.

## Handoff and closeout

Keep a handoff file current with candidate and workflow SHAs, run URL/ID, inputs, actual per-job
runner names, first failures and owners, build counts per target, artifact names/sizes/SHA256,
full/headless server/CLI equality, extraction/checksum readback, smoke receipts and every unrun or
UNVERIFIED platform. Record signing/notarization/update gaps and any routing/config changes explicitly.
Deliver results once through `domios agent report`; milestones and blockers may be short updates.

After publication, verify the updater/install/restore surfaces authorized by the owner. Cleanup
follows the lane-cleanup and build skills, with ancestry checks and preserved unmerged work;
never sweep caches or delete worktrees merely because the release ended.

Implementation entry points: [release stage](../../../scripts/release-stage.mjs),
[macOS x64 helper](../../../scripts/release-macos-x86_64.sh).

<!-- revision: 11 -->
