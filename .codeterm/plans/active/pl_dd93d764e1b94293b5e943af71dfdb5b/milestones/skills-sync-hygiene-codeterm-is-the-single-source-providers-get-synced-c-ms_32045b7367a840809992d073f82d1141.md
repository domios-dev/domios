---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_32045b7367a840809992d073f82d1141
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c-ms_32045b7367a840809992d073f82d1141
title: status_update
---

EXECUTED BY PLANNER (round-82, direct user order 'удали или перенеси'). Done by hand: (1) MOVED to canon + mem-registered: check-disk-space (real infra skill -> skill_check_disk_space), mem-testing (real A/B bench procedures -> skill_mem_testing_...); (2) DELETED: mac-dev-test-skill (unfilled scaffold, placeholders verbatim) from both provider dirs; (3) DELETED all clones from ~/.claude/skills and ~/.agents/skills — both now contain EXACTLY ONE entry: codeterm (the pointer skill, per Andrey's architecture). Canon ~/.codeterm/skills holds 9 (7 original + 2 moved). REMAINING SCOPE for this task (unchanged): rewrite the codeterm skill as the compact pointer ('knowledge lives in codeterm docs; orchestrator -> codeterm docs ORCHESTRATION'); port codeterm-orchestrator content (general -> docs/ORCHESTRATION lane, build-capacity -> mem preference) then delete it from canon + mem index; doctor check for orphans/drift. The actively-harmful part (stale orchestrator clones feeding agents) is DEAD as of now.
