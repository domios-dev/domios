---
name: build-machine
description: "Provision or restore Domios self-hosted build machines and runners."
---

## Build machine setup

How the Mac and ROG runners produce
signed/notarized macOS, Windows (NSIS), and Linux (`.deb`) artifacts **without GitHub Actions
minutes**, in clean builds.

> Scaffolding lives in [`.codeterm/build-machine/`](../../.codeterm/build-machine) (Dockerfiles +
> orchestrator). The release pipeline itself is the `github-release` skill
> (`.agents/skills/github-release/`). This doc is the from-scratch *setup* record.

## Topology: platform jobs across three runners

The single release workflow starts three platform jobs in parallel. The Mac job overlaps its two
macOS architectures and builds arm64 Linux in Docker; the ROG runners build x86_64 Linux and both
Windows architectures.

| Engine | Where | Builds |
|--------|-------|--------|
| **mac host + Docker** | native Apple Silicon and baked Linux arm64 image | macOS aarch64 + x86_64 (`.dmg` + updater `.app.tar.gz`, Developer ID + notarized); Linux arm64 `.deb` + `.AppImage` |
| **Linux (ROG/WSL2)** | self-hosted runner `release-linux.yml` | Linux x86_64 `.deb` + `.AppImage`, native |
| **Windows (ROG)** | self-hosted runner `release-windows.yml` (RETIRED Mac/QEMU/SSH path) | Windows arm64 + x86_64 NSIS `.exe` |

Builds are **clean**: each engine fresh-clones the tag (via an auth-free `git bundle`), no cargo
target cache leaks. The slow parts (toolchains, system deps) are pre-baked into the Docker images
and the VM disk, so per-build startup is fast.

Orchestrate: `.codeterm/build-machine/scripts/build-all.sh vX.Y.Z`.

---

## Engine 1 — macOS native

The Mac builds and signs macOS artifacts; its local minisign key also signs the arm64 Linux
AppImage.

**Prereqs (once):**
- Xcode + command line tools.
- `rustup target add x86_64-apple-darwin` (for the Intel cross-build).
- **Developer ID Application** cert in the login keychain:
  `security find-identity -v -p codesigning | grep ANDREY` → `Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)`.
  (Obtained by signing into Xcode with the Apple Developer account.)
- Notary keychain profile `codeterm`:
  ```bash
  set -a; source ~/.claude/.secrets/apple-notary.env; set +a
  xcrun notarytool store-credentials codeterm \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD"
  ```
- Minisign updater key at `~/.tauri/codeterm.key` (+ `.pub`) for macOS and arm64 Linux. Windows and
  x86_64 Linux updater bundles use `TAURI_SIGNING_PRIVATE_KEY` from Actions secrets on their runners.

**Secrets — restore from the Bitwarden vault** (`codeterm mem secret get --name <slug>`):

| Vault slug | Restore to | chmod |
|---|---|---|
| `codeterm-apple-notary-env` | `~/.claude/.secrets/apple-notary.env` | 600 |
| `codeterm-tauri-minisign-private-key` | `~/.tauri/codeterm.key` | 600 |
| `codeterm-tauri-minisign-public-key` | `~/.tauri/codeterm.key.pub` | 644 |

(The Developer ID cert is **not** in the vault — it lives only in the login keychain. Back it up
via Keychain Access → export `.p12`.)

**Build:** `.codeterm/build-machine/scripts/build-all.sh vX.Y.Z` runs the Mac arm64 build, starts
its unchanged notarize/staple/spctl verification in the background, and compiles the x86_64 Mac
build while Apple processes the first DMG. Both architectures use `src-tauri/target`,
`CARGO_INCREMENTAL=0`, and the `embeddings` feature set; `release-inputs` prepares both the app
and embed bins in one Cargo invocation before Tauri bundles the already-built graph. Each
architecture still verifies Developer-ID + hardened-runtime + arch, notarizes + staples the DMG,
packs the `.app.tar.gz` updater bundle, and minisign-signs it.
x86_64 goes through `scripts/release-macos-x86_64.sh` (ONNX Runtime 1.23.2 Intel dylib + `CARGO_BUILD_TARGET`
plumbing — see the `macos-x86_64-release` skill).
The x86_64 embedder also bundles LLVM `libc++.1.dylib` from
`CODETERM_MONTEREY_LIBCXX_DIR` so ONNX Runtime does not depend on the newer
system libc++ float `std::to_chars` symbols missing on macOS 12 Monterey.

---

## Engine 2 — Windows (RETIRED: now the ROG self-hosted runner)

