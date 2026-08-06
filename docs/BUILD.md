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
| Build and test a local production installer | `./build.sh release-preflight`, then `./build.sh build` | Only the explicit self-install step | this section |
| Publish to all users | PR to `master`, then `release-X.Y.Z` tag | Yes; CI and public updater mirror | `docs/RELEASE.md` and `github-release` skill |

Never use a dev build as a local-install artifact, and never turn a local
installer build into a public release by uploading it manually.

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

### Local production troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `Release lock version mismatch` before build | Version manifests and one or both Cargo lockfiles differ | Run `release-preflight`, commit both lockfiles, then retry. |
| A direct `npm run` or `pnpm build` reports `'node' is not recognized` while `node --version` works | The package manager selected `cmd.exe` for lifecycle scripts in a shell whose effective command lookup differs from the parent PowerShell session | Use the named build operation (`build.ps1 frontend` for the production UI gate). For standalone version validation, run `npm run check:versions --script-shell pwsh.exe`. |
| `release-preflight` reports `'rustc' is not recognized` from `prepare-embed-stub.js` even though Git Bash finds rustc | Native Node launched `cmd.exe` with Git Bash's colon-delimited PATH, so shell lookup could not resolve Cargo tools | Update the stub generator to invoke rustc directly from `CARGO_HOME`/the user Cargo directory, then rerun `release-preflight`. Do not rewrite PATH or bypass the stub. |
| `release source is not clean` | A source or lockfile change has not been reviewed/committed | Inspect `git status --short`; commit the intentional change or stop. Never bypass provenance. |
| `source SHA/status mismatch` after a bundle was created | A build step changed tracked source inputs after the provenance snapshot | Do not install that artifact. Identify the mutating step, fix it, and rerun from a clean tree. |
| Self-install logs `starting` and the window never returns | A helper inherited the calling PTY Job and died when `taskkill` closed that Job | Rebuild the current CLI; it creates the helper through the out-of-job Windows process broker. Re-run `install-local`, inspect `%LOCALAPPDATA%\codeterm\self-install.log`, and verify port 7685 health. |
| CodeTerm restarts but the initiating tab does not resume | The continuation send failed or the Tab id changed before session persistence | Inspect `%LOCALAPPDATA%\codeterm\self-install.log`; the retained `%TEMP%\codeterm-continuation-*.txt` contains the undelivered intent. |
| Self-install succeeds but gateway history reports `owner_lost` | An older build script kept the gateway owned until after the installer restarted CodeTerm and killed the calling Tab | Update the build scripts and run `build.ps1 install-local`; the current operation releases the verified build before detached installer handoff. Prove installation separately with `self-install.log`, port 7685 health, and `codeterm identify`. |
| Repeated local install recompiles the full release graph | The clean-room `build` route was used instead of the persistent local cache | Use `build.ps1 install-local`; reserve `build` for public-release-equivalent verification. |
| A focused Rust test spends most of its runtime linking app binaries | The test omitted `--lib`, so Cargo built unrelated binary test targets | Run the focused filter through `scripts/test-capped.sh ... --lib <filter>`. |
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

Use `./build.sh help` as the command index. `test-one <app|plan|pty|types|mem|mesh|plugin|cli> <filter>`
hides manifest/package details for focused tests; `verify` runs the standard
check, full Rust workspace library suite, and source-size integration gate.
Rust tests keep the repository-owned `scripts/test-capped.sh` wrapper but wait
for Cargo and libtest to finish by default. For a targeted diagnostic only, set
`TEST_CAP_SECS=<positive-seconds>`; an enforced cap terminates the test process
group and exits 124, while ordinary child success or failure is propagated.

`./build.sh cli` is intentionally independent from the app/dev build gateway:
Cargo serializes its separate `cli/target` itself. The command also regenerates
`docs/cli-manifest.json` and the generated block in `docs/CLI.md`, then installs
the release binary only into the active development data directory. Dev builds
never overwrite `~/.codeterm/bin`: production CLI updates come from the bundled
production app during startup, in-app update, or `install-local`.

