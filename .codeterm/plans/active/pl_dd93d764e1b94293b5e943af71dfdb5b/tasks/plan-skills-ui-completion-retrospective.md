---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9f965e293ab64ba0825fd8a8024645f4
handle: plan-skills-ui-completion-retrospective
title: Plan skills UI + completion retrospective
status: done
---

Two user-facing halves. (1) SKILLS IN THE PLAN UI: plan document view + plan header get a Skills section — slug, title, purpose one-liner, check badge when present; click opens SKILL.md rendered + the edit ledger (working points, kind=skill) as a timeline; read-only in v1 (editing stays CLI/agent-side). (2) COMPLETION RETROSPECTIVE: when is_plan_complete holds (reuse the predicate from the nudge fix), the orchestrator's one-time archive suggestion also asks for a retrospective: `codeterm plan retrospective <plan> --body ...` records a final stride (kind retrospective) — what worked, what failed, which plan skills proved out, which deserve promotion to machine-level mem. The retrospective renders at the top of the plan document view for archived/complete plans.

## EXACT PROMPT TEXT (implement verbatim — pinned by the Planner)

### `PLAN_RETROSPECTIVE_NUDGE` — shared constant; delivered ONCE, appended to the complete-plan archive suggestion from the nudge-fix task

```
This plan is complete. Before archiving, write its retrospective:
`codeterm plan retrospective <plan> --body "..."` — what worked, what failed and why, which plan skills proved out, and which deserve promotion to machine memory (`codeterm mem skill`). Honest beats flattering: the discarded approaches are the most useful part.
```

### Wiring notes

- Constant lives in `src-tauri/src/prompts.rs`; the complete-plan suggestion path (is_plan_complete one-time message) concatenates it after the archive line. Fires once — same one-shot mechanics as the archive suggestion itself.
- The retrospective stride is kind `retrospective`, one per plan; a second write supersedes with a working point noting the replacement (append-only discipline).
