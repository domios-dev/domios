# Domios local build machine

One `release-X.Y.Z` tag starts a single `.github/workflows/release.yml` run. Its three self-hosted
platform jobs run in parallel; the final manifest job publishes and verifies only after they all
succeed. Nothing is cross-built where a native runner exists.

## Build farm

| Runner | Machine | Workflow | Builds |
|--------|---------|----------|--------|
| **`codeterm-builder`** | this Mac (ARM64) | `release.yml` → `release-builder.yml` | macOS `x64`+`arm64` (Developer ID + notarized) **and** `arm64` Linux `.deb`+`.AppImage` (Docker) |
| **`ROG-windows-build`** (`windows-build`) | ROG, Windows (x86_64) | `release.yml` → `release-windows.yml` | Windows `x64`+`arm64` NSIS, signed on the runner |
| **`linux-build`** | ubunta, Ubuntu 24.04 (x86_64) | `release.yml` → `release-linux.yml` | `x86_64` Linux `.deb`+`.AppImage`, native; also plugin publication and release preparation |
| **`linux-ci`** | ubunta and BLACKSTAR-wsl-ci (WSL2 Ubuntu 24.04 on BLACKSTAR) | `tests.yml` Linux jobs and the PR lint workflows | test pool only: either idle runner takes the job; never packaging or signing |
| **`windows-ci`** | ROG and BLACKSTAR-windows-x64 | `tests.yml` Windows lib and CLI jobs | test pool only; release packaging keeps `windows-build` (arm64 on ROG) and `windows-x64-build` (x64 on BLACKSTAR) |

Strength tiers (runner labels, independent of the release owners above): `heavy` = eight (`codeterm-builder-2`), BLACKSTAR-wsl-ci, BLACKSTAR-windows-x64; `medium` = ROG; `light` = ubunta, plus every heavy runner that can also take quick checks (eight, BLACKSTAR-wsl-ci). Platform-independent quick checks such as `fmt` ask for `light` and run on any free one; the heavy Rust jobs start only after `fmt` passes.

Rust test distribution uses the generic `rust-shard` label on one qualified runner on
**each** machine: eight, air, ubunta, ROG, BLACKSTAR and green. New machines join
this pool after provisioning the native Rust/app dependencies, Git Bash on Windows,
PowerShell, and Node. Keep the OS labels alongside it. Do not add a machine name or
OS to the shared shard job: twelve nextest hash partitions are queued together,
and any idle qualified runner takes the next one. Native `linux-ci`, `macOS`, and
`windows-ci` jobs execute the cfg-gated tests for their OS.

Registration: include `rust-shard` in `config.sh --labels` / `config.cmd --labels`
alongside the existing custom labels; the Windows bootstrap defaults include it.
For an already registered runner, add it without replacing the other labels:
`gh api --method POST repos/rollacode/codeterm/actions/runners/<runner-id>/labels -f 'labels[]=rust-shard'`.
Verify the repository runner inventory lists at least one online pool member per
machine before dispatching `gh workflow run tests.yml --ref ci/<branch>`.

Compile once per supported OS/architecture before distributing tests: macOS
ARM64 on eight (`codeterm-builder`), Linux X64 on ubunta (`linux-build`), and
Windows X64 on one `windows-ci` runner. Each builder publishes workspace and CLI
nextest archives plus a classified inventory. Shared jobs retain only the generic
`rust-shard` label; after assignment they download the artifact matching
`runner.os` and `runner.arch`. Native cfg jobs use those same archives.
Add an archive-builder matrix entry before qualifying a runner with a new
OS/architecture combination. Absent or mismatched archives fail explicitly.

Consumers install nextest without a Rust toolchain, remap the archive workspace
to their source checkout, and run hash partitions without compilation. Alone
confirmations use the same archive. The semantic-test embed sidecar travels
inside the workspace archive. Build receipts report one compile/archive duration
per platform; shard receipts report extraction/execution/audit time separately.

The classifier in `scripts/ci-test-classifier` follows Rust source modules and
inherited cfg gates, including platform cfg attributes inside test bodies.
`ci-rust-shards.mjs` joins this with nextest's compiled inventory for workspace
libraries and CLI test targets. Unknown tests and reasonless ignores fail explicitly; every source-authored ignore
is named with its reason in logs, receipts and the final summary, including
counts of distinct tests and OS/test entries per reason.
The coverage job requires all fifteen successful receipts, identical shared
inventories across every OS, and exactly one primary shard assignment per
runnable shared test. Candidate timing reruns are recorded separately.
Normal suite groups use nextest default parallelism; listener groups remain
serial for their native isolation boundary. Each nonempty nextest subgroup
emits every per-test duration with `--status-level pass`; its captured log must
pass the `check-slow-unit-tests.mjs` ratchet against the fetched
`origin/develop` baseline. Parallel measurements select exact binary/test
candidates; only those candidates rerun serially using saved binary and Cargo
metadata. The guard judges alone measurements and reports both timings in the
receipt and summary. Logs accompany the receipts.

