---
name: build
description: "Choose and run Domios build operations, including dev work, local installs, and troubleshooting."
---

# Domios — Build Guide

Shared Workers use `./build.sh fmt --paths <owned-files>` after declaring edit paths; repository-wide formatting belongs to the integrator. Local mutating dispatch shares canonical by default, selecting a lane for overlap or `build-breaking` work. Stage and commit only owned paths. Shared compile failures identify foreign source paths without changing their nonzero exit. Remaining lanes retain the serialized shared intermediate cache and separate final targets. Sccache's Rust cwd hashing is not normalized by `SCCACHE_BASEDIRS`; do not claim equal worktree keys from that setting. DEV policy remains incremental without sccache.

## Prerequisites

### Windows
- **Rust** 1.99.1 (pinned by `rust-toolchain.toml`)
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

## Local Linux package

Use the normal gateway entry for a local Debian package, including its frontend,
CLI and embedded resources:

```bash
cat > /tmp/codeterm-local-bundle.json <<'EOF'
{"bundle":{"createUpdaterArtifacts":false}}
EOF
./build.sh release-bundle --bundles deb --config /tmp/codeterm-local-bundle.json --ci
```

`release-bundle` forwards trailing Tauri flags, including config paths with spaces.
This override produces a local installer without updater artifacts; public
publishing keeps the signed release workflow. The package is written under
`src-tauri/target/release/bundle/deb` unless the normal target policy selects
another existing target root.

Hosted Linux/Windows arm64 use `release-arm64-compile TAG SOURCE_SHA ARCHIVE` with
an explicit `CARGO_BUILD_TARGET` and `CODETERM_USE_SCCACHE=0`. This operation builds
the shared inputs and unsigned bundles, then freezes the handoff inside the same
gateway session so Windows Cargo receipt paths stay accessible. No signing or S3
credentials may be present. Private finalizers consume the digest-pinned archive;
they do not invoke a compiler. Setup and measurement: [hosted arm64](../../../docs/release/hosted-arm64.md).

Queued builds watch only the gateway root, queue entries and the current owner
directory. Recursive slot watchers also traverse Cargo artifacts and transient
`state.lock.pending.*` directories; Node can race their removal and abort a
queued build with `ENOENT`. Keep subscriptions established before checking the
captured snapshot, and reattach the owner watcher after directory replacement.

Packaging entrypoints (`release-bundle`, `build`, `install-local`) prepare the
checked frontend and bundled CLI, then let Tauri compile the native application
once with the final merged config. Do not precompile the app before Tauri:
`--config` changes `TAURI_CONFIG`, a tracked build-script input, and recompiles
the app even when Cargo's feature graph matches. Standalone `release-inputs`
still compiles the app for callers that request those artifacts explicitly.

Measure the full installer command above, not only `release-inputs`. Record
wall time and whether Cargo outputs and compiler-cache entries were retained.
A new commit rebuilds the stamped CLI and app; a backend edit can also rebuild
large crates such as service-core or API. A warm installer below five minutes
does not establish the same bound for an empty Cargo and compiler cache.

For a committed, clean-tree measurement without any existing Cargo outputs,
insert `--fresh-target` immediately after `release-bundle`. It uses the existing
fresh release-target allocator, disables incremental compilation as release
builds do, and preserves compiler-cache entries and the normal warm targets.
The gateway log prints the new target path. This is cold Cargo with a retained
compiler cache; do not call it fully cold. Check disk space first and retain
the resulting package before removing any inactive measurement outputs.

