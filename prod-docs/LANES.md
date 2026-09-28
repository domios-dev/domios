# Lane Lifecycle Standard

### Lane safety: two hard stops, everything else advisory

A **mutating** Worker dispatch is refused before any tab is created when its
resolved worktree is the canonical checkout, or is the lane another task owns.
Both would destroy work in progress, so both are hard stops carrying a stable
violation id (`lane.mutating_write_in_canonical_checkout`,
`lane.mutating_write_in_foreign_lane`) and the exact `plan task update` command
that supplies an isolated lane.

**A mutating dispatch with no recorded lane provisions one.** With no lane on
the task, the resolved worktree is the caller's `--cwd`; when that is canonical
the dispatcher branches an isolated lane from it, records the lane on the task,
and spawns the Worker there. Record a lane yourself only when its placement must
be specific:

```bash
git worktree add -b lane/<plan>-<task>-<slug> ../<repo>-worktrees/<slug> <base>
codeterm plan task update <plan> <task>   --worktree <abs-path> --branch lane/<plan>-<task>-<slug>   --base <base> --destination <integration-branch>
codeterm plan task dispatch <plan> <task> --agent <provider> --execution-intent mutating
```

Record `--destination` in that same call; omitting it dispatches successfully but
raises the `lane.merge_destination_unrecorded` nudge, and integration then has to
guess where the work lands.

Every other lane finding is a **non-blocking nudge**: the operation succeeds and
the response carries `safety_nudges`, each with a stable id, why it matters, and
the exact fix. Read-only dispatch, General-Agent creation, and plain shell tabs
are never obstructed, including in canonical.

**The one mutating canonical placement is an explicit opt-in.** `plan task
dispatch --execution-intent mutating --canonical-writer` places a single Worker
in the canonical checkout when sequential work benefits from fast iteration. It
is never inferred from `--cwd`, a recorded path, or the task's shape, and it is
refused for a read-only dispatch, a non-repository task, or a task that records
a lane. The attempt persists `placement: canonical_writer`. The checkout
occupancy preflight makes the placement exclusive: a second live attempt in
canonical is refused with `checkout_occupied`, and `plan review accept` refuses a
lane task while the writer is live (`lane.integration_while_canonical_writer_holds_checkout`).
The writer executes only its `--authorized-gate` commands, commits on the
integration branch, and reports with a clean tree. The report CLI uses the
attempt's explicit placement to validate the SHA against the plan's canonical
checkout, even if that attempt has no worktree path; any dirt refuses the report,
even with `--acknowledge-dirty`.
Other attempts with an unresolved lane still receive the lane worktree refusal.
Without the flag every rule above is unchanged.

For a plan-bound mesh Worker, the peer owns its lane and the origin owns review
and integration. After accepting the attempt, run
`codeterm plan task merge-remote-lane <plan> <task> <attempt>` on the origin.
The command fetches the authenticated peer bundle into a CodeTerm-owned ref,
uses `git merge --no-ff`, and records the merge only after the peer lane tip is
an ancestor of the target. It never accepts cherry-pick or squash as integration.

Competition has no CLI verb; use the `plan.task.compete` route printed by `codeterm plan task show` for competition-enabled tasks. Each
**mutating** competitor gets its own worktree, branch, and base before spawning,
so competitors never share a checkout or fall back to canonical. Partial
provisioning/spawn failures remove lanes created by that call. Read-only
competition is unchanged.

<!-- revision: 17 -->

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
   and worktree gets one verdict: merged and cleaned, bounced, deferred
   with a recorded reason, or abandoned with rationale.

Mechanism ownership:

| Rule | Mechanism owner |
|------|-----------------|
| Code-only worker template and report refusal | `task_5a6d58c5` |
| State transition invariants | `task_7140a28c` |
| Lane liveness and stale labels | `task_1d0572d0` |
| Board/card lane visibility | `task_03bf8da6` / `task_73ea1b9d` |
| Auto cleanup | `task_540e7afa` |
| Polling-smell nudges | `task_25dbe180` |