The Mac no longer builds Windows. The old path — a headless Windows-11-on-ARM QEMU guest driven
over SSH (`vm.sh`, `~/.ssh/codeterm-win-build`, `WIN_SSH*`/`WIN_VM_SSHPASS`, the
`build-windows.sh`/`setup-win-ssh-key.sh` scripts) — is **gone**. Windows now builds + signs
**natively on the ROG self-hosted runner** (`ROG-windows-build`, label `windows-build`) via
the reusable Windows job in `.github/workflows/release-windows.yml`, called by the single
`.github/workflows/release.yml` run for `release-X.Y.Z`. Runner toolchain and provisioning:
`.codeterm/build-machine/README.md`.

### Pinned cache maintenance and same-job task sessions

If the session function returns an array instead of its session record, check for
unconsumed method results. Suppress the Boolean from candidate-list `Remove` with
`[void]`; PowerShell otherwise includes it in the function output stream.

`archive_refused:unexpected_archive_shape` with the pinned digest can mean the
allowlist omitted the release README/LICENSE. The verified 0.18.0 ZIP contains
the root directory, executable, README.md and LICENSE; only the executable is installed.


If cleanup reports a property-assignment error for the captured server, verify its
record declares `ExitCode` before launch. A PowerShell custom object cannot acquire
an undeclared property through assignment; retain the handle and fail cleanup honestly.


The focused Windows task workflow keeps its existing no-install path by default. Its existing
`rust-windows-task-local` selector continues to inspect the runner's current `sccache` and
requires the build wrapper's existing prestarted-server check. `cache_session_opt_in` defaults
to `false`; only an explicit reviewed dispatch wraps those same fixed task cases in an owned
cache session. That session requires the additive receipt at
`%USERPROFILE%\tools\sccache\0.18.0-x64` and ends in the same job that ran the cases.

The manual `windows-sccache-maintenance` scope supports `contract-only` and
`provision-verify-cleanup`; provisioning remains the default. Both use
`ROG-windows-build` / `X64` as account `amigo`. Dispatch either only after source review of the
exact full source SHA. The `contract-only` branch checks the event, workflow, input and checkout
SHA join, a clean checkout, and the runner/account identity before loading the helper. It compiles
the extracted native C# declarations, then calls only shared pure decisions, the production TCP
table decoder on supplied rows, and synthetic lease/capture state controls. It does not download,
install, query listeners, launch children, create a Job, change cache/tool environment, or write a
fresh artifact receipt. Its one JSON log record has `groups[]` with selected/passed/failed case
records plus root selected/passed/failed/outcome fields; any failed or unselected control exits
nonzero. It never reports a native result. Exact deferred invocations after canonical merge and
Fermi review are:

```powershell
pwsh -NoLogo -NoProfile -NonInteractive -File scripts/windows-sccache-session.ps1 -ContractOnly
```

```sh
gh workflow run tests.yml --ref <full-reviewed-sha> -f evaluator_scope=windows-sccache-maintenance -f expected_source_sha=<full-reviewed-sha> -f maintenance_operation=contract-only
```

The second command is Fermi-owned and is not run in a source lane. Only after its real Windows
source compile and pure controls pass may Fermi separately admit one native maintenance run.
Provisioning installs the official Mozilla
`sccache` `0.18.0` x64 MSVC release asset from
`https://github.com/mozilla/sccache/releases/download/v0.18.0/sccache-v0.18.0-x86_64-pc-windows-msvc.zip`
with the published size `9,694,400` bytes and SHA-256
`8965c74d5e8a225244f741e18ad2f3f504f48228dc1bac948fc22761a348363d`. The release tag resolves
to source commit `01c35e69b97f516c2ebfbdb418b5cd6277bde280`; its tag is unsigned, and the digest
does not independently attest that the binary was built from that source. The operation checks
the downloaded bytes, ZIP entry descriptors, x64 PE identity, measured executable hash, exact
version output, runner/account/SHA joins, and a receipt before using or retaining the versioned
tool. It never overwrites Cargo's `sccache.exe`, another version, runner service state, or global
PATH. Reusing an existing receipt also fetches the same fixed archive, validates the pinned ZIP
and entry shape, then compares the installed executable byte-for-byte with the executable stream
from that verified archive before launch. This is verification only; the opt-in task session does
not install or replace the tool, and the default no-install path is unchanged.

The helper creates a non-breakaway, kill-on-close Windows Job before its first owned executable.
The owning launch registers one direct server lease, starts the pinned executable suspended with
`SCCACHE_START_SERVER=1` and no command, assigns it to that Job, then resumes it. The sccache child
uses the pinned `InternalStartServer` path; on Windows its daemonizer is a no-op. Before launch the
helper creates a random first-instance local named pipe that rejects remote clients and starts its
asynchronous reader. The server receives its path in `SCCACHE_STARTUP_NOTIFY`. The reader bounds the
4-byte big-endian frame and bincode payload to 4 KiB, requires a complete frame followed by EOF, and
accepts only `Ok { addr: "127.0.0.1:4226" }`. It refuses malformed, partial, oversized, error,
address-in-use, timeout, and unknown variants. The connected pipe client's PID must match the
retained direct server handle; PID, creation time, liveness, and `IsProcessInJob` are checked around
the notification. Pipe identity or completion uncertainty is a refusal. The direct server inherits an existing parent console; finite helpers retain `CREATE_NO_WINDOW`.
Process-group and Job ownership remain unchanged. Exact-head native proof must establish the member inventory.

