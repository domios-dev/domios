---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_305e1ad40dd743a0a000ea2a7ea92788
handle: plan-as-mode-composer-activity-bubble
title: Plan As Mode Composer Activity Bubble
---

**HOW Plan becomes a third pane mode.** Chat and Terminal are persisted `PaneUiMode` states; Plan is deliberately *not* — it's a transient live view toggle kept out of the binary mode machinery so it never ripples storage. `panePlanMode.ts` is the whole store, a `useSyncExternalStore` over a `Set<paneId>`:

```ts
// Per-pane transient "Plan" view toggle (§7.1). Plan is a third pane mode peer to
// Chat|Terminal but, unlike them, never persists — it is a live view state only.
const active = new Set<string>();
export function setPanePlanMode(paneId: string, on: boolean): void {
  const had = active.has(paneId);
  if (on === had) return;
  if (on) active.add(paneId); else active.delete(paneId);
  notify();
}
export function usePanePlanMode(paneId: string): boolean {
  return useSyncExternalStore(subscribe, () => active.has(paneId), () => false);
}
```

Selecting Plan highlights the `PlanButton` and renders the canvas in the pane body; toggling off restores Chat/Terminal. The button **label is role-driven** — "Plan" for orchestrators, "Tasks" for workers — read from the pane's role. The floating composer (`CanvasComposer.tsx`) is context-addressed: it resolves the orchestrator owning the *focused region* and posts to that pane via `canvasApi.sendToPane`, clearing on send; a small non-modal activity bubble surfaces that orchestrator's live status so you can see something is happening without leaving the board.

## Key decisions
- **Transient, not a persisted `PaneUiMode`** — Plan is a *lens* over a pane, not a fourth saved state; no storage migration, no restore semantics to get wrong.
- **`useSyncExternalStore` over React context** — any pane can flip its own toggle and only subscribers re-render; the server snapshot is `false` (SSR-safe).
- **Composer addressed to the region's owner** — you talk to the agent responsible for what you're looking at, not a global inbox; the non-modal bubble keeps it a board overlay, never a modal that steals the canvas.