It publishes runner names, test counts and durations in the workflow summary;
failures identify the shard and runner in their logs. Fixture tests enter shared
shards automatically after their fixture-only platform gates are removed.

Why this split:
- **macOS** must be native (Developer ID + notarization need a Mac keychain).
- **arm64 Linux** is native-in-Docker on the ARM Mac.
- **x86_64 Linux** is built natively on the ROG's WSL2 Ubuntu — the ARM Mac can cross-build the
  `x86_64` `.deb` but **not** the `x86_64` `.AppImage` (`linuxdeploy-x86_64` is an x86_64 binary),
  so x86_64 Linux moved to the ROG to get a real AppImage.
- **Windows** is native on the ROG (no SSH, no Mac round-trip; the old SSH path is gone).

The Mac side (`build-all.sh`) runs 2 engines (mac + Docker-arm64) × 2 waves. Builds are clean: a
fresh `git clone` of the tag via an auth-free bundle; the slow toolchains/deps are pre-baked into
the Docker images so per-build startup is fast. The ROG jobs sign on their own runners via
`TAURI_SIGNING_PRIVATE_KEY`; each uploads its assets before the final manifest job updates the
mirror.

## Internal QA gates

Serial named gates for warm-checkout verification (orchestrator stride paste-back):
`scripts/qa-gate.sh` + catalog in [`QA-GATES.md`](QA-GATES.md). No daemon/socket — repo scripts only.

## Files

```
docker/Dockerfile.linux-arm64   baked aarch64 Linux env (webkit/gtk + rust 1.95 + node/pnpm)
docker/Dockerfile.linux-x64     ORPHANED — x86_64 Linux now builds natively on the ROG/WSL runner
                                (release-linux.yml), not cross-compiled here. Kept for ad-hoc use.
scripts/build-linux.sh          thin in-container step: clone tag → pnpm install → tauri build → collect
scripts/build-all.sh            orchestrator: Mac overlap + optional arm64-Linux Docker, signs
scripts/mac-overlap.sh           Mac arm64 notarization overlaps the x86_64 build
```
Windows + x86_64-Linux release builds run on the ROG self-hosted runners
(`release-windows.yml` / `release-linux.yml`); there is no Mac-driven SSH/QEMU path anymore.
macOS build/notarize/pack is reused from `../skills/github-release/scripts/` (`mac-bundle.sh`);
the overlap coordinator is `scripts/mac-overlap.sh`.

## One-time setup

```bash
cd .codeterm/build-machine/docker
docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .
docker build -t codeterm-build-linux-x64   -f Dockerfile.linux-x64   .
```
Plus: macOS Developer ID + notary profile, `~/.tauri/codeterm.key` (minisign updater key).

**Windows host (the ROG), one-time:** registered as a **self-hosted GitHub Actions runner**
(`ROG-windows-build`, labels include `windows-build`) — NOT driven over SSH. Toolchain on the runner:
rustup on the repo-pinned channel (1.95.0) with targets `x86_64-pc-windows-msvc` &
`aarch64-pc-windows-msvc` installed, VS2022 C++ build tools (x64 **and** ARM64 — the aarch64 build
loads `vcvarsall x64_arm64`), node 22 + pnpm 10.32.1, git, LLVM. Windows signing uses the
`TAURI_SIGNING_PRIVATE_KEY` / optional `TAURI_KEY_PASSWORD` repo secrets directly on the runner;
publishing to the mirror uses `PACKAGE_REPOS_TOKEN`. No SSH, no remote-build env — the old
Mac→Windows SSH/QEMU path is gone.