Once notification and direct-process checks complete, the helper queries
`JobObjectBasicAccountingInformation` and `JobObjectBasicProcessIdList` once each. The accounting
API's `ActiveProcesses` can include exited processes until their references are released; the
process-list API separately returns assigned and listed PID counts, with a smaller returned list
indicating incomplete buffer capacity. The direct retained live server lease remains a candidate in
the list; it is not treated as an exited helper. The helper records both API results, errors,
counts, required/returned sizes, PIDs, and query timestamps before readiness classification. It
requires a complete unique PID list (`assigned == listed == returned PID count`) and accepts
accounting only in this range: `listed PID count <= ActiveProcesses <= listed PID count + verified
owned exited launch handles absent from that list`. Each allowance requires a retained launch
handle whose PID and creation time still match, confirmed process exit, and completed captures; all
launch facts and excluded PIDs remain in the receipt. Any query error, incomplete or inconsistent
snapshot, unknown member (including an additional `conhost.exe`), or out-of-range accounting
refuses. See Microsoft's definitions for
[`JOBOBJECT_BASIC_ACCOUNTING_INFORMATION`](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)
and [`JOBOBJECT_BASIC_PROCESS_ID_LIST`](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_process_id_list).

At the same single snapshot boundary, collect handle-backed diagnostic identities for every
candidate PID, capped at 32. Record total, inspected count, truncation/completion, PID from the
retained handle, creation time before and after, image, account SID, Job membership, liveness or
exit observation, and bounded API errors before classifying. Register each candidate lease before
opening its handle and keep every acquired handle through owned Job cleanup. Incomplete or
truncated inventory refuses; diagnostics never choose a winner. Do not capture command lines,
process environments, or output bodies for these candidates.

Exactly one remaining live PID may be the server candidate. A retained process handle must prove
its PID and creation identity, Job membership before and after inspection, liveness, exact image,
hash and account, plus the loopback listener owner. Listener refusal evidence retains at most 32
rows plus the total and a truncation flag. Only after this proof does the helper run bounded
`--show-stats` as diagnostic evidence.
The captured output must be complete and untruncated, with each of the 16 pinned aggregate integer
counters exactly once and zero: `Compile requests`, `Compile requests executed`, `Cache hits`,
`Cache misses`, `Cache timeouts`, `Cache read errors`, `Forced recaches`, `Cache write errors`,
`Cache errors`, `Compilations`, `Compilation failures`, `Non-cacheable compilations`,
`Non-cacheable calls`, `Non-compilation calls`, `Unsupported compiler calls`, and
`Failed distributed compilations`. Parsed output counters are retained in the receipt; nonzero,
absent, malformed, duplicated, or incomplete data refuses the session. Pinned `commands.rs` may print
default counters when the stats connection fails, so these values remain diagnostic and do not prove
connection to the server. Readiness still requires the separate startup/process/Job/account/listener
proof before and after stats.
An occupied endpoint is a refusal; later endpoint reuse never changes shutdown authority.

Cleanup deliberately calls `TerminateJobObject` only through the retained Job handle. It records
whether the server exited naturally or the helper terminated the owned Job, then requires the
captured process exit, Job active-zero completion event and zero accounting, complete output
captures, and closed handles inside the original 30-second cleanup reserve of the 300-second
maintenance bound. Cleanup also cancels and settles the startup pipe reader and closes its pipe and
completion handles. The retained server handle requests `PROCESS_QUERY_LIMITED_INFORMATION` and
`SYNCHRONIZE`; waits classify signal, bounded timeout, and `WAIT_FAILED` with its Win32 error
separately. No `--stop-server` protocol request is sent. Job termination is not a graceful cache
flush and may lose in-flight disposable cache writes. The persistent cache under
`%LOCALAPPDATA%\Mozilla\sccache` is never removed. A fresh run/attempt receipt directory is
uploaded only when the helper created it. Unconfirmed cleanup remains a failed qualification, even
if a termination request succeeded.

