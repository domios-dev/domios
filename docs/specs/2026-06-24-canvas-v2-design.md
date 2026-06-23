# Canvas v2 — Corrections & Generalization

<!-- revision: 1 -->

**Supersedes** the broken parts of `2026-06-23-canvas-design.md` (kept for history).
This is a clean re-derivation, not a patch. The v1 implementation diverged from
intent AND the v1 spec itself baked in a layout model that produces an unusable
board. Both are corrected here. Goal: **Figma, but for reviewing agent
orchestration plans** — polished, connected, navigable, workspace-isolated.

Implementation is a **generalized refactor** (no band-aids). Use the
`frontend-design` skill for every UI surface.

## 0. What was accepted wrong in v1 (root causes)

1. **Layout model (the big one).** v1 §6.1 said a plan is "a bounded frame whose
   contents are section-cascade-left → task-rows → milestone-columns." Rendered,
   that is a giant box with three disconnected bands: sections pile top-left, task
   cards float in a middle column, milestones fly off to the right **with no edges
   drawn between them**. It reads as noise. → **Replace** with: one *compact
   sectioned plan node*, a separate *spec node*, *satellite task nodes* joined to
   their section rows by curved edges, and *milestone bubble clusters* hung off each
   task. Nothing floats unconnected. (§2)
2. **Navigation under-specified.** v1 only did middle-mouse-drag pan + wheel-zoom.
   A Figma-grade board needs trackpad two-finger pan, cmd/pinch zoom. → §3.
3. **Workspace isolation not enforced.** Plans are per-project but the canvas leaked
   across workspaces. → §4. (One workspace = one `.codeterm/` = its own board.)
4. **Spec never rendered as a document.** The source spec was scattered as loose
   section cards instead of one readable spec node. → §2.3.
5. **Milestones rendered as a far-off grid**, not grouped bubbles near their task,
   not visibly progressing. → §2.4.
6. **Orchestrator invisible.** Plans are owned by an orchestrator but the board never
   shows the orchestrator as a node/grouping you can click. → §2.5.
7. **No standalone tasks.** v1 assumes every task nests under a plan; intent is tasks
   can exist plan-less, taken by a worker *or* an orchestrator. → §5.
8. **No graph/task query surface.** No CLI to "list active tasks", "what's left",
   "take this task", "focus the canvas on a pane's tasks". → §6.
9. **Button behavior incomplete.** Plan/Tasks button next to Chat|Terminal, pulse on
   change, worker-focus-and-blink — specced loosely, never delivered. → §7.

## 1. The model in one picture

Per workspace (one `.codeterm/` project root), the board shows:

```
[Orchestrator group: "Canvas Orchestrator" ───────────────────────────────]
   ┌─ Spec node ─┐      ┌─ Plan node ──────────┐         ◌ ◌  milestone
   │ # Title     │──────│ title  · 18/24 · 72% │         ◌    bubbles
   │ full spec   │ src  │ ▸ Phase 1   [4]  ✓   │──┐  ┌──▶ (grouped by kind,
   │ markdown    │      │ ▸ Phase 2   [3]  ●   │  │  │    clustered per task)
   │ (sections,  │      │ ▸ Phase 3   [0]  ⚠   │  └──│ TaskCard  ◌◌◌
   │  subs,      │      │ ▸ Phase 4   [2]  ●   │     │ name·model │
   │  bodies)    │      │ coverage ▓▓▓▓▓░ 72%  │     │ status     │
   └─────────────┘      └──────────────────────┘     └────────────┘
                         section-row → task edges     task → bubble edges
```

- **One sectioned plan node** (not a container). **One spec node** (the prose).
- **Tasks are satellites** joined by curved edges to the *exact section row* they
  cover (`section_ids`, many-to-many).
- **Milestones are bubble clusters** per task, grouped/colored by kind, iterations
  stacking, `links` drawn inside the cluster.
- **Orchestrator** is a labeled group wrapping all its plans; clickable → its pane.
- **Uncovered section rows pop** (⚠ dashed/tinted) — the whole point of coverage.

## 2. Nodes & rendering (replaces v1 §6.1)

### 2.1 Plan node — compact, sectioned
Single card. Header: plan title, owner chip (orchestrator), `done/total tasks`,
coverage % + bar. Body: a vertical list of **section rows** (recursive, subsections
indented). Each row: section title · task-count badge `[n]` · status glyph
(✓ all done / ● in progress / ⚠ uncovered, n=0). The row's right edge is the **edge
anchor** for task connections (layout exposes `anchors: Map<sectionId, Point>`).
Archive button. NOT a giant frame — height = header + rows, width fixed.

