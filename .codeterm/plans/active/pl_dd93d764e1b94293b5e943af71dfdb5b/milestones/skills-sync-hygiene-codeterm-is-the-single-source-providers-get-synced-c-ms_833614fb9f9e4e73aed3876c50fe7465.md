---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_833614fb9f9e4e73aed3876c50fe7465
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c-ms_833614fb9f9e4e73aed3876c50fe7465
title: decision
---

SCOPE ADDITION (round-78, Andrey): ORCHESTRATOR-SKILL DEDUPE. ~/.codeterm/skills/codeterm-orchestrator/ (machine skill folder, mem-indexed as skill_codeterm_orchestrator) duplicates the product's embedded docs/ORCHESTRATION — the canonical home for orchestration discipline is 'codeterm docs ORCHESTRATION' ONLY. Disposition: (a) diff the skill folder against docs/ORCHESTRATION; (b) GENERAL discipline content -> port anything unique into the embedded doc (via a normal lane, wiring/doc revision bump); (c) MACHINE-SPECIFIC content (the build-capacity section — the only part machines were meant to adapt) -> convert to a mem preference item (kind=preference, area=orchestration, per machine); (d) DELETE the skill folder + its mem index entry after porting (mem delete skill_codeterm_orchestrator); (e) docs/ORCHESTRATION gains a short note: machine/project deltas live in mem (kind=preference, area=orchestration) and are pulled at orchestrator start — see the auto-pull task. Same disposition-table discipline as the rest of this task: every line of the skill accounted for (ported/converted/dropped-with-reason).