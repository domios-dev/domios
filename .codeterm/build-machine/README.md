# Domios local build machine

Each push to `release/X.Y.Z` starts one `release.yml` run. Fast checks run first and gate both the
Rust tests and four platform callers: macOS, Linux arm64, Linux x86_64, Windows. macOS and Windows each have
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

Strength tiers (runner labels, independent of the release owners above): `heavy` = eight (`codeterm-builder-2`), BLACKSTAR-wsl-ci, BLACKSTAR-windows-x64; `medium` = ROG; `light` = ubunta, plus every heavy runner that can also take quick checks (eight, BLACKSTAR-wsl-ci). Platform-independent quick checks such as `fmt` ask for `light` and run on any free one; release Rust tests wait for the fast gate like the builds; standalone test runs keep the fmt prerequisite.

### Pausing a machine (owner is working on it)

`./build.sh runner-pause BLACKSTAR` parks every `BLACKSTAR-*` runner: its custom labels become `paused`, so no workflow job routes there, and any label only it served (today `windows-x64-build`) is lent to an online runner of the same OS (ROG). The saved labels live in `.codeterm/build-machine/paused/BLACKSTAR.json`; commit it so the pause is visible in git. `./build.sh runner-resume BLACKSTAR` restores the labels, takes the lent ones back and deletes the file; `./build.sh runner-status` lists runners and paused machines. Run `scripts/check-release-runners.sh` after either. A pause refuses when a label would be left with no online runner of that OS.

### Observed release/test runner overlap

Inventory read with `gh api repos/rollacode/codeterm/actions/runners` on 2026-10-14:

| Runner | Test labels | Default release labels | Queue overlap |
|---|---|---|---|
| codeterm-builder-2 (eight, macOS ARM64) | `rust-tests` (pool, to add), `rust-shard` (macOS platform subset) | `codeterm-builder` (both macOS architectures and Linux arm64 Docker) | full suite or macOS subset |
| ubunta-linux-build (Linux X64) | `rust-tests` (pool, to add), `linux-ci` (Linux platform subset) | `linux-build` (Linux x86_64) | full suite or Linux subset |
| BLACKSTAR-windows-x64 (Windows X64) | `windows-ci` (Windows platform subset; paused) | `windows-x64-build` | Windows subset |
| ROG-windows-build (Windows X64) | `rust-tests` (pool, to add), `windows-ci` (Windows platform subset) | `windows-build` (Windows arm64 cross-build) | full suite or Windows subset |
| air-macos (macOS ARM64) | `rust-shard` (macOS platform subset), `macos-ci` | none | tests only |
| BLACKSTAR-wsl-ci (Linux X64) | `linux-ci`, `light`, `heavy` | none | fast checks/native tests |
| BLACKSTAR-wsl-light-1 / -2 (Linux X64) | `light` | none | fast checks only |
| green-windows-ci (Windows X64) | `windows-light` | none | no Rust tests (no MSVC) |

All nine runners were online when inventoried. Matching is capability/label based,
so any idle qualified runner takes a job; no runner name is pinned in the workflows.

When the fast gate is green, the Rust tests and all four artifact callers queue
together and run in parallel. A failed test never cancels the run: builds finish,
the run is red, and publication still needs both test stages green.

Rust tests compile and run in place; nothing ships a nextest archive between
runners (an archive download alone took ~12 minutes at ~0.4 MB/s, against a full
app suite of ~2 minutes after an 8-9 minute compile from the shared S3 cache).

- `rust-tests` runs the whole suite (app workspace libraries and the CLI) once, on
  any free runner carrying the `rust-tests` label. The pool is eight (macOS ARM64),
  ROG (Windows X64) and ubunta (Linux X64); green lacks MSVC and BLACKSTAR is paused.
- `rust-platform` runs on macOS (`rust-shard`: eight, air-macos), Linux (`linux-ci`)
  and Windows (`windows-ci`) in parallel. Each runs only the cfg(target_os)-gated
  tests for its OS plus the native input crates (`codeterm-input-macos`,
  `codeterm-input-windows`).
- `rust test receipts` requires exactly one passing full-suite receipt and one
  passing platform receipt per OS, each with a non-zero executed test count.

