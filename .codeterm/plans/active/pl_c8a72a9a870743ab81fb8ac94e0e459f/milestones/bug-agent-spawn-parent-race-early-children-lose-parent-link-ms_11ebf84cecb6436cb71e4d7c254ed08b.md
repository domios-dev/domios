---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_11ebf84cecb6436cb71e4d7c254ed08b
handle: bug-agent-spawn-parent-race-early-children-lose-parent-link-ms_11ebf84cecb6436cb71e4d7c254ed08b
title: status_update
---

Integrated the spawn parent race lane into the hot checkout.

Verified by Orchestrator:
- cargo test --manifest-path src-tauri/Cargo.toml early_workers_under_uncommitted_orchestrator_all_link --lib -- --nocapture: passed
- cargo test --manifest-path src-tauri/Cargo.toml multiplexer::state::role::tests --lib -- --nocapture: 7 passed
- CODETERM_API_PORT=7686 ./build.sh check: passed
- git diff --check: passed
- live dev smoke after restart: `codeterm agent spawn codex --role Worker --parent fcb00202 ...` returned pane 8e748585, and dev `pane list` showed it nested under parent fcb00202.

Notes:
- The fix auto-promotes a free top-level parent to Orchestrator when attaching a Worker, preserving the parent link for rapid early spawns.
- Spawn callers now fail/close the newly-created tab if role application or post-condition verification fails instead of silently leaving an unlinked worker.
