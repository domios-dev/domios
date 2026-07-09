---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_f21b9253cf424753adf69945b313ff71
handle: sidebar-living-subagent-tree-sort-glow-task-line
title: 'Sidebar: living subagent tree (sort, glow, task line)'
status: done
---

## What

Make working subagents visibly ALIVE in the sidebar tree. Live status already streams from PaneStatusTracker (`statuses: Record<string, PaneStatus>` — Working/Waiting/Idle/Dead) but the UI barely uses it: `HierarchyTree.tsx` renders every subagent row equally muted with a static 8px StatusDot.

**Design principle (from user feedback): normal rows stay completely plain — zero added treatment, nothing that draws the eye. ALL the design budget goes into working rows. No left accent bars / border stripes (reads as generic "Claude Code" styling) and no provider-glyph changes — the row itself carries the life.**

## Design spec (canonical CodeTerm style: quiet, muted palette, CSS vars, no loud neon)

**1. Subtree ordering — working agents float up.** Within each `HierarchyTree` level sort: Working -> Waiting -> Idle -> Dead (stable within a group, original order as tiebreak). Re-sorting must be calm: apply a stability window (only reorder after a status persists >= ~2s) and animate row movement with a FLIP/transform transition so rows glide, never jump. Sorting is per-subtree-level; children never leave their parent.

**2. Working-row treatment — alive, not striped.** Working rows get, together:
- Title at full `--ct-fg` brightness (vs the muted default).
- A soft ambient wash behind the row: a very-low-alpha radial/linear gradient in the pane's presence color (`presenceColor(paneId)`), breathing slowly (~3s opacity cycle) and/or drifting a few px horizontally (aurora feel). Alpha stays whisper-quiet (roughly 0.04-0.08 peak) — visible as warmth, not as a highlight bar.
- The existing StatusDot gains a soft halo pulse (expanding faint ring, presence color), replacing the plain animate-pulse.
The exact blend is the worker's craft call — prototype 2-3 variants and screenshot them in the report — but the constraints are hard: no border/edge stripes, no saturated fills, nothing that survives a squint on idle rows.

**3. Non-working rows — deliberately plain.** Idle and Waiting rows keep today's muted look with NO added accents or backgrounds. Waiting differs only by its existing yellow StatusDot (steady) and the "waiting for input" task line (below). Dead rows: extra dimmed (~55% opacity) + red StatusDot stays.

**4. Current-task line under the title.** For Working/Waiting subagent rows add a second line (tiny type, `--ct-muted-fg`): the task the agent is working on — task title from the shared activeWork selector (task_5c00ad82793443f180f838d0b5c7523a step 1: in_progress/in_review task bound to this pane) truncated with ellipsis. While Working, append an animated three-dot typing indicator (three 2px dots, staggered opacity pulse — the canonical chat "typing" pattern). Waiting shows "waiting for input" instead of dots. No bound plan task -> show nothing extra (single-line row as today), do NOT invent placeholder text.

**5. Motion discipline.** All animations CSS-only (no rAF/JS timers). `prefers-reduced-motion: reduce` -> every breath/halo/dots animation becomes a static bright state. Animations pause when the sidebar is not visible (CSS `animation-play-state` tied to a container class if cheap, otherwise skip).

## Data flow / perf

- Statuses: existing store record — zero new polling, zero new fetches.
- Task titles: consume the shared activeWork selector from task_5c00ad82 (dependency: its step 1 must land first or be extracted as a standalone module this task can land).
- Sorting + stability window implemented as a pure function (`sortHierarchyNodes(nodes, statuses, prevOrder, now)`) — unit-testable without React.

## Files

`src/components/Sidebar/HierarchyTree.tsx` (+ new tests), `src/components/Sidebar/StatusDot.tsx` (halo variant), shared styles/keyframes in the sidebar stylesheet, `src/components/Canvas/planView/activeWork.ts` (consume only).

## Out of scope

TabItem / ProviderGlyph changes (explicitly cut by user — do not touch), Board/List/Canvas surfaces (covered by task_5c00ad82), remote mesh worker rows (RemoteWorkerRow — follow-up if this lands well), sound/notifications.

## Round-6 addition: remove the row clutter (gear + dot)

User feedback (screenshot): the Worker rows currently carry a gear glyph (RoleIcon "⚙" from ROLE_ICONS in HierarchyTree.tsx) and a standalone StatusDot — unreadable noise. Remove BOTH from subagent rows:
- The ⚙ role icon goes away entirely; a worker's role is already conveyed by its position in the tree (child of an orchestrator). The orchestrator's ★ may stay or go — worker's call, screenshot both in the report.
- The standalone StatusDot leaves the row; status is carried by the new treatment (bright title + breathing wash for Working, three-dots task line, plain muted for Idle). This supersedes the "StatusDot halo pulse" wording in the earlier spec — the halo idea moves onto whatever minimal marker remains, if any.
- Dead panes must remain distinguishable without the red dot: extra dimming + a red-tinted title or small trailing ✕ — worker prototypes, screenshot in report.
- Waiting rows: the yellow dot is also removed; "waiting for input" reads from the task line. If a squint-test shows Waiting became invisible, propose a minimal marker in the report instead of silently re-adding dots.
