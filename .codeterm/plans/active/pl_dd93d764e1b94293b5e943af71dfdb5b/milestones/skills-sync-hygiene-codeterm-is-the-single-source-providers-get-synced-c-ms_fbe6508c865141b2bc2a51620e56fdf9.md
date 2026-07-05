---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_fbe6508c865141b2bc2a51620e56fdf9
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c-ms_fbe6508c865141b2bc2a51620e56fdf9
title: decision
---

CORRECTION (round-83, Andrey — supersedes the 'compact pointer' framing from round-81/82): the codeterm skill STAYS THE FULL CLI REFERENCE (494 lines: env, panes/workspaces, spawning, message routing, mesh, cron, services, telegram bridge, updates, plugins, usage, sessions, keepawake, config, mem, built-in docs section). Rationale: it is the universal entry point for EVERY agent on the machine — general agents, personal assistants, one-off Claude/Codex sessions — not only orchestration roles; they all learn to drive CodeTerm from it. It ALREADY ends with 'Built-in docs (read on demand)' pointing at codeterm docs — the pointer function exists as a section, not as the whole skill. REVISED remaining scope: (1) do NOT shrink or rewrite the codeterm skill; ensure canon (~/.codeterm/skills/codeterm) is the maintained source and the single provider-dir copy matches it (the drift found earlier gets reconciled: diff, port wanted edits to canon, overwrite copies; document the manual update step since no sync exists by contract); (2) codeterm-orchestrator porting + deletion unchanged (general -> docs/ORCHESTRATION, build-capacity -> mem preference); (3) doctor orphan/drift check unchanged — it now also guards the codeterm skill copy staying equal to canon.