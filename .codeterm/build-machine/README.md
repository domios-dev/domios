# Domios local build machine

Each push to `release/X.Y.Z` starts one `release.yml` run. Preparation queues Rust archives and parallel fast checks together; fast checks gate
four platform callers: macOS, Linux arm64, Linux x86_64, Windows. macOS and Windows each have
independent architecture matrix jobs with `fail-fast: false`. The final manifest needs every
platform caller and both test stages, verifies downloaded draft artifact signatures, then publishes.

## Build farm

| Runner | Machine | Workflow | Builds |
|--------|---------|----------|--------|
| **`codeterm-builder`** | this Mac (ARM64) | `release.yml` → `release-macos-build.yml` + `release-linux-arm64.yml` | independent macOS `x64`/`arm64` (Developer ID + notarized) and independent `arm64` Linux `.deb`+`.AppImage` (Docker) |
| **`ROG-windows-build`** (`windows-build`) | ROG, Windows (x86_64) | `release.yml` → `release-windows.yml` | Windows `x64`+`arm64` NSIS, signed on the runner |
| **`linux-build`** | ubunta, Ubuntu 24.04 (x86_64) | `release.yml` → `release-linux.yml` | `x86_64` Linux `.deb`+`.AppImage`, native; also plugin publication and release preparation |
| **`linux-ci`** | ubunta and BLACKSTAR-wsl-ci (WSL2 Ubuntu 24.04 on BLACKSTAR) | `tests.yml` Linux jobs and the PR lint workflows | test pool only: either idle runner takes the job; never packaging or signing |
| **`windows-ci`** | ROG and BLACKSTAR-windows-x64 | `tests.yml` Windows lib and CLI jobs | test pool only; release packaging keeps `windows-build` (arm64 on ROG) and `windows-x64-build` (x64 on BLACKSTAR) |

Strength tiers (runner labels, independent of the release owners above): `heavy` = eight (`codeterm-builder-2`), BLACKSTAR-wsl-ci, BLACKSTAR-windows-x64; `medium` = ROG; `light` = ubunta, plus every heavy runner that can also take quick checks (eight, BLACKSTAR-wsl-ci). Platform-independent quick checks such as `fmt` ask for `light` and run on any free one; release Rust archives queue independently of fast checks; standalone full tests keep the fmt prerequisite.

### Observed release/test runner overlap

Inventory read with `gh api repos/rollacode/codeterm/actions/runners` on 2026-10-14:

| Runner | Test labels | Default release labels | Queue overlap |
|---|---|---|---|
| codeterm-builder-2 (eight, macOS ARM64) | `rust-shard`, `codeterm-builder` (macOS archive) | `codeterm-builder` (both macOS architectures and Linux arm64 Docker) | Mac archives/shards |
| ubunta-linux-build (Linux X64) | `rust-shard`, `linux-ci`, `linux-build` (Linux archive) | `linux-build` (Linux x86_64) | Linux archives/shards |
| BLACKSTAR-windows-x64 (Windows X64) | `rust-shard`, `windows-ci` (Windows archive/native tests) | `windows-x64-build` | Windows archives/shards |
| ROG-windows-build (Windows X64) | `rust-shard`, `windows-ci` (Windows archive/native tests) | `windows-build` (Windows arm64 cross-build) | Windows archives/shards |
| air-macos (macOS ARM64) | `rust-shard`, `macos-ci` | none | tests only |
| BLACKSTAR-wsl-ci (Linux X64) | `linux-ci`, `light`, `heavy` | none | fast checks/native tests |
| BLACKSTAR-wsl-light-1 / -2 (Linux X64) | `light` | none | fast checks only |
| green-windows-ci (Windows X64) | `windows-light` | none | no current Rust shard assignment |

All nine runners were online. The inventory has five `rust-shard` runners;
green currently lacks that label. Matching remains capability/label based, so
any idle qualified runner can take a job; no runner name is pinned in the workflows.
Every default release builder overlaps tests. At source preparation completion,
Rust archives and the parallel fast checks queue together; archives have no fast-gate
prerequisite. When the fast gate is green, all four artifact callers queue immediately,
with no archive-completion dependency. A free runner matching a build's labels may
start it while archives/tests still run. Shards and native tests queue as soon as
archives complete, followed by coverage. Any failed Rust test requests cancellation
before receipt upload; a failed gate also cancels the caller run.

GitHub has no runner queue priority. Earlier archive submission gives tests a head
start on shared machines; queue order is best effort, not a guarantee. Builds are
never serialized behind an OS archive by the graph. Refresh this inventory when
changing repository routing variables or runner labels; test-free builders need no
extra dependency and already start as soon as fast checks pass.

