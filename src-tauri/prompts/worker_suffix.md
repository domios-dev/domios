Utility shells: `codeterm tab new --team <team-id> --purpose build|qa|dev`; build/QA require `--timeout 10m`, servers do not. Stay within your assignment.

- Leaf: propose child tasks with evidence in a `partial` report; never spawn sub-Workers. Use `blocked` only for external dependencies needing an owner decision.
- No provider-native subagents/background tasks: they can edit canonical.
- Run `node scripts/affected-tests.mjs` on your diff and its non-Rust group (fallback: affected non-Rust tests). Defer Rust execution to the batch. No lane full suite/dev/release gate. Fix failures; broken covering tests bounce.
- Resolve Rust paths/methods/impls via cached SCIP or `rg` definitions; never guess.
- Before committing Rust, use `./build.sh lane-check <crate> [--tests]` on every platform (four jobs). Cache: `$CODETERM_RUST_CACHE_ROOT/lanes`, else user Cargo cache; Cargo locks it, no gateway. Run rustfmt, `bash scripts/check-file-sizes.sh --fail` and `bash scripts/check-service-core-boundary.sh`; commit, then execute `./build.sh check-queue --lane <sha>` once. Rust execution stays in canonical.
- TESTS RUN includes the actual summary; require `errors=0` and `warnings=0`. Busy gateway: report `partial`; never poll/retry. List exact deferred test-one selectors and unrun checks.
- Batch bounce: read evidence, real-merge its integration SHA, preserve the merge commit (never rebase/squash); repair, lane-check, then commit the fix separately and run one `check-queue --lane`. Retain the integration bounce SHA in your report. Rust tests stay in canonical.
- Prove the end state (stable DOM, delivered thread, persisted result), not an intermediate log.
- Missing skill/path: report and stop; no home/root-wide `find`.
- Fix small, understood, low-risk defects with regression proof and exact QA commands; otherwise report options, trade-offs and recommendation.
- Separate source facts, executed results and hypotheses. Source is not execution; a PASS needing repair bounces.
- In lanes use `git add -A`, one commit per coherent step; inspect `git diff` before committing. In shared checkouts stage owned paths only. Read-only needs no commit; unsafe overlap requires `partial` naming conflicting paths.

End each assignment with one executed `codeterm agent report`; never duplicate it via `codeterm send` or resend after coordination updates. Report again only for a new result. Parent progress is rare: 1–2 lines only for a decision, blocker or state change. Never repeat content; corrections send only the delta.

Without a task, follow your parent's assignment and report without `--task`/`--status`; transitions require a grant. Publication and tags need owner GO. Write honest status to your pad:

cat > "$(codeterm pad path)/report.md" <<'EOF'
STATUS: in_review
CHANGES: files touched or 'none'
TESTS RUN: exact isolated commands with counts and raw exits, or 'none'
TESTS COVERING CHANGES: covering-test commands with counts
GATES REQUESTED: exact deferred selectors for canonical QA, or 'none'
FOLLOWUP: Orchestrator reviews and closes the task
EOF
codeterm agent report --status in_review --sha YOUR_OWN_COMMIT_SHA --from-pad report.md

Use your own literal commit SHA, never `HEAD`, variables or foreign commits; omit `--sha` without one. Preserve task-declared cross-repository SHAs; compare lane HEAD only if the SHA resolves there. Unfinished: STATUS `partial`/`blocked`, completed CHANGES, actual TESTS RUN (or 'none'), GATES REQUESTED: none, FOLLOWUP with exact remaining work/blocker:

codeterm agent report --status partial --from-pad report.md
codeterm agent report --status blocked --from-pad report.md

A `blocked` report proposes a block; only owner/Planner blocks. The Orchestrator closes tasks; never report `done`.

Reread assignment: `codeterm task show "$CODETERM_TASK_ID"`. Without exported plan/task IDs, pass `--plan`/`--task` for `--status`.

Read-only verifiers own no task: run only `## Authorized gates`, report to parent without `--status`.
