---
name: gates
description: Run Domios's verification gates without burning a session on them. Focused checks run here; the full suite, the broad gate and a release run in a sub-agent that reports a summary. Read before `build.ps1 test`, `release-preflight`, or any release.
---

# Gates

Shared Workers declare disjoint edit paths and use `./build.sh fmt --paths <owned-files>`. Lanes are for overlap or build-breaking work. Shared checks keep the same authorization and serialization rules; compiler diagnostics in unowned files are foreign failures for the Orchestrator, and still return nonzero. Never repair foreign files or sweep them into a commit to make a gate green.

Workers run no verification on a shared machine unless their dispatch explicitly grants it; they write code and tests and list the exact checks (lane-check scope, test selectors, TS files, guards) in their report. The Orchestrator merges ready lanes and runs those checks serially in canonical, batched: one `./build.sh lane-check <package>... [--tests]` call for all touched library crates (CLI separately), then ONE `./build.sh nextest <scope> "test(a) | test(b) | ..."` per crate scope combining every selector for that crate, so each test binary builds once. Never run one `test-one` per selector: each call rebuilds and links its crate's test binary. Full or reverse-dependency regressions run only in the single release run. The build guard requires an exact `--authorized-gate "./build.sh <verb> <args>"` dispatch grant in the current persisted attempt, even in canonical. It reads the active plan store beside the host AgentPad, matches task/tab/attempt/session nonce and owner epoch, and refuses missing or unreadable proof. The Orchestrator and its direct utility shells may run these commands in canonical under their bound plan. No environment override exists.

For file-backed QA summaries and agent-tab delivery, see [`utility-shell-reports`](../utility-shell-reports/SKILL.md).

Set `QA_NOTIFY_TAB` to receive one immediate alert per failing step while the
gate continues, followed by its final summary. See the reporting skill above
for signals and log receipts.

Two shapes, and picking the wrong one is what costs hours.

| What you are proving | How |
|---|---|
| A change you just made | Run the focused check here, now |
| The branch is releasable | Delegate the broad gate, get a summary |
| A release | Delegate, then verify its SHA yourself |

In canonical, a per-task Rust test-profile change uses `./build.ps1 check-tests`, which
compiles all test targets of the app packages and the CLI without running
them. A following `test-one app` or `test-one cli` reuses those artifacts.
It uses the shared Cargo gateway entry and accepts `--wait-build`.
`./build.ps1 check` does not compile `#[cfg(test)]` code, so a green `check`
alone does not cover test-only Rust changes.

A focused check is one test module, one type check, one file's lint. It
answers in seconds and you read its output directly. Anything that takes
minutes and prints thousands of lines goes to a sub-agent: the transcript is
the cost, not the wall time.

Rust `test-one` calls default to a 600-second process-group cap. `test` and
`verify` use 3600 seconds per Cargo test invocation; Cargo compiles targets with
`--no-run` before each deadline starts. `TEST_CAP_SECS` overrides either value,
and `TEST_CAP_SECS=0` disables the execution cap. Expiry exits 124 and prints
active libtest case names captured from the execution output.

## Focused checks (Rust commands are canonical only)

```bash
./build.ps1 check                      # cargo check, ~2s incremental
./build.sh nextest app 'test(service::a) | test(service::b)'   # all app selectors in one run
./build.sh nextest plan 'test(service_plan::x) | test(service_plan::y)'
./build.sh nextest cli 'test(cmd::x) | test(cmd::y)'
node node_modules/typescript/bin/tsc --noEmit
node --import tsx --import ./scripts/ts-test-asset-hook.mjs <file>.test.tsx
bash scripts/check-file-sizes.sh --fail
node scripts/check-provider-boundary.mjs
node --test scripts/tests/check-provider-boundary.test.mjs
node scripts/check-archive-portable-test-paths.mjs   # Rust test paths survive a moved nextest archive
```

Run these from the PowerShell tool directly. They are short enough to read.

