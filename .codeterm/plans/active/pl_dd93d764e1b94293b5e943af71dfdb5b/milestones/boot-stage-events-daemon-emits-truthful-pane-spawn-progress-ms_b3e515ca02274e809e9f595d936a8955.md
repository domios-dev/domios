---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_b3e515ca02274e809e9f595d936a8955
handle: boot-stage-events-daemon-emits-truthful-pane-spawn-progress-ms_b3e515ca02274e809e9f595d936a8955
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/boot-stage-events (pushed, sha 4b7b441a)

CHANGES:
- NEW src-tauri/src/service/boot_stage.rs — BootStage enum (pty_spawned|shell_ready|agent_launching|agent_ready|spawn_failed, serde snake_case), BootStageSink trait, ServiceContext::emit_boot_stage (sets pane.boot_stage + emit_everywhere 'pane-boot-stage' — native webview + SSE, fire-and-forget). HAPPY_PATH const = single source of truth for order. +3 unit tests.
- launch.rs (launch_agent_in_background): emits at REAL transitions, no timers — PtySpawned after PTY claimed (both frontend+backend paths), ShellReady after readiness strategy, AgentLaunching before send_shell_spawn_verified, AgentReady after task delivery. SpawnFailed(reason) on all 3 error returns (backend spawn, launch_cmd, task delivery).
- multiplexer/types.rs Pane: added boot_stage: Option<BootStage> (skip_serializing_if None) — in live snapshot for late attachers, omitted from hand-built session snapshot. +mid-boot snapshot test. Field added to all 5 Pane literals.
- service/mod.rs: registered pub mod boot_stage.
- src/lib/types.ts: mirrored BootStage union + PaneBootStageEvent + Pane.boot_stage.

TESTS (per task body, all 3 present, NOT run — see below):
- boot_stage::tests::happy_path_emits_stages_in_order (ordered mocked-emitter)
- boot_stage::tests::failure_path_ends_at_spawn_failed_with_reason_and_no_ready
- multiplexer::types::tests::mid_boot_snapshot_exposes_current_stage

BUILD/TYPECHECK/TESTS: NOT RUN. Started 'CODETERM_API_PORT=7686 ./build.sh check' (cargo check), orchestrator directive hit mid-run (machine load >40, no cold lane builds) — terminated it (SIGTERM, exit 144). Verified zero lingering cargo/rustc from this worktree. No Rust build re-attempted per directive.

FOLLOWUP (orchestrator gate, run when load sane):
- Rust check:  CODETERM_API_PORT=7686 ./build.sh check
- Focused tests: scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml boot_stage
                 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml mid_boot_snapshot
- Live dev integration (real agent spawn): subscribe SSE/webview 'pane-boot-stage', spawn an agent, assert ordered sequence pty_spawned->shell_ready->agent_launching->agent_ready with post-race-fix deltas; and a failure-injection spawn yields spawn_failed+reason. This live event gate is left to orchestrator (needs real agents).
- FE consumption (reducer listener / busy-state UI) intentionally out of scope here — belongs to the 'Universal busy states' task; only the TS type contract is landed.
