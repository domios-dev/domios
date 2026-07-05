---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3d9999fd8a684831a4e0c27bfd029030
handle: sidebar-taxonomy-role-badges-managers-workers-sections-planner-onboardin-ms_3d9999fd8a684831a4e0c27bfd029030
title: decision
---

ADDITION to the open bounce round (round-32b, user): the planner intro is a wall of agent-jargon ('draft GOAL, Spec, Plan structure, task briefs...') — inhuman for the user. SPLIT the two audiences: the agent CONTRACT stays as the collapsed machine-facing spawn brief (unchanged semantics); the USER-VISIBLE first message becomes a compact intro CARD, not a paragraph. PINNED COPY (implement verbatim, constants module):

PLANNER_INTRO_CARD — title: 'Planner — {plan_title}'; subtitle: 'working under {orchestrator_name}'; bullets:
- I turn your goal into a plan: requirements, phases, tasks with clear done-criteria
- I ask the right questions before work starts, so nobody builds on guesses
- Changed direction? Tell me — I reshape the plan, nothing is set in stone
footer: 'Planning input goes here; execution questions go to {orchestrator_name}.'

ORCH_GREETING stays as shipped ('Paired up — {planner} is my Planner. Tell me the goal…') — it is already human.

PRESENTATION: intro renders as a structured card in chat (icon + title + subtitle + tight bullet list + muted footer), same visual family as the existing Reminder card — NOT a text paragraph. The card is the pattern for any future role intro (watchers inherit it). Acceptance addition: pair spawn on dev -> planner chat shows the card (screenshot), the collapsed brief below it holds the full contract; no role-intro paragraph blobs remain.