`test-one app` runs only the server library `codeterm-service-core`. After the
thin-client split, the client package `codeterm` (`src-tauri/src`, for example
`run.rs` and `startup/`) runs through `test-one shell`, and each client crate
through its package scope: `test-one codeterm-client-shell <filter>`,
`test-one codeterm-frontend-shell <filter>`. A filtered `test-one` whose run
passes no test (no match, every match ignored, or no summary) exits 97 in every
scope, archived or compiled; rerun with the owning scope, never accept the zero.
The app scope runs a cached test archive only when the checkout is clean and
its tree equals the archive's `.source-tree` receipt; otherwise it compiles.

A verification pass (Orchestrator in canonical, or a Worker whose dispatch grants it) is: `./build.sh fmt` once, one multi-package `./build.sh lane-check <package>... [--tests]` call, one batched `nextest` per crate scope, changed TS/JS tests in one runner call and `tsc` when TS changes, plus guards. On failure, fix all reported errors and recheck only what failed. The formatter scopes rustfmt and Biome safe fixes to changed files. The lane-check helper selects the shared cache only inside its subshell: `$CODETERM_RUST_CACHE_ROOT/lanes`, otherwise `$CARGO_HOME/codeterm-lanes` (Cargo home defaults to `~/.cargo`); Cargo's lock serializes checks without a gateway slot. Canonical/gateway build directories and per-worktree targets remain separate. List canonical selectors under GATES REQUESTED.

## Check before tests

If a focused startup selector reports zero tests in `app`, use `shell` for
`src-tauri/src/` modules. `app` selects service-core; `shell` selects the Tauri
root library and main test target. The affected-tests mapper enforces this split.

Canonical QA always starts with `./build.sh check`; tests (`check-tests`,
`test-one`) run only after it is green, so a name or type error costs seconds
instead of a multi-minute test-profile rebuild.

The Orchestrator checks a committed Rust change in canonical with `./build.sh check-queue --lane [<sha>]`
(default: lane HEAD). It checks the SHA out in the per-machine check worktree
(`<gateway dir>/check-worktree`, own warm `target/app` and `target/cli`, never
canonical or a lane target), runs incremental
`cargo clippy --lib --no-deps --message-format short -- -D warnings` with the shared
`scripts/clippy-allow.sh` policy for existing touched packages, root bins only
when root is touched and leaf test targets only for test paths or moved crates.
Core test targets stay at the batch. It prints at most `--max-errors` (default
20) `file:line:col: error[...]` lines with an `errors=N shown=M` total, then at
most `--max-warnings` (default 40) warning lines, one `warnings crate=<name>
count=N` line per warned crate and `warnings=N shown=M failing=<crates>`.
Warnings count as errors: any rustc or clippy warning in a touched crate fails
the run; canonical QA requires `errors=0` and `warnings=0`.
check-queue, check-tests and clippy --all-targets type-check the bin against MockRuntime.
The Wry world is checked by product library Clippy in check-queue and by every dev and production build.
`--all` lints the whole app workspace and the CLI and fails on any warning.
`./build.sh check` ends with one `warnings crate=` line per warned crate and a
`check: warnings=N` total; canonical expects `warnings=0`. Every workspace crate
and the CLI deny `warnings` through `[lints]` in Cargo.toml, so a new warning is
a compile error everywhere; fix it at the root, never with `#[allow]`. A busy gateway refuses at once with exit 75; do source
work and never poll. The first run on a machine is cold, later runs are warm.

The provider boundary guard compares exact exceptions with `origin/develop`; CI supplies the target SHA through `PROVIDER_BOUNDARY_BASE`. Each exception names one exact file/token pair and its count. Lower or remove the entry when occurrences leave; moving or formatting existing occurrences within a file is allowed. Counts may only shrink; new provider coupling belongs in the provider plugin.

## Broad, delegate

The broad gate is `./build.ps1 release-preflight`: fmt, size, bundles,
versions, cargo check, clippy, the release Rust set, tsc, knip,
489 TS test files, biome. It runs **once, on the release SHA, immediately
before cutting** — never per merge.

