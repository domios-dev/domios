---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_6cfc6aeb930040319a1767ec01161876
handle: w0d-id-ergonomics
title: Wave 0d — Short ids and reference resolution
---

Wave 0d (CLI/backend ergonomics lane; parallel with waves 0/0b/0c; not gating P0/P1 or UX lanes but strongly wanted for dogfood comfort):

Short ids + parent-free task addressing + unified reference resolution.

Implementation areas:
- Id minting: service/plan id generation (plan/spec/task/section/milestone creation paths in src-tauri/src/service/plan/mutations.rs and spec.rs) switches to short base36 slugs (6-8 chars, store-collision-checked, regenerate salt on collision), keeping type prefixes.
- Reference resolution: one shared resolver (daemon-side) accepting full id | unique prefix | handle for plans, tasks, sections, specs; wired through every CLI arg that names an entity (cli/src/args + cmd/plan*, cmd/agent) and REST handlers that accept ids. Ambiguity/unknown => loud error with candidates.
- Parent-free addressing: task-scoped commands (task update/rm/assign, plan record --task, agent report --task) resolve the owning plan from the task ref; plan arg optional. Apply the same to spec-section and plan-section scoped commands.
- Output surfaces: human-readable outputs print short id/handle; --json keeps full ids.
- Tests: resolver unit tests (prefix uniqueness, handle refs, ambiguity errors, old long ids still resolve); CLI contract tests for parent-free forms (cli/src/subcommand_contract_tests.rs pattern).

Verification: ./build.sh check; narrowed cargo tests; manual: update a task by handle alone, by 4-char prefix, and via old long id on dev 7686.
