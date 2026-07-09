---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_70c4a935dd6a4abf87a679915a3636b5
handle: plan-skills-emergent-principles-in-prompts-testing-methodology
title: 'Plan skills: emergent principles in prompts + testing methodology'
status: done
---

## What

Teach the agents the emergent principles — prompts and docs, written so the behavior emerges rather than being a checklist. Shared constants (dedup discipline), consumed by orchestrator suffix, planner suffix, worker charter, ORCHESTRATION doc.

## The principles to encode (from references/autoresearch — see the research working point on the plan)

1. **Skills are the plan's org-code.** When you learn something reusable in THIS effort — a test recipe, a seeding script, a gotcha — record it as a plan skill instead of leaving it in chat. Improve existing skills before writing new ones.
2. **Separate harness from target.** A good skill names what is FIXED (the oracle, the metric, the eval command) and what is MUTABLE. Never let an agent edit the oracle to make results look better.
3. **Measurable over vibes.** A skill with a check command (--check) is worth two without; keep/discard decisions cite the check.
4. **Simplicity criterion.** Complexity is a cost: a marginal gain that adds hacky steps is a loss; a deletion that preserves results is a win. Applies to skills AND to code.
5. **Ledger honesty.** Every skill edit records keep/discard/superseded + why. Discarded approaches are data, not embarrassment.

## Testing methodology (planner prompt gap — confirmed missing)

Planner suffix gains a short testing paragraph: a task's test_plan defines the ORACLE first (what command/observation proves it, autoresearch-style fixed harness), then the method (TDD for code, fixtures for parsers, live dev for UX); tasks whose oracle is expensive route through the QA queue instead of per-lane gates.

## Docs

ORCHESTRATION doc: "Plan skills" section (lifecycle, ledger, recall pointer, promotion path: a plan skill that proves generally useful graduates to machine-level mem via codeterm mem skill — suggest at retrospective time).

## EXACT PROMPT TEXTS (implement verbatim — wording is pinned by the Planner, do not paraphrase)

### 1. `PLAN_SKILLS_PRINCIPLES` — shared constant, included in `orchestrator_suffix.md` AND `planner_suffix.md`

```
Plan skills — the plan's own know-how:
- Learned something reusable in this effort (a test recipe, a seeding script, a gotcha)? Record it: `codeterm plan skill new <slug>`. Improve an existing skill before writing a new one.
- A skill names what is FIXED (the oracle: the command or observation that proves success) and what is MUTABLE. Never edit the oracle to make results look better.
- Prefer skills with a --check command; cite the check when keeping or discarding an approach.
- Simplicity wins: a marginal gain that adds hacky steps is a loss; a deletion that preserves results is a win.
- Every skill edit logs keep/discard/superseded + why. Discarded approaches are data, not embarrassment.
```

### 2. `PLAN_SKILLS_WORKER_LINE` — shared constant, appended to the worker charter ONLY when the plan has >=1 skill

```
This plan has skills — procedures earlier agents proved out. Check before inventing: `codeterm plan skill list`. Learned something reusable yourself? `codeterm plan skill new <slug>` — future workers inherit it.
```

### 3. `PLAN_SKILLS_REGISTRY_LINE` — template for the orchestrator heartbeat block (single line, never full content)

```
Plan skills ({n}): {slug1}, {slug2}, … — codeterm plan skill show <slug> | new | edit.
```

### 4. Planner testing methodology — new paragraph in `planner_suffix.md`, inserted after workflow item 5 (task authoring)

```
Test plans start from the oracle: name the command or observation that PROVES the task done before choosing the method. Then pick the method by the work: TDD for code paths, fixtures for parsers and formats, live dev walkthrough for UX, seeded eval cases for prompt or behavior changes. If the oracle is expensive (full build, broad suite, e2e), route it through the QA queue instead of per-lane gates. A criterion without an oracle is an opinion.
```

### Wiring notes

- Constants live in `src-tauri/src/prompts.rs` next to the PEER_ family; tests assert via constant references (dedup discipline — zero re-typed prose).
- `orchestrator_suffix.md`/`planner_suffix.md` include the principles block once; the worker charter gets WORKER_LINE via the same conditional render path as the registry line (plan has skills -> append).
- Word budget: principles block is ~110 words — fits the tier discipline; the ORCHESTRATION doc carries the long-form rationale, prompts carry only the above.
