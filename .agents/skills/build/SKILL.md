---
name: build
description: "Choose and run Domios build operations, including dev work, local installs, and troubleshooting."
---

# Domios — Build Guide

## Prerequisites

### Windows
- **Rust** 1.93+ (`rustup install stable`)
- **Node.js** 22+ with **pnpm** (`npm install -g pnpm`)
- **MSVC Build Tools** — Visual Studio 2022 Build Tools with:
  - `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`
  - `Microsoft.VisualStudio.Component.Windows11SDK.22621`
- **WebView2** — comes with Windows 11, install separately on Windows 10

### macOS
- Xcode Command Line Tools
- Rust, Node.js, pnpm

### Linux
- `build-essential`, `libwebkit2gtk-4.1-dev`, `libssl-dev`, `libgtk-3-dev`
- Rust, Node.js, pnpm

## Quick Start

```bash
git clone <repo-url> codeterm && cd codeterm
pnpm install
./build.sh dev          # development (hot-reload, foreground)
./build.sh build        # production build
```

### Windows

```powershell
# Daily commands; run from the repository root.
.\build.ps1 dev           # foreground dev on 7686
.\build.ps1 check         # incremental compile check
.\build.ps1 worktree-check # canonical integration compile check
.\build.ps1 cli           # rebuild only the installed CLI
.\build.ps1 install-local # incremental production build + self-install
```

`build.ps1` is the Windows command hub; do not add a Makefile or hand-expand
its underlying Cargo, pnpm, gateway, or installer commands. `install-local`
already resumes the calling tab with a default continuation intent. Keep the
command short unless the continuation itself must be customized.

## Choose one operation

These flows are deliberately separate. Full rebuilds, dev instances, full
suites and gates run from the warm canonical checkout under Orchestrator
authority. A Worker lane may run isolated checks only; in this repository that
means `tsc --noEmit`, a single node test file, `cargo fmt --check`,
`./build.sh test-one plan|pty <NAME>`, and `./build.sh check-queue` (see below). `check-tests` compiles app and CLI test
targets without running them; its artifacts are reused by matching `test-one`
app and CLI calls in the same worktree. These app builds are deferred to the
Orchestrator's canonical wave batch.

| Goal | Entry point | Touches production? | Read first |
|---|---|---:|---|
| Develop and inspect changes | `./build.sh dev` / `.\build.ps1 dev` | No; port 7686 only | `.agents/skills/dev-run/SKILL.md` |
| Compile both Rust test profiles (a full rebuild, run in canonical) | `./build.sh check-tests` / `.\build.ps1 check-tests` | No; shared Cargo gateway | `.agents/skills/gates/SKILL.md` |
| Build and test a local production installer | `./build.sh release-preflight`, then `./build.sh build` | Only the explicit self-install step | this section |
| Publish to all users | PR to `master`, then `release-X.Y.Z` tag | Yes; CI and public updater mirror | `.agents/skills/github-release/SKILL.md` |

Never use a dev build as a local-install artifact, and never turn a local
installer build into a public release by uploading it manually.

The utility and browser crates use `./build.sh test-one util|browser <selector>`; these focused operations acquire the build gateway and accept `--wait-build`.
The isolated detection crate uses `./build.sh test-one agent-detect <selector>` through the build gateway.
The notification domain crate uses `./build.sh test-one notification-core <selector>`; focused runs enter the build gateway and support `--wait-build`.

The isolated billing crate uses `./build.sh test-one billing <selector>`; workers defer Rust execution to canonical.

The isolated usage crate uses `./build.sh test-one usage <selector> --wait-build`; it queues through the machine build gateway.

The isolated account crate uses `./build.sh test-one account <selector>`; workers defer Rust execution to canonical.

The repository watcher crate uses `./build.sh test-one repowatch tests::`; its Tauri event adapter remains in the app.

The native recording crates use `./build.sh nextest input-platform` to compile
both library test targets once and run their focused cases. macOS IOHID model
cases also run on Linux/Windows; Raw Input cases compile only on Windows.
`./build.sh test-one input-macos|input-windows <selector>` targets one crate;
select Windows only on Windows. Workers defer Rust execution to canonical.

## Generated artifacts and the embedded-doc boundary

`prod-docs/` is the only documentation embedded in the binary. Its membership is the `cli/src/cmd/docs.rs` registry, and every registered name maps to a same-named file; both directions are enforced by tests. `essences/` and `.agents/skills/` remain repository sources and are not embedded.

A generated artifact is produced from a source of truth and is never hand-edited. Regenerate it in the same lane and commit it with its source change before handoff, so no consumer ever sees a stale artifact.

| Source of truth | Generated artifact | Regenerate | Verify |
|---|---|---|---|
| CLI clap definitions in `cli/src/args/` | `generated/cli/cli-manifest.json`, `generated/cli/cli.md`, and `packages/chat-ui/src/codeterm/commandPaths.ts` | Build the checkout's CLI, then run `cli-manifest --write` (`./build.sh cli` also installs the CLI into the active dev data directory). Never use the installed `codeterm` from `PATH`. | `cli_manifest_is_fresh`, `docs_cli_md_is_fresh`, and `chat_ui_command_paths_are_fresh` |
| `essences/capabilities.md` and `essences/core.md` | `essences/map.jsonl` | `codeterm essence-map generate --root .` | `codeterm essence-map validate --root .` and `essence_map_contract_committed_artifacts_are_fresh` |
| `src-tauri/prompts/**` and their loader consts | the `.agents/skills/prompts/inventory.md` inventory | edit the inventory row by hand | `bash scripts/check-prompts-doc.sh` |
| Registered public routes in `src-tauri/src/api/routes.rs` | `prod-docs/API.md` | edit the API index by hand | `bash scripts/check-api-docs.sh` |
| `prod-docs/*.md` and `cli/src/cmd/docs.rs` | the embedded `codeterm docs` set | update the source and registry; there is no separate document generator | `registered_documents_have_one_title_and_fit_the_reading_boundary` and `codeterm docs <NAME>` from the built checkout |

A lane Worker runs isolated checks only. When a generator needs the checkout's built CLI (`essence-map` also refuses a linked worktree), list the exact regeneration and verification commands under `GATES REQUESTED` for the Orchestrator's canonical wave batch.

## Build timing and warm reuse

`./build.sh build-history [--last N] [--verb V] [--json]` (Windows:
`.\build.ps1 build-history`) reads the existing machine gateway job records,
without acquiring the build slot or starting a cache server. The default is 50
jobs, newest start/queue time first. Verb filtering happens before the limit.
The table shows UTC time, verb, elapsed/duration, exit, canonical/lane/check
worktree, SHA, known crates or touched-file count, and sccache hit rate.
Queued jobs have no duration; running jobs show elapsed time at the query clock.
Successful completed jobs in that same selected window contribute to per-verb
n, median, nearest-rank p90 and max. Trend compares the newest ten successful
durations with the preceding ten; fewer than twenty reports insufficient samples.
A positive trend means slower builds. JSON durations are milliseconds; hit rate
is a fraction of hits plus misses, excluding non-cacheable requests.

New gateway jobs retain start/end sccache snapshots and request/hit/miss deltas
in their existing JSON. Missing/offline stats, storage changes, detected counter
resets and gateway-owned server restarts yield an unavailable delta. These are
server-interval counters: compiler clients outside the gateway can contribute.
Old records have no cache evidence; `Compiling` alone never proves a cache miss
or an S3 hit. Check-queue records the requested SHA, selected crate set and
touched-file count, and preserves its full compiler log beside that job.

Cargo fingerprint tracing is off by default. Set `CODETERM_BUILD_FINGERPRINTS=1`
for an evidence run: trace lines go directly to `jobs/<id>.fingerprint.log`,
bypassing the FIFO and tee. The job log prints only bounded dirty-reason counts,
and compact unit/stale-reason entries enter the job JSON. History shows up to ten
rebuilt units per job, ranked by observed dirty events; the complete list is in
JSON. Missing Cargo reasons stay unknown. This counts invalidations, not CPU time.

Normal idle cache-server starts enable `sccache::server=debug` and write
`sccache-server.log` in the gateway directory (or use `SCCACHE_ERROR_LOG`). Each
job copies only its new `CannotCache` lines into `jobs/<id>.sccache.log`, preserving
the observed Rust unit, crate type and reason (including build scripts and
proc-macros). A pre-existing server keeps its logging configuration: per-unit
evidence remains unavailable until a normal owned idle server transition enables
logging. History says so alongside aggregate non-cacheable reason counts; do not
restart a matching live server solely to populate history. Raw side logs remain
diagnostic artifacts, not a second job store. No history can reconstruct old reasons.

Focused proof: `node --test scripts/build-history.test.mjs scripts/build-gateway-evidence.test.mjs`,
`bash -n build.sh && bash -n scripts/build-gateway.sh && bash -n scripts/cache-policy.sh`, and
`.\build.ps1 build-history --last 5 --json` on BLACKSTAR.

`release-inputs` (also used by `build` and `install-local`) records Cargo HTML
timings for CLI and app/embed preparation under the operation’s target
`cargo-timings/` directory. App preparation enables `tauri/custom-protocol`,
matching Tauri packaging so its dependency graph can be reused. Preserve that
feature when changing release preparation.

If packaging recompiles native dependencies, Tauri and the app after preparation,
compare `release/.fingerprint/*/run-*.json` environment records. Tauri CLI sets
`MACOSX_DEPLOYMENT_TARGET` from `bundle.macOS.minimumSystemVersion`; a transition
from unset to that value invalidates native build scripts and their dependents.
`prepare_release_bundle_inputs` exports the same config value before either Cargo
build. Keep this environment shared through packaging; do not hardcode a second
minimum version or change the tuned profiles to hide the rebuild.

CLI builds rely on `cli/build.rs` watching Git HEAD and its branch reference;
do not touch `cli/src/main.rs` to force a rebuild. Repeated builds at the same
SHA reuse Cargo artifacts, while commits refresh the embedded CLI SHA.

## Shared compile cache


Release-input preparation now calls the same cache policy before invoking Cargo.
`ci-archives` owns archive compilation through that policy; on Windows use
`.\build.ps1 ci-archives`, which locates Git Bash explicitly without requiring it
on PATH. Native release jobs export `RUSTC_WRAPPER=sccache` at job scope so the
separate Tauri invocation inherits it, verify shared storage before compiling,
and print `cache-stats` in their final step.

`cache-stats` queries the same `SCCACHE_SERVER_PORT` or Unix
`SCCACHE_SERVER_UDS` as the compiler wrapper and requires a live listener.
Sccache 0.16's `--show-stats` can return synthetic zero counters and a configured
S3 location when no server is reachable. These are not a live-server receipt.
`cache-status` now labels actual storage unavailable in that case, and
`cache-verify` fails. No listener is started by these read-only operations.
A live socket plus stats describes the selected endpoint; it does not prove that
a completed compiler invocation used it. Capture before/after compile requests
and hits to prove usage, and distinguish fresh Cargo targets from cache hits.


`build.sh` owns cache setup through `scripts/cache-policy.sh`; `build.ps1`
forwards to it. Release, DEV and capped Rust tests use the same configuration.
On eight, air, ubunta, ROG, BLACKSTAR and green, first run
`codeterm mem secret sync`. The two vault entries are
`domios-sccache-s3-access-key` and `domios-sccache-s3-secret-key`.
Build setup resolves both directly into process environment variables
`AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`; never write them to a file,
argv, logs or memory. It exports `SCCACHE_ENDPOINT=http://100.64.10.5:9000`,
`SCCACHE_BUCKET=sccache`, `SCCACHE_REGION=us-east-1` and
`SCCACHE_S3_USE_SSL=false`. Setup reads the running server's actual location
first. Matching S3 bucket or local disk storage is reused, even while
Cargo is active. Only a storage mismatch restarts the server; an active
Cargo/rustc process then refuses the change with exit 75. Serialize machine
build ownership before changing storage. Each setup checks the actual server;
no inherited process marker can bypass a required storage transition.
A caller that cannot resolve the keys reuses any running S3 server with its
server-owned credentials; it never downgrades that server to local disk.
A matching S3 bucket keeps its running key prefix, even if a caller requests
another prefix. Prefix/credential/size maintenance needs an explicitly owned
idle server transition; ordinary builds never reset a matching server.
Every setup prints `sccache: shared S3 cache at <endpoint>/<bucket>` or
`sccache: local disk cache at <dir> (reason)`, without credentials.
When either secret is unavailable and no S3 server is running, one log line
announces local disk fallback;
`SCCACHE_CACHE_SIZE` retains its existing default of `10G` and custom limits.
A configured shared endpoint that cannot start is an error, not a claimed hit.

`./build.sh cache-status` (Windows: `.\build.ps1 cache-status`) resolves the
configured location, reads the running server's actual `Cache location`, and
checks the shared endpoint's health URL. It starts no build or cache server,
and changes no hooks or compiler environment. Configured and actual locations
can differ until the next idle setup restarts the server.

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

For cross-machine Rust reuse, macOS and Windows CI use identical **physical** paths.
Linux uses user-owned per-machine paths for 1.14.7; cross-machine Linux hits are
not expected because the usernames differ. No sudo or shared user is required:

| Builders | Checkout | CARGO_HOME |
|---|---|---|
| eight, air | `/Users/Shared/domios-ci/work/codeterm` | `/Users/Shared/domios-ci/cargo` |
| ROG, BLACKSTAR, green | `C:\domios-ci\work\codeterm` | `C:\domios-ci\cargo` |
| ubunta | `/home/dev/domios-ci/work/codeterm` | `/home/dev/domios-ci/cargo` |
| BLACKSTAR WSL | `/home/ci/domios-ci/work/codeterm` | `/home/ci/domios-ci/cargo` |

