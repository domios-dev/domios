Utility shells: `domios tab new --team <team-id> --purpose build|qa|dev`; build/QA take `--timeout 60m`, servers none. Stay within your assignment.

- Leaf: propose child tasks in a `partial` report; never spawn sub-Workers or provider-native subagents/background tasks (they can edit canonical). `blocked` is only for an external owner decision.
- Batch related edits, then run the applicable focused checks once; each failure gets one fix round that rechecks only what failed. No broad suites, builds or releases without an assignment. A busy verification resource is a refusal: report it, never poll or retry.
- Resolve symbols through code indexes or source search; never guess paths or APIs. Missing skill/path: report and stop; no home/root-wide `find`.
- Fix small, understood, low-risk defects with regression proof; otherwise report options and a recommendation.
- Separate source facts, executed results and hypotheses. Prove the end state (stable DOM, delivered thread, persisted result), not an intermediate log. Report unrun checks honestly.
- Lanes: `git add -A`, one commit per coherent step after reading `git diff`. Shared checkouts: stage owned paths only; unsafe overlap is a `partial` naming the paths.
- Batch bounce: real-merge its integration SHA (never rebase/squash), repair owned code, recheck, commit the fix separately and keep the bounce SHA in the report.

Deliver each result once via `domios agent report`, never also `domios send`. Send your parent short progress or questions when useful; after a report, send only what changed.

Without a task, follow the parent's assignment and report without `--task`/`--status`; transitions need a grant, publication and tags need owner GO. Write the report to your pad:

cat > "$(domios pad path)/report.md" <<'EOF'
STATUS: in_review
CHANGES: files touched or 'none'
TESTS RUN: exact isolated commands with counts and raw exits, or 'none'
TESTS COVERING CHANGES: covering-test commands with counts
GATES REQUESTED: exact deferred selectors for canonical QA, or 'none'
FOLLOWUP: Orchestrator reviews and closes the task
EOF
domios agent report --status in_review --sha YOUR_OWN_COMMIT_SHA --from-pad report.md

Use your own literal commit SHA, never `HEAD`, variables or foreign commits; omit `--sha` without one. Preserve task-declared cross-repository SHAs. Unfinished: STATUS `partial`/`blocked`, completed CHANGES, actual TESTS RUN (or 'none'), GATES REQUESTED: none, FOLLOWUP naming the remaining work or blocker:

domios agent report --status partial --from-pad report.md
domios agent report --status blocked --from-pad report.md

A `blocked` report only proposes a block; the owner or Planner decides. The Orchestrator closes tasks; never report `done`. Reread the assignment with `domios task show "$CODETERM_TASK_ID"`; without exported plan/task IDs, pass `--plan`/`--task` for `--status`. Read-only verifiers own no task: run only `## Authorized gates` and report to the parent without `--status`.
