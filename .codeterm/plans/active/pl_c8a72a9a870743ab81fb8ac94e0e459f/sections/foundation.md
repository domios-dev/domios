---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_7f17ba5016294c42bafdf74db3832bdd
handle: foundation
title: Foundation
---

**HOW the foundations are laid.** This group covers the bedrock the rest of the build stands on: the spatial model (a workspace-scoped board over the live orchestration graph), the persisted data model (plan → sections → tasks → milestones as per-project JSON), the chat-marker plumbing the canvas reuses, and the cross-provider brainstorm process that produced the v3 design.

The through-line for every child below: **the canvas renders the orchestration data that already exists** (plans, the report ledger, agent panes) rather than inventing a parallel store. The data model (`src-tauri/src/plan/types.rs`) is the single typed contract; the daemon is the single writer (`service/plan/mod.rs::write_atomic`); the frontend (`PlansProvider` → `layoutWorkspace` → canvas renderer) is a pure projection of that data. So "Foundation" is mostly *wiring discipline*, not new mechanism — get the types, the ownership, and the marker envelope right, and the spatial layer is a deterministic function of them.

- **Concept Vision** — the projection: workspace → plans → Spec/Task/milestone cascade, two entry points (workspace overview vs per-pane Plan focus).
- **Data Model** — the typed tree + identity-over-restart + per-entity canvas position.
- **Core chat-render delimiters** — the `-=-codeterm:…-=-` marker envelope that injected blocks (and the canvas provenance chips) ride on.
- **Display methodology brainstorm** — the two-provider design process that fed the substrate decisions.