### 2.2 Section rows are anchors, not loose cards
`SectionCard` folds into a `PlanNode` row sub-component. No free-floating section
cards on the board. The layout emits per-row world-coord anchors so `CanvasView`
draws `task → section-row` edges to the precise row.

### 2.3 Spec node — the full document
A separate, taller card rendering the plan's spec as a readable document: title,
then each section heading + `section.body` markdown, subsections nested, scrollable
inside the node. Edge `plan ↔ spec` (kind `plan-spec`). The compact plan node =
overview; the spec node = prose. (Source = section bodies; later a `document`
element can override.)

### 2.4 Task nodes & milestone bubbles
- **TaskCard** (satellite): marker(status color) · name · `type` · `model` chip ·
  `model_rationale` ("why this model?" expandable) · `status` + `status_label` ·
  `intent` · `body` markdown. A link chip to `assigned_pane_id` (→ opens that pane).
- **Edges:** one curved bezier per `section_id` from that section row's anchor to the
  task's left edge. Many-to-many.
- **MilestoneBubble:** a circle, colored by its `kind` (declared `milestone_kinds`
  carry the color lane). Clustered around the owning task; **grouped by kind**;
  iterations (loops) stack downward; `m.links` draw bubble→bubble inside the cluster
  so the *sequence/progress* is visible. Edge `task → cluster`. Each bubble shows its
  kind label + short text on hover/near-LOD; click → milestone detail.

### 2.5 Orchestrator group
Plans owned by the same orchestrator are wrapped in a soft labeled group (owner
title + live/▰ status). One orchestrator → many plan nodes side by side in its group.
The group label is clickable → focuses/opens that orchestrator's pane. Unowned plans
go to an "Unowned / claimable" group.

### 2.6 Edges (all the universal `links` primitive, dispatched by kind)
`section-row→task`, `task→milestone-cluster`, `milestone→milestone`, `plan↔spec`,
`plan↔plan` (`Plan.links`), `task→assigned-pane` (badge link, not a drawn line).
All curved, subtle, no arrowheads (direction implicit). Virtualized with the nodes.

## 2.7 No manual dragging — deterministic realtime auto-packing
Plan nodes are **not draggable** (drop the v1 drag-persist path for plan/section/task
nodes). Position is decided entirely by a deterministic layout that **packs cleanly as
nodes stream in at realtime** — adding a task/section/milestone grows the arrangement
in place without reshuffling existing nodes or overlapping. Same data → same layout.
- **Coverage = section-row fill tint, not edges-only.** A covered section row is filled
  a different (accent) color than an uncovered one; the task→row edge reinforces it but
  the tint is the primary signal. Uncovered rows read instantly.
- **Markdown everywhere.** Section bodies, task bodies, milestone text, spec node — all
  render markdown (reuse `RichMarkdown` from `@codeterm/chat-ui`). Nodes must size to
  content and never clip/overflow their box.
- **Internal packing algorithm.** Within a plan node, section rows stack with consistent
  rhythm; task satellites pack around the plan without overlap (bin-pack by nearest
  covered row, collision-avoided); milestone bubbles cluster per task without spilling.
  All gaps/sizes from `settings.ts`.

> **Authoring guideline (put in CANVAS docs):** plans are meant to be filled
> **completely and in detail** — every section with a real body, every task with model
> + rationale + body, milestones as work progresses. A sparse plan is a smell; the
> board's value is a fully-authored, self-documenting plan.

## 3. Navigation — Figma-grade (rewrite `useCanvasViewport`)

- **Two-finger trackpad scroll → PAN** both axes. `wheel` with `ctrlKey === false`
  ⇒ pan by `(-deltaX, -deltaY)`; honor `deltaMode` (lines×16 vs pixels).
- **Cmd/Ctrl + scroll → ZOOM** around the pointer. `e.ctrlKey || e.metaKey` ⇒ zoom.
  Browsers set `ctrlKey` on trackpad **pinch**, so pinch-zoom works for free.
- Keep **middle-drag** pan; add **space+drag**. Smooth via RAF batching.
- Wheel listener attached **non-passive** (native `addEventListener("wheel", h,
  {passive:false})`) so `preventDefault` stops page rubber-banding.
- Fit-to-view on first mount; persist last `Plan.canvas.viewport`.

## 4. Workspace isolation (fix the leak)

One workspace ↔ one project root (`git toplevel` of the workspace cwd) ↔ one
`.codeterm/plans/`. The canvas in a workspace shows **only** plans whose
`project_path` canonicalizes to *this workspace's* root. Two modes, both scoped to
the workspace project:
- **Workspace overview** (click workspace, no pane focused): every active plan +
  every orchestrator that owns one, **no orchestrator filter** — the whole board for
  this project.