### Development troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| Linux CI reaches `cargo fmt --check` and prints formatting diffs across otherwise green Rust changes | An integration wave merged source-only lanes without applying the repository rustfmt version before push | Run the two manifest-scoped `cargo fmt` commands from canonical, inspect the resulting file list, then rerun both checks before pushing. |
| `pnpm exec tsc --noEmit` reports TS2367 after a test first asserts two literal provider ids and then compares them | The assertion function narrowed both values to disjoint literals, making the later boolean expression statically unconditional even though it documents invalidation | Use `assert.notEqual` for the cross-snapshot comparison so the mechanism remains checked without constructing an always-true expression. |
| `npm run test:unit` terminates healthy workers at the same elapsed time while files are still passing | The TypeScript scheduler applied an implicit suite deadline, so repository growth was misclassified as a hang | Use the current uncapped default and let the suite finish. Set `TS_UNIT_TIMEOUT_SECONDS` or `TS_UNIT_SUITE_TIMEOUT_SECONDS` only for a targeted diagnostic. |
| `bash scripts/test-capped.test.sh` exits 141 before printing an assertion | Its host-triple probe closed the `rustc -vV` pipe early under `pipefail`, so rustc received SIGPIPE | Keep the probe consuming the complete rustc output; then rerun the shell contract. |
| The test-capped shell contract exits 2 when simulating a Windows target on macOS | The fixture asked the wrapper to synthesize a foreign-platform embed sidecar with the local rustc host | Seed the fixture's inert Windows sidecar before testing Windows target selection. |
| A build exits with `Timed out acquiring build gateway state mutex` | Another live or stale gateway owner held the state mutex through the acquisition deadline. The failed invocation now terminates only its own Cargo/rustc descendants before returning. | Run `./build.sh gateway-status` to inspect the active and queued job IDs. If a job is stale, run `./build.sh gateway-cancel <job-id>`, then rerun the original command. |
| `build.ps1 check` exits 75 because the visible dev owns the gateway | The canonical incremental target is intentionally single-owner; waiting behind a foreground dev process cannot complete. | Stop and make an explicit canonical integration decision. Do not move the command into a Worker lane, add `--wait-build`, or hand-compose `CARGO_TARGET_DIR`. |
| A Windows dev build fails while writing `codeterm_lib.lib` with `os error 112` or a localized “not enough disk space” message | The canonical `src-tauri/target` exhausted the checkout drive while Cargo was replacing the debug archive. | Verify the failure owns the canonical target and no dev build is running, then run `.\build.ps1 clean`. Confirm free space before restarting `.\build.ps1 dev`; do not delete another worktree's target or touch production. |
| A Windows native dependency fails with `LNK1106` and `dumpbin` shows a cached `.o` ending before its COFF string-table length | `sccache` retained a truncated C/C++ object; a clean Cargo target can restore the same corrupt cache entry. | In PowerShell set `$env:SCCACHE_RECACHE='1'`, rerun the failing `.\build.ps1` operation once, then remove the environment variable. Verify the rebuilt object with `dumpbin` before retrying normally. |
| `rustc` exits with `0xc0000005 (STATUS_ACCESS_VIOLATION)` while compiling an unchanged dependency | The Windows compiler process crashed before producing a source diagnostic; this is not a Rust assertion or test failure. | Rerun the same bounded `build.ps1` operation once so completed artifacts are reused. If it repeats, stop, preserve the exact rustc command and Windows crash evidence, and report the toolchain fault; do not clean targets or raise test deadlines. |
| After an OS crash, `rustc` panics while linking with `Invalid archive terminator` | A generated `.rlib` in the canonical Cargo target was truncated; incremental metadata cleanup cannot repair the archive payload. | Confirm no dev build is running, run `.\build.ps1 clean`, then restart the original named operation. Do not delete individual archives or reuse the damaged target. |
| Dev compiles and opens but port 7686 never listens, with a trust-boundary policy error in `~/.codeterm-dev/logs/codeterm.log` | The retained dev config requests a non-loopback host without a bearer token, or contains a wildcard/malformed CORS origin. | Keep dev local by setting `api.host = "127.0.0.1"`, or configure a token for an intentional network-facing instance. Remove `*` and malformed entries from `api.cors_allow_origins`; an empty list is valid and denies all cross-origin browser requests. |
| `test-one plan` fails to compile old `Task` fixtures after an orchestration schema change | The app gate compiled the plan crate as a dependency but did not compile its own unit-test fixtures. | Update every direct `Task` fixture and changed helper signature, then rerun the exact `test-one plan` filter. Do not remove the plan gate or move it to an app filter. |
| `build.ps1 check` cannot resolve a newly re-exported plan mutation type | A split mutation module relied on `use super::*`, but the parent module did not import that public service type. | Import the type explicitly from `crate::service_plan`, then rerun the same canonical check. |
| A focused app test passes `&ServiceContext` to a helper that only borrows service state but compilation expects `&Arc<ServiceContext>` | The helper accepted the runtime `ServiceState` alias even though it did not need shared ownership, making ordinary test fixtures incompatible. | Narrow the helper parameter to `&ServiceContext`; runtime `Arc` callers deref-coerce while fixtures pass the same service directly. Rerun the exact focused test. |
| Removing an unused production import makes an inline `#[cfg(test)]` module fail to resolve the same type | The test relied on its parent module's unconditional import instead of declaring test-only scope. | Reintroduce the type behind `#[cfg(test)]` or import it inside the test module, then rerun the focused test. |
| A live Plan owner with a valid current session and nonce is refused after the tab session rotates | The owner predicate authenticated the live tab and then redundantly compared its session to stale Plan metadata. | Keep live session and nonce validation at the tab boundary, identify Plan ownership by the bound tab plus Team authority, and rerun the rotated-session authority table. |
| `build.ps1 e2e` fails on Windows with `python3: command not found` even though Python is installed | The E2E operation hardcoded the Unix executable name instead of resolving the available Python launcher. | Use the build operation that resolves `python3` or `python` and invokes pip through that same interpreter, then rerun `build.ps1 e2e`. |
| A foreground `build.ps1 dev` exits with `unexpected EOF` after `build.sh` is edited during startup | Bash was still reading the running script while its file contents changed underneath that process. | Finish build-script edits before launching dev, validate with `bash -n build.sh`, then restart dev through the named operation. |
| Tauri dev starts from PowerShell but its `beforeDevCommand` reports `node is not recognized` | The nested Tauri hook re-resolved Node through a child Windows shell and lost the launcher environment. | Run dev preparation directly in `build.sh` with the already-resolved Node process, then launch Tauri with the generated runtime config that disables the duplicate hook. |
| A piped macOS build reports `wait: pid ... is not a child of this shell` and exits 74 after the command succeeded | Bash 3 detached the process-substitution log reader from the pipeline subshell. | Update to a build script that uses the explicit FIFO-backed reader child, then rerun the original named build operation without filtering its output. |
| Semantic-search tests execute a zero-byte `codeterm-embed` | A compile stub reached a real embedding test. | Build the debug sidecar before Rust library tests. |
| macOS tests panic creating a mock Tauri `EventLoop` | A Windows-only fixture ran off the main thread. | Ignore it off Windows; test portable logic headlessly. |
| Unix REST lifecycle bind gets `Address already in use` | A guessed test port can collide; restart listeners overlap during handoff. | Prebind OS-assigned generation one; enable `SO_REUSEPORT` on every Unix bind. |
| REST port-release fails only in the parallel macOS suite | Closing a real occupier races Darwin. | Inject the first bind failure, then prove recovery binds a healthy listener. |
| Linux clippy rejects a test-only parameter | Only `cfg(test)` uses it. | Keep injection at the test call site, outside production signatures. |
| Standalone crate tests fail after local `verify` passed | The old gate ran only the app package. | Use current `verify`; it runs every workspace library package to completion. |
| CLI docs report `BUILD exceeds 24 KiB` | Troubleshooting grew too large. | Compress it or move detail to `BUILD-REFERENCE`. |
| Windows plan-index reports `queue did not drain` | An inner deadline raced background work. | Wait on the queue condition and rely on the test process exit. |
| Linux semantic search gets `Permission denied` or `Exec format error` for `codeterm-embed` | Clippy replaced the built runtime sidecar's mode or bytes. | Preserve the verified ELF before clippy, then restore its bytes and executable mode before tests. |

## Related documents

Tooling, outputs, platform notes, and repository layout: `codeterm docs BUILD-REFERENCE`. Test workflows: `codeterm docs TESTING`.

<!-- revision: 87 -->
