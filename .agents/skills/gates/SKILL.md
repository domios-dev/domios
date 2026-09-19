---
name: gates
description: Run CodeTerm's verification gates without burning a session on them. Focused checks run here; the full suite, the broad gate and a release run in a sub-agent that reports a summary. Read before `build.ps1 test`, `release-preflight`, or any release.
---

# Gates

Two shapes, and picking the wrong one is what costs hours.

| What you are proving | How |
|---|---|
| A change you just made | Run the focused check here, now |
| The branch is releasable | Delegate the broad gate, get a summary |
| A release | Delegate, then verify its SHA yourself |

A per-task Rust test-profile change uses `.\build.ps1 check-tests`, which
compiles all targets in both the app and CLI workspaces. `.\build.ps1 check`
does not compile `#[cfg(test)]` code, so a green `check` alone does not cover
test-only Rust changes.

A focused check is one test module, one type check, one file's lint. It
answers in seconds and you read its output directly. Anything that takes
minutes and prints thousands of lines goes to a sub-agent: the transcript is
the cost, not the wall time.

## Focused, run here

```bash
.\build.ps1 check                      # cargo check, ~2s incremental
.\build.ps1 test-one app  <filter>     # one app test
.\build.ps1 test-one plan <filter>     # one codeterm-plan test
.\build.ps1 test-one cli  <filter>     # one CLI test
node node_modules/typescript/bin/tsc --noEmit
node --import tsx --import ./scripts/ts-test-asset-hook.mjs <file>.test.tsx
bash scripts/check-file-sizes.sh --fail
```

Run these from the PowerShell tool directly. They are short enough to read.

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

## Release

`.agents/skills/github-release/SKILL.md` owns the sequence. This skill only
fixes where the gate sits in it: after the version bump, on the exact SHA you
are about to tag, once.