Read [Cargo timings](https://doc.rust-lang.org/cargo/reference/timings.html),
[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html), and
the [Rust Performance Book](https://nnethercote.github.io/perf-book/compile-times.html)
before changing code generation. Prefer eliminating duplicate work and expensive
generic expansion; changing optimization or codegen units trades runtime behavior
for compile time and needs separate measurements. Preserve tuned profiles,
embeddings, warning checks, package resources and signing policy.

A native compiler wrapper enables reuse across targets. Named DEV selects it
automatically; fresh release targets retain the opt-in:
`CODETERM_PORTABLE_COMPILER_CACHE=1 ./build.sh release-bundle --fresh-target ...`.
Sccache otherwise hashes `CARGO_TARGET_DIR`, `CARGO_BUILD_BUILD_DIR` and
`CARGO_INCREMENTAL` for every crate, even when only Cargo uses them. The wrapper
moves these driver settings out of the blanket `CARGO_*` hash and restores them
before rustc runs.
Values read by `env!`/`option_env!` still appear in rustc dep-info and remain
cache inputs. Optimization flags, generated bytes, checked-in source paths,
working directories and per-worktree isolation stay intact; basedir rewriting
remains disabled. Compiler identity and wrapper source select an immutable
native launcher, including on Windows, without cmd.exe argument-length limits.

Generated Rust source locations under an explicit target or configured Cargo
build directory map to a
machine-stable virtual output prefix under the existing Cargo-home wrapper
cache. Identical wrapper source and toolchain share one immutable launcher
across checkouts; its key does not include the script's checkout path.
Otherwise Serde's generated `private.rs` path changes its `.rmeta` bytes and
invalidates every dependent cache entry.
The policy resolves the configured build directory through Cargo metadata;
it neither changes that directory nor exports a new Cargo driver value.
The policy version and prefix enter the cache key; `OUT_DIR` itself remains
tracked and its runtime string value is preserved. Dep-info still names the
real generated files. The proxy is named `sccache` and forwards C/C++ compilers
to the real cache client, preserving cc-rs automatic acceleration for SQLite,
QuickJS and other native dependencies. Never rename it to an unrecognized
wrapper basename without replacing that cc-rs integration.
The launcher binds the real sccache client by absolute path. Windows searches
the executable's own directory before PATH; a bare `sccache` child would select
the launcher itself and break Cargo's compiler probes.

Run `./build.sh cache-wrapper-check` for a real-cache fixture proving relocation
hits, tracked-variable misses, unset values, compiler probes and error exits.
It also proves identical generated-source metadata, a downstream consumer hit,
unchanged runtime `OUT_DIR` strings and a real C compiler cache hit.
Native wrapper identity tests run through this operation with
`CODETERM_CACHE_WRAPPER_TEST=1`; ordinary script tests do not require real sccache.
Release use remains opt-in until measured on the other build machines. Keep
compiler-cache capacity large enough for the release graph; once populated,
new empty Cargo targets can reuse cacheable crates. Initial population still
requires compilation. Binaries, proc macros and crates using generated absolute
paths can remain non-cacheable or miss; do not promise a fully cold time from a
populated-cache measurement.

## Docker smoke inputs

On native Ubuntu CI, use `CI=true DOMIOS_DOCKER_SMOKE=1
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu bash ./build.sh docker-smoke-inputs`
(or the native `aarch64-unknown-linux-gnu` target). This operation builds the
frontend and CLI/server with the release profile, binds the exact CLI digest,
checks the headless closure and writes the archive and checksum under
`target/docker-smoke/archives/`. It installs nothing and publishes nothing.
Only the Dockerfile's explicit `ARCHIVE_SOURCE=ci-smoke` plus `ALLOW_CI_SMOKE=1`
path accepts these unsigned inputs; production defaults to minisign verification.
Run the resulting image only through `.github/workflows/docker-smoke.yml`.

## Debug staging without runtime launch

Native Neighbor Firestore uses Google TLS for cloud targets. Only the existing
explicit DEBUG `DOMIOS_FIREBASE_EMULATOR=1` account selection admits the fixed
`http://127.0.0.1:18080`, `demo-domios/(default)` transport; QA/emulator mixed
configuration remains refused. No `FIRESTORE_EMULATOR_HOST` override is consumed.
Use the real selected account ID token and normal emulator protocol/rules.
Parent QA must prove state, registry, inbox and pending reception on the matched
DEBUG capture before first receiver/key and signed delivery acceptance. A protocol
or authentication refusal is evidence to stop, never authority to replay mutations.
The parent adapter union is `./build.sh nextest codeterm-neighbors 'test(adapter::)'`.
Its named router must enable `--features runtime`; the generic
`--no-default-features` package route excludes the adapter. Require nonzero
policy, stream and authentication discovery before accepting a test receipt.

`./build.sh dev --build-only` uses the existing named DEV target and normal
CLI/server/client/frontend build, staging and schema3 provenance. Supply the
existing `CODETERM_DEV_INSTANCE` and its private `CODETERM_DEV_API_PORT`; all
instances from that target, including the selected instance, must already be
stopped. Running or unknown liveness refuses; this mode never stops a process.
After staging and provenance verification it releases mapping/gateway ownership
and returns `BUILD/STAGED`, not READY. There is no GUI/server/account initialization,
retained-window swap or prebuilt handoff. `--wait` is accepted but unnecessary.
Empty/qa `DOMIOS_ACCOUNT_TARGET` remains the ordinary compile-time selection;
source/account changes require a fresh build, never receipt edits or endpoint
redirection of a running QA artifact. See dev-run for the stopped-instance recipe
and separate isolated common-layout capture boundary.

Deferred focused coverage after source review:
`node --test scripts/tests/dev-gateway-handoff.test.mjs scripts/tests/dev-shared-target.test.mjs`.
Pure fixtures prove no launch/stop callbacks, staging failure, account selection,
provenance-before-release ordering and liveness refusals; actual build-only staging
still requires the parent-authorized canonical build and artifact readback.

## Rebuild cost

Named `dev` and `dev-server` compile incrementally (owner rule). Every named
instance of a checkout shares the one warm DEV target `src-tauri/target/named-dev`,
which rebuilds only what changed. They run with `CARGO_INCREMENTAL=1`, without
the portable wrapper, and with `RUSTC_WRAPPER=` set empty so the machine-level
`build.rustc-wrapper = "sccache"` does not apply. sccache rejects an explicit
`CARGO_INCREMENTAL=1`, and incremental crates are uncacheable anyway. Release,
CI, `install-local` and `worktree-check` keep `CARGO_INCREMENTAL=0` with sccache.
Several instances run from the one build (`dev-prebuilt`); `dev`/`dev-server`
refuse while another DEV from it runs. Never use a new instance name to get a
fresh target (AGENTS.md). The dev-run skill covers instance identity, the shared
build record and kill scoping.

Fingerprints when switching an existing target to this policy (verified with
Cargo 1.99 on a scratch crate): Cargo's unit fingerprint hashes neither
`RUSTC_WRAPPER` nor wrapper-added rustc flags such as `--remap-path-prefix`.
Cargo forces `incremental=false` for non-path (registry/git) packages, so
dropping the wrapper and enabling incremental keeps every registry dependency
`Fresh`. Moving the target directory also keeps them `Fresh`; Cargo rewrites a
moved `OUT_DIR` in build-script output. Only path packages (workspace members,
`workspace-hack`, local path dependencies) recompile, once, to seed their
incremental state. A build script that declared
`rerun-if-env-changed=RUSTC_WRAPPER` or `CARGO_INCREMENTAL`, or an absolute
`rerun-if-changed` into the old target, would rerun after the switch. The warm
qa8 target had none (`grep` over `debug/build/*/output`); recheck before
relying on another target.

The one-time adoption renames a warm legacy `src-tauri/target/dev-instances/<name>`
to `named-dev` (dev-run skill). Instance identity, frontend URL and browser/CDP
settings are applied at runtime in debug Development launches; source and CLI
artifact changes can still relink the roots. Canonical QA and lane-check retain
their own profile and isolation policies.

DEV pins the configured public QA project once across CLI, server and client
Cargo phases. Shared entitlement code otherwise flips between absent and
configured compile-time project values, rebuilding mesh and server consumers.
Firebase keys remain scoped to server compilation; release configuration and
the entitlement verification policy retain their existing behavior.

Local `dev` and `dev-server` read build secrets from the main checkout's ignored
`.env`, resolved through Git's common directory. Lanes read that same local file;
never copy it into a lane, CI job, archive or artifact. Values remain private to
the launcher until their existing compiler phase needs them. Release and CI keep
their existing secure sources and never load this file.

Run `./build.sh dev-env` once to create it from the configured vault, and rerun
after rotating a build key. The operation preserves unrelated entries, atomically
updates the build entries, refuses CI or a Git-tracked path, and limits access to
the current user (mode 0600 on Unix, protected user-only ACL on Windows). It prints
key names only. Verify exclusion with `git check-ignore .env` in the main
checkout. Missing configuration reports the missing key name without its value.
The keys are `DOMIOS_FIREBASE_API_KEY`, `DOMIOS_FIREBASE_PROJECT_ID`,
`DOMIOS_FIREBASE_APP_ID`, `DOMIOS_ACCOUNT_REDEEM_URL`, `DOMIOS_QA_PROJECT_ID`,
`DOMIOS_QA_FIREBASE_API_KEY`, `DOMIOS_QA_FIREBASE_APP_ID`,
`DOMIOS_BILLING_FUNCTIONS_URL`, `DOMIOS_SCCACHE_S3_ACCESS_KEY`,
`DOMIOS_SCCACHE_S3_SECRET_KEY` and `DOMIOS_CI_STORE_ENDPOINT`. The cache keys are
needed only when the machine selects shared cache storage; a pinned local cache
keeps its existing behavior.

`release-inputs` keeps the normal release checks and profiles. TypeScript stores
incremental diagnostics in `node_modules/.cache`; Vite builds in a temporary
directory there and promotes successful output into `dist`. Equal output bytes
retain their timestamps, including the Linux/Windows bundled CLI and generated
CLI contract files. The CLI generator must preserve equal `commandPaths.ts`
bytes: plugin-local pnpm symlinks otherwise make its timestamp a Cargo input.
This avoids
rerunning Cargo resource embedding when nothing changed. macOS still follows
its copy/sign path. Failed type checks or Vite builds preserve the last working
frontend, and warning ratchets still block promotion.

For measurements, keep commit, toolchain, features and job count identical.
Set `CODETERM_BUILD_TIMINGS=1` on a named DEV measurement to emit timestamped
`[dev-phase]` boundaries through gateway setup, Cargo, frontend, staging,
provenance and native readiness. Markers contain fixed phase names and process
IDs; command arguments and credentials are omitted. Separate gateway time from
the later `dev-prebuilt` launch, and distinguish source-change builds from an
unchanged-source second instance.
On Linux, `/usr/bin/time -v ./build.sh release-inputs` records wall time, CPU
and peak memory for the normal pipeline. Repeat without edits for a warm run.
Distinguish empty Cargo outputs, a populated compiler cache, and unchanged
Cargo outputs; report each separately. Do not compare a partially cached old
toolchain build with a fully cold new toolchain build. Use gateway logs and
`CODETERM_BUILD_FINGERPRINTS=1` to diagnose unexpected rebuilds.

## Quick Start

```bash
git clone <repo-url> codeterm && cd codeterm
pnpm install
./build.sh dev          # development (hot-reload, foreground)
./build.sh build        # production build
```

### Isolated server launch

The isolated launch verifies a debug or release server without production paths. To diagnose manually,
create distinct existing temporary data and config directories and choose a
private port other than 7685 or 7686:

```text
Debug: domios-server start --verify-isolated --instance <explicit-lowercase-slug> --data-dir <absolute-data-dir> --config-dir <absolute-config-dir> --port <private-port>
Release: domios-server start --verify-isolated --data-dir <absolute-data-dir> --config-dir <absolute-config-dir> --port <private-port>
```

Debug verification requires an explicit valid instance slug; absent, invalid or
repeated selection refuses. The host installs it before resolving IPC or child
environments: the server and its General/PTY/chat-backend children use the same
`domios-dev-<slug>.sock` and custom API port while retaining the exact private
paths. Do not infer a slug from a directory or port. Choose an unused instance;
a socket/port collision refuses rather than attaching elsewhere. Release
verification rejects `--instance` and retains its existing PID socket behavior;
this debug repair does not establish release General CLI verification.

This mode binds `127.0.0.1`, and refuses relative paths and
paths under the user's `.codeterm*` directories. It uses the private bootstrap
pipe to start and stop the server; stdout contains one JSON line of the form
`{"type":"READY","build":"<server-build-sha>","port":<bound-port>}`.
Keep stdin open while using the server. Closing stdin on any platform or sending
SIGTERM on Unix asks the supervisor to perform the normal authenticated
shutdown. The API credential stays in `<data-dir>/.secrets` and the private
bootstrap pipe. Probe `http://127.0.0.1:<port>/api/health`. Capture the credential
with the packaged CLI's local-only `system api-token` command while setting
`CODETERM_DATA_DIR=<data-dir>`, then set it with `CODETERM_API_TOKEN`;
also set `CODETERM_DATA_DIR` to `<data-dir>` and `CODETERM_API_URL` to
`http://127.0.0.1:<port>`. For debug, use the exact launched `CODETERM_INSTANCE`
and `CODETERM_SOCKET_PATH=domios-dev-<slug>.sock`; never replace a lifecycle-owned
General child's selectors or credential with the external owner's values.
For release external-only verification, a fresh matching `CODETERM_INSTANCE`
and `CODETERM_SOCKET_PATH` (for example `verify-smoke-<random>` and
`domios-dev-verify-smoke-<random>.sock`) selects a private HTTP fallback without
scanning or connecting to another local server; it does not rename server IPC
or repair inherited General tools. Set
`CODETERM_API_PORT=<private-port>` explicitly, then run `domios identify --json` to verify
authenticated CLI access. Do not put the token in argv, the URL, or test output.

### Windows

```powershell
# Daily commands; run from the repository root.
./build.ps1 dev           # foreground dev on 7686
./build.ps1 check         # incremental compile check
./build.ps1 worktree-check # canonical integration compile check
./build.ps1 cli           # rebuild only the installed CLI
./build.ps1 install-local # incremental production build + self-install
```

PowerShell accepts the forward-slash launcher `./build.ps1 <operation>`.
`build.ps1` is the Windows command hub; do not add a Makefile or hand-expand
its underlying Cargo, pnpm, gateway, or installer commands. `install-local`
already resumes the calling tab with a default continuation intent. Keep the
command short unless the continuation itself must be customized.

## Choose one operation

The service-core Tauri reach ratchet runs in verify, release-preflight and the
file-sizes QA/CI gate. Run `bash scripts/check-tauri-reach.sh` and
`node --test scripts/tests/check-tauri-reach.test.mjs` for focused proof. Its
per-file baseline counts `tauri::`, `AppHandle` and `app_handle`; lower counts
must lower the baseline, and zero removes the entry. Growth against the branch
fork point is refused. Root host adapters remain outside the guarded core.

The provider-boundary ratchet uses the branch fork point, floored at the audited
allowlist migration `86b30145e90e9d2f31294ead517b82716fd44da8` when that commit is
on the branch and descends from the fork point. This re-anchors stale allowances
left by historical crate moves without raising any token's total budget. Newer
fork points retain their own lower budgets; unrelated histories retain their own
baseline. Never move this anchor to excuse new occurrences. Run
`node scripts/check-provider-boundary.mjs` and
`node --test scripts/tests/check-provider-boundary.test.mjs` for focused proof.

These flows are deliberately separate. Full rebuilds, dev instances, full
suites and gates run from the warm canonical checkout under Orchestrator
authority. On a shared machine Workers run no checks unless dispatch grants them;
they list the exact checks. The Orchestrator runs them in canonical after merging,
as one wave per batch of merges.

### One compile per wave

Every Cargo invocation that touches a crate pays for its compile, so a wave
compiles each crate once per profile:

1. Merge every ready lane and finish canonical edits first. Never merge or edit
   sources while a wave compiles; queue the next batch for the next wave.
2. Run writers before checkers: `./build.sh fmt`, the generators the batch needs
   (`./build.sh cli-docs` when `cli/src/args/` changed, `essence-map generate`
   when essence sources changed), then commit their output once.
3. Rust: one `./build.sh nextest workspace '<filterset>'` covering every crate
   with selectors; it compiles those crates' test targets, so it is also their
   compile check. Do not precede it with `lane-check` for the same crates.
   `lane-check <package>... --tests` covers only touched crates that have no
   selectors in this wave. `check-workspace` runs once per candidate, not per
   wave.
4. Never run one `test-one` per selector: each call rebuilds and links the
   crate's test binary.
5. `./build.sh cli` builds a release CLI (a separate profile) and installs it;
   use it to install, not to regenerate contracts. `cli-docs` builds the debug
   CLI, which shares dependency artifacts with the test build.
6. Non-Rust checks (`fmt-check`, `tsc`, changed TS/JS tests, guards) cost no
   Cargo compile; run them first so cheap failures surface before the long step.
`check-tests` compiles app and CLI test
targets without running them; its artifacts are reused by matching `test-one`
app and CLI calls in the same worktree. These app builds are deferred to the
Orchestrator's canonical wave batch.

| Goal | Entry point | Touches production? | Read first |
|---|---|---:|---|
| Develop and inspect changes | `./build.sh dev` / `./build.ps1 dev` | No; port 7686 only | `.agents/skills/dev-run/SKILL.md` |
| Compile both Rust test profiles (a full rebuild, run in canonical) | `./build.sh check-tests` / `./build.ps1 check-tests` | No; shared Cargo gateway | `.agents/skills/gates/SKILL.md` |
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

## Frontend warning collection

`pnpm build` runs TypeScript, then Vite, through `scripts/frontend-build.mjs`.
It preserves compiler failures and writes `ci-rust-warnings/frontend.log`,
`frontend-warnings.md` and `rust-warnings-<platform>-frontend.json` even when
the type check fails. Failed stages mark collection incomplete. New Vite warnings
fail the shared identity ratchet against `scripts/frontend-warnings-baseline.json`.
Its initial entries record the Vite plugin-timing and chunk-size diagnostics
observed at the recorded dispatch SHA; subsequent entries may only shrink. The summary
checks both baselines independently, keeping Rust accounting separate. Never
raise the chunk warning threshold to make a failing build pass.

For a frontend build failure, inspect that log first. Repair TypeScript errors
before rerunning the bundle; for oversized chunks, defer unused surfaces and
check their runtime loading. Focused collector proof:
`node --test scripts/tests/frontend-warnings.test.mjs scripts/tests/collect-rust-warnings.test.mjs`.

Use `node scripts/frontend-bundle-metrics.mjs dist` before and after frontend
splitting. Compare `initialBytes` (the static import closure) as well as
`entryBytes`; a smaller entry alone does not prove less startup JavaScript.

## Generated artifacts and the embedded-doc boundary

`prod-docs/` is the only documentation embedded in the binary. Its membership is the `cli/src/cmd/docs.rs` registry, and every registered name maps to a same-named file; both directions are enforced by tests. `essences/` and `.agents/skills/` remain repository sources and are not embedded.

A generated artifact is produced from a source of truth and is never hand-edited. Regenerate it in the same lane and commit it with its source change before handoff, so no consumer ever sees a stale artifact.

| Source of truth | Generated artifact | Regenerate | Verify |
|---|---|---|---|
| CLI clap definitions in `cli/src/args/` | `generated/cli/cli-manifest.json`, `generated/cli/cli.md`, and `packages/chat-ui/src/codeterm/commandPaths.ts` | `./build.sh cli-docs` (debug CLI, then `cli-manifest --write`; `./build.sh cli` writes them too but builds and installs the release CLI). Never use the installed `domios` from `PATH`. | `cli_manifest_is_fresh`, `docs_cli_md_is_fresh`, and `chat_ui_command_paths_are_fresh` |
| `essences/capabilities.md` and `essences/core.md` | `essences/map.jsonl` | `domios essence-map generate --root .` | `domios essence-map validate --root .` and `essence_map_contract_committed_artifacts_are_fresh` |
| `src-tauri/prompts/**` and their loader consts | the `.agents/skills/prompts/inventory.md` inventory | edit the inventory row by hand | `bash scripts/check-prompts-doc.sh` |
| Registered public routes in `src-tauri/src/api/routes.rs` | `prod-docs/API.md` | edit the API index by hand | `bash scripts/check-api-docs.sh` |
| `prod-docs/*.md` and `cli/src/cmd/docs.rs` | the embedded `domios docs` set | update the source and registry; there is no separate document generator | `registered_documents_have_one_title_and_fit_the_reading_boundary` and `domios docs <NAME>` from the built checkout |

A lane Worker runs isolated checks only. When a generator needs the checkout's built CLI (`essence-map` also refuses a linked worktree), list the exact regeneration and verification commands under `GATES REQUESTED` for the Orchestrator's canonical wave batch.

## Build timing and warm reuse

`./build.sh build-history [--last N] [--verb V] [--json]` (Windows:
`./build.ps1 build-history`) reads the existing machine gateway job records,
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
resets and gateway-owned server restarts yield an unavailable delta. Rust-only
hits, misses and hit rate are retained alongside aggregate counters so native
compiler hits cannot be mistaken for Rust reuse. These are
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
`./build.ps1 build-history --last 5 --json` on BLACKSTAR.

Standalone `release-inputs` records Cargo HTML timings for CLI and app/embed
preparation under the operation's target `cargo-timings/` directory. App
preparation enables `tauri/custom-protocol`, matching Tauri packaging so its
dependency graph can be reused. Packaging entrypoints compile only the CLI
during preparation and let Tauri own the app compile with the final config.

If packaging recompiles native dependencies, Tauri and the app after preparation,
compare `release/.fingerprint/*/run-*.json` environment records. Tauri CLI sets
`MACOSX_DEPLOYMENT_TARGET` from `bundle.macOS.minimumSystemVersion`; a transition
from unset to that value invalidates native build scripts and their dependents.
`prepare_release_bundle_inputs` exports the same config value before either Cargo
build. Keep this environment shared through packaging; do not hardcode a second
minimum version or change the tuned profiles to hide the rebuild.

For inner-loop build measurements, use `./build.sh dev-link-measure
[app|cli-release|cli-dev] --wait-build` in a fresh Team build shell with
`--timeout 10m`. The default app scope uses DEV's configured profile and
features, prepares resource placeholders, and builds only its binary with
`--timings`. CLI scopes select the existing release or dev profile. This
operation installs nothing and starts or stops no application; it measures
compilation rather than relaunch readiness. Keep the target and intermediate
directories isolated from running DEV.

On Windows, set `CODETERM_LINK_TIMING=1` for linker `/TIME` output and rustc
link logging. The recipe excludes `LINK` from MSYS environment path conversion;
otherwise `/TIME` becomes a Git directory path and linking fails with
`LNK1181: cannot open input file C:\Program.obj`.

If Cargo succeeds but build.sh later reports an unexpected `;;`, an edited
driver may have grown beyond Bash's original read position. The driver now
exits or returns inside its already parsed invocation block, preventing a
second parse of the changed tail. Keep scripts fixed during measurements;
repair the driver and repeat the affected case, preserving the failed log.

Record gateway queue time separately from wall and Cargo times. HTML timings
separate frontend and codegen where Cargo receives those events; a binary or
multi-output library's duration is not a pure linker measurement. Fresh shells
avoid carrying a previous measurement shell's deadline into a queued run.
`lane-check <package> --tests --timings --wait-build` is the serialized lane
measurement path; ordinary lane-check retains its existing Cargo locking.

The repository selects `rust-lld.exe` for `x86_64-pc-windows-msvc`; Rust ships
this linker, while build.sh still loads the MSVC/SDK libraries. A linker change
invalidates Cargo fingerprints once. Measure warm edits after that seed;
compare actual `/TIME` output, not the seed's full rebuild against a warm run.
Cargo can replay old rustc stderr for fresh artifacts, so count link output
only when the timing report also shows that target compiled in this run.
Canonical DEV and check-queue retain incremental compilation.

DEV builds its CLI with the existing dev profile and passes the exact artifact
to the resource bundler. If DEV reports a missing release CLI after a profile
change, update the artifact selection rather than rebuilding release as a
workaround. Production/standalone CLI builds still select release. The CLI
has no service-core/API dependency; daemon edits alone do not rebuild that
release chain, while Git HEAD changes refresh the CLI build stamp.

CLI builds rely on `cli/build.rs` watching Git HEAD and its branch reference;
do not touch `cli/src/main.rs` to force a rebuild. Repeated builds at the same
SHA reuse Cargo artifacts, while commits refresh the embedded CLI SHA.

With sccache, explicitly setting `CARGO_INCREMENTAL=1` is rejected by the cache
client. Leave it unset to use Cargo's incremental default; use `0` only for
operations whose measured policy disables incremental. Do not turn off DEV
incremental compilation merely to clear that diagnostic.

Lane cache troubleshooting: `sccache: Compiler not supported` during `rustc -vV`
can mean Cargo passed the workspace wrapper to sccache as the compiler. Lane
wrappers now forward each other and invoke the cache only immediately before
rustc. Cargo still hashes the per-worktree workspace-wrapper path, preserving
metadata isolation; do not remove that identity to chase workspace cache hits.
Ordinary lane checks use an already live sccache server when available and
respect `CODETERM_USE_SCCACHE=0`; they never start or restart the cache server.
Run `./build.sh lane-wrapper-check --wait-build` in a fresh Team build shell
for the small two-worktree metadata fixture and native forwarding tests.

## Shared compile cache


Release-input preparation now calls the same cache policy before invoking Cargo.
`ci-tests full|platform` owns CI test compilation and execution through that
policy (full suite, or this OS's cfg-gated subset plus the native input crates); on
Windows use `./build.ps1 ci-tests <mode>`, which locates Git Bash explicitly without
requiring it on PATH. Native release jobs export `RUSTC_WRAPPER=sccache` at job scope so the
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
`domios mem secret sync`. The two vault entries are
`domios-sccache-s3-access-key` and `domios-sccache-s3-secret-key`.
Build setup resolves both directly into process environment variables
`AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`; never write them to a file,
argv, logs or memory. It exports `SCCACHE_ENDPOINT` from the single store endpoint in
`scripts/ci-store.sh` (override: `CODETERM_CI_STORE_ENDPOINT`),
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

`./build.sh cache-status` (Windows: `./build.ps1 cache-status`) resolves the
configured location, reads the running server's actual `Cache location`, and
checks the shared endpoint's health URL. It starts no build or cache server,
and changes no hooks or compiler environment. Configured and actual locations
can differ until the next idle setup restarts the server.

CI uses `./build.sh cache-verify` (Windows: `./build.ps1 cache-verify`). It prints
configured and actual storage plus endpoint health, exits 0 only when both storage
checks select the shared S3 bucket, and otherwise exits 2. It never restarts a
server or checks for an idle machine. When no server is live and both S3 keys
resolve, it starts the shared server with `SCCACHE_IDLE_TIMEOUT=0`, which disables
idle shutdown. Cache counters belong to the live server process and reset when that
process restarts; gateway job deltas remain in the per-job records. Set
`CODETERM_SCCACHE_VERIFY_ONLY=1` in each CI build process so build setup follows
that same verification path. Provision local-to-S3 storage separately while idle.

CI never reads the vault. Every workflow that builds Rust or verifies the cache
sets workflow-level `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` from the repo
secrets `SCCACHE_AWS_ACCESS_KEY_ID` and `SCCACHE_AWS_SECRET_ACCESS_KEY`, plus
`CODETERM_SCCACHE_CREDENTIAL_SOURCE=runner`, so cache-policy takes the keys from
the job environment on every runner, including the BLACKSTAR WSL runners that
have no `domios` CLI. Reusable workflows receive them through `secrets: inherit`;
containerized builds forward the names with bare `-e NAME`. Values stay in process
environment, never runner `.env`, command arguments, files or logs; both are
required. The vault entries above remain the source on dev machines and peers.
Select pinned Rust 1.99.1 only in the build process, preserving the machine's
default toolchain.

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
| Local fallback line on an intended builder | One or both vault entries could not resolve | Run `domios mem secret sync` locally, inspect secret backend status, then rerun with the machine idle. Never print the values. |
| Lane check fails in dependency compilation with `sccache: ... os error 10048` | The cache path failed to acquire a Windows socket; this receipt does not establish a Rust source error | Preserve the failed receipt and use the documented process-local `CODETERM_USE_SCCACHE=0` for one bounded lane check. Do not restart the shared cache server or raise the deadline. |
| Shared server startup fails | Endpoint unreachable, credentials rejected, or installed sccache lacks S3 support | Verify Tailscale connectivity and tool features; inspect metadata and server health without secret logging. Preserve the failed receipt. |
| Windows build reports MSVC not found despite an installed VS 2022 C++ workload | The VC tools are installed outside the default BuildTools directory | Use `setup_env` discovery through the installed `vswhere` utility; the default tools remain first choice, and discovered host/cross compiler paths also enter process PATH. Verify `source build.sh env` and `bash scripts/check-msvc-discovery.sh`; install nothing or change no global PATH for a relocated installation. |
| Windows PowerShell refuses `build.ps1` because scripts are disabled | The shell process uses a restrictive execution policy | Invoke the authorized build through `powershell.exe -NoProfile -ExecutionPolicy Bypass -File build.ps1 <operation>`; this changes only that process, never machine policy. |
| Gateway policy fixture expects `CARGO_TEST_BUILD_CMD="cargo nextest run"` | The guard predates nextest inventory compilation and metadata reuse | Check `cargo nextest list` and the reuse execution command separately; rerun `bash scripts/check-build-gateway-policy.sh`. |
| Gateway policy exits after signing fixtures without starting the serial catalogue | Its DEV ordering assertion still searches for `kill_dev` after startup adopted `stop_dev_before_replacement`. | Check the current stop helper precedes gateway acquisition; preserve ordering and use the failure line and retained fixture logs. Run `bash scripts/check-build-gateway-policy.sh`, then the serial catalogue. |
| Same SHA has few cross-machine Rust hits | Physical checkout, registry, toolchain, target or build environment differs | Compare the path contract and `rustc -vV`; do not enable BASEDIRS to conceal Rust path differences. |

The gateway mutex, four-client queue, metadata/readers, cache-start, owner-reap and failed-Cargo regressions, the four `check-build-gateway-*.sh` guards (serialization, policy with the release-signing and source-clean checks, mutex cleanup, owner reap) run through `./build.sh script-guards`, which `./build.sh release-preflight` runs; no PR workflow runs them. This also runs `scripts/check-crate-layering.sh` and its fixture tests: lower crates cannot import service-core or Tauri, and the process layers in `scripts/crate-layers.json` hold: the native client (`codeterm`, `codeterm-client-shell`) links only client and shared crates, shared crates never reach a server crate, and nothing else depends on the client. Release-preflight and the file-sizes CI workflow run the same guard. Add a crate the window needs to `shared` only when its own normal dependencies are shared; otherwise move the needed item into a shared crate. Developer harnesses (`mem-bench`, `parser-eval`, `mem_ab_bench`), the `render_timeline` example and cross-domain integration tests live in `src-tauri/tools/codeterm-devtools`; build a harness with `cargo run -p codeterm-devtools --features dev-tools --bin <name>`.

## Canonical integration check queue

The Orchestrator runs `./build.sh check-queue [<sha>] [--max-errors N] [--max-warnings N] [--touched-only] [--all] [--lane]` as the canonical integration/Clippy gate for a committed lane SHA. It takes the machine gateway slot (fail-fast, exit 75 when busy, `--wait-build` only when blocking is intended), verifies and publishes the SHA in the single per-machine check worktree `<gateway dir>/check-worktree`, and runs `cargo clippy --lib --bins --no-deps --message-format=short -- <scripts/clippy-allow.sh>` with the worktree's own `target/app` and `target/cli`. Product Clippy retains default features. Full checks additionally type-check service-core tests with `--profile test --tests --no-default-features`; there is no duplicate dev-lib check. Packages come from `git diff <merge-base with develop> <sha>`: `src-tauri/crates/<name>` maps to `-p <name>`, `src-tauri/src` to `codeterm`, `cli/` to the CLI manifest; a crate change also checks `codeterm` unless `--touched-only`. Output is the first N `file:line:col: error[...]` lines, an `errors=N shown=M` total (a nonzero Cargo exit with no parsed diagnostic contributes one failure), the first `--max-warnings` (default 40) unique warning lines, `warnings crate=<name> count=N` per warned crate, `warnings=N shown=M failing=<crates|none>`, and the full log path. Any rustc or clippy warning in a touched crate fails the run (exit 1); `--all` checks the whole app workspace plus the CLI and fails on any warning. `./build.sh check` prints the same per-crate warning lines and a `check: warnings=N` total. Crates deny `warnings` through Cargo `[lints]`, so check-queue warnings normally surface as errors. It never builds in canonical or a lane target. The first run on a machine is cold; subsequent runs are incremental.

Troubleshooting:
- Symptom `sccache: incremental compilation is prohibited` from `rustc -vV`: cause an exported `CARGO_INCREMENTAL`; recovery is to leave it unset (the verb unsets it) and let the profile decide.

## Peer build gateways (mesh build farm)

Mesh peers run the same verbs in their own gateway, with their own warm cache and stable build path, so several machines check lanes in parallel instead of queueing on one gateway. Peers and their fetch-only checkouts are listed in `.codeterm/build-machine/peers.json`; a peer marked `"routable": false` (eight, the release builder) is refused until its owner frees it, and `CODETERM_PEERS_ENABLE=eight` enables it for one call. All remote work goes through `domios tab new --mesh <peer> --cmd ...`; never ssh by hand, never put a credential in a command, and never touch a machine's runner pause.

| Verb | What happens |
|---|---|
| `./build.sh peer-setup <peer>` | One bounded build tab on the peer clones `domios-dev/domios` into the configured checkout if missing, sets the push URL to `fetch-only.invalid`, detaches at the pushed `origin/develop` (or `--sha REF`, which must be pushed and contain `scripts/build-peer.mjs`), runs `./build.sh peer-toolchain` (rustc must equal `rust-toolchain.toml`, plus MSVC on Windows, Xcode CLT on macOS, `cc` on Linux, node, cargo-nextest, sccache) and opens the peer's `Buildpeer` status shell. `--install-toolchain` first runs `peer-toolchain --install`: a user-scope rustup (`rustup-init -y --no-modify-path --profile minimal`, the pinned toolchain with clippy and rustfmt) and the prebuilt cargo-nextest into `~/.cargo/bin`; no machine PATH or env change. |
| `./build.sh check-queue <sha> --peer <name\|auto> [--lane] [--os OS]` | The peer fetches `develop` and the SHA, detaches its checkout at the SHA and runs `./build.sh check-queue <sha> [flags] --wait-build` in its gateway. `--lane`, `--all`, `--touched-only`, `--max-errors` and `--max-warnings` pass through unchanged. |
| `./build.sh nextest <scope> [filterset] --peer <name\|auto> [--sha SHA]` | Same, running `./build.sh nextest <scope> [filterset] --wait-build` at the SHA (default HEAD). |
| `./build.sh check-linux [sha] [--force]` | Once per batch: `./build.sh check-workspace <sha>` (`cargo check --workspace --all-targets` for the app workspace and the CLI, any warning fails) on the idle Linux peer, so `cfg(unix)` paths compile. A second call for the same SHA prints the existing receipt; `--force` dispatches again. |
| `./build.sh check-series --spec FILE --peer <name>` | Build-speed measurement in one serial peer job. The spec JSON has `marker`, `probe0`, `probe1`, optional `patches: [{file, find, replace}]` and `bases: [{label, sha, areas: [{name, file}]}]`, and every base must be pushed. Each patch is an identical prerequisite committed on every base first; `find` must occur exactly once (for example, a unix compile fix the bases predate). For each area the runner builds never-pushed scratch commits with git plumbing on a private index, so the peer checkout stays at the runner SHA and one `build.sh` drives every base. The commits are a seed (probe0 appended), an edit (probe0 changed to probe1) and a revert, which must restore the base tree. The runner then runs `check-queue <seed>` twice (warm, then the no-op floor) and `check-queue <edit>` once. It stops at the first failed, busy, deadline or `no Rust changes` run. The event has one line per area: wall time with cargo `Finished` seconds for each run, exit codes and gateway job ids, plus other gateway jobs in the window. Full receipts are in `<checkout>/.git/codeterm-peer-jobs/<id>.series.json`. The scratch commits have no refs. |
| `./build.sh test-one <scope> <name> --peer <name>` and `--prereq DIFF` on any peer verb | `test-one` runs the same focused test in the peer's gateway. `--prereq DIFF` ships a unified diff (base64 in the job command) that the peer applies with `git apply --index` and commits locally on top of the job SHA before the runner starts. Use it for an identical prerequisite such as a unix compile fix the SHA predates. The commit is never pushed, the next job's checkout discards it, and check-queue then checks `HEAD`. The dispatcher prints and records the diff's name and sha256. A diff that does not apply reports `outcome=bootstrap_failed stage=prereq` and leaves the checkout clean at the SHA. |
| `./build.sh peer-route [--os OS]` | Shows what `--peer auto` would choose. |

Dispatch returns at once with `peer-job: id=... peer=... remote-tab=... deadline=...` and writes a record under `<gateway dir>/peer-jobs/`. The SHA must be on a remote-tracking branch (push first). Completion is one message to the calling agent tab (`CODETERM_TAB_ID`, or `--reply-tab`) through `domios send --require-agent --mesh <origin instance id>`; nothing polls. The message starts `[peer-job <id>] peer=<peer> verb=<verb> sha=<sha> outcome=passed|failed|busy|deadline exit=N wall=Ns`, then the same summary lines the local verb prints (`errors=`, `warnings=... failing=`, `app-test ...` receipts, `check-queue: sha=...`, nextest `Summary [...] N tests run: ...`, `check-workspace: ...`), up to 12 error or failing-test lines, and `remote-log=<peer path> gateway-log=<peer gateway log>`. Bootstrap failures report `outcome=not_setup|busy|bootstrap_failed stage=<step>` instead.

`--peer auto` reads each OS-matching routable peer's load over mesh: `domios tab list --mesh <peer>` finds its `Buildpeer` tab and `domios tab exec --mesh <peer>` runs `./build.sh peer-status`, which first reaps a dead gateway owner or queue ticket exactly as `gateway-status` does (so a cancelled CI job's orphaned lock never reads as busy), then prints one `PEER_STATUS {json}` line (gateway state, active job, queue length, last job end, peer job lock). The least-loaded idle peer wins, ties going to the peer idle longest; a peer that is unreachable, not set up or slower than the 20 s probe deadline is skipped, and when no peer is idle the dispatch exits 75 with each peer's state.

Bounds: a mesh tab cannot carry `--timeout` (only a local owning agent can), so each job tab is `--eager` and stays open after the job for inspection (close finished ones with `domios tab close --mesh <peer>`); the verb deadline (check-queue 45m, nextest 30m, check-linux 45m, setup 20m; override with `--deadline 90m`) is enforced by the peer runner, which kills the build tree at the deadline and still reports `outcome=deadline`; clone and fetch abort below 1 KB/s for 60 s; one job holds the checkout's `.git/codeterm-peer-job.lock` (a lock older than 4 h is stale) and a second job reports `busy`. Peer logs live in `<checkout>/.git/codeterm-peer-jobs/<id>.log`. Each dispatch creates a new tab for its own job identity; `tab restart` renews a shell's previous lifetime.

Peer `path` (in `peers.json`): mesh shells are non-login, so a peer whose node or cargo is outside the default PATH lists those directories (ubunta: `$HOME/.local/node22/bin`); the job command and the status probe export them first, and a job without node reports `outcome=bootstrap_failed stage=node_missing`.

Measurements: ROG (Windows) via one `check-series` on develop 7fc9cf309, area root crate `src-tauri/src/lib.rs` (`-p codeterm`, the full app dependency graph), job peer-20261016T003924Z-52b3. The first check in a never-compiled gateway check worktree (cold populate, sccache on the shared S3 cache) took 177 s: cargo `Finished` 110 s for the dev profile plus 64 s for the test profile. A no-op recheck (floor) took 4 s. A one-line edit in the root crate (warm lane check) took 11 s, 9.5 s of it cargo. eight (macOS) warm `--lane -p codeterm-sentinel` measured 32 s, and ubunta (Linux) `check-linux` workspace checks 73-94 s.

Peer toolchain records: ROG (user amigo) had toolchains in `~/.rustup` but no proxies in `~/.cargo/bin`; `peer-setup ROG --install-toolchain` added rustup 1.29.1 proxies and cargo-nextest 0.9.148 there (default toolchain 1.95.0) and changed nothing under `C:\domios-ci` or the machine PATH. To remove it, delete those exes except `sccache.exe`, never `rustup self uninstall`, which deletes the `~/.rustup` other runs use. Never run `--install-toolchain` while another run uses that peer's toolchain.

Troubleshooting:
- No completion message arrives and the remote tab is gone: the shell exited before the runner started (for example a Windows peer without the `git-bash` profile). Check `domios tab list --mesh <peer>` and the peer's `peers.json` `shell`.
- `outcome=not_setup stage=checkout_dir`: run `./build.sh peer-setup <peer>` first.
- `outcome=bootstrap_failed stage=runner_missing`: the SHA predates the peer runner; dispatch a SHA that contains `scripts/build-peer.mjs`.
- `outcome=bootstrap_failed stage=checkout`: the peer checkout has local changes; it is fetch-only and must stay clean.
- `peer routing: busy; X=unknown(not_setup)`: the peer has no `Buildpeer` tab (a peer reboot closes it); rerun `peer-setup`.
- Windows peer, `outcome=busy exit=75` with `check-worktree git worktree failed (128): ... Filename too long` in the gateway log: provider fixture paths exceed MAX_PATH. `peer-setup` sets repo-local `core.longpaths=true`, which the gateway check worktree inherits; rerun it.
- `Slow unit ratchet FAIL: added baseline: ...` on a peer nextest of a lane behind develop: the ratchet compares the checkout's baselines against `origin/develop`, so baselines develop has since dropped read as added. The tests themselves passed (see the `tests:` line); merge develop into the lane to clear it.
- `resource path ../target/domios-package/LICENSE doesn't exist` during app checks or CI Clippy: the Tauri pair resource map needs compile-only stubs. `prepare_bundle_stubs` creates marked missing resources before non-package compiles; the release stage replaces them, while ordinary packaging refuses a remaining marker. Run the focused stub fixture, then verify Clippy from a clean checkout on an eligible remote runner.
- A job's runner comes from the dispatched SHA, so a lane SHA older than a runner fix behaves like the old runner (no `tests:` summary line in the event, lock released after the event, which makes the next dispatch report `busy` at `peer_lock`).

## Cache retention and troubleshooting

Gateway acquisition triggers retention at most once per hour; there are no timers or background sweep jobs. `cache-retention.json` in the machine gateway records the last completed run. Exceptional targets carry `.codeterm-cache.json` with their physical checkout path, last-use timestamp, measured byte count, and live build-owner PIDs. Preparation refreshes the marker while its gateway job owns the build slot.

Missing-checkout targets and targets unused for `CODETERM_CACHE_MAX_AGE_DAYS` (default **3**) are removed first. Unmarked targets older than **2 days** are eligible. Remaining idle targets are evicted in least-recently-used order until they fit `CODETERM_EXCEPTIONAL_CACHE_GB` (default **40 GiB**). The current checkout and targets with live build owners are protected, including during manual GC; protection can temporarily exceed the cap and is reported. PID reuse conservatively protects a cache rather than risking removal of an active build.

At the same hook, installed `cargo-sweep` prunes the current checkout's `src-tauri/target` and `cli/target` with the same configurable age. An unavailable sweep tool prints one installation hint; sweep failures do not fail a build. `SCCACHE_CACHE_SIZE` is explicitly exported with default **10G** before starting sccache. An already-running server retains its startup configuration: apply changed sccache limits only by restarting it with the gateway idle, never during compilation.

Run `./build.sh gc --dry-run` to preview or `./build.sh gc` to reclaim immediately (Windows: `./build.ps1 gc --dry-run` / `./build.ps1 gc`). Manual GC acquires the machine gateway, bypasses the hourly throttle, and reports exceptional bytes reclaimed plus sweep output. Dry-run preserves artifacts and the hourly timestamp. Normal targets have an age policy; current artifacts and protected exceptional targets can exceed their usual retained size.

Check-queue worktrees live in `check-worktrees/<common-gitdir-hash>/check-worktree`
inside the gateway. Linked lanes share their repository's checkout; independent
clones use different checkouts. The old top-level `check-worktree` is untouched.
Manual GC removes registered broken/pending leftovers belonging to the calling
repository, plus unregistered directories without Git metadata in its hashed
namespace. Foreign, unknown top-level and interrupted-publication paths remain.
Prove changes with `CT_BUILD_GATEWAY_SCOPE=fixture` and
`CT_BUILD_GATEWAY_FIXTURE_DIR=<private-directory>`; never repair another clone's
live checkout. Focused regression: `node --test scripts/check-worktree.test.mjs`.

Retention also checks exact lane `target`, `src-tauri/target` and `cli/target`
directories under the provisioned worktree root. It trims them only when all
artifact timestamps exceed `CODETERM_CACHE_MAX_AGE_DAYS`, the index tracks no
content there, and the lane is not the calling checkout. Linked lanes and target
roots are refused; nested output links are unlinked without entering their
targets. Preview and safety fixtures: `node --test scripts/lane-target-gc.test.mjs`.

Orphan build-directory GC runs in manual GC and in the hourly automatic retention.
It maps every existing Git worktree of this repository and the sibling
`codeterm-plugins` repository, plus the gateway's stable `X:` checkout, through
`cargo metadata --format-version 1 --offline --no-deps`. Cargo 1.95 exposes
`build_directory`; no hash approximation is used. Set `CODETERM_PLUGINS_CHECKOUT`
when the plugin repository is elsewhere. Missing repositories or failed metadata
refuse deletion. Each workspace mapping is printed. The configured two-level
`{workspace-path-hash}` cache roots are discovered from these paths, rather than
hardcoding BLACKSTAR's RustCaches location.

`X:` exists only inside a gateway job. Whenever GC sees it mapped it records the
gateway build directory in `stable-build-dirs.json` in the machine gateway dir;
when `X:` is unmapped (for example during `check-queue`) that record protects it,
and with neither the mapping nor a record GC refuses. Automatic retention runs
after the job's stable mapping so the record stays fresh. A worktree created or
removed mid-run triggers a remap; a directory that became live is skipped.
Automatic orphan-GC refusals are logged and the build continues.

**Protection rule.** Never delete the build directory of the canonical checkout,
of the gateway (`X:`), or of any live checkout, regardless of idle time: cold
rebuilds of those stall every agent on the machine. Only unmapped build
directories (whose checkout no longer exists) are removed. Directory mtime is
not a usage signal: Cargo writes deep inside the hash directory, so a live
directory can look idle for days. `CODETERM_CACHE_MAX_AGE_DAYS` governs
exceptional targets and cargo-sweep, not deletion of live build directories.
`domios plan lanes --clean` removes worktrees upstream: their build directories
become orphans on the next hourly retention.

GC takes nonblocking exclusive locks on every `.cargo-lock` below each orphan and
holds them through removal; a busy or unreadable lock protects the entire directory.
The lock protocol follows Cargo 1.95's
[layout](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/compiler/layout.rs)
and [flock](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/util/flock.rs):
Windows `LockFileEx`, Unix `flock`. Python 3.12+ is required on Windows for junction
validation. Links and unexpected hash layouts refuse collection. Each orphan and
the total are reported in GiB. Fixtures: `node --test scripts/cargo-build-gc.test.mjs`.
For an explicitly blocking cleanup use `./build.ps1 gc --wait-build` in a Team
build shell; capture `Get-PSDrive C` before and after, and preview first.

If GC exits 2 on a live lane's malformed Cargo manifest, no orphan deletion is
allowed: its build directory cannot yet be mapped. Ask that lane's owner to fix
the manifest, then rerun the preview. Never skip the checkout or guess its hash.

Every candidate is physically resolved using `pwd -P` and must be a direct child of the exceptional target root, or the checkout's exact target directory for sweep. Empty, root, parent, symlink/junction, redirected ancestor, and nested-link paths are refused. Deletion does not follow links. If retention reports an unsafe path, inspect and remove the redirection separately; never relax the boundary to make GC pass. Retention and marker updates use an exclusive gateway metadata lock and refuse concurrent modification. Automatic failures print a hint and let builds continue; explicit GC returns failure for invalid configuration or an unavailable lock.

Canonical shell policy verification: `bash scripts/check-build-gateway-policy.sh`. Its retention fixture suite checks marker persistence, removed checkouts, configurable expiry, legacy grace, LRU cap order, live-owner/current-checkout protection, hourly throttle, dry-run, and unsafe paths including real Windows junctions. It requires no Rust compilation.

## Local production build and self-install

Use this only when testing a production-mode build on this machine. It does
not publish a release or alter GitHub assets. Steps 1-4 describe Windows; step
5 covers macOS.

A build handed to the owner to install (an owner hotfix build such as
`X.Y.Z-bs.N`) first needs a green core-journeys run on its exact SHA; the
gates skill's Release section owns that step.

1. Start from the intended, clean release branch in the canonical checkout.
2. After a version bump, run the idempotent preflight. It refreshes the Cargo
   lockfile (the CLI is a workspace member) and verifies it matches `package.json`:

   ```powershell
   ./build.ps1 release-preflight
   git add src-tauri/Cargo.lock
   git commit -m "release: sync Cargo lock versions"
   git status --short
   ```

3. For repeated local production testing, build and self-install through the
   persistent canonical cache:

   ```powershell
   ./build.ps1 install-local
   Invoke-RestMethod http://127.0.0.1:7685/api/health
   ```

   From an agent tab, launch it with a receipt (PowerShell and bash lines:
   [utility-shell-reports](../utility-shell-reports/SKILL.md#launcher-recipes-copy-paste)):
   `domios tab new --team <team-id> --purpose build --timeout 10m --cwd "<checkout>" --cmd "./build.ps1 install-local -NotifyTab $env:CODETERM_TAB_ID"`.
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
   ./build.ps1 install-local --continue-prompt "Verify the installed build, then resume release QA."
   ./build.ps1 install-local --continue-tab "$env:CODETERM_TAB_ID" --continue-prompt "Continue the interrupted task."
   ./build.ps1 install-local --no-continue
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
   ./build.ps1 build
   domios update install --from-file "<printed-installer-path>"
   ```

   `build` always creates a fresh nonincremental target under
   `targets\release\<job>\` under the gateway cache home
   (`CODETERM_RUST_CACHE_ROOT`, else `%LOCALAPPDATA%\domios-build-gateway`). It remains
   intentionally slower and isolated. The updater helper stops only the
   current-user installed Domios, runs NSIS, and restarts it.

   `build` packages through `bundle_common_stage`, the same shared stage the
   release workflows use (`CODETERM_RELEASE_COMMON_STAGE=1`): one native client
   compile, `release-stage.mjs stage-full` with the signed CLI and server, then
   NSIS on Windows, deb and AppImage on Linux, or the signed app and DMG from
   `release-macos-bundle.mjs` on macOS. It prints each `Local package ready:`
   path. Focused proof: `node --test scripts/tests/package-stage.test.mjs`.

5. macOS uses the same route, from the canonical checkout on the Mac:

   ```bash
   ./build.sh build                 # or: ./build.sh build --no-notarize
   domios update install --from-file "<printed-dmg-path>"
   ```

   Signing uses the login keychain's single Developer ID Application identity
   and its team; `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID` or `RELEASE_KEYCHAIN`
   override it. Notarization uses the notarytool keychain profile `codeterm`
   (`APPLE_KEYCHAIN_PROFILE` overrides) and is checked before compiling; a
   missing profile refuses the build unless `--no-notarize` is passed. No CI job
   keychain or updater key is needed: the local app carries no updater bundle.

### Build gateway location (Windows)

Gateway state lives in `%LOCALAPPDATA%\domios-build-gateway`, outside every
per-user install folder. While a pre-existing `%LOCALAPPDATA%\CodeTerm\build-gateway`
remains, gateway commands keep sharing it with older checkouts; each gateway
acquisition tries to move it by one same-volume rename under its metadata mutex
when no slot or queue ticket is held. A busy or held directory prints a notice
and stays in use; nothing is ever copied or merged.

`./build.sh gateway-relocate` performs the move on demand. It refuses while
Cargo or rustc runs, stops the idle sccache server (its log is held open inside
the directory), then exits 0 when moved or nothing remains, 75 while busy, and 3
when both directories exist. Exit 3 means an older checkout recreated the legacy
directory: delete it once no older checkout builds.

### Repairing a corrupted user PATH (Windows)

Domios adds only its own `bin/` directory to `HKCU\Environment` `Path`, and
now refuses any directory under the system temp directory. Machines that ran the
Windows CLI-install tests before that refusal existed accumulated dead
`<temp>\.tmp*\.codeterm\bin` entries, which count against the 32,767-character
user-PATH cap.

The repair is never automatic. Run it yourself, once:

```powershell
$exe = @(
  "$env:LOCALAPPDATA\Domios\domios-app.exe",
  "$env:ProgramFiles\Domios\domios-app.exe"
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

#### Dependencies disappear during lane installs

An empty package directory or missing `.modules.yaml` indicates directory
deletion, not merely a removed shared-store hardlink. On Windows, inspect lane
`node_modules` with `Get-Item -Force` and its `Target`, and compare UTC directory
timestamps with the failed build. `fsutil hardlink list <package-file>` identifies
shared store inodes; unlinking one name does not remove the others.

pnpm can purge the target of a `node_modules` junction when recreating an
incompatible install, including during an offline install. Never junction or
symlink canonical dependencies into a lane. The repository `.pnpmfile.cjs`
refuses linked dependency directories during configuration, before pnpm's purge.
It preserves pnpm's package import method and shared-store reuse.
Do not bypass it with `--ignore-pnpmfile`. Normal package links inside
`node_modules` remain supported.

Stop the operation using the junction. Unlink only the junction, without
recursion (`[IO.Directory]::Delete(<junction>, $false)` on Windows), then install
inside that checkout with `pnpm install --frozen-lockfile`. Preserve evidence
before repairing a damaged dependency directory, and prove it is not a link.
Canonical repair belongs to its integrator. A failed network install in an isolated real
directory does not explain missing pathnames in canonical.

Frontend staging creates and removes only its unique
`node_modules/.cache/codeterm-frontend-*` directory; that cleanup is not a
package-store prune. Verify isolation and the destructive unguarded mechanism in
throwaway directories with
`node --test scripts/tests/pnpm-isolation.test.mjs scripts/tests/lane-cleanup.test.mjs`.
The cleanup test exercises Windows PowerShell; run the pnpm tests on each OS.
After editing `.pnpmfile.cjs`, regenerate its lockfile checksum with
`pnpm install --lockfile-only --ignore-scripts`, then verify frozen installation.

The app's non-daemon CLI shim delegates to an installed CLI before daemon
initialization. Smoke the produced bundled CLI directly as well.

| Symptom | Cause | Recovery |
|---|---|---|
| DEV rebuild refuses with `DEV process tree owns another listener` while Browser or a provider server is running | The stop guard treated owned descendant listeners as foreign ports. | Use the stop guard that accepts verified descendants and revalidates captured PID/port pairs; keep production, nested instances and foreign API owners protected. Verify with `node --test scripts/tests/dev-rebuild-stop.test.mjs`. |
| A broad gate or its gateway metadata write fails with `ENOSPC` | Repeated compiler/profile experiments retain superseded incremental graphs on a small cloud disk | Stop compilation, inspect `df` and target sizes, and remove only inactive session-owned caches after preserving final outputs. Keep at least 6 GiB free before a full build. Run `gateway-status` to reap a finished failed owner; never delete an unknown state lock. Then rerun the interrupted gate. |
| A Rust pin upgrade reports `double_must_use` from `async_trait` | Old macro expansions add a redundant attribute to boxed futures | Keep the workspace lockfile on `async-trait` 0.1.92 or newer and run `./build.sh ci-clippy`; do not suppress application warnings. |
| An unchanged CLI recompiles on every invocation | A `rerun-if-changed` input names an absent `packed-refs` file | Watch existing Git metadata; for packed branch refs watch the nearest existing ref parent so loose-ref creation remains visible. Preserve SHA-triggered rebuilds. |
| Release executable ignores `CODETERM_INSTANCE` and still selects prod data | `codeterm-paths::instance_suffix` and `dir_name` select named instances only with debug assertions; release always uses `~/.domios`. A separate port does not isolate its databases. | Never launch a release daemon/GUI for testing. Validate a fresh `build` bundle and each binary's non-daemon `--version`/`--help` entry; full launch proof belongs to the owner-approved `install-local`, with the next Windows release workflow as an independent build proof. |
| Fresh release build ends as `owner_lost` after its Team shell deadline | The utility owner exited before the command result; a rustc child of the detached sccache server can outlive that owner. This is not a compiler diagnostic. | Preserve the job log and partial target, inspect compiler command lines for the exact owned scratch root, and terminate only those orphaned compilers. Obtain a new bounded build grant before retrying; retain intermediates, keep the fresh final target, and require the completed bundle before claiming success. |
| Lane dispatch fixture exits 127 at `configure_product_cargo` | Its extracted dispatch fragment omitted the real product feature helper. | Load the helper from build.sh and assert selected features with `node --test scripts/check-queue-plan.test.mjs`. |
| Lock-storm fixture reports a null child exit with empty output on Linux | The child never launched: Linux runners have `python3`, not bare `python`. | Select `python3` on Unix and `python` on Windows; assert launch errors and report signals before diagnosing a runtime crash. Run `node --test scripts/tests/qa-lock-storm.test.mjs`. |
| `install-local` fails in seconds with rustfmt `Diff in ...` | The cheap gates run before the gateway | Run `./build.sh fmt` once after the edit batch, commit, rerun; with `--notify-tab` the receipt names the step and log. |
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
| The CLI docs test reports `API exceeds 24 KiB` | The API index repeats adoption details already owned by `API-ADOPT.md` | Keep `API.md` to the endpoint summary and move the full contract to `API-ADOPT.md`; bump the production doc revision and run `build.ps1 test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary`. Never raise the 24 KiB limit. |
| The CLI docs test reports `API-TABS exceeds 24 KiB` | The tab API's notification contract repeats channel policy wording | Preserve every contract statement while compacting redundant prose; move a passage to a registered linked doc only if it cannot fit. Bump the production doc revision and rerun the focused docs test; never raise the 24 KiB limit. |
| Embedded docs differ across platforms or exceed 24 KiB only on Windows | CRLF checkout conversion enlarged the bytes embedded by `include_str!` | Keep `prod-docs/*.md text eol=lf` in `.gitattributes`, refresh clean doc files in existing checkouts, and compile the CLI again. The boundary test measures the actual embedded bytes without normalizing them; never raise the limit. |
| Several registered docs exceed 240 lines or 24 KiB after parallel lanes merge | Each lane appended prose, often hard-wrapped, and restated contracts another doc owns | Reflow hard-wrapped paragraphs to one line each, remove statements owned by another doc, then split a coherent section into a new registered doc (`cli/src/cmd/docs.rs`) with a `domios docs <NAME>` pointer; bump each changed doc's revision. Never raise the limits. |
| `essence_map_contract_committed_artifacts_are_fresh` reports a stale source digest | `essences/capabilities.md` or `essences/core.md` changed after `essences/map.jsonl` was generated | Run `domios essence-map generate --root .`, validate with `domios essence-map validate --root .`, and commit the generated map. Never edit generated nodes by hand. |
| A release-time test fails in `#[cfg(test)]` code while `check` is green | `check` compiles non-test targets and does not compile the Rust test profile | Run `./build.ps1 check-tests` on Windows or `./build.sh check-tests` on Unix, fix the test-profile compile error, then rerun the same gate. |
| A real newly spawned QA tab reports stale identity although its hook binding is live, while fixture-only report tests pass | Plan evidence authorization required a session environment variable that a fresh provider process does not have; tests always supplied one | Exercise `plan_evidence_session_test` with absent session metadata and an attested nonce. Resolve only the exact daemon-registered tab session, retain all grant/session/nonce refusals, and verify the real CLI after installation. |
| `release-preflight` fails the action-card or Web fork inventory suite after a transport/presentation change | A card fixture bypassed the batch router, or a new platform branch lacked its required transport classification | Test single help invocations in the action card and multi-action batches through `ToolCallBubble`; register the exact Web transport branch in `webForks.test.ts`. Run both focused suites, then repeat preflight. |
| `build.ps1 verify` fails only `cli_manifest_is_fresh` with sccache `os error 10055` / socket buffer exhaustion | The freshness test launched a nested `cargo run` when no standalone debug CLI binary existed, competing with the parent Cargo test process for compiler/socket resources | Keep freshness generation in-process through `manifest::to_json()` / `to_markdown()`. Run `build.ps1 test-one cli cli_manifest_is_fresh`, then rerun `build.ps1 verify`; do not regenerate committed CLI artifacts for this infrastructure error. |
| `release-preflight` prints a Rust formatting diff and exits before lock validation | The changed Rust files were not formatted with their package editions | Run `./build.sh fmt` once after the edit batch, inspect the changed files, then rerun `release-preflight`. |
| CI `guards` or `scripts/check-rustfmt.sh` flags a file that `cargo fmt --check` on Windows reported clean | On Windows, cargo fmt's module discovery skipped tracked test modules; the guard lists files from git instead | Run `./build.sh fmt` once after the edit batch, then rerun `bash scripts/check-rustfmt.sh`. |
| `release-preflight` reports a source file one or two lines over 1200 | Test responsibilities accumulated in an inline or monolithic test module until formatting crossed the hard limit | Move a cohesive test group into a child module, verify both focused suites, then rerun `release-preflight`; do not hide the overflow by compacting whitespace. |
| `release-preflight` fails under `-D warnings` for an enum field read only by tests | Production already logs the diagnostic, but an outcome variant retained a duplicate payload solely for a test assertion | Make the production outcome carry only consumed state and assert that state in the focused test; do not suppress `dead_code`. |
| macOS `release-preflight` fails under `-D warnings` because a PE/ELF byte reader is unused | A platform-specific image parser helper was compiled on macOS even though only Windows and Linux extractors call it | Gate the helper to the exact target OSes that consume it, then rerun `release-preflight`; do not suppress `dead_code`. |
| `release-preflight` fails with Clippy `never_loop` in a bounded silence oracle | The helper wrapped one `recv_timeout` in a loop even though every match arm returned on the first pass | Express the oracle as one bounded event wait, keep timeout/disconnect as silence, then rerun `release-preflight`; do not add a settle loop or sleep. |
| `release-preflight` reports Clippy `err_expect` in a spawn refusal test | The assertion used `result.err().expect(...)` instead of matching the failure. | Use `expect_err` or `let Err(error) = result else { panic!(...) }` when the success type is not `Debug`; rerun preflight. |
| `release-preflight` fails compiling `state_doctor` after a classifier signal rename/removal | The standalone testbench still consumed an obsolete `BoundarySignals` field that ordinary library tests do not compile | Update the doctor to the current structural signal and subtype ownership, then run the exact `state_doctor` test target before retrying preflight. |
| PR `source-of-truth` reports prompt files missing from `.agents/skills/prompts/inventory.md` | A new runtime or frozen-reference Markdown file was added under `src-tauri/prompts/` without registering its constant and ownership in the prompt inventory | Add every reported path to the prompt table, explain any frozen never-edit contract, bump the doc revision, then run `bash scripts/check-prompts-doc.sh` before pushing. |
| `Release lock version mismatch` during a version bump | An older preflight checked lock versions before Cargo refreshed the workspace packages | Use the current `release-preflight`, which checks both manifests before verifying both lockfiles; commit the refreshed lockfiles, then retry. |
| A direct `npm run` or `pnpm build` reports `'node' is not recognized` while `node --version` works | The package manager selected `cmd.exe` for lifecycle scripts in a shell whose effective command lookup differs from the parent PowerShell session | Use the named build operation (`build.ps1 frontend` for the production UI gate). For standalone version validation, run `npm run check:versions --script-shell pwsh.exe`. |
| `release source is not clean` | A source or lockfile change has not been reviewed/committed | Inspect `git status --short`; commit the intentional change or stop. Never bypass provenance. |
| `source SHA/status mismatch` after a bundle was created | A build step changed tracked source inputs, or rewrote an identical LF-only manifest and left Git's Windows stat cache reporting a false name-only change | Do not install when the patch is non-empty. Current provenance hashes the binary patch plus working-tree bytes, so a metadata-only rewrite is accepted; rerun `install-local` from a clean tree. |
| Self-install logs `starting` and the window never returns | A helper inherited the calling PTY Job and died when `taskkill` closed that Job | Rebuild the current CLI; it creates the helper through the out-of-job Windows process broker. Re-run `install-local`, inspect `%LOCALAPPDATA%\codeterm\self-install.log`, and verify port 7685 health. |
| Domios restarts but the initiating tab does not resume | The continuation send failed or the Tab id changed before session persistence | Inspect `%LOCALAPPDATA%\codeterm\self-install.log`; the retained `%TEMP%\codeterm-continuation-*.txt` contains the undelivered intent. |
| Self-install succeeds but gateway history reports `owner_lost` | An older build script kept the gateway owned until after the installer restarted Domios and killed the calling Tab | Update the build scripts and run `build.ps1 install-local`; the current operation releases the verified build before detached installer handoff. Prove installation separately with `self-install.log`, port 7685 health, and `domios identify`. |
| Repeated local install recompiles the full release graph | The clean-room `build` route was used instead of the persistent local cache | Use `build.ps1 install-local`; reserve `build` for public-release-equivalent verification. |
| Local install cannot read Tauri permissions under a deleted release directory | A relocated Cargo target retained absolute build-script `root-output` paths. | `install-local` now detects mismatched output roots before Cargo, preserves that exact cache in a sibling `.relocated-*` directory, and regenerates its local target; it leaves current warm caches and the machine sccache intact. |
| `test-one app usage_routes` exits 97 with zero matches | The usage route module had no tests, so the assigned selector had no oracle. | Add focused query and persisted wire-contract coverage in that module, then rerun the same selector; never accept zero matches or replace it with the whole app suite. |
| A focused Rust test spends most of its runtime linking app binaries | The test omitted `--lib`, so Cargo built unrelated binary test targets | Run the focused filter through `scripts/test-capped.sh ... --lib <filter>`. |
| PowerShell drops the standalone `--` before `--nocapture`, so Cargo rejects `--nocapture` as its own option | Native argument forwarding consumed the test-harness separator before Git Bash received it | Run the exact wrapper command through `bash -lc 'scripts/test-capped.sh ... <filter> -- --nocapture'` so the separator reaches Cargo's test binary. |
| `verify` fails `daemon_windows_spawns_use_no_window_helper` after adding a Rust test fixture that invokes Git | The fixture constructed `std::process::Command` directly, so Windows could flash a console and the repository spawn audit rejected it | Build the command in a mutable variable and call `crate::process_util::no_window_std(&mut command)` before execution; run the spawn audit and the owning focused test, then retry `verify`. |
| A fresh lane's focused Rust test reports missing `../dist` or `bundled-cli/*` | Ignored Tauri resource directories have not been staged in that worktree | Run `node scripts/dev-resource-preflight.mjs` and `node scripts/prepare-cli-stub.js`, then rerun the same focused test. Do not substitute a frontend build. |
| `fmt` cannot find a Cargo manifest for a relocated example | The example's source is outside its owning package directory | Declare its path in the owning Cargo manifest; the formatter resolves external target directories and their child modules through Cargo metadata. Run `node --test scripts/tests/format-changed.test.mjs`, then retry `build.sh fmt`. |
| Direct `bash scripts/clippy-gate.sh` fails in a fresh lane with a missing `bundled-cli` resource | The direct gate assumes Tauri's ignored compile resources are staged. | Run authorized `./build.sh check-tests` first; its resource preflight prepares frontend and CLI stubs. Then rerun the direct clippy gate. |
| macOS `local-install-cache.test.mjs` refuses ordinary fixtures under `/var` as redirected | `realpath` expands macOS's standard `/var` alias to `/private/var`, so comparing the whole lexical target to its real path mistakes an ancestor alias for a redirected cache leaf | Compare the real target with the real parent plus the expected cache basename, retain an explicit redirected-leaf refusal test, then rerun the focused Node test and `release-preflight`. |
| `sccache: incremental compilation is prohibited` | `CARGO_INCREMENTAL=1` was combined with the shared sccache wrapper | Keep `CARGO_INCREMENTAL=0`; reuse the persistent completed target and sccache entries instead. |
| Release-signing negative test passes despite a missing fixture key | The shell inherited `TAURI_SIGNING_PRIVATE_KEY` from the canonical `.env` | Run the repository check; its missing-key fixture explicitly removes the inherited key before asserting failure. |
| Push unexpectedly starts compilation or formatting checks | A stale local `core.hooksPath` still points at a removed hook directory | Run `git config --unset core.hooksPath`; the repository has no hooks, verification belongs to explicit gates and CI. |
| `Build gateway job id already exists` immediately from a fresh command | The calling Domios tab inherited a completed gateway job id from an earlier build | Update the build scripts; production gateway entry now always creates a fresh job id and ignores inherited internal ids. |
| `pnpm build` fails only in focused TSX test files during `install-local` | A test mock violates the generic bridge signature or imports an unused JSX runtime symbol | Run `pnpm build`, fix the test type/import at the source, then retry `install-local` from the same clean canonical checkout. |
| `pnpm build` reports stale role literals or duplicate fixture fields during `install-local` | A role-model refactor left TypeScript-only production or fixture surfaces outside the Rust gate | Run `pnpm build`, converge each surface on `Manager` plus `role_profile`, and retry `install-local` from a clean canonical checkout. |
| `install-local` reports `tsc is not recognized` although `node_modules/.bin/tsc` exists | Git Bash supplied the MSVC environment to Tauri, but pnpm selected a nested Windows lifecycle shell that lost the local-bin lookup | Use the current build operation, which pins pnpm lifecycle scripts to Git Bash and supplies frontend provenance without a nested shell lookup, then retry `install-local`. |
| Tauri `beforeBuildCommand` reports `pnpm is not recognized` during `install-local` | Tauri launched the configured hook through `cmd.exe` with Git Bash's POSIX-formatted `PATH`, so Windows could not resolve pnpm, Node, or Cargo | Use the current build operation, which prepares frontend, CLI, and embed inputs directly before invoking Tauri; retry `install-local` from canonical. |
| Installed health on port 7685 reports a dev identifier or Vite/CDP ports | The detached installer helper inherited `CODETERM_*` and Cargo build variables from the calling tab | Use the current installer helper, which scrubs Domios and build variables before launching the installed daemon; reinstall and verify `/api/health`. |
| NSIS reports `Error opening file for writing` for `conpty\<arch>\OpenConsole.exe` | A bundled ConPTY host from the installed Domios instance survived PTY teardown and still holds its executable | Current builds reap path-matched owned hosts before NSIS. On an older build, verify the process image is under `%LOCALAPPDATA%\Domios\conpty\`, end only that `OpenConsole.exe`, then click **Retry**; never end `conhost.exe` or an `OpenConsole.exe` from another path. |
| Windows self-install relaunches but continuation reports an incompatible CLI, or x64 startup installs an ARM64 `~/.codeterm/bin/codeterm.exe` | Parallel Windows release targets shared the generic `bundled-cli/codeterm.exe` resource, allowing one architecture to enter the other installer | Rebuild with target-qualified CLI resources. Startup now rejects a PE whose machine type differs from the app, preserving the prior working shim instead of overwriting it. |
| `test-cli-target-dir-paths.sh` reports an extra `MSVC env loaded` line or rejects a foreign cross-target before an otherwise valid path probe | Windows build initialization writes diagnostics and validates target toolchains before the probe | Use the current contract test, which selects a host-compatible fixture target and compares the final probe line while `pipefail` preserves failures. |
| Installed Domios exits during startup with a missing `tab_id`/`tabId` field or `delayed_send init` panic | A pane-era or malformed persisted record reached an all-or-nothing startup deserializer | Rebuild and install the current revision, then restart. Inspect `[plan.store]`, `[delayed_send]`, `[remote-workers]`, `[graph-workers]`, and `[session]` diagnostics for the isolated record; do not delete `plans.db` or an entire state JSON file. |
| Planned rollback to a build without the relational plan store (before `b2d388012`) after a newer build opened `plans.db` | Startup moved tasks, milestones and attempts out of `plans.document_json` into `plan_tasks`, `plan_task_milestones` and `plan_task_attempts`; an older binary reads every split plan as task-less and its next save persists that task-less document, so the rows a newer binary rejoins are stale | Never run an older binary on a split `plans.db`. Stop Domios, restore the `plans.db` copy taken before the newer build first started (with its `-wal`/`-shm` siblings), then install the older build. With no pre-split copy, stay on the newer build and fix forward. |
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
| `nextest <new-crate>` on Windows ends with `creating test list failed` and `--list` aborted with `0xc0000139` (procedure not found) | The extracted crate's test binary links tauri but has no Common Controls v6 manifest; only crates with the manifest `build.rs` get it | Copy `src-tauri/crates/codeterm-service-core/build.rs` into the new crate, then rerun the same nextest scope. Every crate whose tests link tauri needs it. |

### Focused Windows test prerequisite troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| A redirected `build.ps1 test-one` exits with `NativeCommandError` on a gateway diagnostic while its gate keeps running | PowerShell 5 converts captured native stderr into an error under `ErrorActionPreference=Stop` | Capture the PowerShell process at the OS boundary, for example `cmd.exe /d /c "powershell.exe -NoProfile -File build.ps1 test-one app <selector> --wait-build > qa.log 2>&1"`. Recover the original gate's log and exit receipt with `build.ps1 gateway-status` before launching anything else; never repeat a successful gate. |
| A focused RPC handshake test fails with E0308 at `tungstenite::Error::Http` | The fixture passed an HTTP response directly, but the error variant owns a boxed response. | Wrap the fixture response with `Box::new`, then rerun the same `test-one app` selector. |
| A native cwd regression accepts the resolved path, but its `cmd.exe` child exits 1 with `The directory or file cannot be created` | Shell startup introduces an uncontrolled dependency after native process creation; literal command-line syntax did not remove it. | Run the current Rust test executable with `--list --exact opaque-filter` instead, retaining the empty-cwd raw error assertion and a successful native child exit. Rerun the focused launch selector. |
| The focused wrapper reports `exceptional worktree compilation requires sccache` before selecting tests | The utility cannot discover the existing machine cache executable. | Add only its verified existing directory to the utility process PATH, confirm exact executable resolution and that `sccache --show-stats` reaches the prestarted shared server, then use the same focused wrapper with a fresh idle gateway. Preserve the failed receipt; never disable, replace, clear, or restart the shared cache to bypass this prerequisite. |
| Multiple crate compilations fail with `sccache: caused by: Failed to read response header` or `error reading compile response from server` (`os error 10054`) | The machine-level cache server reset compiler connections during the build; it may answer again after the failed job exits. | With the gateway idle, check `sccache --show-stats`. If it responds, retry the same bounded operation once; if it does not, use the sccache restart recovery above. Never bypass the shared cache or raise the test cap. |
| nextest reports Unknown nextest scope: api before compiling | The short api alias belongs to test-one, while nextest accepts the package name through its codeterm-* scope. | Use ./build.sh nextest codeterm-api with the same focused filter. Preserve the refused run; do not repeat a successful check. |
| Multiple cache-fronted crate checks fail with sccache socket address-in-use (os error 10048), while cache stats still respond | TCP address allocation failed; inspect local ephemeral-port pressure instead of assuming the cache server is wedged. A recorded incident had 15045 TIME_WAIT sockets against a 16384-port dynamic range, mostly to the local app HTTP port. | Preserve the failed job. At an idle gateway inspect cache stats and socket counts; keep a responsive shared server running. One CARGO_BUILD_JOBS=1 invocation can test a reduced connection burst; the recorded serial attempt also failed, so further attempts require the host pressure to be resolved. Do not repeat successful selectors, change system port ranges or kill production. Only on an explicit owner request, one process-local diagnostic check may use CODETERM_USE_SCCACHE=0 RUSTC_WRAPPER= ./build.sh check; this leaves the shared cache server and machine configuration untouched. Both variables are required because the opt-out does not clear an inherited wrapper. This recipe does not bypass exceptional test/dev paths, which explicitly require or restore sccache; do not claim those paths were uncached. |
| `test-one app` fails with `expected item after attributes` in `report_plan_ctx/tests.rs`, and the moved desktop reporter is unused | The test split left a trailing `#[test]` in the shared module and omitted it from the first moved function in `tests/desktop_reports.rs`. | Move `#[test]` onto that function, remove the trailing attribute, and rerun the same focused app filter. |

### Visible Windows dev session

For interactive verification, do not use `dev --wait`. It is the detached
readiness path for scripts and CI. Use one visible Domios Tab as the sole
foreground owner:

```powershell
# Verify no previous dev listener/process remains, then open one visible tab.
domios tab new --team <team-id> --purpose dev --cmd "Set-Location '<checkout>'; ./build.ps1 dev"
```

Keep that Tab open and require all of the following before reporting ready:

- port 7686 is owned by the checkout's staged `target\debug\domios-dev-app.exe`;
- `/api/health` reports `status=ok`, the exact checkout `build_sha`, API 7686,
  Vite 1420, CDP 9223, and identifier `com.domios.dev`;
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
parallelism. Integration batch checks that only need pass/fail set
`SLOW_UNIT_TESTS_MEASUREMENT=off`, which skips the serial re-measurement of
candidates (it can take longer than the tests themselves).
The app baseline is `scripts/slow-unit-tests.json`; separately bootstrapped
crate measurements live in `scripts/slow-crate-unit-tests.json`. Each baseline
may only shrink after its first landing. Baseline entries are
`{key, measuredSeconds}` with the crate-independent `module_path::test_name`
key of `scripts/rust-test-inventory.mjs` (`testKey`), so a crate move never
reads as a new slow unit. When crate-split turns an app module into a crate
root the key loses one leading segment; the inventory's `facadeMatch` pairs the
two keys when the match is unique, whatever the crate is named, and the
inventory diff uses the same rule. Any other module path change is a new key. `scripts/unit-test-benchmarks.json`
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
tab of that dev instance runs. Dev builds never overwrite `~/.domios/bin`:
production CLI updates come from the bundled production app during startup,
in-app update, or `install-local`.

### Crash symbols for dev builds

A dev link overwrites `src-tauri/target/debug/domios_app.pdb`, so a crash artifact
stops being symbolizable as soon as anyone rebuilds — that is how the 2026-09-05
abort lost its frame names nine minutes after it was written.

`./build.sh dev` therefore retains the symbol file of each build it runs under
`~/.domios-dev/pdb/<key>/domios_app.pdb`, keeping the newest three
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
`build-sha.txt`, informative only. The copy keeps the name `domios_app.pdb` because
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
| A gateway cargo run is `Finished` in seconds with an empty `targets/exceptional/<id>/debug` | `~/.cargo/config.toml` sets `build.build-dir` to `RustCaches/build/{workspace-path-hash}`; the stable `X:` path gives every gateway worktree the same hash, so intermediates are shared (gateway-serialized) and only final artifacts go to the target dir. | Expected. For a cold measurement remove that one hash directory and the target dir; never point two checkouts at one `CARGO_TARGET_DIR`, and only `lane-check` shares `$CODETERM_RUST_CACHE_ROOT/lanes` (fallback `${CARGO_HOME:-$HOME/.cargo}/codeterm-lanes`). Canonical gateway operations keep the user-level per-workspace build directory. Only the gateway may map the stable path, which `scripts/check-build-check-queue.sh` guards. |
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
| Linux CI reaches `cargo fmt --check` and prints formatting diffs across otherwise green Rust changes | An integration wave merged lanes without applying the repository rustfmt version before push | Run `./build.sh fmt` once on the integrated batch, inspect the changed files, then rerun both checks before pushing. |
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
| `check-queue` and dev features drift | Product library Clippy and dev must retain the manifest defaults, including embeddings. | The ordinary test profile disables defaults; `ci-tests embeddings` covers the memory feature separately. Do not remove product defaults to speed up tests. |
| An owned check-queue exits 75 because Cargo/rustc is running elsewhere | Sccache restart used a whole-machine process scan even after the caller acquired the gateway slot. | The current step validates the slot's job/pid/start tuple and uses that ownership to configure under the shared startup mutex. Only callers without exact ownership retain the global idle refusal; every supported Cargo verb uses the common entry. Verify the busy-machine owned/foreign-caller cases in `scripts/check-shared-sccache.sh`, then rerun the owned canonical check. |
| Gateway exits with `Cannot establish publication helper identity` under load | A one-shot Windows process lookup was unknown, and publication guard construction threw before the bounded mutex refusal handler. | The current guard waits on ownership filesystem events before trying the lookup again. Its deadline returns retry-safe exit 75 with no helper ownership published. Verify the transient helper-identity regression in `scripts/build-gateway-mutex.test.mjs`; unknown identity never authorizes reclamation. |
| Tracked modules or CLI manifests vanish through `X:` while another fixture runs | Fake gateway jobs held an isolated fixture slot but remapped the global production `subst` drive. Production release could also unmap that fixture's alias. | Fixture scope now skips both mapping and release; mapping requires the exact production job/pid/start owner, and release checks the mapped job still owns the slot. Verify the stable-drive isolation regression in `scripts/build-gateway-cache.test.mjs`. Never repair a live drive manually. |
| Warm check reports E0583 for tracked modules or a missing `cli/Cargo.toml` after a rename wave | In-place `git checkout --force` published no verified physical-tree boundary before Cargo. An interrupted switch could leave HEAD/index and disk inconsistent. | The current check prepares a detached staging worktree, verifies requested HEAD tree, index tree and tracked files, then publishes under the exact gateway owner. Its journal recovers interrupted publication on the next job and retains warm target caches. Verify `node --test scripts/check-worktree.test.mjs`; any tree mismatch refuses Cargo with exit 75. |
| Every `check-queue` exits 75 with `check-worktree publication incomplete; Cargo refused ... check-worktree git worktree failed (128): fatal: '<gateway dir>/check-worktree' is not a working tree` | The live `check-worktree/.git` points at a worktree registration that a `git worktree prune` in the source repository removed, so journal recovery cannot move it and the next job fails the same way | With no gateway job running, move `<gateway dir>/check-worktree` aside (`check-worktree.broken-<ts>`), move its `target/` into the `check-worktree.pending-*` directory named by `check-worktree-publication.json` to keep the warm cache, and rerun `check-queue`; delete the broken copy after a green run |
| `test-one` fails before compiling with `[cache-policy] ERROR: external CARGO_TARGET_DIR rejected: /x/target-check` while DEV is live | Under the gateway's `X:` mapping `test-capped.sh` places the isolated Windows test target at `$ROOT/target-check` = `/x/target-check`; `pwd -P` keeps the subst drive while `git rev-parse --show-toplevel` returns the real checkout, so the path comparison saw an external target | `ct_verify_target_dir` also accepts a target whose git worktree root is the checkout (`ct_target_in_worktree`); verify with `bash scripts/check-shared-sccache.sh`. Never work around it with `CODETERM_ALLOW_EXTERNAL_TARGET` |
| Queue updates time out during cache retention | Retention held gateway metadata while measuring/deleting caches and running cargo-sweep. | Retention and target marking now share a separate per-machine mutex. Verify `cache retention holds its own mutex while gateway metadata stays available`; the normal queue can update while cache work owns its lock. |
| A warm check starts with one guard implementation and finishes with another | Helper paths followed cwd after checking out the requested SHA. | Gateway/cache helper paths are pinned to the launching scripts. Verify `gateway helpers stay pinned to the launching scripts after checkout cwd changes`. |
| `--wait-build` repeatedly reacquires the metadata mutex while queued | The former slot wait used a sleep loop. | Update the gateway scripts; queue/slot filesystem events now drive the wait after releasing the mutex. The captured slot snapshot closes the release-to-subscribe race; expiry is a failure deadline. Verify with `node --test scripts/build-gateway-slot.test.mjs` (four fixture check-queue clients finish in ticket order). A caller lost without a filesystem update is reaped on the next gateway operation; the wait has a bounded failure deadline. |
| `EPERM rename state.lock.guard -> state.guard.released.owner.*` under parallel checks | Directory guard release raced concurrent readers on Windows, leaving a guard that could cause state-mutex timeouts. | Update the gateway scripts; the publication guard now uses atomic exclusive file creation and owner-checked unlink. Incomplete ownership stays a bounded refusal; a proven dead owner is reclaimed by one exclusive reaper. A crashed reaper leaves a conservative refusal requiring an owner decision, never manual deletion during live checks. Verify with `node --test scripts/build-gateway-mutex.test.mjs` (six concurrent Bash callers all complete). |
| A build exits with `Timed out acquiring build gateway state mutex` | Another live or stale gateway owner held the state mutex through the acquisition deadline. Owner liveness probes now run outside publication exclusivity; the owner tuple is revalidated before reclamation, so a slow probe cannot block release or remove a successor. The failed invocation terminates only its own Cargo/rustc descendants before returning. | Run `./build.sh gateway-status` to inspect the active and queued job IDs. If a job is stale, run `./build.sh gateway-cancel <job-id>`, then rerun the original command. |
| Checks queue silently behind a running DEV (fixed) | The old Tauri dev parent retained the gateway for the app lifetime. | Update and run `dev`: Cargo builds under the gateway, then `dev-prebuilt` validates and launches outside it. Confirm `gateway-status` is idle while the app runs and a `check-queue` completes. |
| A Windows dev build fails while writing `codeterm_lib.lib` with `os error 112` or a localized “not enough disk space” message | The canonical `src-tauri/target` exhausted the checkout drive while Cargo was replacing the debug archive. | Verify the failure owns the canonical target and no dev build is running, then run `./build.ps1 clean`. Confirm free space before restarting `./build.ps1 dev`; do not delete another worktree's target or touch production. |
| A Windows native dependency fails with `LNK1106` and `dumpbin` shows a cached `.o` ending before its COFF string-table length | `sccache` retained a truncated C/C++ object; a clean Cargo target can restore the same corrupt cache entry. | In PowerShell set `$env:SCCACHE_RECACHE='1'`, rerun the failing `./build.ps1` operation once, then remove the environment variable. Verify the rebuilt object with `dumpbin` before retrying normally. |
| A build in one worktree could reuse a cached artifact built from a different worktree's sources (codeterm#327). Never reproduced on demand — this is a configuration hazard, not an observed rebuild; see `scripts/check-sccache-basedir-isolation.sh`. | `scripts/cache-policy.sh` and `scripts/test-capped.sh` defaulted `SCCACHE_BASEDIRS` to the current worktree root. That tells sccache to strip the worktree path before hashing, so two worktrees sharing one `SCCACHE_DIR`/server can derive the same cache key even though their absolute paths differ. | Basedir rewriting is disabled for this repo: `ct_reject_basedir_rewriting` in `scripts/cache-policy.sh` unsets `SCCACHE_BASEDIRS` (warning if a parent shell leaked one in) before every `ct_configure_sccache`/`ct_require_prestarted_sccache` call, and `scripts/test-capped.sh` no longer sets it. Machine-level `sccache` and per-worktree `CARGO_TARGET_DIR` isolation are unaffected. Verify with `bash scripts/check-sccache-basedir-isolation.sh`; do not reintroduce `SCCACHE_BASEDIRS` for this repo. |
| `rustc` exits with `0xc0000005 (STATUS_ACCESS_VIOLATION)` while compiling an unchanged dependency | The Windows compiler process crashed before producing a source diagnostic; this is not a Rust assertion or test failure. | Rerun the same bounded `build.ps1` operation once so completed artifacts are reused. If it repeats, stop, preserve the exact rustc command and Windows crash evidence, and report the toolchain fault; do not clean targets or raise test deadlines. |
| After an OS crash, `rustc` panics while linking with `Invalid archive terminator` | A generated `.rlib` in the canonical Cargo target was truncated; incremental metadata cleanup cannot repair the archive payload. | Confirm no dev build is running, run `./build.ps1 clean`, then restart the original named operation. Do not delete individual archives or reuse the damaged target. |
| Dev compiles and opens but port 7686 never listens, with a trust-boundary policy error in `~/.domios-dev/logs/domios.log` | The retained dev config requests a non-loopback host without a bearer token, or contains a wildcard/malformed CORS origin. | Keep dev local by setting `api.host = "127.0.0.1"`, or configure a token for an intentional network-facing instance. Remove `*` and malformed entries from `api.cors_allow_origins`; an empty list is valid and denies all cross-origin browser requests. |
| Windows `build.ps1 kill` reports that the clean shutdown deadline elapsed after a high-tab-count dev fixture | Closing many PTYs and watcher leases can exceed the bounded graceful-shutdown window even though the instance is otherwise healthy. | Let the named operation use its port-scoped force-stop fallback, then verify 7686 is free and 7685 still listens. Never replace it with a process-name kill. |
| `test-one plan` fails to compile old `Task` fixtures after an orchestration schema change | The app gate compiled the plan crate as a dependency but did not compile its own unit-test fixtures. | Update every direct `Task` fixture and changed helper signature, then rerun the exact `test-one plan` filter. Do not remove the plan gate or move it to an app filter. |
| `test-one plan` reports E0599 for `ReportStatus::InProgress` in a new fixture | The fixture confused task lifecycle status with the separate report-status enum. | Use the existing `ReportStatus::Partial` for a non-review progress report, preserve the rejection assertion, and rerun the failed focused selector before remaining gates. |
| `test-one cli` cannot find a private helper after a command module split while `check` stays green | The binary compiles, but the test target still relied on a parent-module helper that moved behind a private child-module boundary. | Give the test its own fixture helper or import a test-only API; do not widen production visibility. Rerun the exact CLI filter that exposed the test target. |
| `test-one cli` reports a non-exhaustive `TabAction` match after adding a command | The CLI command was added without its mesh API-map consumer. | Add the capability-equivalent route in `mesh_cli/api_map.rs`, then rerun the focused CLI test. |
| `npm run test:plugins` reports `'node' is not recognized` before the suite header, or Git Bash cannot find Node | An oversized/polluted Windows `PATH` was lost at a `cmd.exe` boundary; the former `npx.cmd` runner added another shell boundary. | Run `node scripts/run-plugin-tests.mjs` through a known Node executable. The aggregator pins `process.execPath` into Git Bash and uses the checked-in local tsx CLI; shorten the inherited `PATH` before retrying the npm wrapper. |
| TypeScript rejects a narrowed optional mutation error inside a state callback | Narrowing an object property is not retained across the deferred callback. | Capture the error in a local constant before the guard and callback, then rerun type checking and the owning Settings suite. |
| `test-one app` reports E0308 after removing a hook event | A fixture retained the old fixed array length after its event roster shrank. | Update the owning fixture length to the actual roster and rerun the failed focused app filter; plugin-only compilation does not cover app test fixtures. |
| `check-queue --all` reports missing lazy-restore fields or an unresolved `multiplexer::restore_policy` import | Full Tab test constructors need new fields; service-core exposes only selected multiplexer facade modules. | Initialize `frozen` and `last_active_at_ms` in full Tab fixtures; import policy through `codeterm_pty::multiplexer::restore_policy`. Preserve the failed SHA receipt and let the owner authorize a corrected-SHA check when the lane has a one-check limit. |
| `test-one app` reports a missing field in `SpawnTabSpec` after adding spawn routing state | A full constructor or test fixture bypassed `Default`, so ordinary source review missed the new required field. | Set the field explicitly in every full constructor and fixture, then rerun the exact focused test before continuing. |
| The docs gate reports `API-TRANSPORT exceeds 24 KiB` after adding an RPC row | The transport index is at its embedded size boundary. | Keep the index row concise and put the purge procedure and storage limits in MEMORY; rerun the focused embedded-doc test without raising its limit. |
| `test-one cli cmd::docs` reports an embedded document over its line boundary | Independently valid reference additions accumulated past the agent reading limit, and ordinary compilation does not exercise the embedded-doc contract. | Compact repeated prose without deleting behavior, or split the document by responsibility and register the new whole document. Rerun the exact docs filter. |
| `verify` fails both an embedded-doc boundary and `cli_manifest_is_fresh` | A documentation edit crossed its registered reading limit while committed CLI metadata already lagged the current command surface. | Compact or split the named document, regenerate with the freshly built `domios cli-manifest --write`, rerun both focused CLI tests, then rerun `verify`. |
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
| `verify` downloads model weights into a temporary test data directory and fails on a model-host outage | The Rust test wrapper let fixture-owned `CODETERM_DATA_DIR` choose the immutable model cache. | Use the current wrapper, which exports one machine-shared `CODETERM_EMBED_CACHE_DIR`; override it explicitly only for cache diagnostics. |
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
| `check-tests` emits allocation failures, rustc `0xc0000409`, then missing-rlib cascades while another app build is active | Concurrent app compilation exhausted available host memory; the later rlib errors are secondary until proven otherwise. | Preserve the first allocation failure and raw exits. Defer the app checks to the owner's canonical batch with dev stopped; use `CARGO_BUILD_JOBS=1` for the named operation if memory pressure persists. Never kill production or clean shared targets from a Worker lane. |
| Windows PowerShell QA capture throws `NativeCommandError` on ordinary Cargo progress and records no exit | Caller-level `*>` redirection turns native stderr into PowerShell errors under `build.ps1` strict error handling. | Run a child `powershell -NoProfile -File ./build.ps1 ...` with OS-level `cmd /d /c` redirection (`> run.log 2>&1`), then record the child exit code. Do not classify the progress line as a compiler failure. |
| `test-one` fails with sccache socket error 10048 and has no build-history row | A scope bypassed gateway acquisition and configured sccache before owning the build slot. | Use the shared cargo gateway entry for every scope and cargo-running verb, including CLI, check-tests, CI inputs and named dev. Check `build-history` for each invocation; run the mixed-verb fixture in `scripts/build-gateway-cache.test.mjs`. After merge, run two focused `test-one` and two `check-queue` calls concurrently with `--wait-build`, and retain all four exits and history rows. |
| `check-tests` fails at catalogue watcher `targets.sort()` / `dedup()` | The watcher parameter was immutable. | Declare `mut targets` in `ProviderBinaryWatchers::watch`; rerun the authorized `check-tests` and focused catalogue gates. |
| A release-time test fails in `#[cfg(test)]` code while `check` was green | `check` compiles the production profile only, so test-only code is never type-checked | Run `./build.ps1 check-tests`, which compiles `--all-targets` for both workspaces through the build gateway. A stale generated map or a document over its reading boundary is a different cause: use the matching row above. |

| Plugin test compilation cannot read an include_str! fixture | The relative fixture path traversed one directory beyond the repository root. | Resolve the path from the owning Rust source directory, correct the traversal, and rerun the same focused plugin gate before proceeding. |

A moved repository-watcher source guard can fail with “expected the repository-watch sources, found 1” if it still scans the app path. Scan the new crate sources and retained app adapter, keep channel failure deadlines in test support, then rerun `./build.sh test-one repowatch tests::`.

## Focused analytics checks

`./build.sh test-one analytics tests::` runs the isolated product analytics crate
through the capped Rust runner in canonical. Workers type-check touched crates using Cargo default CPU parallelism unless the caller explicitly overrides it and format their Rust sources;
its HTTP fixtures use a local fake destination with synthetic retry timestamps.

## macOS endpoint-security hazard

On managed Macs an Endpoint Security agent (seen: BeyondTrust `defendpointd`, with CrowdStrike Falcon also present) evaluates every fresh executable on first exec. Under the `nextest --list` burst of many large fresh test binaries it can block one with a bare SIGKILL (signal 9, no AMFI or kernel line), different crate each time; the same binary runs fine by hand afterwards. Tiny fresh binaries do not trigger it. The same exec latency can also fail in-test deadlines spuriously.

Diagnose before treating it as a test failure: `systemextensionsctl list`, then `/usr/bin/log show --start '<t-30s>' --end '<t+30s>' --predicate 'process == "defendpointd" OR eventMessage CONTAINS "Blocking"'` around the kill (call `/usr/bin/log` explicitly: zsh's builtin `log` shadows it). A `Blocking <pid> <path>` line for the test binary confirms it. The fix is environmental: exclude the developer build directories in the endpoint policy (owner/IT decision). The gateway does not retry killed binaries.

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

Inject these environment variables before the app, CLI or server build. Firebase
web config uses the existing same-name GitHub repository secrets. Release
compilation refuses a missing or blank key, project ID or app ID. Auth domain is
optional and defaults to `<project_id>.firebaseapp.com`; the account build script
fingerprints all four effective fields as a compiler input.

- DOMIOS_FIREBASE_API_KEY
- DOMIOS_FIREBASE_AUTH_DOMAIN
- DOMIOS_FIREBASE_PROJECT_ID
- DOMIOS_FIREBASE_APP_ID
- DOMIOS_ACCOUNT_REDEEM_URL
- DOMIOS_BILLING_FUNCTIONS_URL
- DOMIOS_AMPLITUDE_API_KEY_TEST
- DOMIOS_AMPLITUDE_API_KEY_PROD

`build`, `install-local` and release packaging resolve key, project and app
as one tuple, preferring the Domios secret store and falling back to a complete
existing build environment. Redeem overrides come from the selected source and
are optional for release; debug compilation still requires an explicit redeem
endpoint to select cloud authentication. The optional billing functions URL prefers the secret store, then
the existing environment. The auth domain also comes from its same-name local
store entry or environment variable. These values are scoped to the packaged build;
a missing billing URL never fails the build. Billing logs only present or absent,
without printing the URL. Complete account values print
`Firebase config: cloud-auth enabled`; missing or partial values print
`Firebase config: local-only`. That line reports compiled config availability,
not whether a Firebase provider is enabled. Separately confirm Google and
email-link sign-in are enabled for the selected Firebase project. Save the
values once with stdin:

```text
domios mem secret save --name DOMIOS_FIREBASE_API_KEY
domios mem secret save --name DOMIOS_FIREBASE_AUTH_DOMAIN
domios mem secret save --name DOMIOS_FIREBASE_PROJECT_ID
domios mem secret save --name DOMIOS_FIREBASE_APP_ID
domios mem secret save --name DOMIOS_ACCOUNT_REDEEM_URL
domios mem secret save --name DOMIOS_BILLING_FUNCTIONS_URL
```

`dev`, `dev-back`, and `dev-server` resolve the same account and billing
configuration for the server compile. They also inject `DOMIOS_QA_PROJECT_ID`
from the secret store into the debug TEST activation. A missing entry prints
`QA project not configured in this build`; that build refuses TEST grants and
stays free. Save the entry with `domios mem secret save --name DOMIOS_QA_PROJECT_ID`
and rebuild with `./build.sh dev`; `dev-prebuilt` cannot change compiled values.
Focused configuration proof: `bash scripts/tests/firebase-build-config.test.sh`
and `node --test scripts/tests/dev-gateway-handoff.test.mjs scripts/tests/dev-server.test.mjs`.

Run each command and provide that value through stdin; omit `--value` so it
stays out of shell history. The `domios mem secret` command uses `save` for
this operation. Do not put these values in repository files.

Account, billing and analytics own one build_config module each, using option_env!.
Only vendor adapters consume it. Runtime account and billing config overrides
the compiled defaults; Amplitude uses a public ingestion key embedded at build time; the secret store
`domios.amplitude.prod.api_key` overrides it for local builds
(`domios.amplitude.test.api_key` for test projects). Availability is cached
once per process, so changing the override requires restarting the daemon.
Debug builds without Firebase values use local; without a billing endpoint billing
returns free locally and reports subscription actions unavailable. Missing
Amplitude credentials leave queued events undelivered with unavailable status.
Changing build-time values requires rebuilding; never commit vendor credentials.
Firebase Functions, admin and hosted sign-in source live in the private
[Firebase repository](https://github.com/rollacode/codeterm-firebase), with the
former cloud contents at its root. Follow its config generation, package checks
and deployment procedures there. Before hosting deployment, Turner generates the
ignored `hosting/signin/config.json` from the same Firebase web configuration:
`firebaseConfig.apiKey`, `authDomain`, `projectId`, and `appId` map to the respective
`DOMIOS_FIREBASE_*` names. Preserve the page's separate App Check settings. Keep
`config.example.json` values as obvious `COPY_*` placeholders. Run
`node scripts/write-signin-config.mjs` in codeterm-firebase before deployment;
its protected deploy workflow injects the corresponding GitHub configuration and
runs the writer. QA uses `scripts/deploy-qa.mjs` with the same writer. Obtain the
unchanged values from Firebase Console → Project settings → the existing Web app,
or the existing local secret store; never print the API key or rotate it for this
build configuration step. CI passes its environment directly and never resolves
machine-local secret-store overrides. The private
[website repository](https://github.com/rollacode/codeterm-web) owns the marketing
site build and deployment, with the former web app at its root. Repository
creation does not establish production readiness or authorize publishing.
Desktop account and billing build variables above remain app dependencies.
Preserve domios://auth/callback, domios://billing/return and authenticated
entitlement refresh when integrating either external service.

Run `./build.sh fmt` in a fresh lane before direct pnpm checks; it prepares the checkout-local dependencies automatically. Direct pnpm commands bypass that wrapper. A failed dependency lookup selected no executable tests.
For intentional gateway queueing, pass `--queue` (alias behavior of `--wait-build`). Lane checks honor the same mode by blocking on the shared intermediate file lock; the default refuses a busy lock. CLI and app workspace crates require separate lane-check invocations. The default stays fail-fast. Queueing uses the existing gateway event and normal completion report. Tab-origin build reports retain tab provenance; they never claim system provenance.

<!-- revision: 60 -->

### Lane and batch scopes

After each batch of edits, Workers run `./build.sh fmt` once before their checks; the Orchestrator runs it once after each integrated edit batch before checks. It applies rustfmt to changed Rust files using their package editions and applies Biome's safe fixes/import organization to changed TS/JS/JSON files. This replaces hand-run rustfmt commands and does not enable Biome formatting. Use `--base REF` to select the comparison ref (default: dispatched lane base, canonical checkout HEAD, then upstream/local develop). Missing, partial or stale lane dependencies are prepared automatically with `pnpm install --frozen-lockfile` from the shared pnpm store before formatting. Each checkout keeps its own `node_modules`. Install failures retain their diagnostics and stop formatting; fix the reported cause and rerun. There is no unfrozen retry. Biome runs in bounded argument batches. Focused proof: `node --test scripts/tests/lane-dependencies.test.mjs scripts/tests/pnpm-isolation.test.mjs`.

Workers run `./build.sh fmt` once per batch, then one verification pass: a single `./build.sh lane-check <package>... [--tests]` call for all touched crates, changed TS/JS tests and `tsc` when TS changes, plus guards. List canonical selectors under GATES REQUESTED; on failure, fix all reported errors and recheck only what failed.

Canonical QA builds `./build.sh nextest-archive <sha>` once. The verified warm worktree builds root and service-core library tests without default features into one atomically published gateway `test-archives/<sha>/app-no-embeddings.tar.zst`. `test-one app <selector>` and `nextest app [filterset]` reuse it on the same clean Rust SHA, without recompilation; a missing archive uses the ordinary build path. rdeps filtersets use the ordinary workspace path. Both app runners omit embeddings. The feature guard keeps the product dev and root/core defaults aligned; ordinary tests explicitly disable defaults. Canonical measurements use `./build.sh nextest-archive <sha> --measure <selector>...`, within the verified warm checkout for that SHA. Archives print build wall time; archived test-one prints selector wall time. Preserve logs and history for measurements; never infer archive reuse from a short duration.

If a completed job reports `logging_degraded`, its command exit and completed/failed state are retained; the warning names the partial log. A tee/EOF drain failure formerly overwrote successful Cargo exit 0 with wrapper exit 74. Inspect the full check log for diagnostics; run `./build.sh gateway-logging-check`. An unknown command exit or metadata publication failure still refuses readiness.
A mutex refusal prints the state.lock owner PID, start token, job ID, publication age and queued waiter tuples. Unknown values remain unknown; age never authorizes reclamation. Capture this evidence with the job receipt before investigating contention.

A Windows `EPERM` while renaming `state.lock.pending.*` to `state.lock` is a publication sharing conflict. The helper releases its publication guard, retains the same candidate, and waits for a mutex ownership event before retrying inside the existing failure deadline. Its own guard changes and unrelated file events do not drive retries. A deadline refusal removes the unpublished candidate. Run `node --test scripts/build-gateway-mutex.test.mjs`; capture the owner/waiter tuple if the bounded event wait expires, without deleting unknown shared ownership.

A queued PowerShell -> Git Bash client must retain its actual Bash waiter identity. Job, queue and slot ownership now record BASHPID with its MSYS start token, mapped Windows PID and Windows start token. On Windows reaping probes the captured native identity, not the MSYS number; unavailable native evidence remains unknown and never reaps. Older incomplete ownership needs an owner decision, not inferred death from a missing proc entry. Run `node --test scripts/build-gateway-owner.test.mjs` for the real build.ps1 chain and namespace model; capture both identity pairs when reporting a future owner-lost incident.

Canonical P3 measurement on core-touching SHA `5df475f92` (205 files): full check 157 s (job `20261014T210857-13756-8399`), `--lane` 148 s with errors=0, warnings=0, exit=0 (`tone-gates-20261014-230937-49731`). The saving is 9 s (about 6%); service-core compilation dominates this lane. Do not promise large savings for core-touching lanes. Archive SHA `d001809b8` built in 367 s (job `20261014T232911-133-4384`); selector timing was invalid because the first archived run refused its argv with exit 2 after 1 s. Selector measurements must be rerun after the argv repair.

An archived nextest run rejects Cargo package/target selection such as `-p` and `--lib` together with `--archive-file`. Select the archived core library using `--filter-expr 'package(=codeterm-service-core) & kind(lib)'`, intersecting any caller filterset, while retaining workspace remapping and zero-match refusal. Both app runners use the same archive argument builder. Run `node --test scripts/nextest-archive.test.mjs`; its exact captured selector argv is checked by the installed nextest parser with a nonexistent archive (no Cargo build or test execution), and the former conflicting argv must refuse as a usage error. Canonical then reruns `./build.sh nextest-archive d001809b8 --measure service::close_history service::team service::workspace service::plugin_host sentinel:: socket::server::plan_review api::mesh_routes::rpc commands::html_preview startup:: update::`.

If an archived selector refuses `workspace root manifest at X:\Cargo.toml does not exist` (job `20261015T000926-46028-23229`), the remap points at the checkout rather than the Cargo workspace. App archive runners and serial slow-test confirmation remap to `<checkout>/src-tauri`, where Cargo.toml contains `[workspace]`. The shared Bash builder refuses a missing manifest before running nextest; archive and serial-confirmation fixtures assert the target manifest. The failed first selector's exit 96 / 4 s is not a test execution measurement. Rerun the canonical measurement command above after merging the repair.

DEV releases its gateway job and closes the build log before invoking the existing
`dev-prebuilt` provenance validator and launch path. App lifetime and dev-only
`kill` no longer depend on a gateway job; a rebuild acquires a new job.

A gateway DEV build records physical artifact paths through the existing provenance
writer, so standalone `dev-prebuilt` verifies after the mapped drive is released.
For a same-build restart, call `dev-prebuilt --wait` directly: provenance and runtime
preflight precede dev-only stop. Never prepend `kill` to a prebuilt restart.

DEV readiness requires healthy status, `startup.state=ready`, exact build and
instance identity, and authenticated native attachment. The prebuilt launcher
waits on startup events inside its existing 30-second phase deadline; listener
availability alone cannot finish launch. An unsupported startup-event endpoint
is an explicit failure. Preserve that receipt and rebuild the intended source,
without extending the deadline. Focused tooling coverage is
`node --test scripts/tests/server-readiness.test.mjs scripts/tests/dev-wait.test.mjs scripts/tests/dev-app-process.test.mjs`.

DEV Cargo runs at the gateway's Windows `X:` cwd: default target directories are
`X:/src-tauri/target` and `X:/cli/target` (the physical checkout targets). Named
`--wait` instances retain their explicit instance target. `check-queue` instead
uses its own check-worktree `target/app` and `target/cli`, so it does not share
DEV target directories. `dev-prebuilt` never invokes Cargo or `dev-prep --no-server`;
its staging is complete before provenance is written under the gateway.

## Product and test feature sets

If `codeterm-mem` lib tests fail without defaults on unused `open_with_embedder`
or `submit_chunks`, their test helpers lack the embeddings gate used by their
callers. Gate the helpers with `cfg(all(test, feature = "embeddings"))`; retain
the tests and denied warnings. After `source build.sh env`, compile both with
`cargo test --manifest-path src-tauri/Cargo.toml -p codeterm-mem --lib --no-default-features --no-run`
and the same command without `--no-default-features` in a Team build shell.

For a CPU embedding parity measurement, retain an old embedding executable before
removing its build artifacts, then run `./build.sh embedding-parity --old-binary
<old-executable> --baseline <baseline.json> --output <report.json>`. The standalone
memory example builds in the checkout target and uses the production embedding worker; the Python oracle compares
25 fixed English/Russian cases, including truncation and Unicode, and fails below cosine 0.999. To reuse a saved
baseline, omit `--old-binary`. It reports cold/warm process RSS and warm p95 CPU
latency; these process measurements do not replace a canonical daemon search
measurement. Windows uses `build.ps1 embedding-parity` with the same arguments.

DEV and check-queue library Clippy retain default product features, including embeddings. Ordinary `test-one`, `nextest`, `check-tests`, archives and CI use `--no-default-features`. Check-queue does not repeat the separate dev-lib check; its library Clippy pass type-checks the product configuration. Full check-queue also checks service-core tests in the test profile without defaults. The CI `rust-embeddings` job runs `./build.sh ci-tests embeddings` once, covering the memory crate and service-core memory tests with embeddings enabled. Receipt validation requires this job. Archives use `app-no-embeddings.tar.zst` so old feature-enabled archives cannot be reused.

## Workspace-hack

`src-tauri/crates/workspace-hack` (cargo-hakari, config `src-tauri/.config/hakari.toml`) gives the server,
CLI and app one feature set per shared dependency, so separate release builds reuse each other's crates.
After a dependency change run `cargo hakari generate` in `src-tauri`. The final-excludes list every crate
that only the app uses on some platform; when the app gains a dependency, recompute that set from
`cargo tree -p <codeterm|domios-server|codeterm-cli> --target <triple> -e normal,build --prefix none`
on all six hakari platforms and add the app-only names. Then compare `domios-server` and `codeterm-cli`
closures before and after: no new crate may appear, and `check-crate-layering.sh` must pass.
`cargo hakari verify` reports the residual app-only feature differences; that is expected.

## Lane crate checks and shared intermediates

Before committing, run `./build.sh lane-check <package>... [--tests]` once for all touched packages on every platform. It checks them in one Cargo invocation, stages frontend/sidecar resources without building them, and defaults to no gateway slot or sccache action. Explicit `--wait-build`/`--queue` acquires the gateway before waiting for the lane lock. `codeterm-cli` selects `cli/Cargo.toml` and `--bin codeterm` and cannot mix with app workspace packages; library packages select the app manifest and `--lib`.

`lane-check` exports `CARGO_BUILD_BUILD_DIR=$CODETERM_RUST_CACHE_ROOT/lanes` inside its own subshell, falling back to `${CARGO_HOME:-$HOME/.cargo}/codeterm-lanes`. Repository Cargo configuration and canonical gateway operations never select this shared directory. An OS-backed lock serializes the entire lane Cargo invocation across final targets. By default a competing lane exits 75 without compiling; `--wait-build`/`--queue` waits on the lock without polling. Python is required for this lock. Final targets remain per worktree. `source build.sh env` loads the toolchain without changing this cache policy. Builds and tests retain the gateway; check-queue remains the committed integration/clippy gate.

Lane-check and retention resolve the shared directory with `scripts/lane-build-dir.mjs`: `$CODETERM_RUST_CACHE_ROOT/lanes`, otherwise `$CARGO_HOME/codeterm-lanes` (Cargo home defaults to `~/.cargo`). GC adds that directory explicitly, independently of canonical/gateway Cargo metadata, and trims it once under Cargo locks. Its `CACHEDIR.TAG`, fixed basename and direct-child boundary are required; links and busy locks refuse trimming. Existing two-level hash directories retain their guarded collector. Verify with `node --test scripts/cargo-build-gc.test.mjs scripts/tests/lane-check.test.mjs`.

`gc` and hourly automatic retention trim incremental caches in every remaining shared build hash, the lane cache and registered checkout targets. For each profile and crate they keep the newest hash directory and newest `s-*` session; older entries must be idle for 12 hours (`CODETERM_INCREMENTAL_IDLE_HOURS` overrides). Write times include descendants. The caller must own the gateway before compilation, Cargo/profile and lane locks must be available, and held rustc session locks protect their variant. Links and paths outside the recognized cache layout are refused. `gc --dry-run` reports reclaimable bytes. Existing lane deps retention keeps two units per identical Cargo fingerprint identity; different feature/profile identities remain protected. Focused fake-tree proof: `node --test scripts/tests/incremental-retention.test.mjs scripts/cargo-build-gc.test.mjs`.

### Canonical or gateway jobs unexpectedly use the lane build directory

A tracked repo-root `.cargo/config.toml` applies to every Cargo invocation beneath the repository, including canonical and the gateway check-worktree. Using it for the lane fallback caused shared lock contention, discarded warm canonical cache reuse and selected a profile cache outside the owner's machine root. Lane-only Cargo settings belong in lane-check's process environment, never in repo-root Cargo configuration or unconditional build setup. Scope both the machine-root choice and fallback to the lane operation. Verify effective build directories for a lane, canonical and the gateway with the machine root both set and unset; lane reuse alone cannot prove cache isolation.

| Dedicated CI mode fails at catalogue inventory with ENOENT on the Windows stable drive root | Git reports the physical checkout while discovery compared it to a subst alias, then tried walking the drive root. | Catalogue discovery resolves the root with `realpathSync.native` before Git identity and file enumeration. Verify the temporary-subst regression with `node --test scripts/tests/test-catalogue.test.mjs`, then run the dedicated CI mode under the normal gateway alias. |

Product check-queue Clippy disables Cargo defaults and explicitly enables the selected root/core/mem package product features. This keeps embeddings enabled while avoiding the extra synthetic `default` feature marker that direct core selection otherwise adds. Ordinary tests still disable defaults without product features. The candle engine and its model dependencies are feature-gated so shell test targets compile without embeddings under denied warnings.

## Repeatable core feature measurements

In a temporary worktree on one fixed Rust SHA, run `./build.sh feature-cycle before mem::dream_status_tests::wip_placeholder_does_not_imply_running` and then the same command with `after`. Both run under the normal gateway. The dev phase is an equivalent product library `cargo check`, without launching DEV; the queue phases use the original all-target/default/test configuration or the aligned product/ordinary-test configuration. One focused core test executes through test-capped after compilation. This is an equivalent-configuration cycle, not a DEV app build or full check-queue timing.

Each job retains phase logs, wall times and `units.json` under its artifact directory. The collector counts distinct completed service-core artifact hashes, including fresh reused units, separately from distinct feature sets; never infer that two feature sets means two Cargo unit hashes. Compare actual full check-queue runs separately, loading the old/new build driver against the same committed source SHA in a temporary worktree. Do not clean shared caches for a measurement or include queue wait in a reported Cargo/check-queue wall time.

On Cargo 1.95 the measured service-core result is four to three `.rmeta` hashes across two feature sets: product/dev, product/Clippy and ordinary tests. Clippy uses `RUSTC_WORKSPACE_WRAPPER`; Cargo deliberately gives wrapper-built workspace units separate filename hashes ([Cargo reference](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-reads)). Dev and Clippy therefore retain separate hashes even with identical product features. Do not alter wrappers or fingerprints to force two hashes. Count linked test executables separately from `.rmeta` files; the collector includes both in its all-artifact count.

One equivalent two-package cycle on Rust SHA `b6d0ef3e14b3787a9678ff436a6ce6739e0df91e` measured queue phases at 294 seconds before and 181 seconds after (38% lower). This excludes queue wait, initial dev checking and final focused test compilation; it is not a full check-queue timing pair or a general speed guarantee.

Queued build invocations parse the entire driver inside `codeterm_build_main` before waiting for a gateway slot. Editing build.sh while an older shell waited previously changed the unread tail of that shell's script, producing an EOF syntax error after a successful Cargo check. The driver snapshot regression waits on an explicit input event, replaces only a temporary script, and requires its already-parsed dispatch to complete. App nextest configurations also replace the initial target arguments so `--no-default-features` appears once; nextest rejects duplicate occurrences. The final shell entry call can still be affected when a running driver is edited in place: a successful Cargo phase followed by `ld_main: command not found` is a harness failure, not a compile verdict. Finish driver edits before launching checks, then rerun the affected focused operation from the final file.

Lane-check prompt additions affect both the 5000-byte worker suffix and the 1450-byte worker charter. The charter includes testing_discipline without worker_suffix, so keep the raw check command in that shared fragment while compressing it. Preserve both ceilings and verify all composed surfaces with the focused core prompts selector.

### CLI package absent from lane-check workspace

`lane-check codeterm-cli --tests` formerly exited 101 before compiling because
it selected the app workspace and a library target for the binary-only CLI.
The verb now selects the CLI manifest and binary. Run
`node --test scripts/tests/lane-check.test.mjs` to verify routing, then rerun
the lane check. Committed check-queue also includes CLI test targets.

### Focused tests for external path dependencies

`test-one paths tests::strip_verbatim_prefix` and `nextest paths` select
`codeterm-paths` through the app manifest without feature-selection flags.
`codeterm-paths` lives in `src-tauri/crates/codeterm-paths` as an app workspace
member. Selecting it with test-only dev dependencies while it lived outside
the workspace refused before test execution; adding a non-hierarchical member
also refused. Move the one package into the workspace and update its path
consumers; retain the same selector and require nonzero executed tests.
`mem-store` and its `codeterm-*` alias remain external path dependencies without
Cargo features. Passing `--no-default-features` for that external package fails with
"cannot specify features for packages outside of workspace" before any test runs.
Release archives select these alongside workspace packages; focused routing
retains the app workspace profiles and target cache without that invalid flag.
Verify the argv fixtures with
`node --test scripts/dev-build-configuration.test.mjs`, then run the focused
selector in canonical and require a nonzero executed test count.


### macOS lane-check fails in the sccache workspace wrapper

With `rustc-wrapper = "sccache"` in Cargo's user config, sccache 0.16 can
mis-probe the lane `RUSTC_WORKSPACE_WRAPPER` passthrough with `-E` and report
`Compiler not supported` before compilation. Scope the workaround to that lane
check:

```sh
RUSTC_WRAPPER= CODETERM_USE_SCCACHE=0 ./build.sh lane-check <package> [--tests]
```

This leaves Cargo configuration and the machine cache unchanged.


### Lane-check reports a missing item that exists in this worktree

Cargo 1.95 can assign the same workspace unit hash to matching relative package
paths in different worktrees sharing a build directory. If the next worktree's
sources predate the cached artifact, Cargo may mark its dependency fresh and
supply the other worktree's metadata, producing a false E0425. No stable drive
mapping is needed; touching the dependency only repairs one check.

Lane-check sets `RUSTC_WORKSPACE_WRAPPER` to the physical worktree's passthrough
wrapper (a native executable on Windows, `scripts/lane-rustc-wrapper.sh` elsewhere). Cargo
includes that path in workspace artifact hashes. Registry dependencies still
reuse one shared cache; lane checks retain Cargo's shared build-directory lock
and per-worktree final targets. The setting is confined to the lane subshell;
canonical operations keep their existing wrapper policy. Do not replace this
with per-lane target/build directories, global Rust flags or cache cleaning.

Run `node scripts/tests/lane-metadata-repro.mjs` in a Team QA shell. It creates
two throwaway Git worktrees with different exports and deliberately old source
timestamps, proves the former policy returns E0425, then checks A/B/A/B through
lane-check and verifies two workspace metadata files but one registry metadata
file. The registry fixture is local and needs no network. The same regression
runs under `node --test scripts/tests/lane-check.test.mjs`.


### Windows lane-check fails with "The command line is too long"

A `.cmd` workspace wrapper passes the compiler invocation through cmd.exe's
8191-character limit. Service-core library and test targets exceed it before
rustc starts. Windows lane-check now compiles `scripts/lane-rustc-wrapper.c`
with the MSVC environment already loaded by build.sh. The small native launcher
forwards the original Unicode command line with `CreateProcessW`, inherits
standard handles, waits for compiler exit and propagates its exit code.

`scripts/lane-native-wrapper.mjs` caches that executable beside the shared lane
build directory in `codeterm-lane-wrappers`, keyed by physical source path,
source bytes and host architecture. Compilation uses a temporary directory and
publishes only a finished executable. Different worktrees retain different
wrapper paths; no Cargo target directory is added. Unix keeps the shell wrapper.

On Windows, run focused tests with the build toolchain loaded in a Team QA shell:
`bash -c 'source build.sh env; CODETERM_LANE_REAL_CRATE_TEST=1 node --test scripts/tests/lane-check.test.mjs'`.
Run this opt-in regression from a lane: it includes the two-worktree E0425
reproduction, a 12000-character Unicode/quotes/exit-code forwarding check, and
real `./build.sh lane-check codeterm-service-core --tests`. Require exit 0 from
the real crate check; a tiny fixture alone does not cover Windows argument limits.

## Independent server process oracle

If Windows refuses probe cleanup with `EBUSY`, retain the failed result and
disposable data; a prior runtime milestone is not a successful full probe.
Report success only after cleanup, remove the verified scratch alias before
deleting data, and never follow that alias or use a broad process kill.

If the probe CLI build reports E0061 at `generate_system_plugin_hashes`, its
build-script caller still uses the old zero-argument API. Pass the CLI's
`CARGO_MANIFEST_DIR` path to the shared bundler, whose parent resolves the same
repository plugin directory; then rerun the process oracle.

If `lane-check domios-server --tests` refuses with `lane-check requires a
codeterm package` before Cargo, the package router predates the independent
server name. Admit exactly `domios-server` alongside `codeterm|codeterm-*` in
`scripts/lane-check.sh`; keep all other refusals and lane resources unchanged.
Verify with `node --test --test-name-pattern='server lane routing'
scripts/tests/lane-check.test.mjs` (stubbed Cargo), then run the authorized
focused lane check. Never broaden the allowlist to a `domios-*` wildcard.

If the extracted server build reports `bundled plugins directory is missing`,
check the server build script's app-manifest input before rebuilding: it must
canonicalize `CARGO_MANIFEST_DIR/../..` before the shared bundler takes `parent()`.
Taking the parent of a literal trailing `..` selects `src-tauri/crates/plugins`
instead of the repository `plugins/` directory. Source review caught and fixed
this extraction path error; no native build failure was executed in that lane.

If the crate-version guard flags the independent server's release version, do
not carry the app version in its shared library package or exempt it from the
guard. Keep its Cargo version at `0.0.0`; the server build script injects
`DOMIOS_VERSION` from the existing `src-tauri/tauri.conf.json` release source,
tracks that input, and installs it through `codeterm_build_info` at startup.
Parent canonical refreshes lockfiles and verifies the shipped version; a source
guard pass alone is not built artifact identity proof.

Parent canonical Team QA owns all Rust and native execution for this slice:

```bash
./build.sh test-one domios-server process_split_contract
./build.sh process-probe --case server-only --instance split-oracle
./build.sh process-probe --case arbitration --instance split-oracle
```

On Windows use the same arguments through `build.ps1`. `test-one domios-server`
selects only the server library, disables default test features, and the existing
capped runner refuses zero selected tests. Moved startup preservation selectors
are `./build.sh test-one domios-server startup::` and core path arbitration is
`./build.sh test-one app instance_lifetime::path_owner::`.

`process-probe` first builds the exact CLI artifact, stages it at
`resources/cli/codeterm[.exe]`, hashes its bytes, and passes
`CODETERM_SERVER_CLI_SHA256` to the server build. It then builds only
`domios-server --bin domios-server` with
`--no-default-features --features domios-server/embeddings` and the explicit host
target. Its Cargo artifact receipt pins the actual executable and digest; the
matching normal/build dependency closure must exclude Tauri, Wry, WebView and
codeterm-client-shell. Packaging must record the shipped release artifact's
own feature/target closure too. Workspace/test feature unions do not establish
this proof. A server built without the CLI digest refuses actual service startup.
The resource is copied and reverified into a private lifetime lease before memory
initialization; process-local CLI selection never falls back to a GUI executable.
The probe starts a unique disposable named DEV identity beneath split-oracle,
scrubs inherited connection/display selectors, and never connects to PROD.
It waits on protected broker replies, listener-ready events and pinned OS exit
handles, then verifies API/socket and a real disposable PTY after its launcher
exits. A missing CLI resource must refuse before data initialization. The probe
also requires the actual existing maintenance spawn, transferred kernel witness
and separately verified executable/creation provenance; early exit or unavailable
witness fails that row without spawning a substitute. Focused observer checks:
`./build.sh test-one mem process_split_contract`.
The arbitration case races a missing identity and an explicit junction/symlink
path alias (probe-only DTO, both canonical roots must equal normal roots),
requires one initialization audit, and rejects identity/build/protocol/epoch/PID
skew and foreign executable/listener cases. Cleanup requires exact server exit;
uncertain cleanup retains disposable data and fails.

A Windows breakaway refusal means the launching job forbids server detachment.
Record the returned infrastructure failure; never disable the PTY kill-on-close
job or retry behind a deadline. The client launch must originate from a context
that can create the independent server. Report Windows, Linux and macOS native
rows separately. An unrun probe, successful source check or fake fixture is never
a native process PASS. Probe receipts contain no API credential; the credential
travels only over private inherited pipes or authenticated local IPC.

### Human headless start

The server's `start` entry uses the same detached child, protected bootstrap pipes,
readiness acknowledgement and process-provenance check as private `launch`:

```bash
./domios-server/domios-server start --production --json
# Windows: .\domios-server\domios-server.exe start --production --json
```

The common resource root defaults to the executable's parent-parent directory;
`--resources PATH` selects an explicit resource root. The packaged layout contains
`domios-server/`, `cli/`, `dist/`, `skills/` and target runtime resources together.
The CLI digest must already be bound into the server build. A missing or mismatched
resource refuses startup; the entry does not install, relocate data or launch a
native client. Final release staging/signing and actual packaged-start proof remain
required before this source entry establishes a supported release artifact.

A debug artifact instead requires `start --development --port PORT`, optionally
`--instance NAME`. Production and debug selectors cannot cross; inherited launch
selectors are scrubbed by the server before resolving its instance. There is no
implicit selector, credential option, probe option or production test command.
`--json` emits only `ServerReady`; ordinary output shows the token-free frontend
address and process/instance identity. Private `launch`, `serve`, `attach` and
`observe` still require inherited pipes and retain their protected DTOs.

Open the reported address and authenticate through the existing WebGate with the
API credential obtained privately from the server's protected local configuration.
Never copy a credential into an argv, URL, build receipt or console log. WebGate
checks authenticated workspace access; `/manifest-bootstrap` already requires a
bearer and is not first-use pairing. No tokenless remote/browser pairing is provided.
Native enrollment remains the authenticated client attachment contract, independently
pinned to the intended server artifact; human status does not authorize native powers.

Focused pure coverage is `./build.sh test-one domios-server process_split_contract_headless`.
Actual production/debug start, refusal and frontend-authentication proofs are
parent-owned platform rows; source fixtures and compilation do not execute them.

### Diagnose an unchanged-SHA test rebuild

`test-one <scope> <filter>` qualifies the filter with the owning Cargo package.
The shared workspace selection includes every Cargo workspace member and all
test targets, plus catalogue path dependencies; membership metadata is cached
by manifest and lockfile contents. CLI target selection is local to its nextest
invocation. Reverse-dependency scopes prepare frontend and bundle resources
before compiling, including in a fresh checkout.

Orphan lane collection requires absolute lane provenance. Rustc can emit
workspace-relative dep-info (for example `crates/codeterm-orchestration/src/lib.rs`)
without its original cwd; those units are retained until reliable provenance
exists. Never resolve that path against whichever checkout runs GC.

Nextest logs `[test-loop]` phases for gateway, sccache, resource preparation,
listing and Cargo metadata. Record two unchanged canonical runs on BLACKSTAR,
including the complete command wall time; the second must report Cargo Fresh
before reusing metadata. A missing binary, changed manifest or dirty artifact
refreshes the selection. Cache receipts live under the selected Cargo target
and never bypass Cargo freshness validation. No lane timing substitutes for
the canonical before/after measurement.

Run the next focused gateway job with `CODETERM_BUILD_FINGERPRINTS=1` (PowerShell:
`$env:CODETERM_BUILD_FINGERPRINTS='1'` before `./build.ps1 test-one <scope> <selector> --wait-build`).
The gateway retains the Cargo dirty reasons in `jobs/<job-id>.fingerprint.log`;
capped test jobs also write `jobs/<job-id>.artifacts/compiler-inputs.json` after
selecting the actual test target. Compare workspace/build/target directories,
wrapper paths and compiler flags between jobs, then read the trace's dirty
dependency or changed-input reason. The snapshot uses an explicit environment
allowlist and never dumps credentials. Historical job sccache logs can include
concurrent lane requests; matching metadata hashes alone do not explain a rebuild.

### Installed identity and CLI aliases

Production packages are Domios (`com.domios.app`, GUI `domios-app[.exe]`);
DEV is Domios Dev (`com.domios.dev`, GUI `domios-dev-app[.exe]`). The CLI owns
`domios`, with `codeterm` as an alias. Keep staged executable/provenance paths
aligned with `mainBinaryName`. Raw Cargo client artifacts and PDBs retain the
internal `codeterm` target name; staging records final renamed client bytes.
Release staging includes both `cli/domios[.exe]` and `cli/codeterm[.exe]` from
identical signed bytes. Deb/RPM file maps install the same CLI bytes at
`/usr/bin/domios` and `/usr/bin/codeterm`; the GUI stays `/usr/bin/domios-app`.
First launch repairs both user CLI names under `~/.domios/bin`, plus the
`~/.local/bin` links on Unix and the user PATH entry on Windows. macOS and
AppImage keep that link mechanism; the user controls their shell PATH.
Never edit shell dotfiles to install CLI commands. See the release skill for
the required manual move from the former app.
