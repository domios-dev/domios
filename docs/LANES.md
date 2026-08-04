# Lane Lifecycle Standard

<!-- revision: 7 -->

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
2. Record the Tab, branch, worktree, workspace, model, and target branch on the
   task when the worker starts. `in_progress` without a live owner is stale
   state, not work.
3. Isolated Workers are source-only. They inspect, edit, review, and commit
   owned source, but never build, run project tests, start dev services, or
   execute validation gates. They author coverage and report exact deferred QA
   commands. After real merge, the Orchestrator runs all executable verification
   in canonical; delegation requires a Worker assigned there plus exact permission.
4. Reports must include branch, SHA, worktree, tests run, and gates requested.
   A report without enough provenance is not ready for review.
5. QA is serialized per physical machine and parallel across machines. The
   Orchestrator works from the repository's canonical integration checkout:
   Workers own isolated source lanes, while merges, executable review, focused
   and broad QA, dev verification, and release gates run only in canonical.
6. Integrate accepted work with a real merge commit from the lane into the
   integration target. Never cherry-pick, squash, replay, or patch-copy lane
   commits. Rebase the lane onto the target when necessary, re-run its bounded
   review, then merge the lane itself.
7. Run `scripts/merge-stack-audit.sh --target <candidate> <accepted-lane>...`
   before final QA and the master PR. `MERGED` is clean; `PATCH_LANDED_STALE`
   is only patch-equivalence evidence, not ancestry proof. `PATCH_LANDED_STALE`
   and `UNLANDED` both fail the release gate until the lane is ancestry-merged
   or explicitly deferred with a reason.
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