The provisioning operation does not run a Rust case and does not leave a daemon for later jobs.
Native success must be reviewed from one separately admitted run at the reviewed full SHA, with
actual archive, PE, process/Job/listener, capture, exit, and handle evidence. Portable contract
controls establish only pure source decisions. The three groups select 123 cases: 14 archive
pin/shape cases, 61 input/ownership/deadline/reuse and actual C# decoder/lease/candidate-inventory/
direct-startup cases, and 48
completion/output/freshness/statistics/rollback and Job membership cases. Membership controls
exercise one candidate, retained exited launches plus one candidate, no candidate, ambiguous or
unverified members, unsettled captures, accounting and PID-list API/incomplete failures, accounting
length failure, and out-of-bound accounting through the production classifier. The independent literal fixture pins the
16-counter zero output schema; fresh-zero statistics remains diagnostic and is not readiness proof.
Candidate-inventory controls exercise two retained diagnostic identities with an ambiguity refusal,
an API-error refusal, and a truncation refusal through the production inventory classifier.
Direct-startup controls use independent literal bytes for the pinned bincode replies and exercise
the production frame decoder/readiness decision, direct live lease classification, foreign sender,
identity/Job/liveness, wrong endpoint, trailing or incomplete frame, reader completion, listener
proof, additional-member, API-error, and incomplete inventory refusals. They establish no native
pipe, Job, process, hash, or listener result.
If candidate validation fails C# compilation on an object-to-IDictionary conversion, keep the
read-only fact predicate on the shared object-based Fact boundary and rerun ContractOnly.
Reuse controls reject invalid receipts, invalid executable identity, or executable bytes that differ
from the verified pinned archive. Capture receipts drop an incomplete leading line when the native
32 KiB tail is truncated, while complete known token forms remain redacted; this is bounded known
pattern handling and does not claim arbitrary-secret detection. Rollback controls exercise the same
production decision as cleanup and require confirmed process, capture, Job and handle settlement with
no cleanup errors. After canonical merge and review, request:

```sh
actionlint .github/workflows/tests.yml
scripts/test-capped.sh --exec pwsh -NoLogo -NoProfile -NonInteractive -File scripts/windows-sccache-session.ps1 -ContractOnly
```

PowerShell 7 must already be available on the canonical host; do not install it to run the pure
controls. If the additive install needs removal before any task tests are admitted, the exact
receipt-owned executable and receipt may be removed only after the owned Job, process handles,
captures, and files are confirmed settled. Keep the cache directory and unrelated tools. For a
successful install, removal is a separate reviewed uninstall action, not part of maintenance.

---

## Engine 3 — Linux via Docker (arm64 native; x86_64 on ROG/WSL2)

Two baked images in [`.codeterm/build-machine/docker/`](../../.codeterm/build-machine/docker):

- **`Dockerfile.linux-arm64`** — `rust:1-trixie` + webkit2gtk-4.1 / gtk3 / etc. dev libs + Node 22
  + pnpm + Rust 1.95. Native aarch64 build → `.deb` (+ AppImage).
- **`Dockerfile.linux-x64`** — retained for ad-hoc cross-builds. Release x86_64 Linux builds
  natively on the ROG's WSL2 Ubuntu runner because the x86_64 AppImage tool cannot run on arm64.

```bash
cd .codeterm/build-machine/docker
docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .
docker build -t codeterm-build-linux-x64   -f Dockerfile.linux-x64   .
```

Run: `build-linux.sh <tag> [bundles]` inside the image for arm64 release builds or ad-hoc
cross-builds (clone tag → `pnpm install` → `tauri build` → collect). **Docker Desktop memory must
be ≳20 GiB** — the `codeterm` lib crate needs ~8 GiB to compile and the OOM killer otherwise
SIGKILLs `rustc`.

### Why these exact choices (hard-won)

- **Release x86_64 Linux runs natively on the ROG's WSL2 runner.** The retained Docker x64 image
  uses native arm64 `rustc` plus a cross-linker for ad-hoc `.deb` builds; QEMU user-mode amd64
  (`docker --platform linux/amd64` / `cross`) **segfaults `rustc`** on an arm64 host.
- **System multiarch libs, not a hand-built sysroot.** Extracting `.deb`s into a `/sysroot` misses
  the usrmerge `/lib`→`/usr/lib`, `/lib64/ld-linux-x86-64.so.2`, and transitive `.pc` chain →
  endless "cannot find … inside /sysroot" / "gdk-3.0 not found" whack-a-mole. Installing the amd64
  `-dev` libs into the real system (with `--force-overwrite` past multiarch arch-indep file
  conflicts) gives genuine system paths and resolves cleanly.
- **Cross builds need embed/cli stubs pre-created** (`touch bundled-embed/codeterm-embed-<triple>`,
  `bundled-cli/codeterm`) before `tauri build`, or its build-script glob validation aborts.
- **x86_64 AppImage cannot be built on an arm64 host** — `linuxdeploy-x86_64` is an x86_64 binary
  that can't run here. x86_64 → `.deb` only; AppImage only natively (arm64) or on a real x86_64 box.
  Hence `latest.json`'s `linux-x86_64` (an AppImage URL) needs a real x86_64 Linux runner.
