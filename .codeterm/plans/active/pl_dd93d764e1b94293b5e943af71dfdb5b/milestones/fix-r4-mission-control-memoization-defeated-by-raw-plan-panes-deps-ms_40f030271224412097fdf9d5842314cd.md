---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_40f030271224412097fdf9d5842314cd
handle: fix-r4-mission-control-memoization-defeated-by-raw-plan-panes-deps-ms_40f030271224412097fdf9d5842314cd
title: report_refused
---

PARTIAL: REPORT NOT SENT
readiness report refused: this git worktree has uncommitted lane changes.
Next commands:
  git status --short
  git add <files>
  git commit -m "<message>"
  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."
Or report WIP with:
  codeterm agent report --status partial "..."
Dirty lane files:
  src-tauri/src/service/plan/body_file_tests.rs
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/active-work.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/canonical-sidecar.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/cleanup-hygiene.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/competitive-execution.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/id-ergonomics.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/loop-mechanics.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/mission-control-canvas-requirements-round-8.md

Original readiness report that was refused locally:
Cleanup stand-down:
- worktree ../codeterm-fix-mission-control-memo: removed
- local branch lane/fix-mission-control-memo: deleted
- remote origin/lane/fix-mission-control-memo: still present — git push --delete aborted by feat/canvas-updates pre-push clippy (unrelated src-tauri/src/service/plan/body_file_tests.rs:154 compile error). gh API delete unavailable (404/auth). Orchestrator: delete remote lane manually or after gate green.

Standing down. No further work from this worker.