**x86_64 Linux runner (the ROG's WSL2), one-time:** install WSL2 with **Ubuntu 24.04** (glibc 2.39 —
required by the onnxruntime static lib), register a **self-hosted GitHub Actions runner** with label
`linux-build`, and install rustup on the repo-pinned channel (1.95.0), node 22 + pnpm 10.32.1, `gh`,
and the apt deps listed in `release-linux.yml` (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`, `patchelf`). Same signing/mirror
secrets as Windows. Runs natively → produces the `x86_64` AppImage the ARM Mac can't.

## Shared sccache path contract

CI builders use the shared S3 compile cache on ainyiva-bot through `build.sh`.
Provision these identical physical paths before checkout and export the listed
`CARGO_HOME` only in the builder process:

| OS / builders | Checkout | CARGO_HOME |
|---|---|---|
| macOS: eight, air | `/Users/Shared/domios-ci/work/codeterm` | `/Users/Shared/domios-ci/cargo` |
| Windows: ROG, BLACKSTAR, green | `C:\domios-ci\work\codeterm` | `C:\domios-ci\cargo` |
| Linux: ubunta | `/opt/domios-ci/work/codeterm` | `/opt/domios-ci/cargo` |

Only these exact CI checkouts use their checkout-local `target-check` for
exceptional test compilation; other worktrees keep their gateway targets.
Keep Cargo targets machine-local and checkout-local. Pin the repository's Rust
toolchain and match target, profile, features and compilation environment on each
same-OS pair. Sccache 0.17's Rust cache key hashes cwd and Cargo paths;
`SCCACHE_BASEDIRS` does not normalize them. Developer checkout paths remain
unchanged; upstream Rust normalization is deferred. Also match
toolchain executable paths: `CARGO` is hashed. Select the pinned toolchain under
`/Users/Shared/domios-ci/rustup/toolchains/` on macOS,
`C:\domios-ci\rustup\toolchains\` on Windows, or
`/opt/domios-ci/rustup/toolchains/` on Linux in the builder process PATH;
do not change developer toolchain defaults.

Run `codeterm mem secret sync` locally on each authorized builder. Build setup
reads `domios-sccache-s3-access-key` and `domios-sccache-s3-secret-key` into process
environment only, exports endpoint `http://100.88.47.88:9000`, bucket `sccache`,
region `us-east-1`, SSL `false`, and reuses a running server whose storage
matches. Missing keys never downgrade an existing S3 server; its own
credentials keep serving builds. Local-to-S3 switches require resolved keys
and an idle machine. Never put
cache credentials in runner `.env`, workflow arguments,
reports or files. If either entry cannot resolve and no S3 server is running,
build setup logs one local disk fallback line and preserves `SCCACHE_CACHE_SIZE` (default `10G`). Shared macOS
paths must permit builder accounts to write source and disposable artifacts.
The build skill owns troubleshooting and the statistics measurement procedure.

## Run

```bash
.codeterm/build-machine/scripts/build-all.sh v1.4.4
# artifacts land in /tmp/codeterm-<ver>-artifacts/
```

## Hard-won notes / limitations

- **Linux x86_64 AppImage cannot be built on an arm64 host** (`linuxdeploy-x86_64` is an x86_64
  binary that can't run here) — which is exactly why x86_64 Linux moved to the ROG/WSL native
  runner (`release-linux.yml`). The historical Mac approach (cross `.deb` only, via
  `Dockerfile.linux-x64` with multiarch amd64 libs — no sysroot) is retired from the release path;
  the Docker x64 image is kept only for ad-hoc local use.
- **Cross builds need embed/cli stubs pre-created** or `tauri-build` glob validation fails
  (`resource path bundled-embed/codeterm-embed-<triple> doesn't exist`). Handled in the scripts and
  in `release-windows.yml` (the aarch64 cross build pre-creates the target-triple sidecar stubs).
- **Release compilation uses one target contract.** Each platform job sets `CARGO_TARGET_DIR` to the
  checkout's `src-tauri/target`; cross-target lanes carry their target in `CARGO_BUILD_TARGET`,
  and all jobs pin the `embeddings` features with `CARGO_INCREMENTAL=0`. `release-inputs` builds both application bins
  before Tauri bundles them, so the dependency graph is not compiled a second time. The three
  platform workflows intentionally have no Rust target-cache action or target upload.
- **Mac notarization overlaps architectures.** `mac-overlap.sh` completes and verifies the arm64
  app, starts the existing `notarize-dmg.sh` (including stapler and spctl), then runs the complete
  x86_64 pipeline. The arm64 updater bundle is packed only after its notarization succeeds.
- **The final manifest job runs on `linux-build`.** It has bash, jq, gh and python3; it runs only
  after the Mac, Linux x86_64 and Windows platform jobs succeed, then verifies the public manifests.
- **Timing ledger.** The 1.10.22 baseline was Mac 418s arm64 + 427s x86_64 sequential, Windows
  release-input/Tauri 7m04s/5m30s (arm64) and 8m01s/5m15s (x86_64), and Linux 18m45s/16m05s;
  target-cache saves added 42s, 43s, and 1m33s. Record the next real release's corresponding
  before/after step durations and tag-to-publish time with `gh api .../actions/runs/<id>/jobs`.
  No after-release timing is claimed until that release runs.
- **Updater signing is per-runner via a secret.** `TAURI_SIGNING_PRIVATE_KEY` signs the `.sig`
  files: the ROG Windows/Linux runners sign directly; the Mac currently signs AppImage with the
  local `~/.tauri/codeterm.key` (could use the same secret). Only **Apple notarization** is
  genuinely Mac-bound (Developer ID + keychain) — minisign is not.
- **Docker memory:** the `codeterm` lib crate needs ≳8 GiB to compile; set Docker Desktop
  memory to ~20 GiB or it OOM-kills `rustc` (SIGKILL).

## Release runbook (PR-based, tag-driven)

The release is **tag-driven**. One `release-X.Y.Z` push creates a single workflow run with three
parallel platform jobs on their self-hosted runners:

- `.github/workflows/release.yml` calls `release-builder.yml` on the **Mac** (`[self-hosted, codeterm-builder]`) — builds
  macOS + arm64 Linux via `build-all.sh`.
- `.github/workflows/release.yml` calls `release-windows.yml` on the **ROG** (`[self-hosted, windows-build]`) — builds
  + signs the Windows installers for **both** arches.
- `.github/workflows/release.yml` calls `release-linux.yml` on the **ROG's WSL2** (`[self-hosted, linux-build]`) —
  builds + signs `x86_64` Linux `.deb`+`.AppImage` natively.

The `release-manifest` job needs all three platform jobs. It merges every updater fragment, updates
the source and public manifests, marks the releases, verifies public served bytes, and then applies
the public issue-closure contract. A platform failure skips that job and leaves the release draft.

0. **Preflight the release head.** The release branch is cut from `master` and dies at the tag, so
   it cannot drift from it — verify it is still descended from `origin/master`
   (`git merge-base --is-ancestor origin/master origin/release/X.Y.Z`) and, if a hotfix landed on
   `master` meanwhile, merge `master` in, run gates and push before continuing. Visually check dev
   from the main checkout, and clean merged lane worktrees under `../codeterm-worktrees/` so old
   artifacts and stale agents do not influence the release.
1. **Land the release on `master` via a PR.** Open a PR of the release branch → `master`
   (`gh pr create --base master --head <branch>`) and merge it. Direct pushes to `master` are
   intentionally gated — always go through the PR.
2. **Bump + sync the version first** (on the branch, before the PR): set `package.json` version,
   `npm run sync:versions`, `npm run check:versions`, commit as `release: vX.Y.Z`.
3. **Tag on the merged `master` SHA:** after the PR is green and merged, verify local `HEAD` and
   `origin/master` equal the intended merge SHA, verify remote `release-X.Y.Z` and `vX.Y.Z` are
   absent, then `git tag release-X.Y.Z <merged-master-sha> && git push origin release-X.Y.Z`.
   That one tag starts all three platform jobs in a single `release.yml` run.
4. **Published → done.** The run's final manifest job verifies the public updater manifests before
   closing eligible `ready-for-release` issues.

Recovery rule: do not move or overwrite a published release tag/artifact by default. If a bad
public release is intentionally deleted, cut the fix as a new patch version through the same
release branch → master PR path and trigger it with a new `release-X.Y.Z` tag.

Closeout rule: once artifacts publish, remove merged lane worktrees under `../codeterm-worktrees/`,
delete merged `lane/*` branches, close finished worker tabs, stop lane-specific dev daemons, and
run `cargo sweep --time 2` on build machines/checkouts that accumulated Rust/Tauri artifacts. Old
worktrees and `target/` layers are release debt, not something to leave for the next run.

> **Manual re-run / verification:** dispatch `release.yml` with `tag=release-X.Y.Z`. Set
> `mirror=false` to leave releases in draft and skip the public mirror; the caller still builds and
> uploads private artifacts so the platform jobs can be inspected in one run.

## Self-hosted runners

Three self-hosted GitHub Actions runners, each assigned to its platform job by label:

- **Mac** (`codeterm-builder`) — `release.yml` (macOS + arm64 Linux).
- **ROG / Windows** (`ROG-windows-build`, label `windows-build`) — `release-windows.yml` (Windows
  x64 + arm64, built + signed on the runner).
- **ROG / WSL2 Ubuntu** (label `linux-build`) — `release-linux.yml` (x86_64 Linux, native).

No SSH between machines; each runner signs and uploads its platform assets, and the final manifest
job publishes the verified release. Keep the repo private / gate fork PRs — self-hosted runners
execute repo workflow code.

<!-- revision: 4 -->