- **Deb and AppImage need separate updater manifests** — Tauri's Linux platform key is only
  `linux-x86_64`, but a deb-installed app rejects AppImage bytes as `invalid updater binary
  format`. Keep AppImage in `latest.json`; publish deb packages through `latest-deb.json`.
- **AppImage in Docker** needs `APPIMAGE_EXTRACT_AND_RUN=1` (no FUSE in containers).

### Fixed Linux Storage foundation qualification

The manual `storage-foundation-linux` scope of `tests.yml` retains its standalone foundation check;
Blueprint-only scopes never count toward it. On `[self-hosted, linux-build]` it joins
`expected_source_sha` to the workflow, event and checkout SHA, refuses a dirty checkout, `.env`,
ambient target or test-command overrides and `CODETERM_*` bindings other than the embed cache, and
refuses unless the tracked Storage facade, both test files and crate equal reviewed
`dbc17edee5c1c9032c1498caa0f04b00eb3468ad`. It then runs one foreground
`./build.sh test-one app storage_contract_` (600-second cap, one test thread) through the existing
fail-fast gateway. The `storage-foundation-linux-<run>-<attempt>-<sha>` artifact keeps the raw log,
raw exit, UTC times, input hashes and `receipt.json`. `outcome: qualified` requires the 23 named
cases once each `ok`, one `23 passed; 0 failed; 0 ignored; 0 measured` summary, raw exit 0 and a
clean checkout after stub cleanup; `command.phase` separates compile from execution failure. Only
Fermi dispatches it, once, after a real merge and canonical structural review of the exact
published SHA; a source lane never runs it, and a busy gateway or uncertain dispatch is reported,
never retried:

```sh
gh workflow run tests.yml --repo rollacode/codeterm --ref <reviewed-published-ref> -f evaluator_scope=storage-foundation-linux -f expected_source_sha=<same-full-reviewed-sha>
```

The combined `storage-domains-linux` scope runs Cron V2 (`storage_cron_`, 28 selected: 25 `ok` and
three `ignored` desktop-runtime controls), Blueprint V3 (`storage_sentinel_`, 17 `ok`), then the
accepted Storage foundation (`storage_contract_`, 23 `ok`) in exactly this foreground order:
`TEST_CAP_SECS=600 RUST_TEST_THREADS=1 CARGO_TERM_COLOR=never ./build.sh test-one app storage_cron_`,
then the same fixed command with `storage_sentinel_`, then with `storage_contract_`. Each selected run
requires its exact full-name/status inventory once, one successful matching summary with zero failed
and measured, and raw exit 0; the three ignored Cron rows must say `requires desktop runtime` and are
not passes. Compilation command wall time and libtest-reported time are recorded separately. The
first raw failure or inventory refusal leaves later commands `not_run`. It joins the exact expected,
workflow, event, and checkout SHA, pins the complete combined
domain input tree to `bae48571f3ec780816aa05b0d47c2289755cc029` (with its one Storage `assets.rs`
change, four reviewed Blueprint probe inputs and the test-only Blueprint `handlers.rs` wrapper removal
since `3972c8bd13bbf666e4470a3fcf6a51c6d67f5447`),
and independently compares unchanged foundation inputs to `dbc17edee5c1c9032c1498caa0f04b00eb3468ad`.
The changed shared Storage registration/schema files and added Cron/Blueprint repository/schema files
are joined to the combined anchor; they cannot equal the older whole-crate foundation tree, whose
existing check remains unchanged. Fresh `storage-domains-linux-<run>-<attempt>` evidence retains a
bounded receipt and full raw logs. The counts are source expectations until the primary Orchestrator
reviews and admits one canonical Linux dispatch at the exact published SHA. The route runs only
isolated synthetic tests and does not import, select, or migrate user JSON/SQL state. Dispatch:

```sh
gh workflow run tests.yml --repo rollacode/codeterm --ref <reviewed-published-ref> -f evaluator_scope=storage-domains-linux -f expected_source_sha=<same-full-reviewed-sha>
```

---

## Release pipeline (tying it together)

Driven by the `github-release` skill. Summary:

1. Bump version on master and push the single `release-X.Y.Z` trigger tag.
2. One `release.yml` run prepares draft releases, then starts Mac + arm64 Linux, x86_64 Linux,
   and Windows platform jobs in parallel.
3. macOS builds signed + notarized DMGs and `.app.tar.gz` updater bundles; Linux builds AppImage
   and Debian packages; Windows builds signed NSIS installers.
4. Each platform job uploads its source artifacts and mirrors large assets. The final manifest job
   waits for every platform job, merges the updater fragments, publishes signed manifests to the
   public **`rollacode/codeterm-releases`** mirror, marks releases, and verifies served bytes.
