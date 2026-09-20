---
name: build
description: "Choose and run CodeTerm build operations, including dev work, local installs, and troubleshooting."
---

# CodeTerm — Build Guide

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

These flows are deliberately separate. Run every build, test, dev instance, and
executable gate from the warm canonical checkout under Orchestrator authority.
Isolated Worker lanes are source-only.

| Goal | Entry point | Touches production? | Read first |
|---|---|---:|---|
| Develop and inspect changes | `./build.sh dev` / `.\build.ps1 dev` | No; port 7686 only | `.agents/skills/dev-run/SKILL.md` |
| Compile both Rust test profiles for one source-only lane (isolated per-task test-profile gate) | `./build.sh check-tests` / `.\build.ps1 check-tests` | No; isolated target, never the build gateway | `.agents/skills/gates/SKILL.md` |
| Build and test a local production installer | `./build.sh release-preflight`, then `./build.sh build` | Only the explicit self-install step | this section |
| Publish to all users | PR to `master`, then `release-X.Y.Z` tag | Yes; CI and public updater mirror | `.agents/skills/github-release/SKILL.md` |

Never use a dev build as a local-install artifact, and never turn a local
installer build into a public release by uploading it manually.

## Generated artifacts and the embedded-doc boundary

Two classes of document exist. `prod-docs/<NAME>.md` files are embedded in the
binary and read by production agents through the docs registry; everything else
lives in `essences/` and never ships. `prod-docs/` holds only embedded
documents, and every registered name maps to a same-named file — both directions
are enforced by tests.

A generated artifact is produced from a source of truth and is never
hand-edited. Regenerate it from its source in the same lane and commit it with
the source change before handoff, so no consumer ever sees a stale artifact.

| Source of truth | Generated artifact | Regenerate (canonical) | Verify |
|---|---|---|---|
| CLI clap tree in `cli/src/args/` | `essences/cli-manifest.json`, the generated block in `essences/cli.md`, `packages/chat-ui/src/codeterm/commandPaths.ts` | `./build.sh cli`, or `cli-manifest --write` run by the CLI built from this checkout | `./build.sh test-one cli cmd::manifest` |
| `essences/capabilities.md` and `essences/core.md` | `essences/map.jsonl` | `codeterm essence-map generate --root .` | `codeterm essence-map validate --root .`; `./build.sh test-one cli essence_map_contract_committed_artifacts_are_fresh` |
| `src-tauri/prompts/**` and their loader consts | the `prod-docs/PROMPTS.md` inventory | edit the inventory row by hand | `bash scripts/check-prompts-doc.sh` |
| Registered public routes in `src-tauri/src/api/routes.rs` | `prod-docs/API.md` | edit the API index by hand | `bash scripts/check-api-docs.sh` |
| A `prod-docs/<NAME>.md` document | its embedded copy, through the docs registry in `cli/src/cmd/docs.rs` | register the document in `DOCS`, or exclude it with a reason | `./build.sh test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary` |

An isolated Worker lane is source-only: it does not run a build or a gate. When
the generator needs the checkout's built CLI or any verification, the lane
reports the exact command under GATES REQUESTED and the Orchestrator regenerates
and verifies it in canonical before the merge. Never hand-edit a generated
artifact, and never regenerate one with the installed `codeterm` on `PATH`.

## Local production build and self-install

