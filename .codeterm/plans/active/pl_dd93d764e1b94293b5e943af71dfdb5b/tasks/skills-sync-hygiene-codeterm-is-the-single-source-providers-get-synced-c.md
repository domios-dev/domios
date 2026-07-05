---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_cd58cc08fda74e689bd0b8b66ed7c575
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c
title: 'Skills sync hygiene: ~/.codeterm is the single source, providers get synced copies only'
status: pending
---

PLANNER AUDIT FINDINGS (round-77, user rule: everything lives in ~/.codeterm ONLY; provider dirs hold synced copies): (1) STRAYS: ~/.claude/skills contains check-disk-space, mac-dev-test-skill, mem-testing — ABSENT from canonical ~/.codeterm/skills (7 canonical: atlassian, codeterm, codeterm-orchestrator, file-share-nextcloud, loom-transcript-fetch, nextcloud-admin, product-bible-author); determine per stray via the sync code + mem registry (codeterm mem skill list): registered-elsewhere (then sync should own or remove it) vs hand-written orphan (then migrate into ~/.codeterm via codeterm mem skill add and remove the stray, or delete if obsolete — mem-testing looks like a leftover test artifact). (2) DRIFT: ~/.claude/skills/codeterm/SKILL.md DIFFERS from ~/.codeterm/skills/codeterm/SKILL.md — either stale sync (fix: resync + verify the sync trigger fires on canonical edits) or hand edits to the copy (fix: port wanted edits to canonical, overwrite copy); diff both ways and disposition line-by-line. (3) ~/.agents/skills same audit (mirrors .claude). (4) ~/.claude/agents/{sub-worker,team-lead}.md — Claude-Code harness agent defs, not skills: verify they are deliberate (used by the Agent tool) and document them as allowed, or relocate. (5) GUARD: extend the sync mechanism (or a doctor check) to detect provider-dir entries with no canonical source and report them — the class dies structurally. Report a disposition table: entry -> provenance -> action taken.