5. `latest.json` keeps the Tauri updater platform records; `latest-deb.json` remains separate for
   Debian-installed Linux. Verification checks every shipped updater URL before issue closure.

## Tag-driven CI (implemented) + persistence

This is live as of v1.4.5. Pushing a `release-X.Y.Z` tag starts one `.github/workflows/release.yml`
run with parallel self-hosted platform jobs and a final manifest verifier (see the `github-release`
skill "Default release path"). Self-hosted runner minutes are free.

For the machine to survive reboots / crashes without anyone babysitting a terminal, the three
engines must each be self-starting:

1. **Runner — launchd service.** Register, then install as a service (auto-start on login +
   auto-restart on crash):
   ```bash
   cd ~/actions-runner
   TOKEN=$(gh api -X POST repos/rollacode/codeterm/actions/runners/registration-token --jq .token)
   ./config.sh --url https://github.com/rollacode/codeterm --token "$TOKEN" \
     --name codeterm-builder --labels codeterm-builder,rust-shard --unattended --replace
   ./svc.sh install && ./svc.sh start        # launchd LaunchAgent, RunAtLoad=true
   ```
   (Runner registration creates a persistent code-execution surface with access to keychain/vault/VM
   — a human action; keep the repo private and never run fork PRs on it.)

2. **Docker — workflow self-heals it.** The release workflow has an "Ensure Docker is running" step
   (`open -a Docker` + wait) so a down/just-rebooted Docker is started before the Linux engines.
   Optionally also enable Docker Desktop → Settings → General → "Start Docker Desktop when you sign
   in". Docker memory ≥ 11 GiB with `CARGO_BUILD_JOBS=2` (set in the images), or ≥ 20 GiB for `=6`.

3. **Windows + x86_64 Linux — their own ROG runners.** The single release run calls the reusable
   Windows job (`release-windows.yml`) and x86_64 Linux job (`release-linux.yml`) on the ROG's
   self-hosted runners. No QEMU VM, no SSH, no `WIN_VM_SSHPASS`; each platform job builds and signs
   its artifacts before the final manifest job publishes the release. Provisioning:
   `.codeterm/build-machine/README.md`.

## Redeploying on a fresh Mac (checklist)

1. Clone the repo; `brew install pnpm node gh docker` (+ Xcode).
2. Restore secrets from the vault (table above) + import the Developer ID `.p12` into the keychain;
   `xcrun notarytool store-credentials codeterm ...`.
3. `docker build` the arm64 Linux image — Engine 3 above.
4. Register + service-install the Mac runner — Engine 1 above.
5. Windows + x86_64 Linux build on the ROG runners (`release-windows.yml` / `release-linux.yml`);
   provision those separately per `.codeterm/build-machine/README.md` — nothing to do on the Mac.
6. Push a `release-0.0.0`-style test tag to validate plumbing, then a real `release-X.Y.Z`.

---

## Windows Runner Bootstrap

This runbook restores the ROG Windows GitHub Actions runner after a clean Windows install or OS wipe. It keeps secrets external, installs non-secret tools into user-writable paths, writes only the runner-local `actions-runner/.env`, and can register the runner with an ephemeral token supplied at runtime.

## Operator Quickstart

1. Open PowerShell as the runner user.
2. Install GitHub CLI or Git if missing enough to fetch this repository.
3. Get a one-use GitHub Actions runner registration token outside the script. Do not save it in files or paste it into logs.
4. Run the bootstrap from the repository checkout:

```powershell
.\scripts\windows-runner-bootstrap.ps1 `
  -InstallVisualStudioBuildTools `
  -ConfigureRunner `
  -Replace `
  -RegistrationTokenCommand 'codeterm paste get paste:REPLACE_ME --pop' `
  -StartListener
```

For service mode, use an elevated shell and add `-RunAsService`. Without elevation, keep the interactive listener running. Service installation is the only supported persistent background mode; do not invent a separate scheduler around `run.cmd`.

## What The Script Installs

User-writable tools:

- Official GitHub Actions runner `win-x64`, under `%USERPROFILE%\actions-runner`.
- Official PowerShell `win-x64` zip, under `%USERPROFILE%\tools\PowerShell\<version>`.
- Official Node.js active LTS `win-x64` zip, under `%USERPROFILE%\tools\Node\<version>`.
- Rust via official `https://win.rustup.rs/x86_64`, with stable MSVC and the repository-pinned `1.95.0-x86_64-pc-windows-msvc` toolchain.
- `pnpm` at the repository-pinned version, plus command shims under `%USERPROFILE%\tools\runner-shims`.
- Visual Studio Build Tools C++ workload and Windows SDK when `-InstallVisualStudioBuildTools` is provided from an elevated shell.

Runner-local environment:

