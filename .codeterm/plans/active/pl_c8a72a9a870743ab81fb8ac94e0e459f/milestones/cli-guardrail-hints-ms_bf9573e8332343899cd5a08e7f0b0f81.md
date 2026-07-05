---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_bf9573e8332343899cd5a08e7f0b0f81
handle: cli-guardrail-hints-ms_bf9573e8332343899cd5a08e7f0b0f81
title: status_update
---

Fixed CLI compile failure on feat/cli-hints and force-pushed 50632812. Root cause was PlanAction::Create returning Result<Option<Value>> inside run_plan's fall-through match while the match is intended to return Result<Value> and be wrapped once by result.map(Some). Changed the arm to return call_plan_rpc(...) directly. Verified with cargo build --manifest-path cli/Cargo.toml and cargo test --manifest-path cli/Cargo.toml hints -- --nocapture. Kept BE-fix batch atomicity and cli-hints behavior intact.
