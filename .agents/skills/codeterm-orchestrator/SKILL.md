---
name: codeterm-orchestrator
description: >-
  Multi-agent orchestration discipline for CodeTerm. Use when Codex is acting
  as an Orchestrator in CodeTerm: decomposing work across Worker panes, planning
  multi-lane fixes, integrating worktrees/branches, handling worker reports,
  setting delayed self-heartbeats, enforcing review/build/UI/Playwright gates,
  preventing stranded branches, or coordinating real-system verification before
  claiming a task is done.
---

# CodeTerm Orchestrator

Use this skill to run CodeTerm work as an engineering manager, not as a polling
loop. The Orchestrator owns decomposition, integration, verification, and the
final claim of readiness. Worker reports are inputs to review, not conclusions.

## Operating Model

- Keep the hierarchy flat: Orchestrator -> Workers. Workers do not spawn
  Workers.
- Give each Worker one bounded task, a clear branch/worktree, expected gates,
  and a required structured report.
- Use event-driven reports as the normal progress signal. Avoid active polling.
- Keep a branch/worktree ledger for the whole run: owner, purpose, base,
  destination, and verification status.
- Prefer one integration branch for combined verification after parallel lanes
  finish. Keep source lane worktrees until their diffs are integrated or
  explicitly discarded.

## Dispatch Checklist

Before spawning Workers:

1. Write the lane map: problem, suspected area, assigned worktree/branch,
   expected output, and acceptance gates.
2. Run a plan-review pass for broad, risky, or multi-lane plans. Review the plan
   for missing integration paths, wrong ownership boundaries, incomplete tests,
   and expensive mistakes before dispatch.
3. Put repo-required gates in the task text: formatting, typecheck, unit tests,
   build wrappers, and any UI/Playwright requirements.
4. Keep task prompts self-contained and in English for agent-to-agent traffic.
5. Avoid backticks inside shell-quoted `--task` strings; zsh can treat them as
   command substitution.

## Worker Reports

Expected report shape:

```text
STATUS: done | blocked | partial
CHANGES: files touched
BUILD/TYPECHECK/TESTS: result
FOLLOWUP: anything the Orchestrator should know
```

### Post-report action set — run EVERY step, EVERY report

This is a fixed checklist, not a menu. Skipping the cleanup step is the most common
failure; it accumulates dead worktrees and GBs of cruft. Run the whole set per report.

1. **Read** as evidence, not proof (status / changes / build-test claims / followup).
2. **Verify** — re-run the meaningful gate yourself, in the integration checkout, **under a
   timeout** (see Build And Test Discipline). A worker's "tests pass" is a claim; a hang is a
   deadlock, not a pass.
3. **Integrate** — if green, merge the lane branch into the mainline (serialized, one at a time).
4. **Update the plan** — mark the task done/re-opened; record a work-point (who/what) so presence
   stays accurate.
5. **CLEAN UP** — remove the worktree, delete the merged branch, kill that instance's dev daemon.
   Do this NOW, not "later". `git worktree remove --force <path>` + `git branch -d <branch>`.
6. **Dispatch next** — pull the next task from the active group; spawn/assign.
7. **Re-arm** the heartbeat and refresh your own current-focus line.

**Under a flood (several reports at once):** queue them and run the FULL set for one before
starting the next. Do not interleave half-cycles — that is exactly when the cleanup and
verify steps get dropped. One report → seven steps → next report.

## Heartbeat Discipline

Use a delayed self-send only as a fallback for long-running work:

```bash
codeterm send --delay 25m "Heartbeat: check active workers only if no reports arrived. Active lanes: <ids>. First run codeterm agent reports; only if no report and worker seems overdue, use pane status/chat once, then re-arm delayed self-check." --pane "$CODETERM_PANE_ID"
```

Rules:

- Reports first.
- No polling loop.
- If reports arrived, process them and do not inspect worker panes.
- If a worker is silent and overdue, check `pane status`/`pane chat` once, then
  re-arm a delayed heartbeat.
- Delete stale delayed heartbeats when the active lane set changes.
- Carry your OWN current-focus in the heartbeat: one line of what you (the
  Orchestrator) are working on right now. The human must be able to see what you
  are doing, not just the workers — state it and refresh it each cycle.

## Verification Gates

Unit-green is necessary and never sufficient for behavior/UI fixes. The
Orchestrator's final readiness claim needs evidence at the layer where the user
experienced the bug.

For each fix, track a ledger:

- Unit or focused test passed.
- Build/typecheck passed through the repo's required wrapper.
- Integration or UI pass run by the Orchestrator.
- Playwright/web-mode pass for UI behavior where practical.
- Manual oracle checked when the bug depends on real session data or visuals.
- Test path exercises the real failing scenario, not a helper-only shortcut.
- Blast radius reviewed across relevant providers/platforms.
- Change is in the version-controlled source, not only in an installed/dev copy.

For CodeTerm UI changes on macOS/Linux, Playwright can run against `/web/` via
the local dev daemon. Do not skip UI gates on the assumption that only Windows
WebView2 can exercise the UI.

Coordinate verification carefully. Running Playwright against the same dev
daemon the user is manually testing can cause focus fights and flakes. Either
coordinate timing or use a separate dev instance when available.

## Build And Test Discipline

**Single hot build, everything through the Orchestrator (weak machine — do NOT parallelize).**
There is ONE hot build checkout (the integration repo with a warm `target/`). ALL builds and
tests happen there, serialized, incrementally. Workers write code and hand you a branch/diff —
they do NOT build, do NOT run tests, do NOT `git worktree add` a fresh checkout (a cold target
rebuilds every C dep — libgit2/libssh2 — from scratch and alone spikes the machine to load 30+).
You pull each worker's change into the hot checkout, build/test it warm, and bounce the result
back. A worker that needs a build iterates THROUGH you: it proposes the change, you build+test,
you return pass/fail. This is slower per round-trip but it is the only thing the machine survives.