- `%USERPROFILE%\actions-runner\.env` prepends shims, Node, PowerShell, durable Rust proxies, and Cargo before machine/user PATH.
- The `.env` file is runner-local and is the persistent mechanism that makes the Actions shell see `pwsh`, `rustup`, `cargo`, `rustc`, `node`, `npm`, `corepack`, `pnpm`, and `git`.
- Durable Rust proxies live under `%USERPROFILE%\tools\rustup-proxies`, outside Cargo's cache path.

## Incident Record

Direct-server maintenance at `ba3f4ffb1` authenticated the startup notification but still observed `sccache.exe` plus `conhost.exe` and refused readiness. Cleanup confirmed active zero and closed handles. The cache-only direct launch now uses `DETACHED_PROCESS`; finite helper launches retain `CREATE_NO_WINDOW`. CI37132916446 at758234 verified one server member, startup identity/listener and complete44ms owned cleanup. The exact-one rule and original cleanup deadline remain unchanged.

| Symptom | Root Cause | Automated check/fix | Manual escalation boundary |
| --- | --- | --- | --- |
| No runner existed after OS wipe; GitHub had no online Windows builder. | Runner install and registration state was lost with the OS. | Download official `actions/runner` `win-x64`, configure with repo URL, name, labels, and caller-supplied one-use token. | If service mode is required, rerun elevated with `-RunAsService`; otherwise keep `run.cmd` listener alive. |
| Runner registration and `.env` are intact, but GitHub reports it offline after a Domios restart. | A per-user Startup launcher owned `run.cmd`; no official runner service or `Runner.Listener` survived the restart. | Preserve `.runner`, remove the Startup fallback, and install/start the bundled official `svc.cmd` service. Verify the service account resolves every `.env` tool and GitHub returns online/idle after a service-only restart. | Service installation requires elevation. Stop and request it rather than adding another scheduler or silently relying on an interactive listener. |
| Orphaned OpenConsole installer lock appeared during recovery. | Unrelated installer state from Windows/app setup, not a GitHub runner issue. | Bootstrap does not touch OpenConsole; it reports only runner prerequisites. | Clear the installer lock separately if it blocks OS/package maintenance. Do not couple it to runner registration. |
| `pwsh: command not found` in `Ensure Rust toolchain` and cleanup. | Clean runner PATH had no PowerShell 7. | Install official PowerShell zip and prepend its directory in `actions-runner/.env`. | If PowerShell download is blocked, install PowerShell 7 manually and add its directory to `.env`. |
| `rustup.exe not found`; no `cargo` or `rustc`. | Clean runner account had no Rust toolchain. | Install rustup stable MSVC, preinstall pinned `1.95.0`, clippy/rustfmt, and Windows MSVC targets; prepend `%USERPROFILE%\.cargo\bin`. | If network blocks rustup, mirror the official rustup installer and toolchain artifacts internally. |
| ARM64 matrix job loses `rustup.exe` after x64 passes. | `Swatinem/rust-cache` post-job cleanup can remove `%USERPROFILE%\.cargo\bin` between sequential matrix jobs. | Bootstrap copies Rust proxies to `%USERPROFILE%\tools\rustup-proxies` and prepends that path before Cargo in the runner `.env`. | Restore proxies with `rustup-init` only if both the Cargo and durable proxy directories are absent. |
| Focused Windows task gate stops before tests because `sccache` is not on PATH. | Rust proxy resolution does not establish the separate Cargo-bin cache-tool location. | Default dispatch remains no-install: it preserves an existing PATH application or resolves one file only from current `CARGO_HOME/bin` and `USERPROFILE/.cargo/bin`, records its hash and unchanged other-tool paths, and requires the existing prestarted cache server. A separately reviewed opt-in uses the pinned receipt and owns the cache server through the same fixed task loop. | Missing or ambiguous files, an invalid receipt, changed tool resolution, or a foreign endpoint are refusals. Additive installation is admitted only through the exact manual maintenance operation; do not start an ad hoc server, broaden discovery, or count preflight as a test. |
| `node` missing in `Prepare compile stubs`. | Clean runner PATH had no Node.js. | Install official Node LTS zip, prepend it in `.env`, install `pnpm`, and add shims so PowerShell does not pick blocked `.ps1` launchers. | If Node download is blocked, install supported Node LTS manually and add it to `.env`. |
| `link.exe` missing in Rust lib tests. | Visual Studio Build Tools and/or Windows SDK were absent. | Bootstrap detects `vswhere`, `vcvarsall.bat`, and Windows SDK inventory; with `-InstallVisualStudioBuildTools` in an elevated shell it installs Build Tools C++ workload, ARM64 tools, and Windows SDK, then writes MSVC `PATH`, `INCLUDE`, and `LIB` into runner `.env`. | If elevation/UAC is unavailable, install Visual Studio Build Tools manually with C++ tools and Windows SDK, then re-run bootstrap. |

