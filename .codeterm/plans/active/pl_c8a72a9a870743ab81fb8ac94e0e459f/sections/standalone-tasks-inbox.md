---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_7155a91ca99b4c54bebe6e98992d8aee
handle: standalone-tasks-inbox
title: Standalone Tasks Inbox
---

**HOW plan-less tasks stay first-class.** A task without a plan would otherwise need a second storage path. Instead it lands in a reserved per-project **Inbox plan** that reuses every bit of plan machinery — a plan with a well-known id:

```rust
// src-tauri/src/plan/types.rs
pub const INBOX_PLAN_ID: &str = "inbox";
pub const INBOX_PLAN_FILE_STEM: &str = "_inbox";
pub const INBOX_PLAN_TITLE: &str = "Tasks";
```

Because the Inbox is just a `Plan`, it gets atomic writes, the file-watcher, coverage, canvas rendering, and the task CLI for free — no new persistence, no new layout, no new node kind. A standalone task is added to plan `inbox` and renders as a card in the Inbox region of the board.

Ownership generalizes the task to an addressable unit: a task can be taken by a worker **or** an orchestrator via `assigned_pane_id`, with a derived `taken_by_role` (from the pane's role) so the card knows whether a worker or orchestrator holds it. A link chip on the card opens that pane (`canvasApi.sendToPane`/focus by pane). This is what makes `codeterm task take/drop` work uniformly whether or not the task belongs to a real plan.

## Key decisions
- **A real plan id over a special-case nullable `plan_id`** — every code path that handles a plan handles the inbox; zero branching on "is this orphaned".
- **`_inbox` file stem** — sorts/leads distinctly on disk and won't collide with a generated `pl_…` id.
- **`assigned_pane_id` + derived `taken_by_role`** — assignment is one durable field; the role is computed from the pane, so it can't desync from who actually holds the pane.
