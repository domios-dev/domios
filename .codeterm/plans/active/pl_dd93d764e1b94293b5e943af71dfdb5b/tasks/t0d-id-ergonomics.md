---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_caf291e932eb49cfbbe1cd81bb0b707c
handle: t0d-id-ergonomics
title: Short ids + parent-free refs + unified resolver
status: done
---

Short ids + parent-free task addressing + unified reference resolution (spec section id-ergonomics; user screenshot evidence of the current absurdity).

Steps:
1. Shared daemon-side resolver: ref -> entity for plan/task/spec/section/milestone accepting full id | unique id prefix (with/without type prefix) | handle. Ambiguous => error listing candidates; unknown => error; never guess. One resolver, used everywhere — no per-command reimplementations.
2. Parent-free addressing: plan task update/rm/assign, plan record --task, agent report --task work with the task ref alone; plan arg optional disambiguator. Same for section-scoped commands (spec section update, plan section update take section ref without spec/plan id when unique).
3. Short id minting for NEW entities: 6-8 char base36 slug with type prefix (task_x7k2mq), collision-checked at mint, salt regenerated on collision (service/plan/mutations.rs + spec.rs creation paths). Existing long ids remain valid refs forever; no migration, no shims.
4. Output pass: human-readable surfaces (plan get/state, task show, agent reports, hints) print short id/handle first; --json unchanged (full ids).
5. Tests: resolver matrix (prefix uniqueness, handle, old long id, ambiguity, unknown); CLI contract tests for parent-free forms; mint collision test.
6. Docs: ORCHESTRATION + API docs examples switch to short/parent-free forms; revision bumps.

Commands: ./build.sh check; narrowed cargo tests (single runs); ./build.sh cli; manual on dev 7686: update a task by handle alone, by 4-char prefix, by old long id.
Report: resolver semantics, id format chosen, before/after command examples, test output.
