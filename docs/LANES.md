# Lane Lifecycle Standard

<!-- revision: 2 -->

This is the portable contract for orchestrated branch work in any project. It is
about lifecycle and evidence, not CodeTerm's own repository layout.

1. Create one lane per task in a dedicated worktree from the current integration
   target. Use `lane/<task-shortid>-<slug>` unless the project has a stricter
   branch prefix.
2. Record the pane, branch, worktree, workspace, model, and target branch on the
   task when the worker starts. `in_progress` without a live owner is stale
   state, not work.
3. Workers are code-only by default. They may run focused checks that match the
   task body, but broad builds, full suites, UI sweeps, packaging, and release
   gates are requested for QA instead of run from every lane. Dev-start waits
   in a fresh worktree are delegated: orchestrators run `./build.sh qa-prep`
   (dist + sidecars, no tsc gate) before handing the worktree to `./build.sh dev`.
4. Reports must include branch, SHA, worktree, tests run, and gates requested.
   A report without enough provenance is not ready for review.
5. QA is serialized per physical machine and parallel across machines. A QA gate
   may fix a failure it directly diagnosed when the fix is mechanical and local
   to that gate; design changes bounce to the worker.
6. Integrate by merging or rebasing unmerged lane work into the integration
   target. Do not cherry-pick. If a branch is unusable, make a new lane and
   manually port the diff with a recorded reason.
7. After acceptance, immediately remove merged branches, clean worktrees, stop
   lane-specific dev daemons, and record cleanup status. Deletion-only branch
   pushes must not run build gates.
8. Before every milestone or release close, run a stale-lane sweep: every branch
   and worktree gets one verdict: merged and cleaned, bounced, intentionally
   parked with unblocker, or abandoned with rationale.

Mechanism ownership:

| Rule | Mechanism owner |
|------|-----------------|
| Code-only worker template and report refusal | `task_5a6d58c5` |
| State transition invariants | `task_7140a28c` |
| Lane liveness and stale labels | `task_1d0572d0` |
| Board/card lane visibility | `task_03bf8da6` / `task_73ea1b9d` |
| Auto cleanup | `task_540e7afa` |
| Polling-smell nudges | `task_25dbe180` |