Use this only when testing a production-mode build on this Windows machine. It
does not publish a release or alter GitHub assets.

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

   This route is Windows-only, requires the clean canonical checkout, reuses a
   persistent release target with sccache, rebuilds only changed Cargo units,
   recreates the NSIS bundle, verifies source provenance, and installs that
   exact artifact. When invoked inside CodeTerm, it saves the session and sends
   a continuation intent to the calling Tab after the new daemon is healthy.
   The default intent verifies the installed SHA and resumes the interrupted
   work. Override or disable it when needed:

   ```powershell
   .\build.ps1 install-local --continue-prompt "Verify the installed build, then resume release QA."
   .\build.ps1 install-local --continue-tab "<tab-id>" --continue-prompt "Continue the interrupted task."
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
   when `CODETERM_TAB_ID` is present; use `--no-continue` only when interruption
   is intentional.
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
   current-user installed CodeTerm, runs NSIS, and restarts it.

### Repairing a corrupted user PATH (Windows)

CodeTerm adds only its own `bin/` directory to `HKCU\Environment` `Path`, and
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
| Local production build reports `TS2304: Cannot find name 'RefreshCw'` in the mesh peer panel | The shared busy spinner consumed the icon without importing it | Import the existing `RefreshCw` icon from `lucide-react`; rerun the owning production build before self-install. |
| `release-preflight` reports TS2347 on OpenCode `sqliteQuery<T>` calls or TS6133 for imports in `provider-opencode/src/plugin.ts` | The helper declared the injected SDK host as `any`, so its generic query signature was unavailable, and obsolete helper imports remained | Type the injected host as the plugin SDK `Host`, remove unused imports, regenerate `plugins/provider-opencode/plugin.js` with `node scripts/build-plugin.mjs provider-opencode`, and commit the source and bundle together before rerunning preflight. |
| Release PR `comment-length` check lists changed source files | The release diff contains inline `//` comment blocks of three or more lines | Reduce each flagged block to two lines without losing its invariant, then rerun `scripts/check-comment-length.sh --fail --changed <changed-files.txt>` and PR checks. Get the explicit approval required by `AGENTS.md` before editing PTY delivery logic. |
| `registered_documents_have_one_title_and_fit_the_reading_boundary` rejects a CLI document | A contract addition exceeds the 240-line reading boundary | Integrate the clarification into its owning contract section and remove duplicate explanation, or move a separate concern to its linked reference. Run the focused CLI document test before the full preflight. |
| The CLI docs test reports `API exceeds 24 KiB` | The API index repeats adoption details already owned by `API-ADOPT.md` | Keep `API.md` to the endpoint summary and move the full contract to `API-ADOPT.md`; bump the production doc revision and run `build.ps1 test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary`. Never raise the 24 KiB limit. |
| The CLI docs test reports `API-TABS exceeds 24 KiB` | The tab API's notification contract repeats channel policy wording | Preserve every contract statement while compacting redundant prose; move a passage to a registered linked doc only if it cannot fit. Bump the production doc revision and rerun the focused docs test; never raise the 24 KiB limit. |
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
| PR `source-of-truth` reports prompt files missing from `prod-docs/PROMPTS.md` | A new runtime or frozen-reference Markdown file was added under `src-tauri/prompts/` without registering its constant and ownership in the prompt inventory | Add every reported path to the prompt table, explain any frozen never-edit contract, bump the doc revision, then run `bash scripts/check-prompts-doc.sh` before pushing. |
| `Release lock version mismatch` during a version bump | An older preflight checked lock versions before Cargo refreshed the workspace packages | Use the current `release-preflight`, which checks both manifests before verifying both lockfiles; commit the refreshed lockfiles, then retry. |
| A direct `npm run` or `pnpm build` reports `'node' is not recognized` while `node --version` works | The package manager selected `cmd.exe` for lifecycle scripts in a shell whose effective command lookup differs from the parent PowerShell session | Use the named build operation (`build.ps1 frontend` for the production UI gate). For standalone version validation, run `npm run check:versions --script-shell pwsh.exe`. |
| `release-preflight` reports `'rustc' is not recognized` from `prepare-embed-stub.js` even though Git Bash finds rustc | Native Node launched `cmd.exe` with Git Bash's colon-delimited PATH, so shell lookup could not resolve Cargo tools | Update the stub generator to invoke rustc directly from `CARGO_HOME`/the user Cargo directory, then rerun `release-preflight`. Do not rewrite PATH or bypass the stub. |
| `release source is not clean` | A source or lockfile change has not been reviewed/committed | Inspect `git status --short`; commit the intentional change or stop. Never bypass provenance. |
| `source SHA/status mismatch` after a bundle was created | A build step changed tracked source inputs, or rewrote an identical LF-only manifest and left Git's Windows stat cache reporting a false name-only change | Do not install when the patch is non-empty. Current provenance hashes the binary patch plus working-tree bytes, so a metadata-only rewrite is accepted; rerun `install-local` from a clean tree. |
| Self-install logs `starting` and the window never returns | A helper inherited the calling PTY Job and died when `taskkill` closed that Job | Rebuild the current CLI; it creates the helper through the out-of-job Windows process broker. Re-run `install-local`, inspect `%LOCALAPPDATA%\codeterm\self-install.log`, and verify port 7685 health. |
| CodeTerm restarts but the initiating tab does not resume | The continuation send failed or the Tab id changed before session persistence | Inspect `%LOCALAPPDATA%\codeterm\self-install.log`; the retained `%TEMP%\codeterm-continuation-*.txt` contains the undelivered intent. |
| Self-install succeeds but gateway history reports `owner_lost` | An older build script kept the gateway owned until after the installer restarted CodeTerm and killed the calling Tab | Update the build scripts and run `build.ps1 install-local`; the current operation releases the verified build before detached installer handoff. Prove installation separately with `self-install.log`, port 7685 health, and `codeterm identify`. |
| Repeated local install recompiles the full release graph | The clean-room `build` route was used instead of the persistent local cache | Use `build.ps1 install-local`; reserve `build` for public-release-equivalent verification. |
| Local install cannot read Tauri permissions under a deleted release directory | A relocated Cargo target retained absolute build-script `root-output` paths. | `install-local` now detects mismatched output roots before Cargo, preserves that exact cache in a sibling `.relocated-*` directory, and regenerates its local target; it leaves current warm caches and the machine sccache intact. |
| A focused Rust test spends most of its runtime linking app binaries | The test omitted `--lib`, so Cargo built unrelated binary test targets | Run the focused filter through `scripts/test-capped.sh ... --lib <filter>`. |
| PowerShell drops the standalone `--` before `--nocapture`, so Cargo rejects `--nocapture` as its own option | Native argument forwarding consumed the test-harness separator before Git Bash received it | Run the exact wrapper command through `bash -lc 'scripts/test-capped.sh ... <filter> -- --nocapture'` so the separator reaches Cargo's test binary. |
| `verify` fails `daemon_windows_spawns_use_no_window_helper` after adding a Rust test fixture that invokes Git | The fixture constructed `std::process::Command` directly, so Windows could flash a console and the repository spawn audit rejected it | Build the command in a mutable variable and call `crate::process_util::no_window_std(&mut command)` before execution; run the spawn audit and the owning focused test, then retry `verify`. |
| A fresh lane's focused Rust test reports missing `../dist` or `bundled-cli/*` | Ignored Tauri resource directories have not been staged in that worktree | Run `node scripts/dev-resource-preflight.mjs` and `node scripts/prepare-cli-stub.js`, then rerun the same focused test. Do not substitute a frontend build. |
| `sccache: incremental compilation is prohibited` | `CARGO_INCREMENTAL=1` was combined with the shared sccache wrapper | Keep `CARGO_INCREMENTAL=0`; reuse the persistent completed target and sccache entries instead. |
| Release-signing negative test passes despite a missing fixture key | The shell inherited `TAURI_SIGNING_PRIVATE_KEY` from the canonical `.env` | Run the repository check; its missing-key fixture explicitly removes the inherited key before asserting failure. |
| Push unexpectedly starts compilation or formatting checks | A stale local hook predates the non-blocking push policy | Run any `build.ps1` command to restore `.githooks`, then retry; verification belongs to explicit gates and CI. |
| `Build gateway job id already exists` immediately from a fresh command | The calling CodeTerm tab inherited a completed gateway job id from an earlier build | Update the build scripts; production gateway entry now always creates a fresh job id and ignores inherited internal ids. |
| `pnpm build` fails only in focused TSX test files during `install-local` | A test mock violates the generic bridge signature or imports an unused JSX runtime symbol | Run `pnpm build`, fix the test type/import at the source, then retry `install-local` from the same clean canonical checkout. |
| `pnpm build` reports stale role literals or duplicate fixture fields during `install-local` | A role-model refactor left TypeScript-only production or fixture surfaces outside the Rust gate | Run `pnpm build`, converge each surface on `Manager` plus `role_profile`, and retry `install-local` from a clean canonical checkout. |
| `install-local` reports `tsc is not recognized` although `node_modules/.bin/tsc` exists | Git Bash supplied the MSVC environment to Tauri, but pnpm selected a nested Windows lifecycle shell that lost the local-bin lookup | Use the current build operation, which pins pnpm lifecycle scripts to Git Bash and supplies frontend provenance without a nested shell lookup, then retry `install-local`. |
| Tauri `beforeBuildCommand` reports `pnpm is not recognized` during `install-local` | Tauri launched the configured hook through `cmd.exe` with Git Bash's POSIX-formatted `PATH`, so Windows could not resolve pnpm, Node, or Cargo | Use the current build operation, which prepares frontend, CLI, and embed inputs directly before invoking Tauri; retry `install-local` from canonical. |
| Installed health on port 7685 reports a dev identifier or Vite/CDP ports | The detached installer helper inherited `CODETERM_*` and Cargo build variables from the calling tab | Use the current installer helper, which scrubs CodeTerm and build variables before launching the installed daemon; reinstall and verify `/api/health`. |
| NSIS reports `Error opening file for writing` for `conpty\<arch>\OpenConsole.exe` | A bundled ConPTY host from the installed CodeTerm instance survived PTY teardown and still holds its executable | Current builds reap path-matched owned hosts before NSIS. On an older build, verify the process image is under `%LOCALAPPDATA%\CodeTerm\conpty\`, end only that `OpenConsole.exe`, then click **Retry**; never end `conhost.exe` or an `OpenConsole.exe` from another path. |
| Windows self-install relaunches but continuation reports an incompatible CLI, or x64 startup installs an ARM64 `~/.codeterm/bin/codeterm.exe` | Parallel Windows release targets shared the generic `bundled-cli/codeterm.exe` resource, allowing one architecture to enter the other installer | Rebuild with target-qualified CLI resources. Startup now rejects a PE whose machine type differs from the app, preserving the prior working shim instead of overwriting it. |
| `test-cli-target-dir-paths.sh` reports an extra `MSVC env loaded` line or rejects a foreign cross-target before an otherwise valid path probe | Windows build initialization writes diagnostics and validates target toolchains before the probe | Use the current contract test, which selects a host-compatible fixture target and compares the final probe line while `pipefail` preserves failures. |
| Installed CodeTerm exits during startup with a missing `tab_id`/`tabId` field or `delayed_send init` panic | A pane-era or malformed persisted record reached an all-or-nothing startup deserializer | Rebuild and install the current revision, then restart. Inspect `[plan.store]`, `[delayed_send]`, `[remote-workers]`, `[graph-workers]`, and `[session]` diagnostics for the isolated record; do not delete `plans.db` or an entire state JSON file. |
| `failed to run custom build command for openssl-sys`, with `sccache: Failed to send data to or receive data from server` in the same output | The machine-level `sccache` server is wedged, so every C/C++ compile it fronts fails; `openssl-src` is simply the first crate to need one | `sccache --stop-server && sccache --start-server`, then rebuild. A cache reported at its 10 GiB max is ordinary LRU eviction and is NOT this failure — the discriminator is whether `sccache --show-stats` answers at all. Never restart a responsive server mid-build; that discards a warm cache for nothing. |
| Build reported as succeeding while the gateway job exited non-zero | A wrapper placed another command after the build in the same compound statement, so the shell's status came from that command instead | Read the `exit:` line of the gateway job log, or capture `${PIPESTATUS[0]}`. `build.sh` propagates the gateway status faithfully, so a wrapper is the only place this can be lost. Treat post-install `/api/health` as the only green light. |

When a new failure is found in this flow, fix the owning script or preflight and
add a short symptom/cause/recovery row here before closing the work.

Do not use `dev --wait` for an interactive Windows session. It is reserved for
automation that needs a detached health gate and does not prove a visible window.

### Visible Windows dev session

For interactive verification, do not use `dev --wait`. It is the detached
readiness path for scripts and CI. Use one visible CodeTerm Tab as the sole
foreground owner:

```powershell
# Verify no previous dev listener/process remains, then open one visible tab.
codeterm tab new --cmd "Set-Location '<checkout>'; .\build.ps1 dev"
```

Keep that Tab open and require all of the following before reporting ready:

- port 7686 is owned by the checkout's `target\debug\codeterm.exe`;
- `/api/health` reports `status=ok`, the exact checkout `build_sha`, API 7686,
  Vite 1420, CDP 9223, and identifier `com.codeterm.dev`;
- the CodeTerm process has a responsive, nonzero `MainWindowHandle`;
- exactly one Cargo process remains, the foreground `tauri dev` watcher owned
  by the visible Tab. Any second Cargo/build owner must be stopped and traced.

The canonical checkout owns shared dev, broad gates, and release checks. This is
a team operating rule, not a branch-name restriction in the build script; the
machine-wide gateway only serializes heavy app work.

Gateway-backed commands fail fast when another job owns the machine slot, so
an agent turn never blocks silently behind a long-lived dev process. Add
`--wait-build` only when synchronous queueing is intentional.

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

Use `./build.sh help` as the command index. `test-one <app|plan|pty|pty-fork|types|mem|mesh|plugin|plugins|cli> <filter>`
hides manifest/package details for focused tests. The `types` and `cli`
selectors include all Cargo targets so integration and binary tests are
reachable; the other selectors retain their existing ownership scopes. Each
target is its own test binary and prints its own `test result` line, so a
target reporting `0 passed` only means the filter matched nothing there. Judge
the run by the per-test `... ok` lines of the target that owns the test.
`verify` runs the standard
check, full Rust workspace library suite, and source-size integration gate.
Rust tests keep the repository-owned `scripts/test-capped.sh` wrapper but wait
for Cargo and libtest to finish by default. For a targeted diagnostic only, set
`TEST_CAP_SECS=<positive-seconds>`; an enforced cap terminates the test process
group and exits 124, while ordinary child success or failure is propagated.

`./build.sh cli` is intentionally independent from the app/dev build gateway:
Cargo serializes its separate `cli/target` itself. The command builds the release
CLI, regenerates the three generated CLI artifacts (`generated/cli/cli-manifest.json`, the
generated block in `generated/cli/cli.md`, and the Chat UI command paths), then installs
the binary into the active development data directory, replacing the CLI every
tab of that dev instance runs. Dev builds never overwrite `~/.codeterm/bin`:
production CLI updates come from the bundled production app during startup,
in-app update, or `install-local`.

## Generated artifacts and the embedded-doc boundary

`prod-docs/` is the only documentation embedded in the binary. Its membership is the `cli/src/cmd/docs.rs` registry; `essences/` and `.agents/skills/` remain repository sources and are not embedded.

| Source | Artifact | Regenerate | Verify |
|---|---|---|---|
| CLI clap definitions | `generated/cli/cli-manifest.json`, `generated/cli/cli.md`, and `packages/chat-ui/src/codeterm/commandPaths.ts` | Build the checkout's CLI, then run `cli-manifest --write` (`./build.sh cli` also installs the CLI into the active dev data directory). Never use the installed `codeterm` from `PATH`. | `cli_manifest_is_fresh`, `docs_cli_md_is_fresh`, and `chat_ui_command_paths_are_fresh` |
| `essences/capabilities.md` and `essences/core.md` | `essences/map.jsonl` | `codeterm essence-map generate --root .` | `codeterm essence-map validate --root .` and `essence_map_contract_committed_artifacts_are_fresh` |
| `prod-docs/*.md` and `cli/src/cmd/docs.rs` | The embedded `codeterm docs` set | Update the source and registry; there is no separate document generator. | `registered_documents_have_one_title_and_fit_the_reading_boundary` and `codeterm docs <NAME>` from the built checkout |

Regenerate each committed artifact in the same lane and commit as its source change, before handoff. A source-only Worker must not run build or validation commands; list the exact regeneration and verification commands under `GATES REQUESTED` for the canonical Orchestrator to run.

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
| Windows `npm run test:plugins` says `node` is not recognized before any suite starts | npm's command shell lost the Node executable directory although the parent PowerShell resolves it | Run `node scripts/run-plugin-tests.mjs provider-codex` from canonical; this repository runner pins its own Node directory for child suites. This launcher refusal is not a passing test result. |
| A TS test runs in Node but verification reports TS2550 for `Array.at` | Node supports a newer library than the project's TypeScript target | Use indexed access in the test; keep the project target unchanged and rerun verification. |
| Linux CI reaches `cargo fmt --check` and prints formatting diffs across otherwise green Rust changes | An integration wave merged source-only lanes without applying the repository rustfmt version before push | Run the two manifest-scoped `cargo fmt` commands from canonical, inspect the resulting file list, then rerun both checks before pushing. |
| `pnpm exec tsc --noEmit` reports TS2367 after a test first asserts two literal provider ids and then compares them | The assertion function narrowed both values to disjoint literals, making the later boolean expression statically unconditional even though it documents invalidation | Use `assert.notEqual` for the cross-snapshot comparison so the mechanism remains checked without constructing an always-true expression. |
| `npm run test:unit` terminates healthy workers at the same elapsed time while files are still passing | The TypeScript scheduler applied an implicit suite deadline, so repository growth was misclassified as a hang | Use the current uncapped default and let the suite finish. Set `TS_UNIT_TIMEOUT_SECONDS` or `TS_UNIT_SUITE_TIMEOUT_SECONDS` only for a targeted diagnostic. |
| `bash scripts/test-capped.test.sh` exits 141 before printing an assertion | Its host-triple probe closed the `rustc -vV` pipe early under `pipefail`, so rustc received SIGPIPE | Keep the probe consuming the complete rustc output; then rerun the shell contract. |
| The test-capped shell contract exits 2 when simulating a Windows target on macOS | The fixture asked the wrapper to synthesize a foreign-platform embed sidecar with the local rustc host | Seed the fixture's inert Windows sidecar before testing Windows target selection. |
| A build exits with `Timed out acquiring build gateway state mutex` | Another live or stale gateway owner held the state mutex through the acquisition deadline. The failed invocation now terminates only its own Cargo/rustc descendants before returning. | Run `./build.sh gateway-status` to inspect the active and queued job IDs. If a job is stale, run `./build.sh gateway-cancel <job-id>`, then rerun the original command. |
| `build.ps1 check` exits 75 because the visible dev owns the gateway | The canonical incremental target is intentionally single-owner; waiting behind a foreground dev process cannot complete. | Stop and make an explicit canonical integration decision. Do not move the command into a Worker lane, add `--wait-build`, or hand-compose `CARGO_TARGET_DIR`. |
| A Windows dev build fails while writing `codeterm_lib.lib` with `os error 112` or a localized “not enough disk space” message | The canonical `src-tauri/target` exhausted the checkout drive while Cargo was replacing the debug archive. | Verify the failure owns the canonical target and no dev build is running, then run `.\build.ps1 clean`. Confirm free space before restarting `.\build.ps1 dev`; do not delete another worktree's target or touch production. |
| A Windows native dependency fails with `LNK1106` and `dumpbin` shows a cached `.o` ending before its COFF string-table length | `sccache` retained a truncated C/C++ object; a clean Cargo target can restore the same corrupt cache entry. | In PowerShell set `$env:SCCACHE_RECACHE='1'`, rerun the failing `.\build.ps1` operation once, then remove the environment variable. Verify the rebuilt object with `dumpbin` before retrying normally. |
| A build in one worktree could reuse a cached artifact built from a different worktree's sources (codeterm#327). Never reproduced on demand — this is a configuration hazard, not an observed rebuild; see `scripts/check-sccache-basedir-isolation.sh`. | `scripts/cache-policy.sh` and `scripts/test-capped.sh` defaulted `SCCACHE_BASEDIRS` to the current worktree root. That tells sccache to strip the worktree path before hashing, so two worktrees sharing one `SCCACHE_DIR`/server can derive the same cache key even though their absolute paths differ. | Basedir rewriting is disabled for this repo: `ct_reject_basedir_rewriting` in `scripts/cache-policy.sh` unsets `SCCACHE_BASEDIRS` (warning if a parent shell leaked one in) before every `ct_configure_sccache`/`ct_require_prestarted_sccache` call, and `scripts/test-capped.sh` no longer sets it. Machine-level `sccache` and per-worktree `CARGO_TARGET_DIR` isolation are unaffected. Verify with `bash scripts/check-sccache-basedir-isolation.sh`; do not reintroduce `SCCACHE_BASEDIRS` for this repo. |
| `rustc` exits with `0xc0000005 (STATUS_ACCESS_VIOLATION)` while compiling an unchanged dependency | The Windows compiler process crashed before producing a source diagnostic; this is not a Rust assertion or test failure. | Rerun the same bounded `build.ps1` operation once so completed artifacts are reused. If it repeats, stop, preserve the exact rustc command and Windows crash evidence, and report the toolchain fault; do not clean targets or raise test deadlines. |
| After an OS crash, `rustc` panics while linking with `Invalid archive terminator` | A generated `.rlib` in the canonical Cargo target was truncated; incremental metadata cleanup cannot repair the archive payload. | Confirm no dev build is running, run `.\build.ps1 clean`, then restart the original named operation. Do not delete individual archives or reuse the damaged target. |
| Dev compiles and opens but port 7686 never listens, with a trust-boundary policy error in `~/.codeterm-dev/logs/codeterm.log` | The retained dev config requests a non-loopback host without a bearer token, or contains a wildcard/malformed CORS origin. | Keep dev local by setting `api.host = "127.0.0.1"`, or configure a token for an intentional network-facing instance. Remove `*` and malformed entries from `api.cors_allow_origins`; an empty list is valid and denies all cross-origin browser requests. |
| Windows `build.ps1 kill` reports that the clean shutdown deadline elapsed after a high-tab-count dev fixture | Closing many PTYs and watcher leases can exceed the bounded graceful-shutdown window even though the instance is otherwise healthy. | Let the named operation use its port-scoped force-stop fallback, then verify 7686 is free and 7685 still listens. Never replace it with a process-name kill. |
| `test-one plan` fails to compile old `Task` fixtures after an orchestration schema change | The app gate compiled the plan crate as a dependency but did not compile its own unit-test fixtures. | Update every direct `Task` fixture and changed helper signature, then rerun the exact `test-one plan` filter. Do not remove the plan gate or move it to an app filter. |
| `test-one cli` cannot find a private helper after a command module split while `check` stays green | The binary compiles, but the test target still relied on a parent-module helper that moved behind a private child-module boundary. | Give the test its own fixture helper or import a test-only API; do not widen production visibility. Rerun the exact CLI filter that exposed the test target. |
| `test-one cli` reports a non-exhaustive `TabAction` match after adding a command | The CLI command was added without its mesh API-map consumer. | Add the capability-equivalent route in `mesh_cli/api_map.rs`, then rerun the focused CLI test. |
| `npm run test:plugins` reports `'node' is not recognized` before the suite header, or Git Bash cannot find Node | An oversized/polluted Windows `PATH` was lost at a `cmd.exe` boundary; the former `npx.cmd` runner added another shell boundary. | Run `node scripts/run-plugin-tests.mjs` through a known Node executable. The aggregator pins `process.execPath` into Git Bash and uses the checked-in local tsx CLI; shorten the inherited `PATH` before retrying the npm wrapper. |
| `test-one app` reports a missing field in `SpawnTabSpec` after adding spawn routing state | A full constructor or test fixture bypassed `Default`, so ordinary source review missed the new required field. | Set the field explicitly in every full constructor and fixture, then rerun the exact focused test before continuing. |
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
| Windows `release-preflight` prints `Build gateway job ... exit: 0` but never returns | Git Bash orphaned the FIFO `tee` reader after every gate completed, so gateway log teardown waited forever despite a successful command status. | Interrupt only the stuck wrapper, set `CT_BUILD_GATEWAY_NO_TEE=1` for the retry, and rerun the complete named operation. Do not treat the printed inner exit as a completed preflight. |
| Every gateway-backed command exits 75 naming a job whose log already ended `exit: 0` | That wedged wrapper still owns the machine slot. `build_gateway_logging_on_exit` waits on the log reader before calling `ct_gateway_finish`, so the lock is held for the whole hang, and the owner is alive — the reaper correctly refuses to reap it. | Interrupt the wedged wrapper. The next acquire then reaps it on its own and records `owner_lost` with the recovered `command_exit_code`; `gateway-cancel` is only needed to inspect. Do not wait it out and do not age the lock. |
| Dev opens with a runtime identity mismatch while `codeterm-web-watch.log` repeats `'vite' is not recognized` | The Windows watcher launched Vite through a nested pnpm lifecycle shell that lost local executable lookup, leaving `dist/` stale while the daemon rebuilt. | Use the current dev operation, which launches Vite through the resolved Node process and package entrypoint, then restart dev and verify the runtime/UI identity banner is gone. |
| `build.ps1 check` cannot resolve a newly re-exported plan mutation type | A split mutation module relied on `use super::*`, but the parent module did not import that public service type. | Import the type explicitly from `crate::service_plan`, then rerun the same canonical check. |
| A focused app test passes `&ServiceContext` to a helper that only borrows service state but compilation expects `&Arc<ServiceContext>` | The helper accepted the runtime `ServiceState` alias even though it did not need shared ownership, making ordinary test fixtures incompatible. | Narrow the helper parameter to `&ServiceContext`; runtime `Arc` callers deref-coerce while fixtures pass the same service directly. Rerun the exact focused test. |
| Removing an unused production import makes an inline `#[cfg(test)]` module fail to resolve the same type | The test relied on its parent module's unconditional import instead of declaring test-only scope. | Reintroduce the type behind `#[cfg(test)]` or import it inside the test module, then rerun the focused test. |
| A live Plan owner with a valid current session and nonce is refused after the tab session rotates | The owner predicate authenticated the live tab and then redundantly compared its session to stale Plan metadata. | Keep live session and nonce validation at the tab boundary, identify Plan ownership by the bound tab plus Team authority, and rerun the rotated-session authority table. |
| `build.ps1 e2e` fails on Windows with `python3: command not found` even though Python is installed | The E2E operation hardcoded the Unix executable name instead of resolving the available Python launcher. | Use the build operation that resolves `python3` or `python` and invokes pip through that same interpreter, then rerun `build.ps1 e2e`. |
| A foreground `build.ps1 dev` exits with `unexpected EOF` after `build.sh` is edited during startup | Bash was still reading the running script while its file contents changed underneath that process. | Finish build-script edits before launching dev, validate with `bash -n build.sh`, then restart dev through the named operation. |
| `build.sh dev` appears idle at dependency installation until Enter is pressed | pnpm opened an interactive confirmation prompt in the visible dev terminal while its diagnostic stream was suppressed. | Use the current build operation, which always runs dependency installation non-interactively, then restart dev. |
| Tauri dev starts from PowerShell but its `beforeDevCommand` reports `node is not recognized` | The nested Tauri hook re-resolved Node through a child Windows shell and lost the launcher environment. | Run dev preparation directly in `build.sh` with the already-resolved Node process, then launch Tauri with the generated runtime config that disables the duplicate hook. |
| A piped macOS build reports `wait: pid ... is not a child of this shell` and exits 74 after the command succeeded | Bash 3 detached the process-substitution log reader from the pipeline subshell. | Update to a build script that uses the explicit FIFO-backed reader child, then rerun the original named build operation without filtering its output. |
| Semantic-search tests execute a zero-byte `codeterm-embed` | A compile stub reached a real embedding test. | Build the debug sidecar before Rust library tests. |
| `verify` downloads FastEmbed into a temporary test data directory and fails on a model-host outage | The Rust test wrapper let fixture-owned `CODETERM_DATA_DIR` choose the immutable model cache. | Use the current wrapper, which exports one machine-shared `CODETERM_EMBED_CACHE_DIR`; override it explicitly only for cache diagnostics. |
| macOS task-authority tests panic creating a mock Tauri `EventLoop` | Six tests used the shared Windows-only `ServiceContext` fixture without the fixture's non-Windows ignore boundary, so libtest created the event loop off the macOS main thread. | Mark every test using that fixture ignored off Windows and keep portable authority logic covered by its headless tests. |
| Unix REST lifecycle bind gets `Address already in use` | A guessed test port can collide; restart listeners overlap during handoff. | Prebind OS-assigned generation one; enable `SO_REUSEPORT` on every Unix bind. |
| REST port-release fails only in the parallel macOS suite | Closing a real occupier races Darwin. | Inject the first bind failure, then prove recovery binds a healthy listener. |
| Linux clippy rejects a test-only parameter | Only `cfg(test)` uses it. | Keep injection at the test call site, outside production signatures. |
| Standalone crate tests fail after local `verify` passed | The old gate ran only the app package. | Use current `verify`; it runs every workspace library package to completion. |
| CLI docs report `BUILD exceeds 24 KiB` or `ORCHESTRATION exceeds 240 lines` | Troubleshooting or the compact operating contract outgrew its reading boundary. | Compress repetition or move detail to the named reference playbook; rerun `build.ps1 test-one cli registered_documents_have_one_title_and_fit_the_reading_boundary`. |
| Windows plan-index reports `queue did not drain` | An inner deadline raced background work. | Wait on the queue condition and rely on the test process exit. |
| Linux semantic search gets `Permission denied` or `Exec format error` for `codeterm-embed` | Clippy replaced the built runtime sidecar's mode or bytes. | Preserve the verified ELF before clippy, then restore its bytes and executable mode before tests. |
| A release-time test fails in `#[cfg(test)]` code while `check` was green | `check` compiles the production profile only, so test-only code is never type-checked | Run `./build.ps1 check-tests`, which compiles `--all-targets` for both workspaces without the build gateway. A stale generated map or a document over its reading boundary is a different cause: use the matching row above. |

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
