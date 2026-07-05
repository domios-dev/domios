---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_89b85a2b688949eabbd9a6d6cdb7fc2e
handle: t0d-id-ergonomics-ms_89b85a2b688949eabbd9a6d6cdb7fc2e
title: status_update
---

IN_REVIEW: STATUS: in_review
Wave 0d — short ids + parent-free refs + unified reference resolver — implemented on branch lane/t0d-id-ergonomics (commit eae828c7).

CHANGES:
- src-tauri/src/plan/ids.rs (new): short prefixed base36 minter (6-8 chars, collision-checked, salt regenerated on collision).
- src-tauri/src/service/plan/resolve.rs (new): unified resolver resolve_{plan,spec}_id / resolve_{task,section}_ref / resolve_spec_section_id + id_taken. Precedence exact-id > exact-handle > unique-prefix; ambiguity lists candidates; unknown never guesses. Inline resolver-matrix tests.
- src-tauri/src/socket/server/plan_refs.rs (new): RPC-boundary ref resolution; plan_id optional for task/section-scoped commands.
- Minting wired into create_plan/create_spec/add_task/add_section/add_milestone (store collision-checked) + all gen_id helpers (batch/spec/ingest/heartbeat/loop/canvas/attempts) now short. Plan::new/Spec::new short. Existing long uuid ids resolve unchanged.
- Every plan/task/section/spec RPC handler (plan.rs, plan_read/elements/goal/handoff/loops/mode, spec.rs, plan_role_helpers) resolves refs via the shared resolver; task.show/take/drop upgraded to prefix+ambiguity.
- CLI: plan task update/rm/assign/close accept task ref alone (plan optional 2nd positional); split_task_ref remap; plan_task_update sends plan_id only when present.

BUILD/TYPECHECK/TESTS:
- ./build.sh check: EXIT 0 (clean, no warnings).
- ./build.sh cli: EXIT 0 (installed to ~/.codeterm/bin + ~/.codeterm-dev/bin).
- cargo test service::plan::resolve::tests: 9 passed (handle, 4-char prefix, ambiguity+candidates, legacy long id, parent-free, section, id_taken, exact-handle-wins, unknown-not-found).
- cargo test plan::ids: 4 passed (short/prefixed, collision escalation 6->7, 2000-unique, base36).
- cargo test (cli) plan_ref: 3 passed (parent-free update/rm parse, plan+task parse).
- File-size discipline: mod.rs 1012->1009, socket/server/plan.rs 1043->1036 (both shrank below baseline); no new file over 1000 (contract tests kept at 949 by moving new tests to cli/src/plan_ref_contract_tests.rs).
- Pre-existing unrelated failures on this branch (9: markdown-trailing-newline body tests + search_index embedding vector_count) confirmed via git stash to reproduce WITHOUT my changes — not caused by this lane.

FOLLOWUP:
- Live GUI smoke on dev 7686 NOT run: the running 7686 daemon is build 0f4e1d96 (my HEAD) but does NOT include my uncommitted work; restarting the shared dev would disrupt the active sibling lanes (e2e-gate cursor pane, executor-role). Recommend the orchestrator run 'plan task update <handle|4-char-prefix|long-id> --status ...' against a dev rebuilt from this branch to confirm end-to-end. All resolver logic is unit-covered on a real PlanStore.
- plan record (batch path) still requires an explicit plan positional (batch is plan-scoped by design); task/section selectors in batch remain id||handle exact. Left as-is; can extend if wanted.
