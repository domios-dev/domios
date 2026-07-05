---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_072e7328c346443eb53a08582e67bc4c
handle: decision-gates-structured-stop-ask-instead-of-silent-waiting
title: 'Decision gates: structured stop/ask instead of silent waiting'
status: done
---

## What

First-class **decision gates**: when the Orchestrator is blocked on a missing product/design/user decision, the stop becomes a structured, routed, nagging object — not a silent wait. Field evidence: the canvas mission-control user gate froze the whole frontier; the orchestrator waited instead of routing the question to the Planner and kept nothing else moving.

## Model

`DecisionRequest` on the plan: {id, question, options?, context_note, asked_by_pane, routed_to (planner_ref pane by default; user-escalation flag), created_at, status: open|answered|withdrawn, answer{choice, note, by, at}, blocking_task_ids[]}.

CLI:
- `codeterm plan decision ask --question "..." [--options "A|B|C"] [--blocking task1,task2] [--task <ctx>]` — creates the gate, writes a working point (kind decision, "gate opened") on each blocking task, and AUTO-SENDS the question to the routed pane (planner first; the ask is a delivered message, not a passive status).
- `codeterm plan decision answer <id> --choice ... --note ...` — records the answer, writes decision working points, auto-nudges the asking orchestrator with the answer.
- `codeterm plan decision list/show` — open gates with ages.
- Planner may re-route to user (`--escalate-user --note why`) — that is the replanning-loop contract: the Planner owns user-facing decision brokering.

## Heartbeat & discipline (shared constants, dedup rule)

1. Open gates render as their own heartbeat block: "Open decision gates: <id> <question> (age, blocking N tasks, routed to <pane>)".
2. Focus discipline extension: a waiting-focus is legal ONLY when it names an open gate id (structural waiting); vibe-waiting without a gate stays a violation.
3. **An open gate parks its blocking tasks, NEVER the plan**: the heartbeat/next-actions explicitly re-derive the frontier minus gate-blocked tasks and push dispatch of everything else. This is the anti-freeze rule — today's failure class.
4. Escalation ladder: gate open > threshold (config, ~2h) -> reminder re-pings the routed pane AND the orchestrator; > second threshold -> surfaces to the user (plan header "Needs decision" badge + companion bubble; UI part below).

## Anti-silent-parking (doctor)

- Task in blocked/parked without park_reason OR a linked open gate = doctor error.
- Gate open with zero activity past thresholds = doctor warning with age.
- Answered gate leaves an audit trail: question + answer + who, permanently in working points.

## UI (minimal, rides existing surfaces)

Plan header chip "Needs decision (N)" when gates are open (click -> gate list -> answer inline for the user); gate entries visible in the activity timeline of blocking tasks. Companion bubble badge when a gate is routed to the user.

## Docs/prompts

ORCHESTRATION: "Decision gates" section (when to open one: any stop that needs a human/product/planner choice; open it IMMEDIATELY, do not wait silently). Orchestrator suffix + heartbeat rules constant gains the gate discipline lines; Planner profile gains the brokering duty (answer or escalate to user).

## Out of scope

Automated decision-making, multi-plan gates, votes.
