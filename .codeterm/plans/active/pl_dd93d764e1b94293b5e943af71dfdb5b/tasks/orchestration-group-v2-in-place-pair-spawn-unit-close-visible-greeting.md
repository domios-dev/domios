---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_e0ddee081c15441281e169b3ae3c71a3
handle: orchestration-group-v2-in-place-pair-spawn-unit-close-visible-greeting
title: 'Orchestration group v2: in-place pair spawn, unit close, visible greeting'
status: done
---

## What

V2 of the paired-creation UX from live dogfood (round-13): the current flow creates stray objects and new tabs. Target shape: exactly TWO elements born in place, grouped as one closable orchestration unit, with a visible conversational opening.

## Current bugs to root-cause first

User reports "что-то ломается, создаются лишние объекты" when spawning the pair from the empty-tab picker: extra tabs/panes appear instead of the picker tab transforming. Reproduce on dev, enumerate every object created by the current create_orchestration_pair frontend path, and fix to the target shape below.

## Target shape

1. **In-place spawn.** Confirming the pair in the empty-tab picker TRANSFORMS that tab into the Orchestrator pane (same tab id — no new tab for the orchestrator). The Planner spawns as its child pane. Net result: exactly 2 panes, one group. No orphan empty tab, no duplicate workspace entries.
2. **Orchestration group semantics (sidebar).** The pair + future workers render as ONE group: Orchestrator as group head, Planner pinned as FIRST child, workers below it (supersedes the same-level sibling rendering — see the round-13 decision record; PairAffordance stays in the picker card and plan-header chips only). Group is closable as a unit (single close affordance on the head with confirm listing what closes: orchestrator + planner + N workers). Children have NO individual close affordance (hidden/disabled with tooltip "part of the orchestration — close the group").
3. **Visible onboarding.** The initial role prompts must not be invisible plumbing: render the spawn brief as the first (collapsed-quotable) message in each chat, and the Orchestrator OPENS with a greeting as its first assistant message: ready state + "Tell me the goal" (wording via a shared prompt constant, e.g. PEER_ORCH_GREETING: "Paired up — <planner name> is my Planner. Tell me the goal and we'll draft the plan together."). Flow after: user describes the goal -> orchestrator hands it to the planner -> planner asks clarifying questions and drafts GOAL/Spec/Plan per the existing planner profile.
4. **CLI parity.** codeterm orchestration new keeps working; when invoked without an empty tab it may open its own tab (current behavior), but still exactly 2 panes + group.

## Files

Frontend: NewSessionPicker confirm path, sidebar group rendering (TabItem/HierarchyTree group head + close), close-confirm dialog. Backend: create_orchestration_pair in-place variant (accept existing tab/pane id), PEER_ORCH_GREETING constant, greeting injection on pair spawn.
