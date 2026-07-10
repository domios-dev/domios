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

After a report arrives:

1. Read it as evidence, not proof.
2. Inspect the diff or relevant source yourself.
3. Re-run the meaningful gate in the Orchestrator/integration checkout.
4. Bounce failures back with exact command output and the next scoped task.
5. Update the branch/worktree ledger.

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

Build/test strategy depends on the host's capacity. Pick the mode that matches
the machine; the universal rules apply in BOTH modes.

### Machine Capacity Modes

**Weak machine (e.g. Mac-Air) — single hot build, serialized through the
Orchestrator.** There is ONE hot build checkout (the integration repo with a
warm `target/`). ALL builds and tests happen there, serialized, incrementally.
Workers write code and hand you a branch/diff — they do NOT build, do NOT run
tests, do NOT `git worktree add` a fresh checkout (a cold target rebuilds every
C dep — libgit2/libssh2 — from scratch and alone spikes the machine to load
30+). You pull each worker's change into the hot checkout, build/test it warm,
and bounce the result back. Slower per round-trip, but the only thing the
machine survives.

**Powerful machine (e.g. eight) — parallel builds/tests are allowed.** Workers
MAY run their own gates (format/typecheck/unit/build) in their own worktrees,
and the Orchestrator MAY run independent verification in parallel. You still own
the final readiness claim and re-verify the integrated result yourself — a
worker's green is an input, not the conclusion. The only thing to avoid is
unsafe shared-target contention: two concurrent `cargo`/`build.sh` runs against
the SAME checkout's `target/` share one `.cargo-lock` and serialize-hang rather
than parallelize. Give each parallel lane its own checkout (or its own
`CARGO_TARGET_DIR`) and per-lane dev instance (`CODETERM_INSTANCE` +
`CODETERM_API_PORT`), then real parallelism is safe.

### Universal rules (both modes)

- **Wrap every gate test in a timeout.** A deadlock does not fail — it HANGS,
  which reads as "still running" and slips through. Run with a hard timeout
  (cargo-nextest per-test timeout, or `perl -e 'alarm 60; exec @ARGV' cargo test
  ...`). A timeout = a failure = bounce back.
- **Lock/concurrency changes get a deadlock check.** `grep` the diff for nested
  same-lock acquisition; run the change's own test under a timeout BEFORE
  merging; vary `--test-threads`. Never merge a concurrency fix on the worker's
  self-report alone.
- **One build/test at a time per checkout.** Runs sharing one
  `target/.cargo-lock` queue, not parallelize. Across DIFFERENT checkouts they
  run truly parallel — that is the lever the powerful-machine mode pulls.
- **Never blind-retry on "database is locked".** The write may have SUCCEEDED
  and still returned the error; retrying duplicates the row. Verify by reading
  back, not by retrying.
- **Direct binary to bypass output filters.** If a tool wrapper eats test
  output, run the toolchain binary directly (`$(rustup which cargo) test ...`).
- **One gate ledger per lane.** Keep the final accepted command list in the plan
  close message: focused unit tests, typecheck/build wrapper, generated artifact
  checks, and any skipped/residual risk.

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

## Handoff Verify-Loop

Before `orchestrator handoff --reseed`, run: compose -> read back -> verify x3
-> reseed. No reseed until three consecutive clean passes. Full procedure:
`.agents/skills/codeterm-orchestrator/SKILL.md` section Handoff Verify-Loop and
`docs/ORCHESTRATION.md` section Handoffs.

```bash
codeterm plan handoff create "$CODETERM_PLAN_ID" --goal "..." --next "..." --from-file body.md
codeterm plan handoff show "$CODETERM_PLAN_ID" --latest --markdown
# Verify x3 vs plan get, spec show, plan in-progress, and agent reports.
codeterm orchestrator handoff --plan "$CODETERM_PLAN_ID" --reseed --mode new --goal "..." --next "..." --from-file body.md
```

Resume from `next_actions`, not a provider generic recap.
