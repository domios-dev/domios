---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_6595bc28352a47ac996b0d4aadb006ec
handle: universal-busy-states-one-waiting-grammar-no-dead-surfaces
title: 'Universal busy states: one waiting grammar, no dead surfaces'
status: done
---

## What

ONE waiting grammar for the whole product: any surface that is waiting on anything shows a live busy state — label + the chat typing-dots motion — within 100ms, and nothing on screen sits static while work is pending. Kills the class the user keeps hitting: blank terminal during agent spawn, static "Loading plans…" text, dead panes during reconnect.

## Design (DRY — one system, delete the bespoke)

1. **Shared component + hook**: `BusyState` (label, optional sub-label, size variants) rendering the canonical typing-dots; `useBusyState`-style wiring where a surface derives its waiting condition from real state (never fake timers). Reduced-motion: static label + ellipsis.
2. **Inventory pass (part of the task, recorded in the report)**: enumerate every awaitable surface and wire it — pane boot (consumes boot-stage events from the sibling task: "Starting shell → Launching Claude → Agent ready"), chat send/response pending (exists — becomes the shared component), plan views loading + reconnecting (coordinate with the resilience task), mem panel ops, mesh ops, settings saves, picker spawn confirm.
3. **Delete legacy**: every existing one-off spinner/loading text is REPLACED by the shared component — grep sweep for spinner/animate-spin/Loading…, zero bespoke leftovers (pre-prod: redesign, don't wrap).
4. **Web/native parity**: identical behavior in web mode and Tauri; webForks inventory updated only if a genuine platform delta is required.

## Executable test plan

1. Component tests: BusyState variants + reduced-motion (static) — exact testids busy-state, busy-label, busy-dots.
2. Playwright: spawn an agent from New session → assert busy-state visible in the pane area within 100ms and label transitions through boot stages until first output replaces it.
3. Playwright: plan view with throttled API (route interception delaying /api/plans) → busy-state shown, never raw text or blank.
4. Grep gate in CI test: `git grep -nE "animate-spin|Loading\.\.\.|Loading…" src/` returns only the shared component's own files (list pinned in test).
