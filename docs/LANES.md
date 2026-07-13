# Lane Lifecycle Standard

<!-- revision: 4 -->

This is the portable contract for orchestrated branch work in any project. It is
about lifecycle and evidence, not CodeTerm's own repository layout.

1. Create one lane per task in a dedicated worktree from the current integration
   target. Use `lane/<task-shortid>-<slug>` unless the project has a stricter
   branch prefix. **All worktrees live in ONE sibling container next to the repo —
   `../<repo>-worktrees/<branch-slug>`, where `<repo>` is the repo directory's own
   name — never nested inside the repo** (a nested worktree gets recursed into by
   builds, file-size checks, and scans) and never scattered as ad-hoc roots in the
   parent dir. One subdir per branch; the dir name mirrors the branch. Remove it on
   cleanup.
2. Record the pane, branch, worktree, workspace, model, and target branch on the
   task when the worker starts. `in_progress` without a live owner is stale
   state, not work.
3. Workers are code-only by default. They may run focused checks that match the
   task body, but broad builds, full suites, UI sweeps, packaging, and release
   gates are requested for QA instead of run from every lane. Dev-start waits
   in a fresh worktree are delegated: the orchestrator runs the repo's
   QA-prep/build step (if it has one) before handing the worktree to a dev run.
4. Reports must include branch, SHA, worktree, tests run, and gates requested.
   A report without enough provenance is not ready for review.
5. QA is serialized per physical machine and parallel across machines. A QA gate
   may fix a failure it directly diagnosed when the fix is mechanical and local
   to that gate; design changes bounce to the worker.
6. Integrate by merging or rebasing unmerged lane work into the integration
   target. Do not silently cherry-pick accepted work. If emergency patch-landing
   is unavoidable, prove patch equivalence, record the landing SHA, then delete
   the lane ref and worktree before the task is closed.
7. Run `scripts/merge-stack-audit.sh --target <candidate> <accepted-lane>...`
   before final QA and the master PR. `MERGED` is clean; `PATCH_LANDED_STALE`
   or `UNLANDED` is a release gate failure until the lane is merged, deleted as
   superseded, or explicitly deferred with a recorded reason.
8. After acceptance, immediately remove merged branches, clean worktrees, stop
   lane-specific dev daemons, and record cleanup status. Deletion-only branch
   pushes must not run build gates.
9. Before every milestone or release close, run a stale-lane sweep: every branch
   and worktree gets one verdict: merged and cleaned, bounced, intentionally
   paused with a reason, or abandoned with rationale.

Mechanism ownership:

| Rule | Mechanism owner |
|------|-----------------|
| Code-only worker template and report refusal | `task_5a6d58c5` |
| State transition invariants | `task_7140a28c` |
| Lane liveness and stale labels | `task_1d0572d0` |
| Board/card lane visibility | `task_03bf8da6` / `task_73ea1b9d` |
| Auto cleanup | `task_540e7afa` |
| Polling-smell nudges | `task_25dbe180` |