## CI job-local Bash path repair

If Rust setup reports Bash is missing although checkout resolves Git, the service
PATH may expose Git's cmd directory without its bin directory. The Rust test
and platform workflows run scripts/ci-runner-paths.mjs before composite actions:
it verifies Bash from standard Git installs or the inherited Git path, appends
that directory and existing account-local Rust proxies to GITHUB_PATH, and
fails clearly if Bash is absent. This changes only the current job environment.
Restore Git for Windows on the runner if preflight refuses; do not remove the
runner from the shared pool or replace its service/global PATH to hide failure.

## CI test classification

CI sets `CODETERM_SCCACHE_VERIFY_ONLY=1`, so build wrappers verify the running
shared cache before `./build.sh ci-tests` compiles; provisioning belongs to idle setup.

If platform classification rejects a generated `include!`, the classifier needs
the package's exact Cargo `build-script-executed` OUT_DIR. `ci-rust-tests.mjs`
compiles with Cargo JSON and passes that directory to the classifier, which traverses
the generated Rust source and refuses missing or unsupported include expressions.

## Verification Checklist

Run this after bootstrap:

```powershell
.\scripts\windows-runner-bootstrap.ps1 -VerifyOnly
```

The output must prove command resolution from the exact runner `.env` PATH:

- `pwsh -> %USERPROFILE%\tools\PowerShell\<version>\pwsh.exe`
- `rustup`, `cargo`, `rustc -> %USERPROFILE%\tools\rustup-proxies\*.exe`
- `node -> %USERPROFILE%\tools\Node\<version>\node.exe`
- `npm`, `corepack`, `pnpm -> %USERPROFILE%\tools\runner-shims\*.cmd`
- `git ->` installed Git command path
- MSVC inventory has `VisualStudioBuildTools: true` and a `vcvarsall.bat`
- Windows SDK inventory has `WindowsSdk: true`
- WebView2 inventory reports an installed Evergreen runtime when release builds need it
- the official runner service is installed and running under the intended runner account; no Startup `run.cmd` fallback remains
- GitHub reports the registered runner online and idle after restarting only that service

Registration labels include `windows-ci` (Windows platform subset) and, for full-suite pool members, `rust-tests`, in addition to packaging labels; preserve OS labels for native cfg tests. See `.codeterm/build-machine/README.md` for the shared pool and coverage contract.

Then dispatch `tests.yml` on a reviewed `ci/` branch. A restored builder must pass its Windows platform job and, if it carries `rust-tests`, the full suite; inspect the `rust test receipts` summary for runner identities, executed counts and durations before release workflows are trusted.

## Secret Handling

Registration tokens, signing keys, package tokens, and Tauri signing passwords stay outside the repository and outside this script. Pass runner registration through `-RegistrationTokenCommand` so the token can be fetched from an ephemeral external source at runtime. The script never writes the token to `.env`, docs, or logs.

Reuse can falsely report `installed_executable_differs_from_pinned_archive` when comparing a ZIP entry stream's unsupported `Length` property. Compare the entry descriptor length, then drain and compare both streams. Three in-memory compressed ZIP controls cover equality across buffer boundaries, changed bytes and changed length; never replace installed bytes to bypass refusal.

<!-- revision: 3 -->

Detached server startup passed the single-member maintenance oracle but exposed compiler windows during
Lookout compilation: pinned sccache creates ordinary compiler children, which lack a console to inherit.
The direct server now requires a live parent console and inherits it; startup records the parent code page
and confirms server membership with GetConsoleProcessList. Missing console or incomplete membership
refuses before task tests. No new console, attach, desktop switch, global window hiding or PTY change is used.
Pure controls prove flags/refusal only; actual compilation and absence of new windows require native proof.

Windows-only Blueprint test compilation can fail on `System::IO::ReadFile`: windows-sys0.61 declares
`ReadFile` in `Storage::FileSystem`, with the existing IO feature for OVERLAPPED. Correct the import;
stop downstream test gates when compilation fails, and qualify the two focused selectors after the fix.

ContractOnly can report `native_control_selection_count_mismatch` after adding native controls while
leaving its independent expected inventory stale. Match the authored group inventory (21 startup controls)
and rerun the exact-head pure gate; never remove the count guard or count its refusal as a behavior failure.

Windows Blueprint native probes can fail immediately with `probe_capture_or_completion_failed` when Job
completion fields are reversed: transferred bytes carry the Job message, while OVERLAPPED carries the PID.
ACTIVE_PROCESS_ZERO legitimately carries a null PID. Decode Job packets before rejecting null capture
operations; failed dequeues with null OVERLAPPED still refuse. Re-run the fixed 11/4 Windows boundary.
