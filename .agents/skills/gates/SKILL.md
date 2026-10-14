---
name: gates
description: Run Domios's verification gates without burning a session on them. Focused checks run here; the full suite, the broad gate and a release run in a sub-agent that reports a summary. Read before `build.ps1 test`, `release-preflight`, or any release.
---

# Gates

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

A per-task Rust test-profile change uses `.\build.ps1 check-tests`, which
compiles all test targets in both the app and CLI workspaces without running
them. A following `test-one app` or `test-one cli` reuses those artifacts.
It runs outside the build gateway and refuses `--wait-build`.
`.\build.ps1 check` does not compile `#[cfg(test)]` code, so a green `check`
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

## Focused, run here

```bash
.\build.ps1 check                      # cargo check, ~2s incremental
.\build.ps1 test-one app  <filter>     # one app test
.\build.ps1 test-one plan <filter>     # one codeterm-plan test
.\build.ps1 test-one cli  <filter>     # one CLI test
node node_modules/typescript/bin/tsc --noEmit
node --import tsx --import ./scripts/ts-test-asset-hook.mjs <file>.test.tsx
bash scripts/check-file-sizes.sh --fail
node scripts/check-provider-boundary.mjs
node --test scripts/tests/check-provider-boundary.test.mjs
```

Run these from the PowerShell tool directly. They are short enough to read.

In a Worker lane only the isolated ones run: `tsc`, a single node test file,
`test-one plan`, `test-one pty`, the file-size script and
`./build.sh check-queue`. `check`, `check-tests` and `test-one app` compile the
app crate in the lane, a full rebuild; report them as deferred selectors for the
Orchestrator's canonical wave batch.

## Check before tests

Canonical QA always starts with `./build.sh check`; tests (`check-tests`,
`test-one`) run only after it is green, so a name or type error costs seconds
instead of a multi-minute test-profile rebuild.

A Worker proves a committed Rust change with `./build.sh check-queue [<sha>]`
(default: lane HEAD). It checks the SHA out in the per-machine check worktree
(`<gateway dir>/check-worktree`, own warm `target/app` and `target/cli`, never
canonical or a lane target), runs incremental
`cargo clippy --all-targets --no-deps --message-format=short` with the shared
`scripts/clippy-allow.sh` policy for the touched packages (plus `codeterm` as
dependent unless `--touched-only`), and prints at most `--max-errors` (default
20) `file:line:col: error[...]` lines with an `errors=N shown=M` total, then at
most `--max-warnings` (default 40) warning lines, one `warnings crate=<name>
count=N` line per warned crate and `warnings=N shown=M failing=<crates>`.
Warnings count as errors: any rustc or clippy warning in a touched crate fails
the run, and a Worker reports only with `errors=0` and `failing=none`.
`--all` lints the whole app workspace and the CLI and fails on any warning.
`./build.sh check` ends with one `warnings crate=` line per warned crate and a
`check: warnings=N` total; canonical expects `warnings=0`. Every workspace crate
and the CLI deny `warnings` through `[lints]` in Cargo.toml, so a new warning is
a compile error everywhere; fix it at the root, never with `#[allow]`. A busy gateway refuses at once with exit 75; do source
work and never poll. The first run on a machine is cold, later runs are warm.

The provider boundary guard compares exact exceptions with `origin/develop`; CI supplies the target SHA through `PROVIDER_BOUNDARY_BASE`. Each exception names one exact file/token pair and its count. Lower or remove the entry when occurrences leave; moving or formatting existing occurrences within a file is allowed. Counts may only shrink; new provider coupling belongs in the provider plugin.

## Broad, delegate

The broad gate is `.\build.ps1 release-preflight`: fmt, size, bundles,
versions, cargo check, clippy, every workspace test, the CLI crate, tsc, knip,
489 TS test files, biome. It runs **once, on the release SHA, immediately
before cutting** — never per merge.

Delegate it:

```bash
codeterm agent spawn codex --model gpt-5.6-luna --reasoning-effort xhigh \
  --title "gate" --cwd "D:\Developer\codeterm" --task-file <pad>/gate-brief.md
```

The brief must say: run `.\build.ps1 release-preflight` with **no**
`CODETERM_INSTANCE` or `CODETERM_API_PORT` set, capture the whole log, and
report back with `codeterm agent report`:

- the exit code,
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

**Do not `codeterm send` build commands into a shell tab.** The prose guard
refuses them without `--to-shell`, and a tab that already has pending input
corrupts the next command. Run them from the PowerShell tool.

**A killed gate leaves the build gateway locked.** `build.ps1 gateway-status`
shows the holder; `build.ps1 gateway-cancel <job-id>` releases it, but
asynchronously — re-check status before retrying rather than concluding it
failed.

**`cargo fmt` needs `--all`** or workspace members are skipped, and a reformat
can push a file past the 1200-line gate that passed minutes earlier. Re-run the
size check after any fmt.

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
canonical, with DEV running that build and its window open, run
`bash evals/core-journeys/run.sh all` once in a Team-bound QA shell. Only a zero
exit with `Verdict: PASS` in `results.md` clears the push; record its wall time
and keep `results.json` with the release evidence. A failed journey blocks the
push until it is fixed and the suite rerun green on the new SHA. An app release
also runs `run.sh upgrade` from the previous release on a disposable named
instance. Offline node tests do not replace the live run. Frontend-only and
plugin-only releases are outside this gate.


<!-- revision: 7 -->

## Rust warning inventory and cleanup

Every release target and test archive/clippy build uploads a `rust-warnings-<lane>-<target>.json`
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
