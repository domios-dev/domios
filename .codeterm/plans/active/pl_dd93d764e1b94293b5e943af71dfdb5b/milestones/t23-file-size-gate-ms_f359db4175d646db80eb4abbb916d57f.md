---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f359db4175d646db80eb4abbb916d57f
handle: t23-file-size-gate-ms_f359db4175d646db80eb4abbb916d57f
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: Split oversized files by responsibility: MessageBubble helpers moved to packages/chat-ui/src/MessageBubble.helpers.ts; plan exported DTO/error types moved to src-tauri/src/service/plan/types.rs; task-query tests moved to src-tauri/src/service/plan/tests/task_queries.rs; socket lane metadata parsing moved to src-tauri/src/socket/server/plan_lane_fields.rs. Target files now: MessageBubble.tsx 892, plan/mod.rs 759, plan/tests.rs 949, socket/server/plan.rs 978.
BUILD/TYPECHECK/TESTS: PASS bash scripts/check-file-sizes.sh; PASS git diff --check and staged diff --check; PASS CODETERM_API_PORT=7686 ./build.sh check after generating dist; PASS pnpm --filter @codeterm/chat-ui exec tsc --noEmit --jsx react-jsx --moduleResolution bundler --module esnext --target es2022 --strict src/MessageBubble.tsx src/MessageBubble.helpers.ts; PASS pnpm exec vite build; PASS CODETERM_API_PORT=7686 cargo test --manifest-path src-tauri/Cargo.toml service::plan::tests::task_queries -- --nocapture. Full pnpm exec tsc --noEmit is blocked by pre-existing Canvas errors around parked TaskStatus/CANVAS_* imports/layout tests, unrelated to this lane.
FOLLOWUP: Review commit fe3b4d5a61ceb2052dd93bec8eae2d24a7b70fd0 on lane/t23-file-size-gate; worktree clean.