Only the exact CI checkout paths above use checkout-local `target-check` for
exceptional test compilation; other worktrees retain gateway target isolation.
Each machine owns its own clean Cargo target; no target directory is shared
across machines or worktrees. Pin the repository toolchain, target triple,
profile, features and build environment on each same-OS pair. Developer
checkouts stay at their existing paths and use shared storage when credentials
resolve, but different paths may miss. Shared macOS directories need write
access for each local builder account; they contain source and disposable Cargo
artifacts, never credentials.

Sccache 0.17 documents `SCCACHE_BASEDIRS`, but its Rust
[`generate_hash_key`](https://github.com/mozilla/sccache/blob/v0.17.0/src/compiler/rust.rs)
hashes cwd and Cargo environment paths directly, without that rewriting.
Keep the issue #327 guard; BASEDIRS alone cannot prove Rust path independence.
Upstream Rust normalization is a separate follow-up. Also match
toolchain executable paths: `CARGO` is hashed. Select the pinned toolchain under
`/Users/Shared/domios-ci/rustup/toolchains/` on macOS,
`C:\domios-ci\rustup\toolchains\` on Windows, or
`/home/dev/domios-ci/rustup/toolchains/` on ubunta or
`/home/ci/domios-ci/rustup/toolchains/` in BLACKSTAR WSL in the builder process PATH;
do not change developer toolchain defaults. See the builder path
contract in `.codeterm/build-machine/README.md`.

Measure a newly created checkout at one SHA with an empty target on each
same-OS pair, first the populating builder and then the consuming builder.
Capture `sccache --show-stats`, `rustc -vV`, the SHA, exact command, raw exit and
wall time. Compute hit rate as hits / (hits + misses), excluding noncacheable
calls; a successful build alone is not cache-hit proof.

| Symptom | Cause | Recovery |
|---|---|---|
| Release workflow compiles crates with no sccache requests | Release inputs omitted cache setup, or a separate Tauri process did not inherit the wrapper | Run the owning `release-inputs` policy and export `RUSTC_WRAPPER` at job scope; verify storage before compilation and capture `cache-stats` afterward. |
| Stats report S3 and zero requests with no server process/listener | Sccache emits configured synthetic stats when its server cannot be reached | Use `cache-verify`/`cache-stats` live-endpoint checks; provision while idle, then measure actual request and hit deltas. Do not classify synthetic stats as reuse. |
| CI refuses with exit 75 while another build is running | CI invoked cache configuration instead of read-only verification | Use `cache-verify` and set `CODETERM_SCCACHE_VERIFY_ONLY=1` for subsequent CI build processes. Provision storage separately while idle. |
| A dev build requests S3 but refuses with exit 75 after another caller switched the server back to local | An unresolved-key caller could downgrade a previously shared server | Keep the no-downgrade guard: missing keys reuse running S3; keys may switch local to S3 only while idle. Verify `bash scripts/check-shared-sccache.sh`. |
| Local fallback line on an intended builder | One or both vault entries could not resolve | Run `codeterm mem secret sync` locally, inspect secret backend status, then rerun with the machine idle. Never print the values. |
| Shared server startup fails | Endpoint unreachable, credentials rejected, or installed sccache lacks S3 support | Verify Tailscale connectivity and tool features; inspect metadata and server health without secret logging. Preserve the failed receipt. |
| Windows build reports MSVC not found despite an installed VS 2022 C++ workload | The VC tools are installed outside the default BuildTools directory | Use `setup_env` discovery through the installed `vswhere` utility; the default tools remain first choice, and discovered host/cross compiler paths also enter process PATH. Verify `source build.sh env` and `bash scripts/check-msvc-discovery.sh`; install nothing or change no global PATH for a relocated installation. |
| Windows PowerShell refuses `build.ps1` because scripts are disabled | The shell process uses a restrictive execution policy | Invoke the authorized build through `powershell.exe -NoProfile -ExecutionPolicy Bypass -File build.ps1 <operation>`; this changes only that process, never machine policy. |
| Gateway policy fixture expects `CARGO_TEST_BUILD_CMD="cargo nextest run"` | The guard predates nextest inventory compilation and metadata reuse | Check `cargo nextest list` and the reuse execution command separately; rerun `bash scripts/check-build-gateway-policy.sh`. |
| Same SHA has few cross-machine Rust hits | Physical checkout, registry, toolchain, target or build environment differs | Compare the path contract and `rustc -vV`; do not enable BASEDIRS to conceal Rust path differences. |

The gateway mutex, four-client queue, metadata/readers, cache-start, owner-reap and failed-Cargo regressions run through `./build.sh script-guards` in script-catalog CI.

## Warm check queue for lanes

`./build.sh check-queue [<sha>] [--max-errors N] [--max-warnings N] [--touched-only] [--all] [--lane]` gives a Worker compile feedback on a committed lane SHA without a cold lane build. It takes the machine gateway slot (fail-fast, exit 75 when busy, `--wait-build` only when blocking is intended), verifies and publishes the SHA in the single per-machine check worktree `<gateway dir>/check-worktree`, and runs `cargo clippy --all-targets --no-deps --message-format=short -- <scripts/clippy-allow.sh>` with the worktree's own `target/app` and `target/cli`. The same job also runs an app `cargo check --lib` with the dev profile, the manifest's dev/default features (excluding custom-protocol), no test cfg, and the same inherited Rust flags/profile overrides as dev. Both dev launch and this check use `configure_dev_cargo`; errors and warnings share the same log and policy. Packages come from `git diff <merge-base with develop> <sha>`: `src-tauri/crates/<name>` maps to `-p <name>`, `src-tauri/src` to `codeterm`, `cli/` to the CLI manifest; a crate change also checks `codeterm` unless `--touched-only`. Output is the first N `file:line:col: error[...]` lines, an `errors=N shown=M` total (a nonzero Cargo exit with no parsed diagnostic contributes one failure), the first `--max-warnings` (default 40) unique warning lines, `warnings crate=<name> count=N` per warned crate, `warnings=N shown=M failing=<crates|none>`, and the full log path. Any rustc or clippy warning in a touched crate fails the run (exit 1); `--all` checks the whole app workspace plus the CLI and fails on any warning. `./build.sh check` prints the same per-crate warning lines and a `check: warnings=N` total. Crates deny `warnings` through Cargo `[lints]`, so check-queue warnings normally surface as errors. It never builds in canonical or a lane target. The first run on a machine is cold; subsequent runs are incremental.

Troubleshooting:
- Symptom `sccache: incremental compilation is prohibited` from `rustc -vV`: cause an exported `CARGO_INCREMENTAL`; recovery is to leave it unset (the verb unsets it) and let the profile decide.

## Peer build gateways (mesh build farm)

Mesh peers run the same verbs in their own gateway, with their own warm cache and stable build path, so several machines check lanes in parallel instead of queueing on one gateway. Peers and their fetch-only checkouts are listed in `.codeterm/build-machine/peers.json`; a peer marked `"routable": false` (eight, the release builder) is refused until its owner frees it, and `CODETERM_PEERS_ENABLE=eight` enables it for one call. All remote work goes through `codeterm tab new --mesh <peer> --cmd ...`; never ssh by hand, never put a credential in a command, and never touch a machine's runner pause.

| Verb | What happens |
|---|---|
| `./build.sh peer-setup <peer>` | One bounded build tab on the peer clones `rollacode/codeterm` into the configured checkout if missing, sets the push URL to `fetch-only.invalid`, detaches at the pushed `origin/develop`, runs `./build.sh peer-toolchain` (rustc must equal `rust-toolchain.toml`, plus MSVC on Windows, Xcode CLT on macOS, `cc` on Linux, node, cargo-nextest, sccache) and opens the peer's `Buildpeer` status shell. |
| `./build.sh check-queue <sha> --peer <name\|auto> [--lane] [--os OS]` | The peer fetches `develop` and the SHA, detaches its checkout at the SHA and runs `./build.sh check-queue <sha> [flags] --wait-build` in its gateway. `--lane`, `--all`, `--touched-only`, `--max-errors` and `--max-warnings` pass through unchanged. |
| `./build.sh nextest <scope> [filterset] --peer <name\|auto> [--sha SHA]` | Same, running `./build.sh nextest <scope> [filterset] --wait-build` at the SHA (default HEAD). |
| `./build.sh check-linux [sha] [--force]` | Once per batch: `./build.sh check-workspace <sha>` (`cargo check --workspace --all-targets` for the app workspace and the CLI, any warning fails) on the idle Linux peer, so `cfg(unix)` paths compile. A second call for the same SHA prints the existing receipt; `--force` dispatches again. |
| `./build.sh peer-route [--os OS]` | Shows what `--peer auto` would choose. |

Dispatch returns at once with `peer-job: id=... peer=... remote-tab=... deadline=...` and writes a record under `<gateway dir>/peer-jobs/`. The SHA must be on a remote-tracking branch (push first). Completion is one message to the calling agent tab (`CODETERM_TAB_ID`, or `--reply-tab`) through `codeterm send --require-agent --mesh <origin instance id>`; nothing polls. The message starts `[peer-job <id>] peer=<peer> verb=<verb> sha=<sha> outcome=passed|failed|busy|deadline exit=N wall=Ns`, then the same summary lines the local verb prints (`errors=`, `warnings=... failing=`, `check-queue: dev-lib ...` and `app-test ...` receipts, `check-queue: sha=...`, nextest `Summary [...] N tests run: ...`, `check-workspace: ...`), up to 12 error or failing-test lines, and `remote-log=<peer path> gateway-log=<peer gateway log>`. Bootstrap failures report `outcome=not_setup|busy|bootstrap_failed stage=<step>` instead.

`--peer auto` reads each OS-matching routable peer's load over mesh: `codeterm tab list --mesh <peer>` finds its `Buildpeer` tab and `codeterm tab exec --mesh <peer>` runs `./build.sh peer-status`, which prints one lock-free `PEER_STATUS {json}` line (gateway state, active job, queue length, last job end, peer job lock). The least-loaded idle peer wins, ties going to the peer idle longest; a peer that is unreachable, not set up or slower than the 20 s probe deadline is skipped, and when no peer is idle the dispatch exits 75 with each peer's state.

Bounds: each job tab has `--timeout` = verb deadline + 10 min (check-queue 45m, nextest 30m, check-linux 45m, setup 20m; override with `--deadline 90m`); the peer runner kills the build tree at the deadline and still reports `outcome=deadline`; fetch aborts below 1 KB/s for 60 s; one job holds the checkout's `.git/codeterm-peer-job.lock` (a lock older than 4 h is stale) and a second job reports `busy`. Peer logs live in `<checkout>/.git/codeterm-peer-jobs/<id>.log`. Each dispatch creates a new tab, never `tab restart`, because a shell's timeout counts from its creation.

Measurements (fill from the first real runs): ROG cold first populate of `check-queue --all` — not yet measured; ROG warm lane `check-queue --lane` — not yet measured.

Troubleshooting:
- No completion message arrives and the remote tab is gone: the tab timeout fired before the runner started (the shell never ran the command, for example a Windows peer without the `git-bash` profile). Check `codeterm tab list --mesh <peer>` and the peer's `peers.json` `shell`.
- `outcome=not_setup stage=checkout_dir`: run `./build.sh peer-setup <peer>` first.
- `outcome=bootstrap_failed stage=checkout`: the peer checkout has local changes; it is fetch-only and must stay clean.
- `peer routing: busy; X=unknown(not_setup)`: the peer has no `Buildpeer` tab; rerun `peer-setup`.

## Cache retention and troubleshooting

Gateway acquisition triggers retention at most once per hour; there are no timers or background sweep jobs. `cache-retention.json` in the machine gateway records the last completed run. Exceptional targets carry `.codeterm-cache.json` with their physical checkout path, last-use timestamp, measured byte count, and live build-owner PIDs. Preparation refreshes the marker while its gateway job owns the build slot.

Missing-checkout targets and targets unused for `CODETERM_CACHE_MAX_AGE_DAYS` (default **3**) are removed first. Unmarked targets older than **2 days** are eligible. Remaining idle targets are evicted in least-recently-used order until they fit `CODETERM_EXCEPTIONAL_CACHE_GB` (default **40 GiB**). The current checkout and targets with live build owners are protected, including during manual GC; protection can temporarily exceed the cap and is reported. PID reuse conservatively protects a cache rather than risking removal of an active build.

At the same hook, installed `cargo-sweep` prunes the current checkout's `src-tauri/target` and `cli/target` with the same configurable age. An unavailable sweep tool prints one installation hint; sweep failures do not fail a build. `SCCACHE_CACHE_SIZE` is explicitly exported with default **10G** before starting sccache. An already-running server retains its startup configuration: apply changed sccache limits only by restarting it with the gateway idle, never during compilation.

Run `./build.sh gc --dry-run` to preview or `./build.sh gc` to reclaim immediately (Windows: `.\build.ps1 gc --dry-run` / `.\build.ps1 gc`). Manual GC acquires the machine gateway, bypasses the hourly throttle, and reports exceptional bytes reclaimed plus sweep output. Dry-run preserves artifacts and the hourly timestamp. Normal targets have an age policy; current artifacts and protected exceptional targets can exceed their usual retained size.

Manual GC also maps every existing Git worktree of this repository and the sibling
`codeterm-plugins` repository, plus the gateway's stable `X:` checkout, through
`cargo metadata --format-version 1 --offline --no-deps`. Cargo 1.95 exposes
`build_directory`; no hash approximation is used. Set `CODETERM_PLUGINS_CHECKOUT`
when the plugin repository is elsewhere. Missing repositories, failed metadata,
or a missing gateway mapping refuse deletion. Each workspace mapping is printed.
The configured two-level `{workspace-path-hash}` cache roots are discovered from
these paths, rather than hardcoding BLACKSTAR's RustCaches location.

Only unmapped build directories are removed, regardless of age. A live checkout's
old build directory remains protected; `CODETERM_CACHE_MAX_AGE_DAYS` continues to
govern exceptional targets and cargo-sweep, not deletion of live build directories.
`codeterm plan lanes --clean` removes worktrees upstream: their build directories
become orphans on the next manual GC. Automatic hourly retention keeps its existing
scope because the stable drive may not yet be mapped when that hook runs.

GC takes nonblocking exclusive locks on every `.cargo-lock` below each orphan and
holds them through removal; a busy or unreadable lock protects the entire directory.
The lock protocol follows Cargo 1.95's
[layout](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/compiler/layout.rs)
and [flock](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/util/flock.rs):
Windows `LockFileEx`, Unix `flock`. Python 3.12+ is required on Windows for junction
validation. Links and unexpected hash layouts refuse collection. Each orphan and
the total are reported in GiB. Fixtures: `node --test scripts/cargo-build-gc.test.mjs`.
For an explicitly blocking cleanup use `.\build.ps1 gc --wait-build` in a Team
build shell; capture `Get-PSDrive C` before and after, and preview first.

If GC exits 2 on a live lane's malformed Cargo manifest, no orphan deletion is
allowed: its build directory cannot yet be mapped. Ask that lane's owner to fix
the manifest, then rerun the preview. Never skip the checkout or guess its hash.

Every candidate is physically resolved using `pwd -P` and must be a direct child of the exceptional target root, or the checkout's exact target directory for sweep. Empty, root, parent, symlink/junction, redirected ancestor, and nested-link paths are refused. Deletion does not follow links. If retention reports an unsafe path, inspect and remove the redirection separately; never relax the boundary to make GC pass. Retention and marker updates use an exclusive gateway metadata lock and refuse concurrent modification. Automatic failures print a hint and let builds continue; explicit GC returns failure for invalid configuration or an unavailable lock.

Canonical shell policy verification: `bash scripts/check-build-gateway-policy.sh`. Its retention fixture suite checks marker persistence, removed checkouts, configurable expiry, legacy grace, LRU cap order, live-owner/current-checkout protection, hourly throttle, dry-run, and unsafe paths including real Windows junctions. It requires no Rust compilation.

## Local production build and self-install

Use this only when testing a production-mode build on this Windows machine. It
does not publish a release or alter GitHub assets.

A build handed to the owner to install (an owner hotfix build such as
`X.Y.Z-bs.N`) first needs a green core-journeys run on its exact SHA; the
gates skill's Release section owns that step.

1. Start from the intended, clean release branch in the canonical checkout.
2. After a version bump, run the idempotent preflight. It refreshes both Cargo
   lockfiles and verifies they match `package.json`:

   ```powershell
   .\build.ps1 release-preflight
   git add src-tauri/Cargo.lock cli/Cargo.lock
   git commit -m "release: sync Cargo lock versions"
   git status --short
   ```

3. For repeated local production testing, build and self-install through the
   persistent canonical cache:

   ```powershell
   .\build.ps1 install-local
   Invoke-RestMethod http://127.0.0.1:7685/api/health
   ```

   From an agent tab, launch it with a receipt (PowerShell and bash lines:
   [utility-shell-reports](../utility-shell-reports/SKILL.md#launcher-recipes-copy-paste)):
   `codeterm tab new --team <team-id> --purpose build --timeout 10m --cwd "<checkout>" --cmd ".uild.ps1 install-local -NotifyTab $env:CODETERM_TAB_ID"`.
   rustfmt, the file-size gate and the clean-checkout check run first, in seconds.

   This route is Windows-only, requires the clean canonical checkout, reuses a
   persistent release target with sccache, rebuilds only changed Cargo units,
   recreates the NSIS bundle, verifies source provenance, and installs that
   exact artifact. When invoked inside Domios, it saves the session with a
   continuation for the calling Tab; the relaunched daemon delivers it once,
   after that Tab's own resumed session is agent-ready, and every other agent
   that was mid-turn receives one default continue. The default intent
   verifies the installed SHA and resumes the interrupted work. Override or
   disable it when needed:

   ```powershell
   .\build.ps1 install-local --continue-prompt "Verify the installed build, then resume release QA."
   .\build.ps1 install-local --continue-tab "$env:CODETERM_TAB_ID" --continue-prompt "Continue the interrupted task."
   .\build.ps1 install-local --no-continue
   ```

   Cargo incremental remains disabled because sccache rejects incremental rustc
   invocations; persistent completed artifacts provide reuse.

   On Windows the CLI hands the installer helper to the system process broker
   before the running daemon is stopped. The helper must not inherit the
   calling Tab's PTY Job: detached stdio, `DETACHED_PROCESS`, `cmd start`, and
   `Start-Process` alone do not escape that Job and are not valid substitutes.
   The handoff invokes the freshly bundled CLI by its provenance-owned path;
   resolving `codeterm` from `PATH` may silently run the previously installed
   helper and is forbidden in this flow.
   Direct `update install --from-file` calls default to a short continuation
   when `CODETERM_TAB_ID` is present. `--continue-tab` must name that same
   caller tab. `--no-continue` durably settles the caller's mark as skipped
   for the next start. Explicit continuation requests are refused with
   `disabled` when `agent.continue_after_restart` is false.
   A successful self-install is proven by `self-install.log` reaching
   `installer exited 0` and `launched`, followed by port 7685 health reporting
   the installed source SHA. The build gateway records the verified artifact as
   complete and releases its machine slot before the detached installer starts;
   the expected daemon restart therefore cannot misclassify that build as
   `owner_lost`.

4. Use the clean-room route when validating a public-release-equivalent
   artifact:

   ```powershell
   .\build.ps1 build
   codeterm update install --from-file "<printed-installer-path>"
   ```

   `build` always creates a fresh nonincremental target under
   `%LOCALAPPDATA%\CodeTerm\build-gateway\targets\release\<job>\`. It remains
   intentionally slower and isolated. The updater helper stops only the
   current-user installed Domios, runs NSIS, and restarts it.

### Repairing a corrupted user PATH (Windows)

Domios adds only its own `bin/` directory to `HKCU\Environment` `Path`, and
now refuses any directory under the system temp directory. Machines that ran the
Windows CLI-install tests before that refusal existed accumulated dead
`<temp>\.tmp*\.codeterm\bin` entries, which count against the 32,767-character
user-PATH cap.

The repair is never automatic. Run it yourself, once:

```powershell
$exe = @(
  "$env:LOCALAPPDATA\codeterm\codeterm.exe",
  "$env:LOCALAPPDATA\Programs\codeterm\CodeTerm.exe",
  "$env:ProgramFiles\CodeTerm\CodeTerm.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
& $exe repair-user-path
Get-Content "$env:TEMP\codeterm-user-path-repair.txt"
```

It removes only entries of the dead `<temp>\.tmp*\.codeterm\bin` shape, leaves
every other entry untouched and in order, reports how many it removed, and is
safe to run twice — a second run removes nothing and does not write the
registry. Where there is no registry it reports zero rather than failing. The
installed app is a GUI binary with no console, so the report is written to the
file above as well as stdout.

Measure before and after:

```powershell
$p = (Get-ItemProperty -Path HKCU:\Environment -Name Path).Path
"{0} entries, {1} chars" -f $p.Split(';').Count, $p.Length
```

Sign out and back in for a repaired PATH to reach new processes.

### Local production troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `install-local` fails in seconds with rustfmt `Diff in ...` | The cheap gates run before the gateway | Run `cargo fmt --manifest-path src-tauri/Cargo.toml --all` (and `cli/`), commit, rerun; with `--notify-tab` the receipt names the step and log. |
| `install-local` refuses with `release source is not clean (N paths)` | Tracked edits or untracked files in the checkout; the first 20 paths follow the ERROR line | Commit or remove them. `src-tauri/gen/schemas/*.json` are LF-pinned in `.gitattributes`, so `check` and `dev` no longer dirty the tree. |
| Release preparation and Tauri packaging compile Tauri and the app dependency graph twice | Preparation omitted `tauri/custom-protocol`, which packaging enables | Keep the packaging feature in `prepare_release_bundle_inputs`; inspect its Cargo timings and the gateway log after a serialized canonical build. |
| `build.ps1 cli` builds and writes the manifest, then fails copying the installed CLI with `Device or resource busy` | Windows has the active dev CLI executable open | Do not kill unrelated callers. For artifact verification, run `cli/target/release/codeterm.exe cli-manifest --write` from that checkout, then run the focused CLI manifest gate. Report the install failure separately; the owner can reinstall when its callers release the executable. |
| `check-tests` fails to mmap `libcodeterm_lib.rlib` with Windows error 1455, followed by missing-crate or macro-resolution errors | The machine exhausted its paging-file commitment during concurrent Rust compilation | Let the owning compile finish; report the resource failure rather than changing source for secondary diagnostics. Run the next authorized focused selector through the gateway. Retry `check-tests` only after the owner has serialized compilation or restored memory capacity. |
| Authenticated configured frontend crashes at `config.font.size` | The static frontend returned SPA HTML for daemon routes and JSON transport accepted it as config. | Use the static server that proxies `/api`, `/frontend`, and `/bundled` to `CODETERM_API_PORT`; JSON transport rejects non-JSON responses. Restart only the owning instance through its named dev operation, then verify stable authenticated DOM. |
| `test-one app` in a fresh canonical checkout fails with `glob pattern bundled-cli/* path not found or didn't match any files` | The focused app operation skipped the compile-only frontend and CLI/embed resource preparation already used by `test` and `check` | Use the focused app operation with `prepare_dev_frontend_dist` and `prepare_bundle_stubs` before compilation. These existing placeholders satisfy Tauri metadata only; the authority test runs in-process and never executes them. Production bundling still replaces the placeholders with real signed binaries. |
| A focused authority test cannot access the private `service::agent_transition` module | The regression was placed outside the service module that owns the recovery helper | Keep the fixture regression under `service::plan_authority::tests`; preserve production visibility, then rerun its focused app filter. |
| Local production build reports `TS2304: Cannot find name 'RefreshCw'` in the mesh peer panel | The shared busy spinner consumed the icon without importing it | Import the existing `RefreshCw` icon from `lucide-react`; rerun the owning production build before self-install. |
| `release-preflight` reports TS2347 on OpenCode `sqliteQuery<T>` calls or TS6133 for imports in `provider-opencode/src/plugin.ts` | The helper declared the injected SDK host as `any`, so its generic query signature was unavailable, and obsolete helper imports remained | Type the injected host as the plugin SDK `Host`, remove unused imports, regenerate `plugins/provider-opencode/plugin.js` with `node scripts/build-plugin.mjs provider-opencode`, and commit the source and bundle together before rerunning preflight. |
| Release PR `comment-length` check lists changed source files | The release diff contains inline `//` comment blocks of three or more lines | Reduce each flagged block to two lines without losing its invariant, then rerun `scripts/check-comment-length.sh --fail --changed <changed-files.txt>` and PR checks. Get the explicit approval required by `AGENTS.md` before editing PTY delivery logic. |
| `registered_documents_have_one_title_and_fit_the_reading_boundary` rejects a CLI document | A contract addition exceeds the 240-line reading boundary | Integrate the clarification into its owning contract section and remove duplicate explanation, or move a separate concern to its linked reference. Run the focused CLI document test before the full preflight. |
| Windows `test-one` fails in `prepare-embed-stub.js` with `spawnSync rustc ENOENT` | The shell PATH contains a Cargo bin directory without Rust proxies | Add the installed Rust toolchain bin directory to this shell PATH, verify `rustc -V`, then rerun `build.ps1 test-one`. If PowerShell blocks scripts, invoke it with `powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1`. |
| The CLI docs test reports `API exceeds 24 KiB` | The API index repeats adoption details already owned by `API-ADOPT.md` | Keep `API.md` to the endpoint summary and move the full contract to `API-ADOPT.md`; bump the production doc revision and run `build.ps1 test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary`. Never raise the 24 KiB limit. |
| The CLI docs test reports `API-TABS exceeds 24 KiB` | The tab API's notification contract repeats channel policy wording | Preserve every contract statement while compacting redundant prose; move a passage to a registered linked doc only if it cannot fit. Bump the production doc revision and rerun the focused docs test; never raise the 24 KiB limit. |
| Several registered docs exceed 240 lines or 24 KiB after parallel lanes merge | Each lane appended prose, often hard-wrapped, and restated contracts another doc owns | Reflow hard-wrapped paragraphs to one line each, remove statements owned by another doc, then split a coherent section into a new registered doc (`cli/src/cmd/docs.rs`) with a `codeterm docs <NAME>` pointer; bump each changed doc's revision. Never raise the limits. |
| `essence_map_contract_committed_artifacts_are_fresh` reports a stale source digest | `essences/capabilities.md` or `essences/core.md` changed after `essences/map.jsonl` was generated | Run `codeterm essence-map generate --root .`, validate with `codeterm essence-map validate --root .`, and commit the generated map. Never edit generated nodes by hand. |
| A release-time test fails in `#[cfg(test)]` code while `check` is green | `check` compiles non-test targets and does not compile the Rust test profile | Run `./build.ps1 check-tests` on Windows or `./build.sh check-tests` on Unix, fix the test-profile compile error, then rerun the same gate. |
| A real newly spawned QA tab reports stale identity although its hook binding is live, while fixture-only report tests pass | Plan evidence authorization required a session environment variable that a fresh provider process does not have; tests always supplied one | Exercise `plan_evidence_session_test` with absent session metadata and an attested nonce. Resolve only the exact daemon-registered tab session, retain all grant/session/nonce refusals, and verify the real CLI after installation. |
| `release-preflight` fails the action-card or Web fork inventory suite after a transport/presentation change | A card fixture bypassed the batch router, or a new platform branch lacked its required transport classification | Test single help invocations in the action card and multi-action batches through `ToolCallBubble`; register the exact Web transport branch in `webForks.test.ts`. Run both focused suites, then repeat preflight. |
| `build.ps1 verify` fails only `cli_manifest_is_fresh` with sccache `os error 10055` / socket buffer exhaustion | The freshness test launched a nested `cargo run` when no standalone debug CLI binary existed, competing with the parent Cargo test process for compiler/socket resources | Keep freshness generation in-process through `manifest::to_json()` / `to_markdown()`. Run `build.ps1 test-one cli cli_manifest_is_fresh`, then rerun `build.ps1 verify`; do not regenerate committed CLI artifacts for this infrastructure error. |
| `release-preflight` prints a Rust formatting diff and exits before lock validation | One or both Rust workspaces were not formatted with the repository toolchain | Run `cargo fmt --manifest-path src-tauri/Cargo.toml --all` and `cargo fmt --manifest-path cli/Cargo.toml --all`, inspect the changed files, then rerun `release-preflight`. |
| `release-preflight` reports a source file one or two lines over 1200 | Test responsibilities accumulated in an inline or monolithic test module until formatting crossed the hard limit | Move a cohesive test group into a child module, verify both focused suites, then rerun `release-preflight`; do not hide the overflow by compacting whitespace. |
| `release-preflight` fails under `-D warnings` for an enum field read only by tests | Production already logs the diagnostic, but an outcome variant retained a duplicate payload solely for a test assertion | Make the production outcome carry only consumed state and assert that state in the focused test; do not suppress `dead_code`. |
| macOS `release-preflight` fails under `-D warnings` because a PE/ELF byte reader is unused | A platform-specific image parser helper was compiled on macOS even though only Windows and Linux extractors call it | Gate the helper to the exact target OSes that consume it, then rerun `release-preflight`; do not suppress `dead_code`. |
| `release-preflight` fails with Clippy `never_loop` in a bounded silence oracle | The helper wrapped one `recv_timeout` in a loop even though every match arm returned on the first pass | Express the oracle as one bounded event wait, keep timeout/disconnect as silence, then rerun `release-preflight`; do not add a settle loop or sleep. |
| `release-preflight` reports Clippy `err_expect` in a spawn refusal test | The assertion used `result.err().expect(...)` instead of matching the failure. | Use `expect_err` or `let Err(error) = result else { panic!(...) }` when the success type is not `Debug`; rerun preflight. |
| `release-preflight` fails compiling `state_doctor` after a classifier signal rename/removal | The standalone testbench still consumed an obsolete `BoundarySignals` field that ordinary library tests do not compile | Update the doctor to the current structural signal and subtype ownership, then run the exact `state_doctor` test target before retrying preflight. |
| PR `source-of-truth` reports prompt files missing from `.agents/skills/prompts/inventory.md` | A new runtime or frozen-reference Markdown file was added under `src-tauri/prompts/` without registering its constant and ownership in the prompt inventory | Add every reported path to the prompt table, explain any frozen never-edit contract, bump the doc revision, then run `bash scripts/check-prompts-doc.sh` before pushing. |
| `Release lock version mismatch` during a version bump | An older preflight checked lock versions before Cargo refreshed the workspace packages | Use the current `release-preflight`, which checks both manifests before verifying both lockfiles; commit the refreshed lockfiles, then retry. |
| A direct `npm run` or `pnpm build` reports `'node' is not recognized` while `node --version` works | The package manager selected `cmd.exe` for lifecycle scripts in a shell whose effective command lookup differs from the parent PowerShell session | Use the named build operation (`build.ps1 frontend` for the production UI gate). For standalone version validation, run `npm run check:versions --script-shell pwsh.exe`. |
| `release-preflight` reports `'rustc' is not recognized` from `prepare-embed-stub.js` even though Git Bash finds rustc | Native Node launched `cmd.exe` with Git Bash's colon-delimited PATH, so shell lookup could not resolve Cargo tools | Update the stub generator to invoke rustc directly from `CARGO_HOME`/the user Cargo directory, then rerun `release-preflight`. Do not rewrite PATH or bypass the stub. |
| `release source is not clean` | A source or lockfile change has not been reviewed/committed | Inspect `git status --short`; commit the intentional change or stop. Never bypass provenance. |
| `source SHA/status mismatch` after a bundle was created | A build step changed tracked source inputs, or rewrote an identical LF-only manifest and left Git's Windows stat cache reporting a false name-only change | Do not install when the patch is non-empty. Current provenance hashes the binary patch plus working-tree bytes, so a metadata-only rewrite is accepted; rerun `install-local` from a clean tree. |
| Self-install logs `starting` and the window never returns | A helper inherited the calling PTY Job and died when `taskkill` closed that Job | Rebuild the current CLI; it creates the helper through the out-of-job Windows process broker. Re-run `install-local`, inspect `%LOCALAPPDATA%\codeterm\self-install.log`, and verify port 7685 health. |
| Domios restarts but the initiating tab does not resume | The continuation send failed or the Tab id changed before session persistence | Inspect `%LOCALAPPDATA%\codeterm\self-install.log`; the retained `%TEMP%\codeterm-continuation-*.txt` contains the undelivered intent. |
| Self-install succeeds but gateway history reports `owner_lost` | An older build script kept the gateway owned until after the installer restarted Domios and killed the calling Tab | Update the build scripts and run `build.ps1 install-local`; the current operation releases the verified build before detached installer handoff. Prove installation separately with `self-install.log`, port 7685 health, and `codeterm identify`. |
| Repeated local install recompiles the full release graph | The clean-room `build` route was used instead of the persistent local cache | Use `build.ps1 install-local`; reserve `build` for public-release-equivalent verification. |
| Local install cannot read Tauri permissions under a deleted release directory | A relocated Cargo target retained absolute build-script `root-output` paths. | `install-local` now detects mismatched output roots before Cargo, preserves that exact cache in a sibling `.relocated-*` directory, and regenerates its local target; it leaves current warm caches and the machine sccache intact. |
| `test-one app usage_routes` exits 97 with zero matches | The usage route module had no tests, so the assigned selector had no oracle. | Add focused query and persisted wire-contract coverage in that module, then rerun the same selector; never accept zero matches or replace it with the whole app suite. |
| A focused Rust test spends most of its runtime linking app binaries | The test omitted `--lib`, so Cargo built unrelated binary test targets | Run the focused filter through `scripts/test-capped.sh ... --lib <filter>`. |
| PowerShell drops the standalone `--` before `--nocapture`, so Cargo rejects `--nocapture` as its own option | Native argument forwarding consumed the test-harness separator before Git Bash received it | Run the exact wrapper command through `bash -lc 'scripts/test-capped.sh ... <filter> -- --nocapture'` so the separator reaches Cargo's test binary. |
| `verify` fails `daemon_windows_spawns_use_no_window_helper` after adding a Rust test fixture that invokes Git | The fixture constructed `std::process::Command` directly, so Windows could flash a console and the repository spawn audit rejected it | Build the command in a mutable variable and call `crate::process_util::no_window_std(&mut command)` before execution; run the spawn audit and the owning focused test, then retry `verify`. |
| A fresh lane's focused Rust test reports missing `../dist` or `bundled-cli/*` | Ignored Tauri resource directories have not been staged in that worktree | Run `node scripts/dev-resource-preflight.mjs` and `node scripts/prepare-cli-stub.js`, then rerun the same focused test. Do not substitute a frontend build. |
| Direct `bash scripts/clippy-gate.sh` fails in a fresh lane with a missing `bundled-embed/codeterm-embed-<target>` resource | The direct gate assumes Tauri's ignored compile resources are staged. | Run authorized `./build.sh check-tests` first; its resource preflight prepares frontend, CLI and embed stubs. Then rerun the direct clippy gate. |
| macOS `local-install-cache.test.mjs` refuses ordinary fixtures under `/var` as redirected | `realpath` expands macOS's standard `/var` alias to `/private/var`, so comparing the whole lexical target to its real path mistakes an ancestor alias for a redirected cache leaf | Compare the real target with the real parent plus the expected cache basename, retain an explicit redirected-leaf refusal test, then rerun the focused Node test and `release-preflight`. |
| `sccache: incremental compilation is prohibited` | `CARGO_INCREMENTAL=1` was combined with the shared sccache wrapper | Keep `CARGO_INCREMENTAL=0`; reuse the persistent completed target and sccache entries instead. |
| Release-signing negative test passes despite a missing fixture key | The shell inherited `TAURI_SIGNING_PRIVATE_KEY` from the canonical `.env` | Run the repository check; its missing-key fixture explicitly removes the inherited key before asserting failure. |
| Push unexpectedly starts compilation or formatting checks | A stale local hook predates the non-blocking push policy | Run any `build.ps1` command to restore `.githooks`, then retry; verification belongs to explicit gates and CI. |
| `Build gateway job id already exists` immediately from a fresh command | The calling Domios tab inherited a completed gateway job id from an earlier build | Update the build scripts; production gateway entry now always creates a fresh job id and ignores inherited internal ids. |
| `pnpm build` fails only in focused TSX test files during `install-local` | A test mock violates the generic bridge signature or imports an unused JSX runtime symbol | Run `pnpm build`, fix the test type/import at the source, then retry `install-local` from the same clean canonical checkout. |
| `pnpm build` reports stale role literals or duplicate fixture fields during `install-local` | A role-model refactor left TypeScript-only production or fixture surfaces outside the Rust gate | Run `pnpm build`, converge each surface on `Manager` plus `role_profile`, and retry `install-local` from a clean canonical checkout. |
| `install-local` reports `tsc is not recognized` although `node_modules/.bin/tsc` exists | Git Bash supplied the MSVC environment to Tauri, but pnpm selected a nested Windows lifecycle shell that lost the local-bin lookup | Use the current build operation, which pins pnpm lifecycle scripts to Git Bash and supplies frontend provenance without a nested shell lookup, then retry `install-local`. |
| Tauri `beforeBuildCommand` reports `pnpm is not recognized` during `install-local` | Tauri launched the configured hook through `cmd.exe` with Git Bash's POSIX-formatted `PATH`, so Windows could not resolve pnpm, Node, or Cargo | Use the current build operation, which prepares frontend, CLI, and embed inputs directly before invoking Tauri; retry `install-local` from canonical. |
| Installed health on port 7685 reports a dev identifier or Vite/CDP ports | The detached installer helper inherited `CODETERM_*` and Cargo build variables from the calling tab | Use the current installer helper, which scrubs Domios and build variables before launching the installed daemon; reinstall and verify `/api/health`. |
| NSIS reports `Error opening file for writing` for `conpty\<arch>\OpenConsole.exe` | A bundled ConPTY host from the installed Domios instance survived PTY teardown and still holds its executable | Current builds reap path-matched owned hosts before NSIS. On an older build, verify the process image is under `%LOCALAPPDATA%\CodeTerm\conpty\`, end only that `OpenConsole.exe`, then click **Retry**; never end `conhost.exe` or an `OpenConsole.exe` from another path. |
| Windows self-install relaunches but continuation reports an incompatible CLI, or x64 startup installs an ARM64 `~/.codeterm/bin/codeterm.exe` | Parallel Windows release targets shared the generic `bundled-cli/codeterm.exe` resource, allowing one architecture to enter the other installer | Rebuild with target-qualified CLI resources. Startup now rejects a PE whose machine type differs from the app, preserving the prior working shim instead of overwriting it. |
| `test-cli-target-dir-paths.sh` reports an extra `MSVC env loaded` line or rejects a foreign cross-target before an otherwise valid path probe | Windows build initialization writes diagnostics and validates target toolchains before the probe | Use the current contract test, which selects a host-compatible fixture target and compares the final probe line while `pipefail` preserves failures. |
| Installed Domios exits during startup with a missing `tab_id`/`tabId` field or `delayed_send init` panic | A pane-era or malformed persisted record reached an all-or-nothing startup deserializer | Rebuild and install the current revision, then restart. Inspect `[plan.store]`, `[delayed_send]`, `[remote-workers]`, `[graph-workers]`, and `[session]` diagnostics for the isolated record; do not delete `plans.db` or an entire state JSON file. |
| `failed to run custom build command for openssl-sys`, with `sccache: Failed to send data to or receive data from server` in the same output | The machine-level `sccache` server is wedged, so every C/C++ compile it fronts fails; `openssl-src` is simply the first crate to need one | `sccache --stop-server && sccache --start-server`, then rebuild. A cache reported at its 10 GiB max is ordinary LRU eviction and is NOT this failure — the discriminator is whether `sccache --show-stats` answers at all. Never restart a responsive server mid-build; that discards a warm cache for nothing. |
| Build reported as succeeding while the gateway job exited non-zero | A wrapper placed another command after the build in the same compound statement, so the shell's status came from that command instead | Read the `exit:` line of the gateway job log, or capture `${PIPESTATUS[0]}`. `build.sh` propagates the gateway status faithfully, so a wrapper is the only place this can be lost. Treat post-install `/api/health` as the only green light. |

When a new failure is found in this flow, fix the owning script or preflight and
add a short symptom/cause/recovery row here before closing the work.

Do not use `dev --wait` for an interactive Windows session. It is reserved for
automation that needs a detached health gate and does not prove a visible window.

### Crate extraction troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| A moved browser locator fails with E0433 at `dirs::home_dir()` | The extracted crate did not declare an existing third-party dependency that the app previously supplied | Declare `dirs = "6"` directly in the browser manifest, refresh the workspace lockfile, and rerun the focused check when the build owner authorizes it. |

### Focused Windows test prerequisite troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| A redirected `build.ps1 test-one` exits with `NativeCommandError` on a gateway diagnostic while its gate keeps running | PowerShell 5 converts captured native stderr into an error under `ErrorActionPreference=Stop` | Capture the PowerShell process at the OS boundary, for example `cmd.exe /d /c "powershell.exe -NoProfile -File build.ps1 test-one app <selector> --wait-build > qa.log 2>&1"`. Recover the original gate's log and exit receipt with `build.ps1 gateway-status` before launching anything else; never repeat a successful gate. |
| A focused RPC handshake test fails with E0308 at `tungstenite::Error::Http` | The fixture passed an HTTP response directly, but the error variant owns a boxed response. | Wrap the fixture response with `Box::new`, then rerun the same `test-one app` selector. |
| A native cwd regression accepts the resolved path, but its `cmd.exe` child exits 1 with `The directory or file cannot be created` | Shell startup introduces an uncontrolled dependency after native process creation; literal command-line syntax did not remove it. | Run the current Rust test executable with `--list --exact opaque-filter` instead, retaining the empty-cwd raw error assertion and a successful native child exit. Rerun the focused launch selector. |
| `test-one app` stops before compilation with `prepare-embed-stub.js: spawnSync rustc ENOENT` | The utility process cannot resolve Rust, and its user Cargo bin has no compiler proxy, although the pinned toolchain is installed. | Prepend the existing `~/.rustup/toolchains/1.95.0-x86_64-pc-windows-msvc/bin` only to that Team utility process PATH; verify `Get-Command rustc,cargo` resolves there and `rustc --version` reports the repository pin. Preserve the failed zero-selected receipt, then run the same focused wrapper from clean canonical with a fresh idle gateway check. Do not install tools or change persistent PATH, runner environment, caches, or production. |
| The focused wrapper reports `exceptional worktree compilation requires sccache` before selecting tests | The utility cannot discover the existing machine cache executable. | Add only its verified existing directory to the utility process PATH, confirm exact executable resolution and that `sccache --show-stats` reaches the prestarted shared server, then use the same focused wrapper with a fresh idle gateway. Preserve the failed receipt; never disable, replace, clear, or restart the shared cache to bypass this prerequisite. |
| Multiple crate compilations fail with `sccache: caused by: Failed to read response header` or `error reading compile response from server` (`os error 10054`) | The machine-level cache server reset compiler connections during the build; it may answer again after the failed job exits. | With the gateway idle, check `sccache --show-stats`. If it responds, retry the same bounded operation once; if it does not, use the sccache restart recovery above. Never bypass the shared cache or raise the test cap. |
| `test-one app` fails with `expected item after attributes` in `report_plan_ctx/tests.rs`, and the moved desktop reporter is unused | The test split left a trailing `#[test]` in the shared module and omitted it from the first moved function in `tests/desktop_reports.rs`. | Move `#[test]` onto that function, remove the trailing attribute, and rerun the same focused app filter. |

### Visible Windows dev session

For interactive verification, do not use `dev --wait`. It is the detached
readiness path for scripts and CI. Use one visible Domios Tab as the sole
foreground owner:

```powershell
# Verify no previous dev listener/process remains, then open one visible tab.
codeterm tab new --team <team-id> --purpose dev --cmd "Set-Location '<checkout>'; .\build.ps1 dev"
```

Keep that Tab open and require all of the following before reporting ready:

- port 7686 is owned by the checkout's `target\debug\codeterm.exe`;
- `/api/health` reports `status=ok`, the exact checkout `build_sha`, API 7686,
  Vite 1420, CDP 9223, and identifier `com.codeterm.dev`;
- the Domios process has a responsive, nonzero `MainWindowHandle`;
- exactly one Cargo process remains, the foreground `tauri dev` watcher owned
  by the visible Tab. Any second Cargo/build owner must be stopped and traced.

The canonical checkout owns shared dev, broad gates, and release checks. This is
a team operating rule, not a branch-name restriction in the build script; the
machine-wide gateway only serializes heavy app work.

Gateway-backed commands fail fast when another job owns the machine slot, so
an agent turn never blocks silently behind a long-lived dev process. Add
`--wait-build` only when synchronous queueing is intentional. `check-tests`
never enters the gateway, so it refuses `--wait-build` with exit 2 instead of
dropping the flag.

### Gateway recovery when an owner never runs its EXIT trap

`ct_gateway_finish` releases the machine slot from the bash `EXIT` trap, and
`SIGKILL` runs no trap. Recovery is `ct_gateway_reap_stale_owner_locked`, which
every `gateway-status`, `gateway-cancel`, and acquire runs while holding the
state mutex. Its trigger is a single observable fact — the owning process
identity recorded with the lock is no longer running. Nothing in that path
consults elapsed time; a stale job is never aged out, and a live owner always
keeps blocking. `bash scripts/check-build-gateway-owner-reap.sh` covers both
directions.

Liveness is decided on exactly one pid namespace. `ct_gateway_pid_namespace`
picks it once per call: a readable `/proc/self/stat` means every gateway pid is
a `/proc` pid, and a pid missing from `/proc` is then authoritatively gone. The
PowerShell fallback is reached only where no `/proc` exists, so an MSYS pid
under Git Bash is never handed to `Get-Process`, which would answer for an
unrelated Windows process holding the same number.

A reaped job records `state=failed` with `outcome=owner_lost`, which is a
different terminal state from the `cancelled` a human `gateway-cancel` writes.
When the job's own log already carries its `Build gateway job <id> exit: <n>`
marker, the reaper recovers that status into `command_exit_code` and prints the
recovery to stderr, so a job that succeeded and lost its owner before the result
was written is visible rather than silently reclassified.

An owner that is wedged rather than dead is a different failure and is not
reapable: see the `release-preflight` FIFO teardown row below.

Use `./build.sh help` as the command index. `test-one <account|app|frontend-bundle|plan|pty|pty-fork|util|browser|types|mem|mesh|plugin|plugins|cli> <filter>`
hides manifest/package details for focused tests. `frontend-bundle` (also
`codeterm-frontend-bundle`) runs the isolated bundle crate through the gateway. The `types` and `cli`
selectors include all Cargo targets so integration and binary tests are
reachable; the other selectors retain their existing ownership scopes. Each
target is its own test binary and prints its own `test result` line, so a
target reporting `0 passed` only means the filter matched nothing there. Judge
the run by the per-test `... ok` lines of the target that owns the test.
`nextest <scope> [filterset]` uses the build gateway, compiles with
`cargo nextest list --list-type binaries-only`, then runs through `scripts/test-capped.sh` with
`--no-tests=fail`; it accepts the same scope names plus Cargo package names.
Every `nextest` operation compiles a binaries-metadata inventory, saves Cargo
metadata and reuses that exact inventory for execution. It captures per-test
timings at normal parallelism. `check-slow-unit-tests.mjs LOG --candidates JSON`
selects new or over-baseline timings above one second; the runner measures only
those exact tests alone, reusing metadata without any Cargo build flags.
`check-slow-unit-tests.mjs LOG --alone ALONE_LOG` reports both measurements and
keeps the one-second target visible: alone times from 1.0 to 1.25 seconds warn;
new alone times above 1.25 seconds fail. Existing regressions fail only above
baseline + max(20%, 0.25 seconds); smaller increases warn.
`scripts/slow-unit-tests.json` records alone durations and may only shrink against
`origin/develop` (`SLOW_UNIT_TESTS_BASE` overrides the ref). Missing or partial
timings fail closed, and test failures retain their original exit codes.
Primary runs explicitly disable fail-fast for every manifest so one failed test
does not cancel the remaining timing inventory.
For an explicitly authorized bootstrap measurement, use `NEXTEST_TEST_THREADS=1
SLOW_UNIT_TESTS_MEASUREMENT=alone` on the exact candidate selector; the runner
refuses that mode without serial execution. Ordinary suites retain default
parallelism.
The app baseline is `scripts/slow-unit-tests.json`; separately bootstrapped
crate measurements live in `scripts/slow-crate-unit-tests.json`. Each baseline
may only shrink after its first landing. `scripts/unit-test-benchmarks.json`
contains exact benchmark identities with explicit throughput/latency reasons.
Exemptions print their identity, timing and reason, preserve test failures,
and cannot be added or changed after their first landing without review.
Benchmarks never receive a slow-unit baseline allowance.

Filtersets use nextest's `--filter-expr` syntax. A filterset containing
`rdeps(...)` runs workspace targets so reverse dependents remain in scope.
`verify` runs the standard
check, full Rust workspace library suite, and source-size and provider-boundary integration gates.
Rust tests keep the repository-owned `scripts/test-capped.sh` wrapper but wait
for Cargo and libtest to finish by default. For a targeted diagnostic only, set
`TEST_CAP_SECS=<positive-seconds>`; an enforced cap terminates the test process
group and exits 124, while ordinary child success or failure is propagated.

`./build.sh cli` uses the shared cargo gateway and single-owner sccache initialization.
The command builds the release
CLI, regenerates the three generated CLI artifacts (`generated/cli/cli-manifest.json`, the
generated block in `generated/cli/cli.md`, and the Chat UI command paths), then installs
the binary into the active development data directory, replacing the CLI every
tab of that dev instance runs. Dev builds never overwrite `~/.codeterm/bin`:
production CLI updates come from the bundled production app during startup,
in-app update, or `install-local`.

### Crash symbols for dev builds

A dev link overwrites `src-tauri/target/debug/codeterm.pdb`, so a crash artifact
stops being symbolizable as soon as anyone rebuilds — that is how the 2026-09-05
abort lost its frame names nine minutes after it was written.

`./build.sh dev` therefore retains the symbol file of each build it runs under
`~/.codeterm-dev/pdb/<key>/codeterm.pdb`, keeping the newest three
(`DEV_PDB_KEEP_DEFAULT` in `build.sh`; override the store with
`CODETERM_DEV_PDB_DIR`, the count with `CODETERM_DEV_PDB_KEEP`). `--wait` retains
once `/api/health` confirms which build is running; a foreground session retains
when it ends, which is also when a daemon that crashed stops holding its binary.

**The key is the symbol file's own debug identity**, not a commit: the GUID's 32
hex digits followed by the age in hex, the canonical Windows symbol-store form.
`scripts/pdb_identity.py` computes it from either a PDB or a binary, and
`crash::image_identity` computes the same string for the sidecar, so the directory
a crash names is the directory retention wrote. A commit sha cannot be the key —
the same source relinks to a different symbol file every time, and a foreground
retention runs after HEAD may have moved. The sha is recorded beside the symbols in
`build-sha.txt`, informative only. The copy keeps the name `codeterm.pdb` because
Windows symbol lookup finds a PDB by the name recorded in the binary.

Every crash report names its own build: `Debug id:` is the RSDS GUID and age, and
`Symbols:` is the retained directory to look in. To read a dump:

```bash
python scripts/symbolicate-minidump.py <dump.dmp> --sidecar <crash-...-native.txt>
```

The tool accepts a symbol file only when the identity it records for itself matches
the dump's, so a stale PDB is refused rather than used to print the wrong names.
Frame names come back from the symbol file alone; x64 unwind data lives in the
binary, so when only the PDB survives read `--scan` rather than the frame order.

### Development troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `worktree-check` refuses with a busy-slot blocker | It enters the build gateway (it maps the stable `X:` build path, a machine-global drive, so it must be serialized); the slot is held by another job. | Report the blocker and continue source-only work; use `--wait-build` only when blocking is intended. Success shows `[cache-policy] stable build path X: -> <checkout>`. |
| A gateway cargo run is `Finished` in seconds with an empty `targets/exceptional/<id>/debug` | `~/.cargo/config.toml` sets `build.build-dir` to `RustCaches/build/{workspace-path-hash}`; the stable `X:` path gives every gateway worktree the same hash, so intermediates are shared (gateway-serialized) and only final artifacts go to the target dir. | Expected. For a cold measurement remove that one hash directory and the target dir; never point two checkouts at one `CARGO_TARGET_DIR`, and never point two unserialized builds at one build-dir: worktrees share intermediates only through the stable gateway path, which the job unmaps on exit and `scripts/check-build-check-queue.sh` guards. |
| `agent_restart` fails the surviving-shell stale-metadata fixture while retaining `agent_type` | The fixture declares a session nonce without its provider-reported match, so lifecycle cleanup correctly refuses the unauthenticated identity. | Seed matching reported nonce for this verified-exit scenario; retain the production authentication gate and rerun `test-one app agent_restart --wait-build`. |
| A touched test file exceeds the 1200-line source limit | One harness accumulated several independent test groups | Move a group into a module with injected shared harness functions, retain the runner entrypoint, and rerun its focused suite plus the size check. |
| `tsc --noEmit` reports TS6133 after a provider delegates parsing to a shared helper | The old parser import remains after its last call was removed | Remove the unused import, regenerate the provider bundle, and rerun TypeScript plus the focused plugin suite. |
| Focused `model_switch` tests panic unwrapping an effort picker after a provider adopts a direct effort command | Generic picker tests depended on the provider's live picker declaration | Give column-picker unit tests an explicit declaration fixture; verify the provider's direct target command against captured stdout, then rerun `test-one app service::agent_restart::model_switch`. |
| Blueprint integration reports E0412, E0308, E0560, E0716 or private journal validation | Journal imports, string transport outcomes, StageSource constructors and borrowed accessors drifted during the merge; validation was scoped only to its child module. | Import `JournalOutcome`, compare wire outcomes as strings, construct only `source`, borrow accessors directly and expose validation within `repositories::sentinel`; rerun `worktree-check` and the focused Blueprint selectors. |
| `test-one app sentinel::` reports `no such table: blueprints` or a pipeline golden identity mismatch | A historical schema fixture used the final table name before migration, and the golden retained the old external identity kind. | Keep historical inserts and refusal reads on `sentinels`; regenerate the pipeline golden with `UPDATE_BLUEPRINT_GOLDEN=1` through its owning test, then rerun Rust and TS contract selectors. |
| Blueprint `check-tests` reports E0282 and a static reason lifetime error | Cleanup strings had an ambiguous inferred vector type; a dynamic detail string was passed as a static outcome code. | Type the vector as `Vec<String>` and pass `ProbeFailure`, retaining detailed `last_reason` and its static primary outcome code; rerun `check-tests`. |
| Restoration integration fails with E0432/E0603 | Cross-crate request/result reexports were removed and the socket imported a private service module. | Export the two store types from task_attempts and route socket imports through the existing service facade; rerun `check-tests` before focused gates. |
| `check-tests` reports E0505 in same-attempt binding restoration | The transaction borrowed `request.plan_id` while its closure consumed the request receipt. | Clone the transaction lookup ID before the closure; rerun `check-tests` and the focused plan/app restoration selectors. |
| CI spawn audit flags a command factory and its subprocess fixture; Linux Clippy also reports `err_expect` | Moving command construction separated it from the audited no-window helper; the fixture omitted that helper and assertions used `err().expect()` | Apply `no_window_std` when constructing each command, retain the spawn audit unchanged, and use `expect_err` for these assertions. Run the focused spawn audit and executor tests, then the failed Clippy gate before a fresh-head platform run. |
| App wiring tests fail to resolve `Motion` after detection extraction | Moving tests outside the classifier module removed an inherited private import | Import `Motion` directly from `signal_extractor` in the wiring test; rerun its focused app selector. |
| `check` passes but `check-tests` reports E0433 for `mpsc` in shared-state tests | Parent import cleanup removed the module alias inherited by child tests through `super::*` | Import `std::sync::mpsc` explicitly in the owning test module, then rerun `check-tests`. |
| `check-tests` reports E0046 for `NativeUpdateFixture`, missing `detect_session_id` | The provider-update parity fixture omitted a required `AgentProvider` method | Implement the fixture's unused session detector with `None`, then rerun `check-tests` and the focused `parity_update` filter. |
| Courier integration reports E0603 for `build_prompt` and E0599 for `tab_is_idle` | App tests called a plugin-private fixture helper; the update refactor removed an idle helper still used by restart and model switching | Construct `PendingPrompt` from its public fields in the test and restore the unchanged pre-merge `tab_is_idle` helper, then rerun `check-tests` and the provider-update and restart filters. |
| `check-tests` reports E0609 `no field workspace_id on type Tab` in plan reminder approval | A lane that could not compile the app crate trusted a grep hit for `workspace_id` that belongs to `ClosedTab`; a live `Tab` has no workspace field, and membership lives in the multiplexer's workspaces | Resolve the workspace under the same mux read with `MultiplexerState::find_workspace_for_tab(&resolved_id)`, then rerun `check-tests`. Before claiming a field in a lane that cannot compile, read the owning struct definition, not a matching line number. |
| `test-one cli cmd::docs` reports `CONFIG repeats a whole-document reference to TAB-LIFECYCLE` | Moving provider update documentation added a second pointer to the existing recovery reference | Combine the section pointers into one whole-document reference, bump CONFIG's revision, and rerun the docs filter. |
| Windows `npm run test:plugins` says `node` is not recognized before any suite starts | npm's command shell lost the Node executable directory although the parent PowerShell resolves it | Run `node scripts/run-plugin-tests.mjs provider-codex` from canonical; this repository runner pins its own Node directory for child suites. This launcher refusal is not a passing test result. |
| A TS test runs in Node but verification reports TS2550 for `Array.at` | Node supports a newer library than the project's TypeScript target | Use indexed access in the test; keep the project target unchanged and rerun verification. |
| Linux CI reaches `cargo fmt --check` and prints formatting diffs across otherwise green Rust changes | An integration wave merged lanes without applying the repository rustfmt version before push | Run the two manifest-scoped `cargo fmt` commands from canonical, inspect the resulting file list, then rerun both checks before pushing. |
| `pnpm exec tsc --noEmit` reports TS2367 after a test first asserts two literal provider ids and then compares them | The assertion function narrowed both values to disjoint literals, making the later boolean expression statically unconditional even though it documents invalidation | Use `assert.notEqual` for the cross-snapshot comparison so the mechanism remains checked without constructing an always-true expression. |
| `npm run test:unit` terminates healthy workers at the same elapsed time while files are still passing | The TypeScript scheduler applied an implicit suite deadline, so repository growth was misclassified as a hang | Use the current uncapped default and let the suite finish. Set `TS_UNIT_TIMEOUT_SECONDS` or `TS_UNIT_SUITE_TIMEOUT_SECONDS` only for a targeted diagnostic. |
| `bash scripts/test-capped.test.sh` exits 141 before printing an assertion | Its host-triple probe closed the `rustc -vV` pipe early under `pipefail`, so rustc received SIGPIPE | Keep the probe consuming the complete rustc output; then rerun the shell contract. |
| The test-capped shell contract exits 2 when simulating a Windows target on macOS | The fixture asked the wrapper to synthesize a foreign-platform embed sidecar with the local rustc host | Seed the fixture's inert Windows sidecar before testing Windows target selection. |
| `state.lock/start_token: No such file or directory` or a missing `state.lock/pid` under contention | The old mutex published an empty directory before writing ownership; another acquirer reclaimed it during initialization. | Update the gateway scripts. Ownership now publishes a complete temporary directory by atomic rename, with publication/reclamation serialized by `state.lock.guard`. Incomplete ownership is never stale: callers observe filesystem changes until the 30-second failure deadline, then exit 75. The guard uses an exclusive-create file containing its helper pid, native process start token and unique owner token; release unlinks that file without renaming a shared directory. A proven missing/reused helper is reclaimed automatically; live or unreadable identities remain bounded refusals. Unreadable ownership remains a bounded refusal; request an owner decision rather than deleting shared state by hand. Verify with `node --test scripts/build-gateway-mutex.test.mjs`. |
| `check-build-gateway-mutex-cleanup.sh` exits 127 with `coproc: command not found` on macOS | The fixture assumed Bash 4 while macOS provides Bash 3. | Update the fixture to its FIFO readiness handshake, then rerun `bash scripts/check-build-gateway-mutex-cleanup.sh`; it uses fake compiler children and compiles no Rust. |
| A queued caller disappears or a live caller is marked `owner_lost` | Queue liveness must use the recorded owner pid and its start identity in one pid namespace; closing the caller ends that owner. Unreadable identities remain unknown and are never reaped as dead. | Inspect the job-specific record and log. The gateway reaps only the dead ticket, preserving live callers. Verify `dead queued owner is reaped without removing the live caller ticket`, `four parallel check-queue gateway clients finish in ticket order with metadata mutex free during slot wait`, and `bash scripts/check-build-gateway-owner-reap.sh`; a lost caller with no filesystem change is reaped on the next gateway operation. |
| `metadata write failed before compilation` or `EPERM ... jobs/<id>.json` | A temporary job record could not atomically replace the published record, and an older gateway ignored its failed running-state update. | The current gateway exits 74 before compilation, releases the metadata mutex, and retains the last valid record. Inspect the error; a later gateway operation reaps the exited caller. Verify `create/running/evidence-start metadata failure aborts acquisition before compile and releases the metadata mutex` in `scripts/build-gateway-record.test.mjs`. |
| History/status sees a missing, partial or delete-pending job record during replacement | A reader observed a transient filesystem view while a writer published a complete temporary JSON file. | Readers skip the unavailable record once, without retries; gateway-status serializes normal observations with the metadata mutex. Verify `readers skip missing, incomplete and inaccessible replacement observations without retry`, `gateway status skips a partial JSON record and keeps a valid job`, and the build-history unreadable-record CLI fixture. |
| Shared-cache fixture says `rg: command not found` in a utility shell | The fixture depended on an optional search executable absent from that shell PATH. | The fixture now uses portable grep from the required Bash toolchain. Run `bash scripts/check-shared-sccache.sh`; do not count a launcher/search failure as a passing policy check. |
| `sccache: error: ... os error 10048` during concurrent check-tests/check-queue | Both gateway and non-gateway verbs observed no server and attempted startup. | All build-script cache configuration, verification and restart paths now share a dedicated per-machine startup mutex. The gateway metadata mutex remains free. Verify `four concurrent cache clients start the server exactly once and leave gateway metadata free` in `scripts/build-gateway-cache.test.mjs`; do not restart or bypass a cache serving active compilers. |
| `check-queue` reports no warnings but dev fails a dead-code lint | All-target test feature/cfg unification covered a helper that the ordinary dev library configuration could not reach. | Check-queue now additionally checks the exact regular app library dev profile/features without test targets in the same gateway job. The summary and full log record `check-queue: dev-lib args=... exit=...` for that pass. An `app-test scope=nextest,test-one args=... exit=...` receipt covers their shared service-core test cfg and default embeddings features; dev, root default, core default and test features must agree. Dev and the check share the configuration helper; verify `scripts/dev-build-configuration.test.mjs`, then rerun canonical check-queue and dev. |
| An owned check-queue exits 75 because Cargo/rustc is running elsewhere | Sccache restart used a whole-machine process scan even after the caller acquired the gateway slot. | The current step validates the slot's job/pid/start tuple and uses that ownership to configure under the shared startup mutex. Only callers without exact ownership retain the global idle refusal; every supported Cargo verb uses the common entry. Verify the busy-machine owned/foreign-caller cases in `scripts/check-shared-sccache.sh`, then rerun the owned canonical check. |
| Gateway exits with `Cannot establish publication helper identity` under load | A one-shot Windows process lookup was unknown, and publication guard construction threw before the bounded mutex refusal handler. | The current guard waits on ownership filesystem events before trying the lookup again. Its deadline returns retry-safe exit 75 with no helper ownership published. Verify the transient helper-identity regression in `scripts/build-gateway-mutex.test.mjs`; unknown identity never authorizes reclamation. |
| Tracked modules or CLI manifests vanish through `X:` while another fixture runs | Fake gateway jobs held an isolated fixture slot but remapped the global production `subst` drive. Production release could also unmap that fixture's alias. | Fixture scope now skips both mapping and release; mapping requires the exact production job/pid/start owner, and release checks the mapped job still owns the slot. Verify the stable-drive isolation regression in `scripts/build-gateway-cache.test.mjs`. Never repair a live drive manually. |
| Warm check reports E0583 for tracked modules or a missing `cli/Cargo.toml` after a rename wave | In-place `git checkout --force` published no verified physical-tree boundary before Cargo. An interrupted switch could leave HEAD/index and disk inconsistent. | The current check prepares a detached staging worktree, verifies requested HEAD tree, index tree and tracked files, then publishes under the exact gateway owner. Its journal recovers interrupted publication on the next job and retains warm target caches. Verify `node --test scripts/check-worktree.test.mjs`; any tree mismatch refuses Cargo with exit 75. |
| Queue updates time out during cache retention | Retention held gateway metadata while measuring/deleting caches and running cargo-sweep. | Retention and target marking now share a separate per-machine mutex. Verify `cache retention holds its own mutex while gateway metadata stays available`; the normal queue can update while cache work owns its lock. |
| A warm check starts with one guard implementation and finishes with another | Helper paths followed cwd after checking out the requested SHA. | Gateway/cache helper paths are pinned to the launching scripts. Verify `gateway helpers stay pinned to the launching scripts after checkout cwd changes`. |
| `--wait-build` repeatedly reacquires the metadata mutex while queued | The former slot wait used a sleep loop. | Update the gateway scripts; queue/slot filesystem events now drive the wait after releasing the mutex. The captured slot snapshot closes the release-to-subscribe race; expiry is a failure deadline. Verify with `node --test scripts/build-gateway-slot.test.mjs` (four fixture check-queue clients finish in ticket order). A caller lost without a filesystem update is reaped on the next gateway operation; the wait has a bounded failure deadline. |
| `EPERM rename state.lock.guard -> state.guard.released.owner.*` under parallel checks | Directory guard release raced concurrent readers on Windows, leaving a guard that could cause state-mutex timeouts. | Update the gateway scripts; the publication guard now uses atomic exclusive file creation and owner-checked unlink. Incomplete ownership stays a bounded refusal; a proven dead owner is reclaimed by one exclusive reaper. A crashed reaper leaves a conservative refusal requiring an owner decision, never manual deletion during live checks. Verify with `node --test scripts/build-gateway-mutex.test.mjs` (six concurrent Bash callers all complete). |
| A build exits with `Timed out acquiring build gateway state mutex` | Another live or stale gateway owner held the state mutex through the acquisition deadline. Owner liveness probes now run outside publication exclusivity; the owner tuple is revalidated before reclamation, so a slow probe cannot block release or remove a successor. The failed invocation terminates only its own Cargo/rustc descendants before returning. | Run `./build.sh gateway-status` to inspect the active and queued job IDs. If a job is stale, run `./build.sh gateway-cancel <job-id>`, then rerun the original command. |
| `build.ps1 check` exits 75 because the visible dev owns the gateway | The canonical incremental target is intentionally single-owner; waiting behind a foreground dev process cannot complete. | Stop and make an explicit canonical integration decision. Do not move the command into a Worker lane, add `--wait-build`, or hand-compose `CARGO_TARGET_DIR`. |
| A Windows dev build fails while writing `codeterm_lib.lib` with `os error 112` or a localized “not enough disk space” message | The canonical `src-tauri/target` exhausted the checkout drive while Cargo was replacing the debug archive. | Verify the failure owns the canonical target and no dev build is running, then run `.\build.ps1 clean`. Confirm free space before restarting `.\build.ps1 dev`; do not delete another worktree's target or touch production. |
| A Windows native dependency fails with `LNK1106` and `dumpbin` shows a cached `.o` ending before its COFF string-table length | `sccache` retained a truncated C/C++ object; a clean Cargo target can restore the same corrupt cache entry. | In PowerShell set `$env:SCCACHE_RECACHE='1'`, rerun the failing `.\build.ps1` operation once, then remove the environment variable. Verify the rebuilt object with `dumpbin` before retrying normally. |
| A build in one worktree could reuse a cached artifact built from a different worktree's sources (codeterm#327). Never reproduced on demand — this is a configuration hazard, not an observed rebuild; see `scripts/check-sccache-basedir-isolation.sh`. | `scripts/cache-policy.sh` and `scripts/test-capped.sh` defaulted `SCCACHE_BASEDIRS` to the current worktree root. That tells sccache to strip the worktree path before hashing, so two worktrees sharing one `SCCACHE_DIR`/server can derive the same cache key even though their absolute paths differ. | Basedir rewriting is disabled for this repo: `ct_reject_basedir_rewriting` in `scripts/cache-policy.sh` unsets `SCCACHE_BASEDIRS` (warning if a parent shell leaked one in) before every `ct_configure_sccache`/`ct_require_prestarted_sccache` call, and `scripts/test-capped.sh` no longer sets it. Machine-level `sccache` and per-worktree `CARGO_TARGET_DIR` isolation are unaffected. Verify with `bash scripts/check-sccache-basedir-isolation.sh`; do not reintroduce `SCCACHE_BASEDIRS` for this repo. |
| `rustc` exits with `0xc0000005 (STATUS_ACCESS_VIOLATION)` while compiling an unchanged dependency | The Windows compiler process crashed before producing a source diagnostic; this is not a Rust assertion or test failure. | Rerun the same bounded `build.ps1` operation once so completed artifacts are reused. If it repeats, stop, preserve the exact rustc command and Windows crash evidence, and report the toolchain fault; do not clean targets or raise test deadlines. |
| After an OS crash, `rustc` panics while linking with `Invalid archive terminator` | A generated `.rlib` in the canonical Cargo target was truncated; incremental metadata cleanup cannot repair the archive payload. | Confirm no dev build is running, run `.\build.ps1 clean`, then restart the original named operation. Do not delete individual archives or reuse the damaged target. |
| Dev compiles and opens but port 7686 never listens, with a trust-boundary policy error in `~/.codeterm-dev/logs/codeterm.log` | The retained dev config requests a non-loopback host without a bearer token, or contains a wildcard/malformed CORS origin. | Keep dev local by setting `api.host = "127.0.0.1"`, or configure a token for an intentional network-facing instance. Remove `*` and malformed entries from `api.cors_allow_origins`; an empty list is valid and denies all cross-origin browser requests. |
| Windows `build.ps1 kill` reports that the clean shutdown deadline elapsed after a high-tab-count dev fixture | Closing many PTYs and watcher leases can exceed the bounded graceful-shutdown window even though the instance is otherwise healthy. | Let the named operation use its port-scoped force-stop fallback, then verify 7686 is free and 7685 still listens. Never replace it with a process-name kill. |
| `test-one plan` fails to compile old `Task` fixtures after an orchestration schema change | The app gate compiled the plan crate as a dependency but did not compile its own unit-test fixtures. | Update every direct `Task` fixture and changed helper signature, then rerun the exact `test-one plan` filter. Do not remove the plan gate or move it to an app filter. |
| `test-one plan` reports E0599 for `ReportStatus::InProgress` in a new fixture | The fixture confused task lifecycle status with the separate report-status enum. | Use the existing `ReportStatus::Partial` for a non-review progress report, preserve the rejection assertion, and rerun the failed focused selector before remaining gates. |
| `test-one cli` cannot find a private helper after a command module split while `check` stays green | The binary compiles, but the test target still relied on a parent-module helper that moved behind a private child-module boundary. | Give the test its own fixture helper or import a test-only API; do not widen production visibility. Rerun the exact CLI filter that exposed the test target. |
| `test-one cli` reports a non-exhaustive `TabAction` match after adding a command | The CLI command was added without its mesh API-map consumer. | Add the capability-equivalent route in `mesh_cli/api_map.rs`, then rerun the focused CLI test. |
| `npm run test:plugins` reports `'node' is not recognized` before the suite header, or Git Bash cannot find Node | An oversized/polluted Windows `PATH` was lost at a `cmd.exe` boundary; the former `npx.cmd` runner added another shell boundary. | Run `node scripts/run-plugin-tests.mjs` through a known Node executable. The aggregator pins `process.execPath` into Git Bash and uses the checked-in local tsx CLI; shorten the inherited `PATH` before retrying the npm wrapper. |
| TypeScript rejects a narrowed optional mutation error inside a state callback | Narrowing an object property is not retained across the deferred callback. | Capture the error in a local constant before the guard and callback, then rerun type checking and the owning Settings suite. |
| `test-one app` reports E0308 after removing a hook event | A fixture retained the old fixed array length after its event roster shrank. | Update the owning fixture length to the actual roster and rerun the failed focused app filter; plugin-only compilation does not cover app test fixtures. |
| `test-one app` reports a missing field in `SpawnTabSpec` after adding spawn routing state | A full constructor or test fixture bypassed `Default`, so ordinary source review missed the new required field. | Set the field explicitly in every full constructor and fixture, then rerun the exact focused test before continuing. |
| The docs gate reports `API-TRANSPORT exceeds 24 KiB` after adding an RPC row | The transport index is at its embedded size boundary. | Keep the index row concise and put the purge procedure and storage limits in MEMORY; rerun the focused embedded-doc test without raising its limit. |
| `test-one cli cmd::docs` reports an embedded document over its line boundary | Independently valid reference additions accumulated past the agent reading limit, and ordinary compilation does not exercise the embedded-doc contract. | Compact repeated prose without deleting behavior, or split the document by responsibility and register the new whole document. Rerun the exact docs filter. |
| `verify` fails both an embedded-doc boundary and `cli_manifest_is_fresh` | A documentation edit crossed its registered reading limit while committed CLI metadata already lagged the current command surface. | Compact or split the named document, regenerate with the freshly built `codeterm cli-manifest --write`, rerun both focused CLI tests, then rerun `verify`. |
| Focused `tsx` tests pass but `verify` reports fixture duplicates, unused bindings, or incomplete interface objects | Runtime transpilation exercised behavior without the TypeScript compiler's static contract. | Repair the fixtures and call sites, run the repository TypeScript compiler boundary, then rerun the focused tests and `verify`; runtime green alone is not integration proof. |
| Focused TS unit tests pass but `verify` reports TS2550 for `Array.at` or TS2367 after repeated count assertions | The direct `tsx` runner does not enforce the repository TypeScript target, and an assertion signature can narrow a mutable observation to a stale literal. | Use target-compatible indexing and a non-narrowing equality helper for repeated observations, rerun the focused unit file, then rerun `verify`. |
| `test-one app <filter>` exits 0 after running 0 tests for code under a workspace crate | The filter was sent to the app test binary instead of the crate that owns the tests, so a zero-match run looked green. | Use the supported owning target from `build.ps1 help` (for example, `test-one mem <filter>`), confirm the output reports a nonzero test count, then rerun the broad gate. |
| A seed fail-closed test expects `Unreadable` but reports a new canonical item was created | The fixture corrupted `title` or `kind`, which are lookup keys, so the seeder could not discover the old row to classify its payload. | Exercise unreadability through a non-key managed field such as an invalid stored scope, assert the raw row remains unchanged, and cover title/kind drift separately as preserved old rows plus distinct canonical seeds. |
| A plan-transfer guard fails with `Tab <planner> not found` for a dead Planner pointer | The lifecycle transition treated the persisted Planner reference as a live incumbent and tried to revoke a tab that no longer exists before clearing the plan reference. | Revoke only a live incumbent tab; let the plan rebind clear an absent Planner reference transactionally, then rerun the dead-Planner transfer guard and the full verify gate. |
| A focused `npx tsx` test on Windows reports `'tsx' is not recognized` even though `node_modules/.bin/tsx.cmd` exists | An inherited PATH longer than the legacy child-shell limit prevents the npm batch shim from resolving Node. | Resolve Node in PowerShell with `(Get-Command node.exe).Source` and invoke `node_modules/tsx/dist/cli.mjs <test-file>` directly; then remove stale PATH entries from the parent environment before the next session. |
| `pnpm test:unit <filter>` starts from PowerShell but its npm child says `node is not recognized` | The pnpm launcher found Node by absolute path, while the nested npm script re-resolved `node` through a child shell whose executable lookup failed. | Invoke the repository unit runner with the resolved Node executable and the same filter (`node scripts/run-ts-unit-tests.mjs <filter>`); do not alter global PATH during a gate. |
| `release-preflight` times out a provider-update test while its focused suite passes | The event-driven test's failure deadline was shorter than full-suite scheduler contention, so a healthy background transition was classified as missing. | Keep the test waiting on the emitted effect and use the full-suite-safe failure deadline; rerun the focused provider-update suite, then `release-preflight`. Do not add a sleep or make elapsed time a success condition. |
| A gateway log ends with `exit: 0` but the wrapper never returns | A FIFO reader can remain blocked after the writer closes, retaining the machine slot. | Update `build.sh`: reader EOF now has a 10-second failure deadline after writer closure; expiry terminates only the matching reader, exits 74, releases the gateway, and preserves `command_exit_code` (including 0). Inspect `gateway-status` for `logging_failed`; do not interpret inner success as wrapper success. Verify with `./build.sh gateway-logging-check`, whose stuck-reader fixtures preserve command exits 0 and 101. For a wrapper running the old script, interrupt only that wrapper and inspect the reaped job before another operation. |

| Dev opens with a runtime identity mismatch while `codeterm-web-watch.log` repeats `'vite' is not recognized` | The Windows watcher launched Vite through a nested pnpm lifecycle shell that lost local executable lookup, leaving `dist/` stale while the daemon rebuilt. | Use the current dev operation, which launches Vite through the resolved Node process and package entrypoint, then restart dev and verify the runtime/UI identity banner is gone. |
| `build.ps1 check` cannot resolve a newly re-exported plan mutation type | A split mutation module relied on `use super::*`, but the parent module did not import that public service type. | Import the type explicitly from `crate::service_plan`, then rerun the same canonical check. |
| A focused app test passes `&ServiceContext` to a helper that only borrows service state but compilation expects `&Arc<ServiceContext>` | The helper accepted the runtime `ServiceState` alias even though it did not need shared ownership, making ordinary test fixtures incompatible. | Narrow the helper parameter to `&ServiceContext`; runtime `Arc` callers deref-coerce while fixtures pass the same service directly. Rerun the exact focused test. |
| Removing an unused production import makes an inline `#[cfg(test)]` module fail to resolve the same type | The test relied on its parent module's unconditional import instead of declaring test-only scope. | Reintroduce the type behind `#[cfg(test)]` or import it inside the test module, then rerun the focused test. |
| Linux CI Clippy rejects unused helpers, identical redaction branches, or a no-effect replacement | A refactor left test-only APIs in production, duplicated route vocabulary, or retained redundant branches. | Gate test-only APIs with `cfg(test)`, use the shared route descriptor, and collapse equivalent operations; rerun focused selectors and the repository Clippy gate without suppressing warnings. |
| A live Plan owner with a valid current session and nonce is refused after the tab session rotates | The owner predicate authenticated the live tab and then redundantly compared its session to stale Plan metadata. | Keep live session and nonce validation at the tab boundary, identify Plan ownership by the bound tab plus Team authority, and rerun the rotated-session authority table. |
| `build.ps1 e2e` fails on Windows with `python3: command not found` even though Python is installed | The E2E operation hardcoded the Unix executable name instead of resolving the available Python launcher. | Use the build operation that resolves `python3` or `python` and invokes pip through that same interpreter, then rerun `build.ps1 e2e`. |
| A foreground `build.ps1 dev` exits with `unexpected EOF` after `build.sh` is edited during startup | Bash was still reading the running script while its file contents changed underneath that process. | Finish build-script edits before launching dev, validate with `bash -n build.sh`, then restart dev through the named operation. |
| `build.sh dev` appears idle at dependency installation until Enter is pressed | pnpm opened an interactive confirmation prompt in the visible dev terminal while its diagnostic stream was suppressed. | Use the current build operation, which always runs dependency installation non-interactively, then restart dev. |
| Tauri dev starts from PowerShell but its `beforeDevCommand` reports `node is not recognized` | The nested Tauri hook re-resolved Node through a child Windows shell and lost the launcher environment. | Run dev preparation directly in `build.sh` with the already-resolved Node process, then launch Tauri with the generated runtime config that disables the duplicate hook. |
| A piped macOS build reports `wait: pid ... is not a child of this shell` and exits 74 after the command succeeded | Bash 3 detached the process-substitution log reader from the pipeline subshell. | Update to a build script that uses the explicit FIFO-backed reader child, then rerun the original named build operation without filtering its output. |
| Semantic-search tests execute a zero-byte `codeterm-embed` | A compile stub reached a real embedding test. | Build the debug sidecar before Rust library tests. |
| `verify` downloads FastEmbed into a temporary test data directory and fails on a model-host outage | The Rust test wrapper let fixture-owned `CODETERM_DATA_DIR` choose the immutable model cache. | Use the current wrapper, which exports one machine-shared `CODETERM_EMBED_CACHE_DIR`; override it explicitly only for cache diagnostics. |
| CLI essence-map test fails after a capability status wording update | The test pinned source prose rather than the status preservation mechanism. | Inject opaque statuses into fixture capability rows and assert those supplied values round-trip; retain node/edge assertions. |
| Nextest metadata reuse exits 2 with `--manifest-path cannot be used with --cargo-metadata` before selecting tests | The reuse invocation retained a Cargo build selector. | Pass only the saved binaries/Cargo metadata and runtime/filter options to the capped execution phase; keep manifest/package/feature selectors exclusively on the inventory compilation. Rerun the same bounded selector, preserving the zero-executed receipt. |
| macOS task-authority tests panic creating a Tauri `EventLoop` | The fixture built Wry on a libtest thread. | Use `app_runtime::builder()` and runtime aliases so `cfg(test)` constructs MockRuntime; remove fixture-only desktop ignores. Keep real native window oracles explicitly platform-gated. |
| Unix REST lifecycle bind gets `Address already in use` | A guessed test port can collide; restart listeners overlap during handoff. | Prebind OS-assigned generation one; enable `SO_REUSEPORT` on every Unix bind. |
| REST port-release fails only in the parallel macOS suite | Closing a real occupier races Darwin. | Inject the first bind failure, then prove recovery binds a healthy listener. |
| Linux clippy rejects a test-only parameter | Only `cfg(test)` uses it. | Keep injection at the test call site, outside production signatures. |
| Storage clippy reports `manual_hash_one` in asset staging tokens | The token helper manually built and finalized a hasher. | Use `BuildHasher::hash_one` on the same salt, sequence, time and process tuple; rerun the owning clippy gate. |
| Unix clippy flags Windows-only probe helpers and test-only deadline receipt fields | Shared code compiled unused platform or test telemetry in the production profile. | Gate helpers and receipt fields by their actual consumers; retain shared resource labels and unchanged cleanup predicates, then rerun clippy and probe controls. |
| Standalone crate tests fail after local `verify` passed | The old gate ran only the app package. | Use current `verify`; it runs every workspace library package to completion. |
| CLI docs report `BUILD exceeds 24 KiB` or `ORCHESTRATION exceeds 240 lines` | Troubleshooting or the compact operating contract outgrew its reading boundary. | Compress repetition or move detail to the named reference playbook; rerun `build.ps1 test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary`. |
| Prompt tests fail a composed contract or launch byte ceiling | Shared fragments and the embedded manual accumulated beyond the whole launch budget. | Compact repetition without weakening authority or raising limits; rerun `test-one app prompts::tests` and the matching composed prompt comparison. |
| Source-size gate counts downloaded SDK declarations in `target-check/` | The traversal included ignored generated benchmark artifacts. | Prune that root output tree only when Git confirms ignored/untracked ownership; all authored source stays guarded. Use a scalar prune path: Bash3.2 with `set -u` rejects empty array expansion inside the traversal. Preserve fixture bytes; rerun the guard controls and size gate. |
| Windows plan-index reports `queue did not drain` | An inner deadline raced background work. | Wait on the queue condition and rely on the test process exit. |
| Linux semantic search gets `Permission denied` or `Exec format error` for `codeterm-embed` | Clippy replaced the built runtime sidecar's mode or bytes. | Preserve the verified ELF before clippy, then restore its bytes and executable mode before tests. |
| `check-tests` emits allocation failures, rustc `0xc0000409`, then missing-rlib cascades while another app build is active | Concurrent app compilation exhausted available host memory; the later rlib errors are secondary until proven otherwise. | Preserve the first allocation failure and raw exits. Defer the app checks to the owner's canonical batch with dev stopped; use `CARGO_BUILD_JOBS=1` for the named operation if memory pressure persists. Never kill production or clean shared targets from a Worker lane. |
| Windows PowerShell QA capture throws `NativeCommandError` on ordinary Cargo progress and records no exit | Caller-level `*>` redirection turns native stderr into PowerShell errors under `build.ps1` strict error handling. | Run a child `powershell -NoProfile -File .\build.ps1 ...` with OS-level `cmd /d /c` redirection (`> run.log 2>&1`), then record the child exit code. Do not classify the progress line as a compiler failure. |
| `test-one` fails with sccache socket error 10048 and has no build-history row | A scope bypassed gateway acquisition and configured sccache before owning the build slot. | Use the shared cargo gateway entry for every scope and cargo-running verb, including CLI, check-tests, CI inputs and named dev. Check `build-history` for each invocation; run the mixed-verb fixture in `scripts/build-gateway-cache.test.mjs`. After merge, run two focused `test-one` and two `check-queue` calls concurrently with `--wait-build`, and retain all four exits and history rows. |
| `check-tests` fails at catalogue watcher `targets.sort()` / `dedup()` | The watcher parameter was immutable. | Declare `mut targets` in `ProviderBinaryWatchers::watch`; rerun the authorized `check-tests` and focused catalogue gates. |
| A release-time test fails in `#[cfg(test)]` code while `check` was green | `check` compiles the production profile only, so test-only code is never type-checked | Run `./build.ps1 check-tests`, which compiles `--all-targets` for both workspaces through the build gateway. A stale generated map or a document over its reading boundary is a different cause: use the matching row above. |

| Plugin test compilation cannot read an include_str! fixture | The relative fixture path traversed one directory beyond the repository root. | Resolve the path from the owning Rust source directory, correct the traversal, and rerun the same focused plugin gate before proceeding. |

A moved repository-watcher source guard can fail with “expected the repository-watch sources, found 1” if it still scans the app path. Scan the new crate sources and retained app adapter, keep channel failure deadlines in test support, then rerun `./build.sh test-one repowatch tests::`.

## Focused analytics checks

`./build.sh test-one analytics tests::` runs the isolated product analytics crate
through the capped Rust runner in canonical. Workers only format its Rust sources;
its HTTP fixtures use a local fake destination with synthetic retry timestamps.

## Related documents

Tooling, outputs, platform notes, and repository layout: `.agents/skills/build-reference/SKILL.md`. Test workflows: `.agents/skills/testing/SKILL.md`.

### Windows provider launch verification

If Codex exits with invalid TOML containing `developer_instructions = {"value":...}`,
Windows PowerShell serialized `Get-Content` filesystem metadata instead of a plain
string. Load instructions through `System.IO.File.ReadAllText`; run
`node node_modules/tsx/dist/cli.mjs plugins/provider-codex/tests/windows-launch.test.ts`
and a real dev spawn before local self-install. The regression executes the generated
PowerShell command with an isolated profile and verifies exact instruction bytes.

If `verify` fails because a review-capacity test expects a refusal at WIP saturation,
update that owning test to the advisory contract and retain separate real-failure
rollback coverage. Run `build.ps1 test-one app spawn_plan_task`, then `verify`;
do not restore a hard gate merely to preserve an obsolete assertion.



## Build-time vendor configuration

Inject these environment variables before the app build; CI reads GitHub secrets
with the same names. Empty or missing secrets do not fail compilation.

- DOMIOS_FIREBASE_API_KEY
- DOMIOS_FIREBASE_PROJECT_ID
- DOMIOS_FIREBASE_APP_ID
- DOMIOS_ACCOUNT_REDEEM_URL
- DOMIOS_BILLING_FUNCTIONS_URL
- DOMIOS_AMPLITUDE_API_KEY_TEST
- DOMIOS_AMPLITUDE_API_KEY_PROD

Account, billing and analytics own one build_config module each, using option_env!.
Only vendor adapters consume it. Runtime account and billing config overrides
the compiled defaults; Amplitude prefers the compiled key over the secret store.
Without Firebase values account uses local; without a billing endpoint billing
returns free locally and reports subscription actions unavailable. Missing
Amplitude credentials leave queued events undelivered with unavailable status.
Changing build-time values requires rebuilding; never commit vendor credentials.
Firebase Functions, admin and hosted sign-in source live in the private
[Firebase repository](https://github.com/rollacode/codeterm-firebase), with the
former cloud contents at its root. Follow its config generation, package checks
and deployment procedures there. The private
[website repository](https://github.com/rollacode/codeterm-web) owns the marketing
site build and deployment, with the former web app at its root. Repository
creation does not establish production readiness or authorize publishing.
Desktop account and billing build variables above remain app dependencies.
Preserve domios://auth/callback, domios://billing/return and authenticated
entitlement refresh when integrating either external service.

Fresh lanes may lack node_modules: pnpm exec tsc or vitest reports "Command not
found", and plugin generation or the unit runner reports ERR_MODULE_NOT_FOUND for
esbuild or tsx. Run
pnpm install --frozen-lockfile in the lane's Team QA shell, then rerun the affected
non-Rust checks. A failed dependency lookup selected no executable tests.
<!-- revision: 30 -->

### Lane and batch scopes

Workers run rustfmt, source-size and service-core-boundary guards locally, then one `./build.sh check-queue --lane <sha>`. Lane checks use Clippy libraries for existing touched workspace packages, root bins only on root changes, and leaf test targets only for changed test paths or moved crates. Service-core tests stay at the batch; CLI uses its own binary-only check when cli/ changes. Require errors=0 and warnings=0, retain the summary and list test-one selectors. Never poll a busy gateway. Batches use the default full check-queue; a batch failure bounces the owning lane with the integration SHA.

Canonical QA builds `./build.sh nextest-archive <sha>` once. The verified warm worktree builds root and service-core default-feature library tests into one atomically published gateway `test-archives/<sha>/app.tar.zst`. `test-one app <selector>` and `nextest app [filterset]` reuse it on the same clean Rust SHA, without recompilation; a missing archive uses the ordinary build path. rdeps filtersets use the ordinary workspace path. Both app runners use default embeddings. The feature guard refuses divergence between root dev, root defaults and core default/test features. Canonical measurements use `./build.sh nextest-archive <sha> --measure <selector>...`, within the verified warm checkout for that SHA. Archives print build wall time; archived test-one prints selector wall time. Preserve logs and history for measurements; never infer archive reuse from a short duration.

If a completed job reports `logging_degraded`, its command exit and completed/failed state are retained; the warning names the partial log. A tee/EOF drain failure formerly overwrote successful Cargo exit 0 with wrapper exit 74. Inspect the full check log for diagnostics; run `./build.sh gateway-logging-check`. An unknown command exit or metadata publication failure still refuses readiness.
A mutex refusal prints the state.lock owner PID, start token, job ID, publication age and queued waiter tuples. Unknown values remain unknown; age never authorizes reclamation. Capture this evidence with the job receipt before investigating contention.

A Windows `EPERM` while renaming `state.lock.pending.*` to `state.lock` is a publication sharing conflict. The helper releases its publication guard, retains the same candidate, and waits for a mutex ownership event before retrying inside the existing failure deadline. Its own guard changes and unrelated file events do not drive retries. A deadline refusal removes the unpublished candidate. Run `node --test scripts/build-gateway-mutex.test.mjs`; capture the owner/waiter tuple if the bounded event wait expires, without deleting unknown shared ownership.

A queued PowerShell -> Git Bash client must retain its actual Bash waiter identity. Job, queue and slot ownership now record BASHPID with its MSYS start token, mapped Windows PID and Windows start token. On Windows reaping probes the captured native identity, not the MSYS number; unavailable native evidence remains unknown and never reaps. Older incomplete ownership needs an owner decision, not inferred death from a missing proc entry. Run `node --test scripts/build-gateway-owner.test.mjs` for the real build.ps1 chain and namespace model; capture both identity pairs when reporting a future owner-lost incident.

Canonical P3 measurement on core-touching SHA `5df475f92` (205 files): full check 157 s (job `20261014T210857-13756-8399`), `--lane` 148 s with errors=0, warnings=0, exit=0 (`tone-gates-20261014-230937-49731`). The saving is 9 s (about 6%); service-core compilation dominates this lane. Do not promise large savings for core-touching lanes. Archive SHA `d001809b8` built in 367 s (job `20261014T232911-133-4384`); selector timing was invalid because the first archived run refused its argv with exit 2 after 1 s. Selector measurements must be rerun after the argv repair.

An archived nextest run rejects Cargo package/target selection such as `-p` and `--lib` together with `--archive-file`. Select the archived core library using `--filter-expr 'package(=codeterm-service-core) & kind(lib)'`, intersecting any caller filterset, while retaining workspace remapping and zero-match refusal. Both app runners use the same archive argument builder. Run `node --test scripts/nextest-archive.test.mjs`; its exact captured selector argv is checked by the installed nextest parser with a nonexistent archive (no Cargo build or test execution), and the former conflicting argv must refuse as a usage error. Canonical then reruns `./build.sh nextest-archive d001809b8 --measure service::close_history service::team service::workspace service::plugin_host sentinel:: socket::server::plan_review api::mesh_routes::rpc commands::html_preview startup:: update::`.

If an archived selector refuses `workspace root manifest at X:\Cargo.toml does not exist` (job `20261015T000926-46028-23229`), the remap points at the checkout rather than the Cargo workspace. App archive runners and serial slow-test confirmation remap to `<checkout>/src-tauri`, where Cargo.toml contains `[workspace]`. The shared Bash builder refuses a missing manifest before running nextest; archive and serial-confirmation fixtures assert the target manifest. The failed first selector's exit 96 / 4 s is not a test execution measurement. Rerun the canonical measurement command above after merging the repair.
