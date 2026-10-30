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
and spawns the Worker there. An explicit `--cwd` must be an existing directory
(the CLI resolves a relative one against the caller's directory) and is never
bypassed: a missing path is refused before any tab exists, and a directory that
is not a usable checkout is refused instead of falling back to the workspace or
canonical checkout. Record a lane yourself only when its placement must be
specific:

```bash
git worktree add -b lane/<plan>-<task>-<slug> ../<repo>-worktrees/<slug> <base>
domios plan task update <plan> <task>   --worktree <abs-path> --branch lane/<plan>-<task>-<slug>   --base <base> --destination <integration-branch>
domios plan task dispatch <plan> <task> --agent <provider> --execution-intent mutating
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
Task-bound direct spawns on an existing git checkout record that checkout on the
attempt, including across restore. Their readiness reports validate the reported
SHA against that checkout HEAD and require a clean tree, even with an explicit
base or `--acknowledge-dirty`. They never use the canonical checkout as a fallback.
A failed task lookup or unusable checkout root refuses readiness reporting instead
of falling back to the caller's directory; repair the binding or report partial progress.
Attempts with neither a lane nor a recorded checkout still receive the worktree
refusal; ask the Orchestrator to dispatch with a worktree and report partial progress.
Without the flag every rule above is unchanged.
For this canonical dispatch, add `--kind qa` or `--kind release_manager` to
select the built-in prompt profile without changing `task.executor_role`; the
kind does not grant canonical placement.
Direct `agent spawn --plan-task` cannot reserve a task attempt and remains refused
for task-bound canonical mutation. Task-less kinds with Plan context retain their
existing parent-directed placement behavior; they do not reserve task attempts or
participate in canonical writer occupancy, so the parent coordinates those writes.

For a plan-bound mesh Worker, the peer owns its lane and the origin owns review
and integration. After accepting the attempt, run
`domios plan task merge-remote-lane <plan> <task> <attempt>` on the origin.
The command requires the peer tip to equal the accepted report SHA and the clean
origin checkout to remain on its dispatch-time branch. It uses `git merge
--no-ff` and records only after ancestry passes; cherry-pick and squash are invalid.

Competition has no CLI verb; use the `plan.task.compete` route printed by `domios plan task show` for competition-enabled tasks. Each
**mutating** competitor gets its own worktree, branch, and base before spawning,
so competitors never share a checkout or fall back to canonical. Partial
provisioning/spawn failures remove lanes created by that call. Read-only
competition is unchanged.

This is the portable contract for orchestrated branch work in any project. It is
about lifecycle and evidence, not Domios's own repository layout.

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
3. Workers inspect, edit, review, and commit owned source in their lane and may
   run isolated checks that do not rebuild the whole project (a type check, one
   test file, a linter, a small standalone package's tests), through the build
   queue when the project has one. A whole-project build is a full rebuild left
   to canonical; lanes start no dev servers, full suites, packaging, or
   self-install. After review and a real merge, the Orchestrator reruns the
   merged wave's focused selectors once in canonical.
4. Reports must include branch, SHA, worktree, each check's category with exact
   commands, counts and raw exits, and the deferred selectors.
   A report without enough provenance is not ready for review.
5. QA is serialized per physical machine and parallel across machines. The
   Orchestrator works from the repository's canonical integration checkout,
   where merges, wave QA, dev verification, and release gates run warm instead
   of rebuilding in every lane.
6. Integrate accepted work with a real merge commit from the lane into the
   integration target. Never cherry-pick, squash, replay, or patch-copy lane
   commits. Rebase the lane onto the target when necessary, re-run its bounded
   review, then merge the lane itself.
7. Before final QA, prove each accepted lane is an ancestor of the integration
   candidate with `git merge-base --is-ancestor <accepted-lane> <candidate>`.
   Patch equivalence does not prove ancestry. Merge any missing lane or record
   an explicit deferral before continuing.
8. After acceptance, immediately remove merged branches, clean worktrees, stop
   lane-specific dev daemons, and record cleanup status. Deletion-only branch
   pushes must not run build gates.
9. Before every milestone or release close, run a stale-lane sweep: every branch
   and worktree gets one verdict: merged and cleaned, bounced, deferred
   with a recorded reason, or abandoned with rationale.

## Transfer an acknowledged Worker between Teams

For an acknowledged task-bound Worker in another Team's approved plan,
the current source parent Orchestrator may use
`domios agent link --parent-tab <destination> --tab <worker>` to transfer it
to that plan's live Orchestrator in the same local workspace. Preserve plan,
task, attempt, lane, session and nonce. The destination cannot pull it.
Mismatched plan owner, binding, session or isolated lane refuses without state
changes; recover or replace stale/ambiguous work through the existing task route.
Local `team adopt-agent` is for unbound roleless tabs. For unbound remote Workers, the current parent or owner may run `domios team adopt-agent <team> <worker> --peer <peer>` on the destination Team's machine. `domios agent reparent <worker> --to <local-orchestrator> [--peer <peer>]` also preserves the running session. The origin verifies caller and destination; only Full-access paired peers participate, and a destination cannot pull a Worker. Remote task-bound Workers refuse both operations: their Plan must retain ownership. Repeating an uncertain remote transfer recovers its durable operation and receipt.
Only hierarchy and report placement move. Shell parent/Team environment values
remain launch provenance, never current membership proof; resolve reports from
live hierarchy and plan ownership. `CODETERM_TAB_ID` stays unchanged.

<!-- revision: 24 -->