The Orchestrator is the single serialized build/verify gate. Hard rules, each learned from a real failure:

- **One build/test at a time.** All `cargo test`/`build.sh` runs in a checkout share one
  `target/.cargo-lock`. A second launched while one is in flight queues (not parallelizes) and
  looks like a multi-minute hang. Never fire concurrent runs; narrow the filter, run once.
- **Wrap every gate test in a timeout.** A deadlock does not fail — it HANGS, which reads as
  "still running" and slips through. Run with a hard timeout (e.g. cargo-nextest per-test
  timeout, or `perl -e 'alarm 60; exec @ARGV' cargo test ...`). A timeout = a failure = bounce
  back. This is exactly how a parking_lot reentrancy deadlock once shipped merged-but-broken.
- **Lock/concurrency changes get a deadlock check.** `grep` the diff for nested same-lock
  acquisition; run the change's own test under a timeout BEFORE merging; vary `--test-threads`.
  Never merge a concurrency fix on the worker's self-report alone.
- **Check load before starting a build.** `uptime`; if load is already high, wait — do not add a
  second compile. A single fresh worktree with a cold target rebuilds all C deps (libgit2,
  libssh2) from scratch and alone spikes load. Reuse a warm target or a shared
  `CARGO_TARGET_DIR`; do not casually `git worktree add` a cold checkout for a one-file fix.
- **Never blind-retry on "database is locked".** The write may have SUCCEEDED and still returned
  the error; retrying duplicates the row. Verify by reading back, not by retrying.
- **Direct binary to bypass output filters.** If a tool wrapper (e.g. an rtk-style proxy) eats
  test output, run the toolchain binary directly (`$(rustup which cargo) test ...`).

## Review Discipline

Use independent review on:

- Broad plans before dispatch.
- Risky lane diffs before integration.
- Final integration branch diff before merge.
- Security, provider-boundary, session-binding, or cross-platform changes.

Frame reviews to find consequences, not to approve effort. Ask for structural
risks, missing callers, wrong abstraction boundaries, untested paths, and
long-term maintainability issues. Keep the tone professional; rigor does not
require abuse.

Reject fixes that are self-contradictory, too broad, or dangerous even if they
appear to satisfy the immediate symptom. Losing user data, cross-pane session
contamination, or corrupting provider boundaries is worse than leaving the bug
open for another iteration.

## Branch And Worktree Hygiene

- Keep Orchestrator planning/integration work separate from Worker worktrees.
- Do not let Workers and the Orchestrator share a checkout when branch switches
  are expected.
- Treat Worker worktrees as temporary lane state. In Andrey's local layout,
  `/Users/rollacode/Developer/codeterm` and
  `/Users/rollacode/Developer/codeterm-plugins` are the canonical repos;
  sibling `codeterm-*` directories are lane worktrees unless explicitly
  preserved.
- Cleanup is part of closing a task. A task is not fully closed while its
  Worker pane, lane worktree, or local lane branch is still dangling without a
  recorded reason.
- Keep the ledger exact for every lane: pane id, worktree path, branch, task id,
  status, integration commit, and cleanup status.
- Before declaring the run complete, inspect unmerged branches:

```bash
git branch --no-merged <mainline>
git log --oneline <mainline>..<branch>
```

- Resolve every branch: integrate it, preserve it explicitly, or discard it with
  a clear reason.
- Prefer one mainline integration branch over several feature-integration
  branches.
- Do not delete a worktree until its diff is integrated, reviewed, or confirmed
  obsolete.
- When a Worker report is accepted, cleanup immediately after integration and
  verification:

```bash
git worktree remove <path>
git branch -d <branch>       # if actually merged
git branch -D <branch>       # if squashed/applied/discarded with a recorded reason
git worktree prune
git worktree list --porcelain
find /Users/rollacode/Developer -maxdepth 1 -type d -name 'codeterm*'
```

- Use `git worktree remove --force <path>` only after the diff is integrated,
  explicitly discarded, or copied into the integration checkout. Never use force
  to bypass an unknown dirty tree.
- If cleanup cannot happen, record the reason in the plan task, heartbeat, and
  final handoff. Include the exact remaining path/branch and the next cleanup
  command.

## CodeTerm-Specific Sharp Edges

- Daemon/plugin code must not shell out to the `codeterm` CLI for hot paths; use
  in-process APIs.
- Blocking work in Tauri commands belongs in `spawn_blocking` or the repo's
  blocking dispatch wrapper.
- Plugin-spawned agents need neutral task preambles; they act through plugin
  verbs.
- Send-path preprocessors must gate on real agent-pane/user-turn signals or they
  can inject context into shell panes.
- QuickJS plugin host calls are synchronous; Promise-wrapping a sync consumer
  can break polling/pump contracts.
- Spawn workers on the Orchestrator's OWN daemon so the hierarchy links and
  `agent report` reaches you; a worker spawned on a different daemon is orphaned.
  Plan/data ops inside the worker's task target whatever daemon owns the data.
- A teammate-inbox message (SendMessage-style) does NOT reach a CodeTerm agent
  pane — it is silently dropped. Brief or steer a running pane via `--task-file`
  at spawn, or `codeterm delayed create --pane <id> --delay 1s --from-file`.

## Final Handoff

A useful handoff includes:

- Current branch/worktree map.
- Integrated diff summary.
- Gates run, with exact commands and outcomes.
- UI/Playwright/manual oracle status.
- Known gaps or residual risk.
- Branches/worktrees intentionally left in place.

Do not report "verified" unless the relevant integration/UI gate was actually
run by the Orchestrator.
