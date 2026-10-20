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
commit `plugin-<id>-v<version>`. Verify the installed version through `codeterm plugin update-system`.
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
   [core journeys](../../../evals/core-journeys/README.md) and
   [upgrade restore](../../../evals/upgrade-restore/README.md), with isolated named instances,
   their documented commands, passing evidence contracts and real startup/restore results.
   Source fixtures or capture/import alone do not replace these gates.
3. With version-bump GO, create the release branch from reviewed develop. Edit package.json's
   version, run `npm run sync:versions`, refresh both workspace lockfiles offline, then run
   `npm run check:versions` and `node scripts/check-release-locks.mjs`. Commit the complete bump;
   plugin versions remain independent. Run `build.sh release-preflight` per the gates skill on
   that committed head and `git diff --check`.
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

For one platform, direct-dispatch `release-linux.yml`, `release-windows.yml` or
`release-macos-build.yml` with explicit `source_ref`, tag, selected architecture, `dry_run=true`
and `mirror=false`, using that workflow's declared inputs. `tests.yml` accepts `phase=fast`.
Direct platform dispatch cannot mutate release assets. Its guard distinguishes the direct workflow
name from a manually dispatched release.yml caller: reusable workflows inherit the caller's event.
Do not disable publication for a legitimate release caller merely because its event is workflow_dispatch.

The full graph separates fast guards/checks from full Rust and platform-specific tests. Required
Rust receipts include nonzero executed counts for the full suite and each OS subset. A failed,
cancelled or skipped required gate cannot publish. Diagnose cancellation from run/job evidence
and workflow conditions; never call cancelled jobs passes or assume who cancelled them.

## One build, two distributions

Per target, build frontend, CLI and release server once, then native client once. Both distributions
consume the same final server/CLI/resources; packaging must not compile them again.

1. `release-inputs --for-bundle` builds inputs and stages the actual frontend at
   `target/domios-package/dist` before Tauri compilation. Compile-only resource stubs are marked;
   they never authorize packaging.
2. `tauri build --no-bundle` compiles the native client (`domios`, with .exe on Windows); the
   packaged CLI stays `codeterm`. Linux's compile-only config empties resources/externalBin to
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
Its client identity contains team_id, identifier `com.codeterm.app`, and exactly:

```text
anchor apple generic and identifier "com.codeterm.app" and certificate leaf[subject.OU] = "<TEAMID>"
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

### Packaged runtime proof

`scripts/release-pair-smoke.mjs` checks extraction, stage/manifest digests and exactly one server
copy, then launches packaged server bytes in isolated temporary data/config roots on a private
loopback port. It waits for READY under a deadline, verifies health/build identity, runs authenticated
packaged CLI and requires clean shutdown. Token stays in child environment, never argv/URL/logs.
Use the committed isolated-verification contract; do not launch against a user's production roots.

Cross-architecture jobs still perform every byte check. Unsupported execution writes an explicit
`unverified_cross_arch` receipt, never PASS. macOS x64 on Apple Silicon tries Rosetta first.
Packaged native-client attach, browser authentication and cold update/restore need their own real
platform evidence; headless smoke alone does not prove those surfaces.

Headless users launch from the extracted root with
`./domios-server/domios-server start --production --json` (Windows: the .exe equivalent).
The default resource root is executable parent-parent; `--resources` overrides it. Authenticate
through WebGate using protected local configuration. Never expose credentials in archive or output.

## Credentials, caches and runners

Use repository secrets and `scripts/release-signing-material.py`. macOS jobs import the approved
certificate and pinned Apple intermediates into a unique RUNNER_TEMP keychain, validate team/identity
and notary credentials, and clean up only their own keychain/files in always(). No persistent login
keychain changes or copied private keys are a release prerequisite. Updater keys use temporary
restricted files; Tauri needs `TAURI_SIGNING_PRIVATE_KEY` pointing to the prepared key file.
Off-CI recovery copies live in Bitwarden, read with `codeterm mem secret get --name <slug>` and
never printed: `codeterm-tauri-minisign-private-key` (updater signing, empty password; its public
key is `tauri.conf.json` `plugins.updater.pubkey`) and `codeterm-apple-notary-env`. The Developer ID
certificate is backed up only as the `APPLE_CERTIFICATE`/`APPLE_CERTIFICATE_PASSWORD` repo secrets.
Windows updater signatures are not Authenticode: no Authenticode signing is currently configured.

Use the existing sccache policy and named cache-verify/cache-stats operations. Configured zero
counters do not prove a live cache or reuse. Container evidence comes from its own cache server,
not host statistics. Recovery/image baking follows the build-machine skill and assigned authority;
no automatic reboots, pruning, runner provisioning or global configuration changes.
Rust warning reports must be complete and zero-new-warning. Fix owning-platform warnings; never
increase baseline allowances or treat missing/cancelled reports as clean. Retrieve CI report evidence
through the existing ci-artifact tooling rather than assuming GitHub artifact storage.

## Monitoring and failure recovery

Read `codeterm docs BLUEPRINTS`. Use one named monitor per actual run ID; validate its probe once
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
must reuse original verified artifacts/fragments, not handwrite a reduced latest.json.

Served bytes are not the whole proof: on a host still running the previous version,
`codeterm update install --auto` must install this release and come back on the new pair with
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
Deliver results once through `codeterm agent report`; milestones and blockers may be short updates.

After publication, verify the updater/install/restore surfaces authorized by the owner. Cleanup
follows the lane-cleanup and build skills, with ancestry checks and preserved unmerged work;
never sweep caches or delete worktrees merely because the release ended.

Implementation entry points: [release stage](../../../scripts/release-stage.mjs),
[macOS x64 helper](../../../scripts/release-macos-x86_64.sh).

<!-- revision: 6 -->
