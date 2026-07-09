---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_a222d06bc2814ca398de09ac1ceb5185
handle: plan-skills-store-cli-ledger-mem-indexing-recall-pointer
title: 'Plan skills: store, CLI, ledger, mem indexing, recall pointer'
status: done
---

## What

Plan-scoped skills: know-how artifacts that live INSIDE a plan and evolve during execution. An agent that figures out "how we test canvas changes in this effort" or writes a seeding script records it as a plan skill; every later agent finds and reuses it.

## Model & storage

- Skill = folder in the plan sidecar: `.codeterm/plans/<id>/skills/<slug>/SKILL.md` (+ optional `scripts/`). SKILL.md: frontmatter (title, purpose, optional check command) + body. Follows the machine-level mem skill folder convention — one format everywhere.
- Every create/edit writes a working point on the PLAN (kind `skill`): slug, keep|discard|superseded verdict, one-line rationale — the append-only ledger from the autoresearch pattern (results.tsv analog). No silent edits.

## CLI

- `codeterm plan skill new <plan> <slug> --title ... [--check "cmd"]` — scaffold + register.
- `codeterm plan skill list/show/edit <plan> [slug]` — list prints slug+title+purpose one-liners (the registry view); edit re-indexes.
- Zero-flag: plan resolves from pane binding (existing resolution).

## Mem integration (user requirement)

Index SKILL.md into mem scoped to the plan on create/edit (Librarian re-index on change, same as machine skills). Result: ONE `codeterm mem search` returns matching plan skills alongside tasks and facts as separate blocks — the agent asks once, gets everything relevant. Degrade gracefully when mem is unavailable (skills still work from the sidecar).

## Recall pointer (the "remembering" mechanism)

NOT full skills in heartbeats. One registry line, injected where charters/heartbeats already assemble: "Plan skills (N): slug1, slug2… — read/extend: codeterm plan skill …". Appears in the orchestrator heartbeat block and the worker charter when the plan has >=1 skill. Shared constant, dedup discipline.
