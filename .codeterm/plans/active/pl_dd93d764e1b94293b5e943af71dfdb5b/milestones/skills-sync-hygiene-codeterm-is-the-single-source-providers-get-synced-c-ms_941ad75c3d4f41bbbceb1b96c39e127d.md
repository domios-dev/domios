---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_941ad75c3d4f41bbbceb1b96c39e127d
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c-ms_941ad75c3d4f41bbbceb1b96c39e127d
title: decision
---

SECOND CORRECTION + RESURRECTION FINDING (round-95, supersedes round-81's 'no sync exists'): the sync EXISTS — src-tauri/src/commands/skills.rs:118-124 syncs canonical data_dir()/skills into ~/.claude/skills AND ~/.agents/skills (plus line 23: an exe-adjacent bundled skills source). My round-81 grep missed it by searching the literal path instead of join() construction; the provider.rs 'MUST NOT write' contract covers only the mem-bridge payload. CONSEQUENCE OBSERVED LIVE: all provider-dir deletions from round-82 RESURRECTED (prod daemon re-synced from canonical, which still holds codeterm-orchestrator pending the porting) — and a fresh orchestrator (Rossini) auto-loaded the STALE orchestrator skill at spawn, exactly the misleading-agents harm we deleted it to stop. REVISED EXECUTION ORDER for this task (the only order that sticks): (1) port codeterm-orchestrator content (general -> docs/ORCHESTRATION, build-capacity -> mem preference) and DELETE IT FROM CANONICAL first; (2) delete strays from canonical-adjacent sources (find mac-dev-test-skill's resurrection source — exe-adjacent bundle at skills.rs:23 or mem materialization); (3) THEN let the sync propagate the clean canonical to provider dirs (the sync becomes the fix, not the enemy); (4) the doctor drift check watches sync health, not manual copies. Read skills.rs:23-124 first — the flow is the spec.