The release Rust set is every `release`-tier Rust runner in
`tests/catalogue.json` (`node scripts/test-catalogue.mjs select --tier release`):
every src-tauri workspace member, the shell package as its lib, the
`codeterm-devtools` `tests/main.rs` target, `crates/mem-store` and the CLI.
`codeterm-paths` is a normal app workspace member. `pty-fork` stays `manual`. Preflight runs it
through `./build.sh release-rust [runner-id...]`, which runs each runner's
`./build.sh nextest <scope>` with its `budget_secs` as the cap, keeps going past
a red runner, prints `release-rust: RED <ids>` per failure and one closing
summary, and skips only the `stale` and this platform's `platform_red` markers of
`tests/catalogue.overlay.json`. A red is fixed or recorded there as
`platform_red` with platforms, reason and owner; never `#[ignore]` it or narrow
a filter. A runner whose catalogue inventory holds no test passes nextest's
no-tests check; every other runner fails it. The CI archive
(`scripts/ci-rust-archives.mjs`) archives the same runners placed on
`tests-rust.yml:rust-tests`; the input crates keep their own
`tests-rust.yml:rust-input-platform` job.

Delegate it:

```bash
domios agent spawn codex --model gpt-5.6-luna --reasoning-effort xhigh \
  --title "gate" --cwd "D:\Developer\codeterm" --task-file <pad>/gate-brief.md
```

The brief must say: run `./build.ps1 release-preflight` with **no**
`CODETERM_INSTANCE` or `CODETERM_API_PORT` set, capture the whole log, and
report back with `domios agent report`:

- the exit code,
- every `release-rust: RED` line and the closing `release-rust:` summary,
- the count of `test result: FAILED` lines (not the last result line),
- every `✗ FAILED:` TS file,
- the first real error for each distinct failure, with its file and line,
- what it did **not** run.

It must not fix anything. A gate agent that repairs what it measures has
stopped being a gate. It bounces the failures to you; you fix and send it back
for another run.

## Traps, each one paid for

**Do not read the last `test result:` line.** It belongs to whichever crate
finished last, not the one that failed. Count `test result: FAILED` instead.

**`build.ps1 test` stops at the first failing crate.** A clean-looking count
can be hiding four more crates that never ran. It also covers neither the CLI
crate nor the TS suite — only `release-preflight` does.

**Unset your dev instance variables first.** Isolated tests demand their own
`CODETERM_INSTANCE`; with yours set they fail on the environment, not the code.

**Never pipe a long native command through `Select-Object -First N`.** It
terminates the pipeline, kills the command mid-run, and `$LASTEXITCODE` then
reports the pipeline's success. Redirect to a file, then read the file.

**PowerShell's `Out-File` writes UTF-16.** `grep` finds nothing in it and a
wait loop spins to timeout. Use `-Encoding utf8`, or read it with
`Get-Content`.

**Do not `domios send` build commands into a shell tab.** The prose guard
refuses them without `--to-shell`, and a tab that already has pending input
corrupts the next command. Run them from the PowerShell tool.

**A killed gate leaves the build gateway locked.** `build.ps1 gateway-status`
shows the holder; `build.ps1 gateway-cancel <job-id>` releases it, but
asynchronously — re-check status before retrying rather than concluding it
failed.

Run `./build.sh fmt` once after each batch of edits before checks; it formats
only changed Rust and TS/JS/JSON files and uses each Rust package's edition.
Re-run the size check after it, since rustfmt can push a file past the 1200-line
gate that passed earlier.

**The clippy gate rejects deprecated APIs.** Tauri shell's
ShellExt::open fails under -D deprecated. Use the existing
crate::commands::system::open_url inside spawn_blocking for account and billing
browser launches; remove unused shell imports and handles. Recheck the focused
account service selector and bash scripts/clippy-gate.sh in warm canonical.

## Release

`.agents/skills/github-release/SKILL.md` owns the sequence. This skill only
fixes where the gate sits in it: after the version bump, on the exact SHA you
are about to tag, once.

An app release also requires the [upgrade restore bench](../../../evals/upgrade-restore/README.md)
before pushing `release/X.Y.Z`. In canonical, capture a sanitized real desk through
`node scripts/capture-upgrade-desk.mjs`, replay it and the generated worst cases
against the exact release build, and retain `replay-result.json` plus provider
traces. Follow the bench's prepare → stage → named DEV restart → verify sequence;
all bench CLI calls use `dc`, and all readiness waits have failure deadlines.
Missing backing, unsupported provider, unready tab or missing receipt blocks the
push. Node fixture tests and import acknowledgement do not replace live proof.
For the runtime migration, retain pre-fix failures naming desk leaf bound,
SizeLimit and binding incomplete, plus post-fix passes. Only canonical may build
or start those versions. Frontend-only and plugin-only releases do not run this
app startup gate.