- **Focus** (Plan button on a pane): only the focused orchestrator's (or worker's)
  plans/tasks.
Bug fix: the list query and the `filterPlansForCanvas` path must both key off the
resolved project root, and the mount must pass the active workspace's cwd. Add a test
that a plan in project A never appears in project B's canvas.

## 5. Standalone tasks (generalize Task to be plan-optional)

Tasks may exist without a plan. **Decision (reuse over new store):** a plan-less task
lives in a reserved per-project **Inbox plan** (`.codeterm/plans/_inbox.json`, stable
id `inbox`, title "Tasks", no sections, coverage N/A), auto-created lazily on first
standalone task. This reuses ALL plan machinery (storage, index, rendering, CLI,
`plan-changed`, archive) with zero new persistence path — the generalization the user
asked for.
- A task is **taken by any pane** — worker *or* orchestrator — via `assigned_pane_id`
  (+ derived `taken_by_role`). The TaskCard shows a link chip → click opens that pane.
- On the board, Inbox tasks render as a task cluster under their taker's orchestrator
  group (or an "Unassigned" group if untaken). Same TaskCard, same edges (to the pane
  link). No sections ⇒ no section edges; that's fine.
- Archive of the Inbox plan is not offered; individual standalone tasks are
  `done/cancelled` like any task.

## 6. Graph / task CLI (talk to the board)

Query+mutate the plan store (NOT mem-graph; per §2 decision #1 plans stay JSON). New
`codeterm task ...` plus a canvas-focus emit:
```
codeterm task list [--active|--all] [--mine] [--unassigned] [--workspace] [--plan <id>] [--json]
codeterm task show <task_id> [--json]              # detail incl plan, sections, owner/worker links
codeterm task take  <task_id>                      # caller's pane (worker OR orch) takes it
codeterm task drop  <task_id>                      # release assignment
codeterm task add   --name <n> --type <k> [...]    # standalone (lands in Inbox) — alias of plan task add w/o plan
codeterm plan gaps  <plan_id>                      # sections with 0 tasks (what's left) — wraps coverage
codeterm canvas focus [--task <id>|--pane <id>|--plan <id>]   # emit canvas-focus → camera + blink
```
- `task list --active` = not done/cancelled across all plans in the workspace project
  → "what's still in flight". `--mine` = assigned to the caller's pane/ancestors.
  `--unassigned` = no `assigned_pane_id` → "what can I pick up".
- `canvas focus` emits a `canvas-focus` event (Tauri + websocket) carrying target ids;
  the open canvas pans/zooms to fit those nodes and **blinks** them. Drives §7 Tasks
  button. Reuses the `plan-changed` fan-out plumbing (`emit_everywhere`).
- Reuse: all of these read the existing in-memory plan index; queries are pure.

## 7. Buttons (next to Chat | Terminal) — one role-driven component

`PaneActionButton` mounted right of `PaneModeSwitch`:
- **Orchestrator pane that owns ≥1 active plan → "Plan".** Click → focus canvas on
  its plans. **Pulses** on `plan-changed` for an owned plan; pulse clears on open.
- **Worker pane bound to a task (`CODETERM_PLAN_ID`/`TASK_ID`) → "Tasks".** Click →
  open canvas and `canvas focus --pane <self>`: camera fits + **blinks** this worker's
  task(s). Pulses on `plan-changed` touching its task.
- **Neither → button hidden.** Workspace overview is reached by clicking the workspace
  (no pane focused), independent of these buttons.
- One component, role-driven label/target — no duplicated logic.

## 7.1 Plan is a THIRD pane mode (Chat | Terminal | Plan)
Plan is not a separate overlay — it is a third mode in the pane mode switch, peer to
Chat and Terminal. Selecting **Plan** highlights it (like the screenshot) and the
pane body renders `<CanvasView mode="focus">` in place of the chat/terminal view.
Clicking **Chat** or **Terminal** returns. Mutually exclusive view state per pane.
- Orchestrator pane → Plan mode shows its plans; Worker pane → label "Tasks", shows
  its tasks focused+blinking. Same role-driven component (§7), now wired as a mode in
  `PaneModeSwitch`, not a floating button.
- The mode still **pulses** on `plan-changed` for the represented plan(s).

## 7.2 Canvas composer fixes & activity bubble
- **Clear-on-send bug:** the talk-to-area composer must clear its input after a
  successful send (currently the text stays). Fix in `CanvasComposer`.
- **Activity bubble:** a small, non-modal bubble on the canvas shows live status of
  the addressed orchestrator — partial streamed message text or a compact "working on
  it…" indicator — so the user sees *something is happening* on the question they
  asked. Source it from the owner pane's status/last-output (reuse pane-status /
  chat-tail plumbing); render near the composer / focused plan. Read-only, truncated.