Rust test distribution uses generic `rust-shard` on qualified runners. Keep the OS
labels alongside it. Twelve nextest hash partitions are queued together, and any idle
qualified runner takes the next one. Native Linux, macOS and Windows jobs execute
cfg-gated tests for their OS from the same compiled archives.

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

CI builds macOS and Linux arm64 independently; `build-all.sh` remains a manual helper.
Linux arm64 receives an auth-free source bundle and runs in the baked native arm64 container.
Each artifact job clears only its own incomplete upload slots and emits its own updater fragment.
Builds use checkout-local targets; the verify-only cache setting prohibits CI cache reconfiguration.
Docker uses its isolated clean target and does not provision a shared cache server.

### Runner routing and retries

| Repository variable | Default existing runner label | Artifact |
|---|---|---|
| `RELEASE_MACOS_ARM64_LABEL` | `codeterm-builder` | macOS arm64 |
| `RELEASE_MACOS_X64_LABEL` | `codeterm-builder` | macOS x64 |
| `RELEASE_LINUX_ARM64_LABEL` | `codeterm-builder` | Linux arm64 Docker |
| `RELEASE_WINDOWS_X64_LABEL` | `windows-x64-build` | Windows x64 |
| `RELEASE_WINDOWS_ARM64_LABEL` | `windows-build` | Windows arm64 cross-build |
| `RELEASE_LINUX_X64_LABEL` | `linux-build` | Linux x86_64 |

Defaults require no runner reconfiguration. Jobs targeting the same runner queue independently;
a Linux arm64 retry never rebuilds or re-signs macOS. To move an artifact, provision its toolchain
on an existing approved machine, add a custom label without removing
current labels, then set the corresponding repository variable:

```bash
gh api --method POST repos/rollacode/codeterm/actions/runners/<runner-id>/labels \
  -f 'labels[]=<artifact-label>'
gh variable set RELEASE_LINUX_ARM64_LABEL --repo rollacode/codeterm --body '<artifact-label>'
```

Confirm an online runner carries the label before changing the variable. New machines/VMs require
owner approval. Update credential preflight routing with the same variable. Linux arm64 needs a
native ARM64 Docker daemon and `codeterm-build-linux-arm64`; CI fails immediately if either is absent.
Use `gh run rerun <run-id> --job <failed-job-id>` or `--failed`, never `--all` for artifact recovery.
See the github-release skill for the graph, source-SHA rule and partial publication recovery.

Every platform unpacks and structurally checks its built bundle before upload. The smoke validates
app/CLI/embed architectures, source SHA, plugins, frontend resources and version; executable CLI
architectures run `--version`. macOS checks strict signatures and DMG staple. Windows logs current
Authenticode status without requiring it. No release app launches on the builder: a future Rust
isolation mode must protect profiles, single-instance routing and global integrations first.

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
Mac release runners need Xcode command line tools, Rust, Node and pnpm; Linux arm64 also
needs Docker and the baked arm64 image. Signing state comes from repository secrets, not
machine provisioning. Every macOS matrix job and the Mac-hosted Linux arm64 job imports
`APPLE_CERTIFICATE` with `APPLE_CERTIFICATE_PASSWORD` into its own `RUNNER_TEMP` keychain,
imports the pinned public Apple Developer ID intermediates, configures codesign access,
and validates the Apple notary credentials there. Existing identities
in other keychains remain untouched. `APPLE_SIGNING_IDENTITY` matches the imported identity
by full name or fingerprint; setup verifies the team and valid certificate chain, then
exports the fingerprint for Tauri and codesign.
`TAURI_SIGNING_PRIVATE_KEY` is written to a mode-0600 job file; `TAURI_KEY_PASSWORD` is passed
through the signer environment. An `always()` step removes the keychain, its search-list entry,
and the updater key, including after failures. Credential preflight uses the same setup.

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
Provision the identical macOS/Windows physical paths and user-owned Linux paths before checkout and export the listed
`CARGO_HOME` only in the builder process:

| OS / builders | Checkout | CARGO_HOME |
|---|---|---|
| macOS: eight, air | `/Users/Shared/domios-ci/work/codeterm` | `/Users/Shared/domios-ci/cargo` |
| Windows: ROG, BLACKSTAR, green | `C:\domios-ci\work\codeterm` | `C:\domios-ci\cargo` |
| Linux: ubunta | `/home/rollacode/domios-ci/work/codeterm` | `/home/rollacode/domios-ci/cargo` |
| Linux: BLACKSTAR-CI WSL | `/home/ci/domios-ci/work/codeterm` | `/home/ci/domios-ci/cargo` |

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
`/home/rollacode/domios-ci/rustup/toolchains/` on ubunta or
`/home/ci/domios-ci/rustup/toolchains/` in BLACKSTAR WSL in the builder process PATH;
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