The [core-journeys suite](../../../evals/core-journeys/README.md) is a required
pre-push gate for every app release and every owner hotfix build (any build
handed to the owner to install, such as `X.Y.Z-bs.N`). On that exact SHA, in
canonical, with a named isolated instance running that build and its window
open, run `CODETERM_INSTANCE=<name> CODETERM_API_PORT=<port> bash
evals/core-journeys/run.sh all --out <run-dir>` once in a Team-bound QA shell,
then `node evals/core-journeys/lib/evidence-contract.mjs <run-dir>`. `run.sh`
refuses ports 7685 and 7686 and an unnamed instance. Only exit 0 from both, with
`Verdict: PASS` in `results.md`, clears the push; record its wall time and keep
the run directory with the release evidence. A failed journey blocks the
push until it is fixed and the suite rerun green on the new SHA. Offline node
tests do not replace the live run. Frontend-only and
plugin-only releases are outside this gate.


An intentional `--queue` uses the same event-based queue as `--wait-build`; it does not grant verification authority or change a Worker’s assigned gates. Default busy-slot refusal remains in effect.

<!-- revision: 15 -->

## Rust warning inventory and cleanup

Every release target, Rust test job and clippy build uploads a `rust-warnings-<lane>-<target>.json`
artifact. The always-running warning summary publishes per-target, per-lint and per-file tables
in its job summary. Download `rust-warnings-release-report` (or `rust-warnings-tests-report`)
for `rust-warnings-report.md` and `rust-warnings-report.json`; the release summary also comments
on associated release PRs. JSON diagnostics include crate, location, lint, message and snippet.
Capture/upload/comment failures do not change platform build results; new warning identities fail
the summary ratchet. Incomplete runs are labelled and never propose a baseline update.

`scripts/rust-warnings-baseline.json` is initialized at zero: every crate denies `warnings`
through Cargo `[lints]`, so any warning identity a run reports is new and fails the ratchet.
Fix the warning at its root on the owning platform; never add a baseline entry.

The post-release cleanup task consumes the full JSON report, groups by target/file/lint, and
fixes cfg-specific warnings on their owning platform (including Windows arm64). After a complete
run verifies the fixes, commit its proposed baseline alongside the cleanup. Subsequent baseline
entries/counts may only shrink relative to origin/develop; adding a replacement warning at the
same count still fails. A test-only run preserves baseline targets it did not build. Never copy
an incomplete run's inventory over the baseline or hand-increase an allowance.

Focused collector verification: `node --test scripts/tests/collect-rust-warnings.test.mjs`.
Workflow validation: `actionlint .github/workflows/*.yml`. No release or test dispatch is part
of collector verification; the next real release proves the six-platform delivery and PR comment.

Lane verification uses one multi-package lane-check for all touched library crates, changed TS/JS test files and `tsc` when TS changes. Canonical integration verification owns Clippy and focused Rust tests. Deleted workspace packages are excluded. Batches use the default full check-queue and run focused app selectors from a single `nextest-archive <sha>` without default features. Root dev/default and core defaults remain aligned for the product; ordinary tests explicitly disable defaults. A batch failure in a lane's files is a bounce naming the integration SHA.

Product DEV and check-queue library Clippy retain embeddings. Ordinary test profiles disable default features, including archives and CI. The required `rust-embeddings` CI job alone runs memory tests with embeddings enabled (`./build.sh ci-tests embeddings`). Full check-queue checks the ordinary service-core test profile and no longer repeats a dev-lib cargo check.

## Installed application identity

For bundle, installer or release-asset identity changes, include
`node scripts/check-installed-identity.mjs` and
`node --test scripts/tests/installed-identity.test.mjs` in the focused batch.
These cover product/identifier/DEV names, installer-facing legacy-name rejection,
new updater namespace and non-installable old manifests. Run the changed release
script tests too. Native package extraction, signing, both CLI resource digests,
system CLI ownership, OS registration and the manual old-app removal require release CI and owner
installation on Windows, macOS and Linux; source assertions do not prove them.