## 8. Archive cascade
Archiving a plan sets `status=archived` (flag, file stays). Everything nested (tasks,
milestones, elements) is in-file so it hides with the plan. Standalone tasks linked
only to that plan and notes/elements `links`-ed only to it also hide. `Plan.links` to
*other* plans do not cascade (independent plans); the edge just stops drawing. Coverage
and `task list` exclude archived. Un-archive restores everything.

## 9. Frontend file map (refactor targets)
- `useCanvasViewport.ts` — rewrite nav (§3). Keep RAF, fit-to-view.
- `layout/intraPlan.ts` — rewrite to plan-node(rows)+spec+satellite-tasks+bubble-
  clusters; emit section-row anchors + cluster boxes. Split if >1000 lines.
- `layout/types.ts` — edges carry explicit anchor endpoints; add `anchors`, cluster.
- `CanvasView.tsx` — `edgeEndpoints` uses anchors; bezier edges; keep virtualization;
  subscribe to `canvas-focus` (camera + blink); orchestrator groups.
- `cards/PlanFrame.tsx` → `PlanNode` (compact sectioned). `cards/SectionCard.tsx` →
  row sub-component. New `cards/SpecNode.tsx`, `cards/OrchestratorGroup.tsx`.
- `cards/MilestoneNode.tsx` → circular `MilestoneBubble`; clustering in layout.
- `cards/TaskCard.tsx` — add pane link chip; tighten visuals (frontend-design).
- `settings.ts` — add row height, anchor inset, bubble radius, cluster spacing, group
  padding. No inline magic numbers.
- `nodeRegistry.tsx` — register `spec`, `orchestrator`; keep open-kind fallback.
- `PlanButton.tsx` → `PaneActionButton` (role-driven, §7).

## 10. Backend file map
- `plan/types.rs` — Inbox plan constant; `Task.assigned_pane_id` already exists; add
  derived `taken_by_role`. `Task.plan_id` is implicit (file) — Inbox covers standalone.
- `service/plan/` — lazy Inbox creation; `task list/show/take/drop` queries; `plan gaps`
  (wraps existing coverage); `canvas focus` emit.
- `api/plan_routes` + socket — routes for the new task/canvas-focus surface; emit
  `canvas-focus` via `emit_everywhere`.
- `cli/src/args|cmd` — `codeterm task ...`, `codeterm canvas focus`, `plan gaps`.
- Workspace-scoping fix in the list handler (project-root canonicalization).

## 11. Dogfood
This very refactor is built AS a plan in this workspace's canvas: sections = the work
areas above (§2 layout, §3 nav, §4 isolation, §5 standalone, §6 CLI, §7 buttons), tasks
= the worker lanes with real model routing + rationale, milestones = each lane's
report chain. The board must end up rendering its own construction.

## 11.1 Visual feedback loop (MANDATORY methodology)
Visual quality cannot be achieved blind. Building this canvas REQUIRES a Playwright
screenshot feedback loop, not just unit tests.
- **Harness:** a Playwright helper (under `tests/playwright/`) that, on macOS web-mode
  (`/web/` against dev on 7686), opens the workspace canvas / a pane's Plan mode,
  **navigates the canvas** (programmatic wheel-pan, cmd-wheel-zoom, fit-to-view, focus a
  node), and captures **screenshots** (full board + per-region). It must be able to
  assert no-overflow / no-overlap structurally (bounding-box checks) AND produce images
  for human/agent visual review.
- **Loop (≥20 iterations):** run dev with the v2 build → screenshot the dogfood plan →
  assess (does it look good? markdown rendered? nothing clipped? sections tinted by
  coverage? bubbles grouped? realtime add of a node packs correctly?) → fix → hot-reload
  → screenshot again. Repeat until the board is genuinely polished. Dogfood realtime:
  add a section/task/milestone via the API mid-loop and confirm it slots in cleanly.
- **Canvas navigation in Playwright is itself a deliverable** — we must be able to drive
  pan/zoom and read the rendered result to evaluate correctness; this is how every later
  canvas change is verified.
- The orchestrator owns running this loop (can view screenshots); the FE lane provides
  the harness + reaches a screenshottable state and iterates on critique.

## 12. Done
Figma-grade board: sectioned plan node + spec node + edge-connected satellite tasks +
grouped milestone bubbles + orchestrator groups; uncovered sections pop; trackpad
pan + cmd/pinch zoom smooth; canvas isolated per workspace; standalone tasks work;
`codeterm task`/`canvas focus` CLI works; Plan/Tasks buttons pulse + focus+blink.
All gates green (tsc, canvas tsx tests, cargo). Dogfood plan renders itself.
