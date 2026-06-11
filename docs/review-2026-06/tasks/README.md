# Task briefs — dispatch protocol

One file = one task = one executor. The arbiter (Lamarr, pane `5c64a651`) dispatches
by sending a single line to a worker pane:

```
codeterm send-text "Execute the task file C:/Developer/codeterm/docs/review-2026-06/tasks/<FILE>.md exactly as written. Do not improvise beyond it." --pane <PANE_ID>
```

Spawned (ephemeral) workers get the same line in `--task`.

## Reporting discipline — the task does not exist without it

Workers go silent on long tasks. These four rules are non-negotiable:

0. **Confirm receipt FIRST.** Before doing anything else, run:
   `codeterm send-text "TASK <ID> ACCEPTED, starting" --pane 5c64a651`
   If you don't know how to use `codeterm`, load the `codeterm` skill or run
   `codeterm --help` — do this BEFORE starting, not when you're done.
1. **Progress pings on M/L tasks.** After each completed Steps group (or roughly
   every 20–30 minutes of work), send a one-liner:
   `codeterm send-text "TASK <ID> PROGRESS: step N done, <one fact>" --pane 5c64a651`
2. **The final report is part of the task.** Work without a delivered report =
   work that didn't happen. The LAST action of every task is the Report command.
   Never end your turn after finishing the work but before sending the report.
3. **Shared tree:** other agents commit to this same checkout. If git fails with
   `index.lock exists`, wait 10 seconds and retry (up to 5 times). Never delete
   another process's lock file.

## Rules baked into every brief (workers: read once, they apply always)

1. **VERIFY FIRST.** Re-run the commands in the brief's Evidence section before
   changing anything. If reality differs from the brief — STOP and report
   "evidence stale", do not adapt the task yourself.
2. **Scope is the file.** Touch only the files the brief lists. No drive-by fixes,
   no formatting sweeps, no "while I'm here".
3. **Exact commands.** Run verification commands exactly as written; paste their
   real output into the report. Output you didn't paste didn't happen.
4. **Forbidden, always:** anything on port 7685 (prod); `git commit`/`git push`
   unless the brief's Steps explicitly contain them; editing
   `docs/review-2026-06/` (arbiter's ledger); co-author lines in commits;
   committing `chat.json`, `sp.json`, or anything under `docs/review-2026-06/`.
5. **Blocked beats wrong.** Any ambiguity, any missing file, any failing
   verification you can't explain → report BLOCKED with what you saw. A blocked
   report is a good outcome; an improvised fix is not.
6. **Report format** — finish by running (fill in, keep the structure):

```
codeterm send-text "TASK <ID> STATUS done|blocked|partial. CHANGED: <files or none>. VERIFIED: <command -> result, one per line>. NOTES: <surprises, follow-ups>" --pane 5c64a651
```

## Brief format (see TEMPLATE.md)

`Goal` (one sentence) → `Evidence` (commands proving the problem exists) →
`Steps` (numbered, imperative, no choices) → `Verify` (commands + expected
outcome) → `Forbidden` (task-specific) → `Report`.

Wave assignments and the full backlog rationale live in
[../stabilization-backlog.md](../stabilization-backlog.md). Workers do not need to
read it; the brief is self-sufficient. If a brief contradicts the backlog, the
brief wins (it is newer) — but report the contradiction.

<!-- revision: 1 -->
