---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_d20d9e5047a8417da906f0d662bc9478
handle: dead-prompt-files-orchestration-toolset-md-and-dogfood-protocol-md-have-ms_d20d9e5047a8417da906f0d662bc9478
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
  cli/src/args/cli.rs
  cli/src/cmd/hints.rs
  cli/src/cmd/manifest.rs
  cli/src/cmd/plan/tests.rs
  cli/src/cmd/plan_errors.rs
  cli/src/cmd/plan_render.rs
  cli/src/main.rs
  docs/CLI.md

Original readiness report that was refused locally:
STATUS: done

BRANCH: lane/fix-dead-prompt-files @ 26c9ecde (2 commits ahead of c00f6fc6)

ORPHANING FORENSICS:
- orchestration_toolset.md: orphaned since a0957f6d (never had include_str! loader; created in a0957f6d as doc pointer, never wired)
- dogfood_protocol.md: orphaned since 7156665c (created for dogfood-session instructions, never wired)
- Confirmed via: git log -S 'include_str' -- src-tauri/src/prompts.rs → zero hits for either file

WIRE/DELETE DECISIONS:
- orchestration_toolset.md → DELETED. All content (planning mechanics pointer to codeterm docs ORCHESTRATION, planning ownership, heartbeat discipline) is already present verbatim in orchestrator_suffix.md which IS loaded. Zero unique live-worthy content.
- dogfood_protocol.md → DELETED. Generic worker-reporting rules already in worker_suffix.md. Dogfood-specific rules 4+5 (debug the system by using it, flag friction) are session-contextual, not universally applicable. Not wired anywhere.

TEXT D PLACEMENT (for task_dc526c35):
TEXT D must land in orchestrator_suffix.md (src-tauri/prompts/orchestrator_suffix.md).
That is the file with the active include_str!() loader (ORCHESTRATOR_SUFFIX const) and where all live orchestration toolset content now lives after the deletion.

STRUCTURAL GUARD TEST:
Added every_prompt_md_has_a_loader test in src-tauri/src/prompts.rs tests mod.
- RED evidence: test failed with 'orphaned prompt files: [orchestration_toolset.md, dogfood_protocol.md]' before deletion
- GREEN evidence: test prompts::tests::every_prompt_md_has_a_loader ... ok after deletion
- All 20 prompts tests pass (test result: ok. 20 passed; 0 failed)

FILES CHANGED:
- src-tauri/prompts/dogfood_protocol.md (deleted)
- src-tauri/prompts/orchestration_toolset.md (deleted)
- src-tauri/src/prompts.rs (+47 lines: every_prompt_md_has_a_loader test)

BUILD/TYPECHECK/TESTS:
- scripts/test-capped.sh -- prompts:: → 20 passed; 0 failed
- Specific guard test ran isolated (RED then GREEN confirmed)
- cargo fmt clean; pre-push hook passed

FOLLOWUP:
task_dc526c35 (prompt-surgery) can now safely target orchestrator_suffix.md for TEXT D — orchestration_toolset.md is gone. No dead file risk.
