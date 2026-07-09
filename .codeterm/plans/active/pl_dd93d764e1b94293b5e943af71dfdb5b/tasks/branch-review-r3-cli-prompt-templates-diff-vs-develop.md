---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_018bce1a101443898ca3dd4c3c604e9f
handle: branch-review-r3-cli-prompt-templates-diff-vs-develop
title: 'Branch review R3: CLI + prompt templates (diff vs develop)'
status: done
---

Review git diff origin/develop...HEAD (merge-base 40ba275ffc43) RESTRICTED to cli/src/, src-tauri/src/prompts.rs, src-tauri/prompts/*.md, src-tauri/src/service/plan/peer_mandate.rs. Hunt CRITICAL/HIGH only: (1) CLI commands that can silently target the WRONG daemon (prod 7685 vs dev 7686 env leakage), destructive commands without confirmation, panics on malformed server responses, exit codes lying about failure; (2) prompt placeholder audit — every {placeholder} used in every prompts/*.md must have a substitution in src-tauri/src/prompts.rs (known-good: plan_skills_principles, planner_testing_methodology both substituted+tested — verify the REST: general_agent.md has 8 placeholders, dreaming_rem.md, mem_extraction.md, bootstrap_instructions.md); (3) contradictions between prompt files that would make an agent misbehave; (4) personal data (real names/machines/emails) leaked into prompts/ — must be generic. OUTPUT CONTRACT: strides + detailed fix tasks for cheap models, same as R1.