CI uses `./build.sh cache-verify` (Windows: `.\build.ps1 cache-verify`). It prints
configured and actual storage plus endpoint health, exits 0 only when both storage
checks select the shared S3 bucket, and otherwise exits 2. It never starts or
restarts a server or checks for an idle machine. Set
`CODETERM_SCCACHE_VERIFY_ONLY=1` in each CI build process so build setup follows
that same verification path. Provision local-to-S3 storage separately while idle.

BLACKSTAR-CI WSL builds are owned by a Windows runner. That Windows parent runs
`codeterm mem secret sync`, resolves both vault entries into `AWS_ACCESS_KEY_ID`
and `AWS_SECRET_ACCESS_KEY`, and sets `CODETERM_SCCACHE_CREDENTIAL_SOURCE=runner`.
Forward these three names through `WSLENV` to the WSL process; append them to any
existing `WSLENV` list. Values stay exclusively in process environment, never
runner `.env`, command arguments, files or logs. This explicit source selects
inherited runner credentials instead of querying an unavailable WSL vault CLI.
Both values are required. No standalone WSL Worker builds are supported; keep
Windows interop and mount settings unchanged. Select pinned Rust 1.95.0 only in
the build process, preserving the machine's default toolchain.

Post-release follow-up: establish a shared Linux user or an approved `/opt` path
and compare cold/consumer hit and miss data at identical checkout, Cargo and
compiler paths. The 1.14.7 Linux contract accepts different paths and does not
claim cross-machine reuse.

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
- **macOS architectures rerun independently.** Matrix siblings each build, notarize, staple and
  pack their own bundle. `mac-overlap.sh` is used only by the manual combined helper.
- **The final manifest job runs on `linux-build`.** It has bash, jq, gh and python3; it runs only
  after macOS, Linux arm64, Linux x86_64, Windows and tests succeed, then verifies the public manifests.
- **Timing ledger.** The 1.10.22 baseline was Mac 418s arm64 + 427s x86_64 sequential, Windows
  release-input/Tauri 7m04s/5m30s (arm64) and 8m01s/5m15s (x86_64), and Linux 18m45s/16m05s;
  target-cache saves added 42s, 43s, and 1m33s. Record the next real release's corresponding
  before/after step durations and tag-to-publish time with `gh api .../actions/runs/<id>/jobs`.
  No after-release timing is claimed until that release runs.
- **Updater signing is per-runner via a secret.** `TAURI_SIGNING_PRIVATE_KEY` signs the `.sig`
  files: the ROG Windows/Linux runners sign directly; the Linux arm64 job signs with the same secret after Docker returns its artifacts. Only **Apple notarization** is
  genuinely Mac-bound (Developer ID + keychain) — minisign is not.
- **Docker memory:** the `codeterm` lib crate needs ≳8 GiB to compile; set Docker Desktop
  memory to ~20 GiB or it OOM-kills `rustc` (SIGKILL).

## Release runbook

The canonical procedure is `.agents/skills/github-release/SKILL.md`. Cut `release/X.Y.Z` from
`develop`, bump and synchronize versions plus lockfiles, then push the release branch and open
its PR into `master`. Each push starts the release; no trigger tag is pushed manually. Publication
runs serialize across versions and never cancel each other mid-upload. The final job stamps the
source tag after the test/platform gates, verifies draft signatures, publishes and verifies served
bytes. Promotion merges only the exact verified PR head, merges master back into develop and
deletes the branch idempotently.

A platform/test failure preserves successful artifact uploads in drafts. Recover an environmental
failure with the original run's single failed job or `--failed`. A changed source SHA needs a full
new run. A partial publication must recover its manifest job in the original run, not silently
skip its unfinished verification. Never overwrite a published version with replacement builds.

Dry-run input `dry_run_tests` defaults true. Dry runs build and sign all artifacts, structurally
validate them and run the full test matrix; they skip all remote release mutations. No workflow
change requires runner label changes before the next run. The runner-routing table above gives
existing-label defaults and later migration instructions.

Closeout: remove merged lanes only after ancestry proof, close finished tabs and reclaim stale
build intermediates through the build skill's safe cache procedure.

## Self-hosted runners

Release runner roles are assigned by labels from the routing table:

- **Mac** (`codeterm-builder`) — `release.yml` (macOS + arm64 Linux).
- **ROG / Windows** (`ROG-windows-build`, label `windows-build`) — `release-windows.yml` (Windows
  x64 + arm64, built + signed on the runner).
- **ROG / WSL2 Ubuntu** (label `linux-build`) — `release-linux.yml` (x86_64 Linux, native).

No SSH between machines; each runner signs and uploads its platform assets, and the final manifest
job publishes the verified release. Keep the repo private / gate fork PRs — self-hosted runners
execute repo workflow code.

<!-- revision: 6 -->
