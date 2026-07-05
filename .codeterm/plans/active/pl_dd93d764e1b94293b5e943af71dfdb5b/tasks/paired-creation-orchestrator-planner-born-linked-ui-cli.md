---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_7e5f9354c8fa4bb3915d6aca7649d52b
handle: paired-creation-orchestrator-planner-born-linked-ui-cli
title: 'Paired creation: Orchestrator + Planner born linked (UI + CLI)'
status: done
---

## What

User-facing flow to create an Orchestrator+Planner PAIR in one action. From "New session" the user can pick "New orchestration": a small form (surface styled like the existing new-session picker) with two rows — Orchestrator: provider/model picker; Planner: provider/model picker — plus workspace target. Confirm creates BOTH panes in the workspace, each initialized with its role prompt, already linked: each knows the other's pane id and the shared plan.

## Design

- **Model.** Plan gains `planner_ref` (pane binding) alongside the existing orchestrator ref — the pair is anchored on the plan object. (Shared model change with the make-orchestrator-mandate task — coordinate, land the field once.)
- **Flow.** New session menu gains "Orchestration" option -> form (orchestrator provider/model, planner provider/model, optional plan title). Create: (1) plan created in planning mode with both refs reserved; (2) orchestrator pane spawned with orchestrator onboarding prompt naming the planner pane and the plan id; (3) planner pane spawned with planner profile prompt naming the orchestrator pane and the plan id. Both prompts state the peer contract: planner owns GOAL/Spec/Plan structure; orchestrator owns dispatch/gates/integration; neither does the other's job.
- **CLI parity.** `codeterm orchestration new --orchestrator <provider[:model]> --planner <provider[:model]> [--workspace W] [--title T]` does the same; prints both pane ids + plan id.
- **Defaults.** Sensible defaults per provider capabilities (never hardcode model ids in core — pull from provider descriptors).

## Files

Frontend: new-session menu + OrchestrationCreate form component. Backend: plan model planner_ref, spawn plumbing composing the paired prompts (shared constants), CLI subcommand.
