---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_c40d72382ef243feb71829a247d226c1
handle: branch-review-r2-rust-daemon-core-locks-async-discipline-api-diff-vs-dev-ms_c40d72382ef243feb71829a247d226c1
title: status_update
---

STATUS: done
CHANGES: none in product code; created planner fix task task_c979592e876847c686f48020d3553a48 and marked review task in_review
BUILD/TYPECHECK/TESTS: not run; read-only review only. Commands run: isolated worktree creation; scoped git diff/stat review; explicit new unwrap audit via git diff origin/develop...HEAD -- src-tauri/src/api src-tauri/src/socket/server | rg '^+.*unwrap()'; focused source inspection for PTY backend reader race, output stream locks, API/socket handlers, provider capability boundary.
FOLLOWUP: Findings: 1 HIGH, 0 CRITICAL. HIGH: src-tauri/src/service/general_agent/workspace.rs:6-16 adds BUNDLED_CHAT_UI_PROVIDERS = [claude,codex,opencode,cursor] and default_chat_view_for_provider branches on provider-id literals instead of AgentProvider::supports_chat_ui()/registry capability. This violates the plugin/provider boundary and misclassifies non-bundled chat-UI-capable providers during General Agent first-run/workspace setup. Fix task: task_c979592e876847c686f48020d3553a48. Unwrap triage: new unwrap() in src-tauri/src/api + src-tauri/src/socket/server is test-only or constrained/infallible route conversion (serde_json simple structs, Retry-After numeric header); no panic-reachable-from-request unwrap found. prepare_backend_reader race fix not regressed: reader clone + backend channel registration still happen before pane link/visibility; blocking PTY reads remain under spawn_blocking.
