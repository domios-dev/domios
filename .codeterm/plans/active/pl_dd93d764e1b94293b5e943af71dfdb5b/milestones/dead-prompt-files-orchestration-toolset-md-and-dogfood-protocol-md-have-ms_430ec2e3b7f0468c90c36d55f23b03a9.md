---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_430ec2e3b7f0468c90c36d55f23b03a9
handle: dead-prompt-files-orchestration-toolset-md-and-dogfood-protocol-md-have-ms_430ec2e3b7f0468c90c36d55f23b03a9
title: status_update
---

IN_REVIEW: STATUS: done

BRANCH: lane/fix-dead-prompt-files @ 26c9ecde (2 commits ahead of c00f6fc6)

ORPHANING FORENSICS:
- orchestration_toolset.md: orphaned since a0957f6d (never had include_str! loader; created as doc pointer, never wired)
- dogfood_protocol.md: orphaned since 7156665c (created for dogfood-session instructions, never wired)
- Confirmed via: git log -S include_str -- src-tauri/src/prompts.rs → zero hits for either file

WIRE/DELETE DECISIONS:
- orchestration_toolset.md → DELETED. All content (planning mechanics pointer to codeterm docs ORCHESTRATION, planning ownership, heartbeat discipline) already present verbatim in orchestrator_suffix.md (loaded). Zero unique live-worthy content.
- dogfood_protocol.md → DELETED. Generic worker-reporting rules already in worker_suffix.md. Dogfood-specific rules 4+5 are session-contextual, not universally applicable.

TEXT D PLACEMENT (for task_dc526c35):
TEXT D must land in orchestrator_suffix.md (src-tauri/prompts/orchestrator_suffix.md).
Loaded via ORCHESTRATOR_SUFFIX const with include_str!(). This is where all live orchestration toolset content now lives after deletion.

STRUCTURAL GUARD TEST:
Added every_prompt_md_has_a_loader in src-tauri/src/prompts.rs tests mod.
RED: test failed with 'orphaned prompt files: [orchestration_toolset.md, dogfood_protocol.md]'
GREEN: test prompts::tests::every_prompt_md_has_a_loader ... ok after deletion
All 20 prompts tests pass.

FILES CHANGED:
- src-tauri/prompts/dogfood_protocol.md (deleted)
- src-tauri/prompts/orchestration_toolset.md (deleted)
- src-tauri/src/prompts.rs (+47 lines: every_prompt_md_has_a_loader test, fmt fix)

BUILD/TYPECHECK/TESTS:
- scripts/test-capped.sh -- prompts:: → 20 passed; 0 failed
- Isolated guard test ran: RED then GREEN confirmed
- cargo fmt clean; pre-push light gate passed

FOLLOWUP:
task_dc526c35 (prompt-surgery) should target orchestrator_suffix.md for TEXT D — orchestration_toolset.md is gone. No dead file risk.
