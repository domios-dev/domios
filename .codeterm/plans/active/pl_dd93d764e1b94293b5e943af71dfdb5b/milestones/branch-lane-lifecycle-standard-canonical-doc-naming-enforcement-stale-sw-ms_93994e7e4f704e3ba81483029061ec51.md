---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_93994e7e4f704e3ba81483029061ec51
handle: branch-lane-lifecycle-standard-canonical-doc-naming-enforcement-stale-sw-ms_93994e7e4f704e3ba81483029061ec51
title: decision
---

CHARTER ADDITION (round-77, Andrey verbatim intent: 'Бери пики под запрет тотальный'): CHERRY-PICKS ARE BANNED, totally. Lifecycle doc gains the rule: lane content moves ONLY by merge (or rebase of an unmerged lane onto the target head) — never cherry-pick; cherry-picking duplicates commits with new shas, breaking merged-vs-unmerged tracking ('иначе мы не знаем что смерджено что нет'), ancestry-based cleanup (auto-cleanup's merged check), and plan lanes verdicts. Worktrees must merge normally; branching stays simple: lane forks from target head, merges back whole. Exception path: none in the standard; salvaging partial work = new lane from head + manual porting, recorded as a stride. Enforcement stitch (small): the report refusal warns when HEAD's ancestry does not include the recorded lane branch point (cherry-picked/duplicated content smell) — warn-tier, not refuse (detection is heuristic).