Coverage trade-off: the full suite exercises only the pool runner's OS. Tests gated
to another OS run only in that OS's platform subset, which the nightly run on
develop repeats daily with the same graph.

Registration: add `rust-tests` to the pool runners without replacing their labels:
`gh api --method POST repos/rollacode/codeterm/actions/runners/<runner-id>/labels -f 'labels[]=rust-tests'`.
Keep `rust-shard` on the macOS runners (the platform job's macOS label) and the OS labels everywhere.

The classifier in `scripts/ci-test-classifier` follows Rust source modules and
inherited cfg gates, including platform cfg attributes inside test bodies.
`ci-rust-tests.mjs platform` joins it with nextest's compiled inventory; unknown
tests and reasonless ignores fail explicitly and every source-authored ignore is
named with its reason in the log. Listener groups stay serial for their native
isolation boundary; other groups use nextest default parallelism and emit every
per-test duration for the `check-slow-unit-tests.mjs` ratchet. Non-release macOS
runs enforce it with serial confirmation of the candidates; release runs and the
Linux and Windows runs report parallel timings as advice.

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
Docker uses an isolated clean target and owns its container-local S3 cache server.
The host passes vault credentials only through process environment; host cache statistics
do not describe container compilations. Native CI only verifies its prestarted server.

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

CI builders use the shared S3 compile cache on build-mac through `build.sh`.
Provision the identical macOS/Windows physical paths and user-owned Linux paths before checkout and export the listed
`CARGO_HOME` only in the builder process:

| OS / builders | Checkout | CARGO_HOME |
|---|---|---|
| macOS: eight, air | `/Users/Shared/domios-ci/work/codeterm` | `/Users/Shared/domios-ci/cargo` |
| Windows: ROG, BLACKSTAR, green | `C:\domios-ci\work\codeterm` | `C:\domios-ci\cargo` |
| Linux: ubunta | `/home/dev/domios-ci/work/codeterm` | `/home/dev/domios-ci/cargo` |
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
`/home/dev/domios-ci/rustup/toolchains/` on ubunta or
`/home/ci/domios-ci/rustup/toolchains/` in BLACKSTAR WSL in the builder process PATH;
do not change developer toolchain defaults.

Run `codeterm mem secret sync` locally on each authorized builder. Build setup
reads `domios-sccache-s3-access-key` and `domios-sccache-s3-secret-key` into process
environment only, exports endpoint `http://100.64.10.5:9000`, bucket `sccache`,
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

## Measured shared-cache reuse

Source fix `872e5793fd331cd28b039f5ee32be300dad961da` wires native release
wrappers and Docker cache setup, routes Windows archive builds through pwsh,
and rejects synthetic offline sccache statistics. Sccache 0.16 can print zero
counters and a configured S3 location without a reachable server; use
`build.sh cache-verify` and `cache-stats` for a live endpoint check.

Air ran two clean-target `release-inputs` builds at that exact SHA, using Rust
1.95.0, sccache 0.16.0, identical checkout/compiler/Cargo paths and the same
S3 prefix. Both exited 0; no suite, packaging or publication ran.

| Phase | Wall seconds | Hits / misses | Rust hits / misses | Hit rate |
|---|---:|---:|---:|---:|
| Producer | 770.687 | 355 / 2027 | 0 / 913 | 14.90% |
| Consumer | 396.604 | 2380 / 2 | 912 / 1 | 99.92% |

Both recorded 2618 compile requests and zero cache errors/timeouts. The shared
cache already contained some native objects before the producer; this was a
clean target, not an empty S3 bucket. Consumer wall time fell 48.54% on air.
The separate Windows app-test measurement reused 784 Rust entries from ROG on
green (87.79% Rust hits), but green took longer overall; compiler installation
paths differed and no Windows wall-time speedup was established.

Backlog qualification: rebuild the existing Linux arm64 Docker image and verify
S3 support inside it; then run two sequential nonpublishing dry runs of the
same merged SHA for all six release variants. Full signed-job cache statistics,
eight-to-air clean app-test reuse, BLACKSTAR WSL runner-context provisioning,
and common Linux physical paths remain separate follow-ups. The historical
air server shutdown/endpoint anomaly is not uniquely explained by its retained
logs; the fixed same-SHA measurement establishes current wrapper usage.

<!-- revision: 7 -->
