---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_fee4e0c77ef342978df4689e510daa5b
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c-ms_fee4e0c77ef342978df4689e510daa5b
title: status_update
---

STATUS: in_review

CHANGES: src-tauri/src/commands/skills.rs + docs/ORCHESTRATION.md (a924c1c8)

ARCHITECTURE EXECUTED (new spec):

1. PORTING codeterm-orchestrator — DONE
   - General discipline (Operating Model, Dispatch, Heartbeat, Verification Gates, Acceptance, State Reconciliation, Review Discipline, Model Routing, Agent/Pane Hygiene, Sharp Edges, Usage check) → docs/ORCHESTRATION.md 'Orchestrator Role Discipline' section (revision 28)
   - Machine-specific build constraints → mem preference id: preference_mac_air_orchestration_constrained_build_machine_serial_gate
   - Deleted from canonical ~/.codeterm/skills/codeterm-orchestrator ✓
   - mem record skill_codeterm_orchestrator updated to note migration

2. mac-dev-test-skill resurrection source — IDENTIFIED
   - Was NEVER in canonical; was NEVER in bundled (repo-root skills/ has only codeterm/). It was placed directly in provider dirs by some tool (not by the mirror). The old mirror could never resurrect it. New mirror (codeterm-only) structurally prevents any non-codeterm entry from being re-created.

3. mirror_skills_to_providers() REWRITTEN
   - Installs ONLY 'codeterm' skill to provider dirs
   - Actively PRUNES any non-codeterm entry (logs warn + rm -rf)
   - Extracted pure inner fn mirror_into_providers(canonical, targets) with injectable paths for testability

4. Provider dir cleanup — DONE
   - ~/.claude/skills: ONLY codeterm remains
   - ~/.agents/skills: ONLY codeterm remains
   - Deleted: atlassian, check-disk-space, codeterm-orchestrator, file-share-nextcloud, loom-transcript-fetch, mem-testing, nextcloud-admin, product-bible-author (all from both provider dirs)

5. Orphan guard INVERTED (inside mirror_into_providers): any non-codeterm dir in provider is logged + removed on every mirror pass

TEST — restart-does-not-resurrect drill (calls REAL mirror_into_providers):
   mirror_installs_only_codeterm_and_prunes_strays:
   - canonical: codeterm + atlassian
   - provider: stray
   - after mirror_into_providers: provider has codeterm, stray GONE, atlassian never copied
   - asserts all three conditions on the real code path

NOT RUN: cargo check (orchestrator gate). ~/.claude/agents/{sub-worker,team-lead}.md: harness agent defs, not skills, left as-is.

FOLLOWUP: GATES REQUESTED — cargo check src-tauri; cargo test commands::skills::mirror_tests::mirror_installs_only_codeterm_and_prunes_strays
