# Task briefs — dispatch protocol

One file = one task = one executor. The arbiter (Lamarr, pane `5c64a651`) dispatches
by sending a single line to a worker pane:

```
codeterm send-text "Execute the task file C:/Developer/codeterm/docs/review-2026-06/tasks/<FILE>.md exactly as written. Do not improvise beyond it." --pane <PANE_ID>
```

Spawned (ephemeral) workers get the same line in `--task`.

## Reporting discipline — the task does not exist without it

Workers go silent on long tasks. These four rules are non-negotiable:

> Arbiter-side rule: no ACCEPTED within ~2 minutes of dispatch → nudge immediately.
> A delivered-but-unconfirmed task is a stalled task (codex sessions can end their
> turn before the message lands and never pick it up), not a long-running one.

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
3. **GIT IS ARBITER-ONLY (as of 2026-06-11, after three shared-tree incidents:
   orphaned commit via scratch-branch switch, cross-contaminated commits via
   `add -A` sweeping a peer's staged files, a peer's commit dropped by
   `reset --soft`).** Workers run NO git mutations: no `add`, `commit`, `rm`,
   `mv`, `reset`, `checkout`, `switch`, `stash`, `push`. Read-only git
   (`status`, `diff`, `log`, `show`, `ls-files`) is fine and encouraged.
   - Edit files with your editor/filesystem tools. Delete with plain `rm`,
     rename with copy+`rm` — then it shows up in `git status` for the arbiter.
   - Where a brief says "Commit: `<message>`" — that step now means: put the
     proposed commit message and the EXACT list of changed/added/deleted paths
     in your report. The arbiter stages by pathspec and commits.
   - Your final report must include your `git status --short` lines so the
     arbiter can diff expectations against reality.
   - **No formatter sweeps.** Never run `cargo fmt` / `pnpm format` beyond the
     exact files of your task (incident: a workspace-wide fmt touched 49
     out-of-scope files and tripped the size gate for everyone). Workspace-wide
     formatting is task A4 — arbiter-scheduled, isolated commit.

## Rules baked into every brief (workers: read once, they apply always)

1. **VERIFY FIRST.** Re-run the commands in the brief's Evidence section before
   changing anything. If reality differs from the brief — STOP and report
   "evidence stale", do not adapt the task yourself.
2. **Scope is the file.** Touch only the files the brief lists. No drive-by fixes,
   no formatting sweeps, no "while I'm here".
3. **Exact commands.** Run verification commands exactly as written; paste their
   real output into the report. Output you didn't paste didn't happen.
   **Windows shell-script rule:** NEVER invoke a `.sh` file bare or as `./x.sh`
   from PowerShell/cmd — Windows pops a "choose an app" dialog on the user's
   desktop instead of running it. Always prefix: `bash build.sh check`,
   `bash scripts/foo.sh` (briefs write `./build.sh ...` for brevity — translate).
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
