---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_4bd24217e904497e93c31a9f0e9272b6
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c-ms_4bd24217e904497e93c31a9f0e9272b6
title: decision
---

BINDING SPEC (round-96, Andrey — SUPERSEDES round-95 'sync becomes the fix'): provider dirs (~/.claude/skills, ~/.agents/skills) hold EXACTLY the 'codeterm' skill (the 494-line universal CLI reference), NOTHING ELSE. mirror_skills_to_providers reduced to install ONLY codeterm; every other entry deleted from provider dirs and STAYS deleted (mirror can no longer resurrect). Personal skills (atlassian, nextcloud, product-bible, codeterm-orchestrator, etc.) live in canonical ~/.codeterm/skills + mem ONLY — agents reach them via mem, not provider-dir copies. Orphan guard INVERTED: any provider-dir entry that is NOT codeterm = flag. restart-does-not-resurrect is an ACCEPTANCE CRITERION (drill the real mirror fn). Porting order unchanged: codeterm-orchestrator general content -> docs/ORCHESTRATION, build-capacity -> mem preference, then DELETE from canonical. Worker's 'in sync OK x9' disposition was the wrong-spec misread (Planner's round-95 wording); same worker re-does with this spec.
