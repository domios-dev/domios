# Canvas v2 — Corrections & Generalization

<!-- revision: 2 -->

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
   "take this task", "focus the canvas on a Tab's tasks". → §6.
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
- **Orchestrator** is a labeled group wrapping all its plans; clickable → its Tab.
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
  `intent` · `body` markdown. A link chip to `assigned_pane_id` (→ opens that Tab).
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
The group label is clickable → focuses/opens that orchestrator's Tab. Unowned plans
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
- **Workspace overview** (click workspace, no Tab focused): every active plan +
  every orchestrator that owns one, **no orchestrator filter** — the whole board for
  this project.
- **Focus** (Plan button on a Tab): only the focused orchestrator's (or worker's)
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
- A task is **taken by any Tab** — worker *or* orchestrator — via `assigned_pane_id`
  (+ derived `taken_by_role`). The TaskCard shows a link chip → click opens that Tab.
- On the board, Inbox tasks render as a task cluster under their taker's orchestrator
  group (or an "Unassigned" group if untaken). Same TaskCard, same edges (to the Tab
  link). No sections ⇒ no section edges; that's fine.
- Archive of the Inbox plan is not offered; individual standalone tasks are
  `done/cancelled` like any task.

## 6. Graph / task CLI (talk to the board)

Query+mutate the plan store (NOT mem-graph; per §2 decision #1 plans stay JSON). New
`codeterm task ...` plus a canvas-focus emit:
```
codeterm task list [--active|--all] [--mine] [--unassigned] [--workspace] [--plan <id>] [--json]
codeterm task show <task_id> [--json]              # detail incl plan, sections, owner/worker links
codeterm task take  <task_id>                      # caller's tab (worker OR orch) takes it
codeterm task drop  <task_id>                      # release assignment
codeterm task add   --name <n> --type <k> [...]    # standalone (lands in Inbox) — alias of plan task add w/o plan
codeterm plan gaps  <plan_id>                      # sections with 0 tasks (what's left) — wraps coverage
codeterm canvas focus [--task <id>|--tab <id>|--plan <id>]   # emit canvas-focus -> camera + blink
```
- `task list --active` = not done/cancelled across all plans in the workspace project
  → "what's still in flight". `--mine` = assigned to the caller's tab/ancestors.
  `--unassigned` = no `assigned_pane_id` → "what can I pick up".
- `canvas focus` emits a `canvas-focus` event (Tauri + websocket) carrying target ids;
  the open canvas pans/zooms to fit those nodes and **blinks** them. Drives §7 Tasks
  button. Reuses the `plan-changed` fan-out plumbing (`emit_everywhere`).
- Reuse: all of these read the existing in-memory plan index; queries are pure.

## 7. Buttons (next to Chat | Terminal) — one role-driven component

`PaneActionButton` mounted right of `PaneModeSwitch`:
- **Orchestrator Tab that owns ≥1 active plan → "Plan".** Click → focus canvas on
  its plans. **Pulses** on `plan-changed` for an owned plan; pulse clears on open.
- **Worker Tab bound to a task (`CODETERM_PLAN_ID`/`TASK_ID`) → "Tasks".** Click →
  open canvas and `canvas focus --tab <self>`: camera fits + **blinks** this worker's
  task(s). Pulses on `plan-changed` touching its task.
- **Neither → button hidden.** Workspace overview is reached by clicking the workspace
  (no Tab focused), independent of these buttons.
- One component, role-driven label/target — no duplicated logic.

## 7.1 Plan is a THIRD Tab mode (Chat | Terminal | Plan)
Plan is not a separate overlay — it is a third mode in the Tab mode switch, peer to
Chat and Terminal. Selecting **Plan** highlights it (like the screenshot) and the
Tab body renders `<CanvasView mode="focus">` in place of the chat/terminal view.
Clicking **Chat** or **Terminal** returns. Mutually exclusive view state per Tab.
- Orchestrator Tab → Plan mode shows its plans; Worker Tab → label "Tasks", shows
  its tasks focused+blinking. Same role-driven component (§7), now wired as a mode in
  `PaneModeSwitch`, not a floating button.
- The mode still **pulses** on `plan-changed` for the represented plan(s).

## 7.2 Canvas composer fixes & activity bubble
- **Clear-on-send bug:** the talk-to-area composer must clear its input after a
  successful send (currently the text stays). Fix in `CanvasComposer`.
- **Activity bubble:** a small, non-modal bubble on the canvas shows live status of
  the addressed orchestrator — partial streamed message text or a compact "working on
  it…" indicator — so the user sees *something is happening* on the question they
  asked. Source it from the owner Tab's status/last-output (reuse tab-status /
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
- `cards/TaskCard.tsx` — add Tab link chip; tighten visuals (frontend-design).
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
  (`/` against dev on 7686), opens the workspace canvas / a Tab's Plan mode,
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

## 13. Interface inventory & text/size budgets (consolidated)

Every interface intent stated across the whole design, with concrete sizing so nodes
render correctly and **never overflow their bounds**. All numbers live in `settings.ts`;
these are the starting values, tuned in the visual loop (§15).

### 13.1 Plan node (compact, the structural overview)
- **Width fixed** (~`planWidth` 340). Header (~56px): title (1 line, ellipsis ~48 ch),
  owner chip, `done/total`, coverage bar.
- **Body = section rows**, one per section (recursive; subsections indented `sectionIndentX`).
  Each row ~36px: section title (1 line, ellipsis), task-count badge `[n]`, status glyph.
  **Row body text is NOT shown here** — only the title; the prose lives in the Spec node.
- **Height grows vertically with section count** — a long, detailed plan = a tall plan
  node (the "портянка"). That is expected and fine: you pan/zoom to read. No horizontal
  growth, so it never sprawls sideways.
- **Coverage = row fill tint:** covered row filled `coverCoveredBg` (accent), uncovered
  `coverUncoveredBg` (warning/dashed). Primary signal; edge reinforces.

### 13.2 Spec node (the full document — the long портянка)
- **Width** ~`specWidth` 480 (max), markdown wraps inside; **never overflows horizontally**.
- Renders the WHOLE spec: plan title → each section heading + `section.body` markdown,
  subsections nested. **Can be very long** — that is OK and expected for detailed plans.
- **Internally scrollable** past `specMaxHeight` (~720) so the board stays navigable, with a
  subtle scroll affordance; below that it sizes to content. Edge `plan ↔ spec`.
- Detailed plans are encouraged (§2.7 authoring guideline): full bodies, not stubs.

### 13.3 Task card (satellite, all the data)
- **Width** ~`cardWidth` 260. Content: status marker · name (1 line, bold) · `type` chip ·
  `model` chip · status badge + `status_label` · `model_rationale` ("why this model?"
  collapsed→expand) · `intent` · `body` markdown.
- **Body clamps** to `taskBodyClampLines` (~5) at mid LOD with fade; full at near LOD.
  Height ∈ [`cardMinHeight` 64, `cardMaxHeight` 320]; beyond max → internal scroll, never
  spill. Tasks **bin-pack near their covered section row**, collision-avoided.

### 13.4 Step bubbles (milestones — see §14 terminology)
- Small circle `stepBubbleSize` (~44) colored by step `kind` (declared color lane), or a
  compact chip (circle + kind label + text truncated ~`stepTextClamp` 36 ch). Full text on
  hover / near LOD.
- **Clustered per task**, grouped by group key (§14); iterations stack downward; `links`
  draw bubble→bubble inside the cluster so the **progress sequence** is visible. Cluster is
  width-bounded — wraps into rows, never streams off to the right (v1 §6.1 chain rule kept).

### 13.5 Groupings (two levels, both flexible)
- **Task groups** (`Plan.task_groups` + `Task.group`, ≈ phases): labeled bands/clusters of
  tasks. Ungrouped tasks are fine. Declared like kinds.
- **Step groups**: steps within a task group by a flexible key — `kind`, or `loop_id`/
  `iteration`, or any future principle. The grouping key is data-driven, not hardcoded, so a
  new grouping principle needs no layout change.

### 13.6 Subtasks (optional)
A task may have child tasks (`parent_task_id`, optional/flexible) rendered as smaller task
nodes clustered under the parent; each subtask has its own steps. Lead with steps; subtasks
are the heavier case, used only when a task genuinely decomposes. Deferred unless needed —
the model leaves room (`parent_task_id` nullable) without forcing it.

### 13.7 Bounds discipline (the overflow fix)
Every node **measures its own content** (text metrics / a measured DOM pass) and reports a
real size to the layout; the layout packs from real sizes so big sections cannot overflow.
Long bodies are confined: full prose only in the Spec node (wrapped, max-width, scroll);
the plan node shows titles only; task bodies clamp. **Nothing renders outside its node box.**

## 14. Terminology — milestones render as "Steps"
Per-task execution progress reads better as **Steps** than "milestones". UI label = "Steps"
(a task's ordered execution steps). The data model keeps the name `milestone` (no churn /
no migration) but every user-facing string says "Step(s)". A step still carries `kind`,
`text`, `links`, `loop_id`, `iteration` — grouping stays flexible (§13.5). Tasks may have
subtasks (§13.6). "Coverage" stays "coverage".

## 12. Done
Figma-grade board: sectioned plan node + spec node + edge-connected satellite tasks +
grouped milestone bubbles + orchestrator groups; uncovered sections pop; trackpad
pan + cmd/pinch zoom smooth; canvas isolated per workspace; standalone tasks work;
`codeterm task`/`canvas focus` CLI works; Plan/Tasks buttons pulse + focus+blink.
All gates green (tsc, canvas tsx tests, cargo). Dogfood plan renders itself.

## 16. v3 — SectionedDocumentNode + semantic zoom (synthesized from 2-provider brainstorm)

<!-- revision: 2 -->

Two independent agents (codex + cursor) converged on the SAME model. Adopted:

### 16.1 One node type: `SectionedDocumentNode`
Plan and Spec are the SAME primitive, `variant: "plan" | "spec"`. Same section tree, same
`@codeterm/chat-ui` `RichMarkdown` renderer (→ markdown + **mermaid** + **code syntax
highlight** for free), same coverage chrome, same block anchors. Plan variant emphasizes
coverage rows + task edges; spec variant emphasizes prose. Block-to-block edges via
`section_id` anchors (spec block ↔ plan row ↔ task).

### 16.2 Never hide info — semantic zoom ADDS, never REMOVES
- **Delete all `lod !== "far"` subtraction gates** (TaskCard, SpecSectionCard, etc.) — they
  blank bodies into empty shells. Content is ALWAYS mounted. This is the #1 user pain.
- **Counter-scale typography:** text uses `font-size: calc(base * clamp(0.55, 1/scale, 1))`
  so at far zoom it shrinks but stays present (like zooming a PDF), with a **~6px screen
  floor**; below the floor → microtype, never omission.
- **Three additive bands** (reuse settings thresholds, invert semantics): overview <0.45
  (full content micro-scaled + a fading summary overlay; mermaid = static thumbnail; code =
  syntax-colored micro lines), approach 0.45–1.1 (chips/status default-on; mermaid 50%
  thumbnail), reading >1.1 (full type, interactive mermaid, code toolbar). LOD adds controls/
  previews, never drops body. No ellipsis on core body (only redundant chrome).

### 16.3 Bound the портянки — fixed frame, internal pagination
- Outer frame is FIXED (A4-ish: plan compact ~340, prose ~420–720, `docMaxPageHeight ~480`).
- Markdown → block AST → measured offscreen → greedy-packed into PAGES (keep-with-next for
  headings/code/mermaid/tables; never split a diagram; split long code only at line
  boundaries with repeated header). 1–3 columns; page rows up to ~9 pages then internal
  pagination. Node layout height = header + bounded page area + footer + pager — NOT unbounded
  content height. On-canvas pager (◂▸ / dots, `data-page` for tests), not a km-tall scroll.
- Extreme plans: root node owns child `SectionChunk` nodes (one per top-level section) — still
  on-canvas, connected, never a hidden accordion.
- Task cards: same frame, header fixed, body paginated (~5 lines/page), `taskMaxHeight ~320`.

### 16.4 Mermaid + code at scale
Pre-render mermaid → SVG cached by content hash; scale via `transform`, never `display:none`;
atomic block for pagination (min thumbnail slot ~180px). Code via chat-ui highlighter;
virtualize long blocks in a bounded viewport with line range; full source always in DOM.

### 16.5 Auto-bounds: measure → pack → route (every plan-changed, debounced)
1. Measure pass: each node renders at reading band offscreen → exact `{w,h,anchors}`.
2. Incremental pack: columns Spec | Plan | Tasks | Steps; bin-pack tasks by nearest covered
   row Y; margins ~16px; STABLE ids keep slots (no global reshuffle → no jitter when flying);
   new nodes to nearest free slot.
3. Edge routing AFTER packing, obstacle-avoiding; nodes never overlap; CanvasView draws from
   layout boxes only. Debounce ~50ms→RAF, cancel stale jobs, apply atomically (no intermediate
   overlap visible during fast navigation). Interpolate small deltas, snap large.

### 16.6 Layout-integrity tests (Playwright, multi-zoom)
Synthetic fixtures (short / huge-section / long-code / mermaid / table / many-tasks /
realtime-insert). At scales 0.25/0.5/1.0/1.75: no node-node overlap (2px tol), content bbox
inside its viewport/scroll, **no zero-height text block when source is non-empty**,
`assertNodeFilled` (reject empty shells: textArea/bbox > threshold), edge endpoints inside
anchor bboxes, font floor ≥6px for titles/status, no horizontal overflow outside declared
scroll. Screenshot matrix in CI golden + the human/agent visual review loop.

### 16.7 Subtasks
`Task.parent_task_id` (backend, nullable). Subtasks render as child clusters under their
parent task card. Steps (milestones) remain per-task. Supersedes §13.6.

> This revises §13: plan node shows section ROWS (the spine) AND, per-row, paginated body via
> the unified node — not "titles only". Pinch-zoom sensitivity raised ~2-3x (§3).

### 16.8 Document↔index continuum + per-section meta (orchestrator + user brainstorm)
Refines §16.2's counter-scale with concrete density rules (from Andrey + orchestrator):
- **Wider nodes (~+30%):** plan ~340→440, spec ~420→560 — real markdown line length + room
  for a per-row meta column. You can "walk through" the doc.
- **TWO counter-scale curves:** TITLES gentle-clamp to a stable ~14–20px visual at all zoom
  (the navigation layer — never giant, never vanishing); BODY aggressive curve down to ~6px
  micro. At distance titles naturally dominate body without literal resizing.
- **Fill-at-distance, never empty:** when body counter-scales below the readable floor, the
  row collapses its body height and the freed space shows the META BAND (ready-state + count +
  1-line digest). No empty boxes (the core complaint) AND no useless text-cramming. If body is
  still readable, keep showing it — don't replace prematurely. "Не переусердствовать" both ways.
- **Per-section badges (tasteful, computed):** after each section title —
  `section.ready` glyph (✓ ready = all covering tasks done & count>0 / ● in progress /
  ⚠ uncovered), `[n]` task count, a small source/owner chip. ONE status dot, not a cluster.
- **Result:** FAR = structured index (titles + ready/count badges dominate, body micro);
  NEAR = a cool structured-by-sections document (full markdown/mermaid/code) with the same
  badges. Same content, re-weighted by zoom — the document↔index continuum.

### 16.9 Milestone bubbles = "Thread and Coil" (opencode + orchestrator + user brainstorm)
Milestones/steps are a task's report chain — render them as a semantic-zoom mini-timeline
OWNED by the task (4px stem, never floating), grouped by kind, coiling for loops:
- **FAR (<0.45) — constellation:** ~6px kind-colored dots, faint chronological arc; the live/
  freshest step pulses (8px, opacity 0.6–1.0, ~2s). Clean far view (the round shape Andrey likes).
- **MID (0.45–1.1) — thread:** 12px circles + kind glyph on a vertical **spine** (chronological
  via `links`); kind-lane tint bands group same-kind steps; loops render as an S-curve **coil**
  with an iteration badge; **a 1-line step title sits next to each dot — READABLE at mid** (the
  breakthrough: text without going full-near). Active step = pulsing ring; spine solid-before /
  dashed-after.
- **NEAR (>1.1) — step cards:** mini cards (~160×72) = kind strip + title + 2–3 lines body
  markdown + iteration badge, stacked along the spine; active card glows; max ~6 then internal
  scroll; click → detail.
- **Grouping:** by kind (lanes) default; `loop_id` present → auto-coil + kind/iteration toggle.
- Same "never hide info, add on approach" grammar as the doc nodes — one consistent canvas
  language. Rejected alternatives: radial bloom (sequence unreadable), horizontal timeline bar
  (fights the column packer).

## 17. Externalized bodies — large text lives in editable .md files (not in JSON)

<!-- revision: 2 -->

Section/task bodies can be LARGE. Storing them inline in the plan JSON makes them painful to
hand-edit (escaped newlines, one giant line) and forces a CLI for every text tweak. Instead,
**JSON holds structure; bodies live as sibling .md files**, wired under the hood.

### 17.1 Layout
```
<project>/.codeterm/plans/<plan_id>.json                 # structure: ids, titles, status,
                                                          #   refs, tasks meta, milestones, canvas
<project>/.codeterm/plans/<plan_id>/sections/<section_id>.md   # section body markdown
<project>/.codeterm/plans/<plan_id>/tasks/<task_id>.md          # task body markdown
```
- Convention-based (daemon derives the path from `plan_id` + `section_id`/`task_id`) — no
  brittle absolute paths stored. The JSON section/task omits the inline `body` (or keeps an
  empty marker); the daemon HYDRATES it from the .md on read and PERSISTS to the .md on write.

### 17.2 Wiring (single-writer + watcher, reuse what exists)
- **Write path:** any `plan section update` / `plan task update` (CLI/API) writes the body to
  its .md file (atomic temp+rename) and touches the JSON; same single-writer discipline.
- **Edit path (the point):** open the .md in vim/VSCode, save → the existing `.codeterm/plans`
  file-watcher (Rex's rescan/watch work) sees the change, reloads that body, re-hydrates the
  plan, and emits `plan-changed` → the canvas updates live. **No CLI needed to edit text.**
- **Read path:** `plan get` / the API `?full=1` hydrate bodies from files so consumers see the
  full content unchanged.

### 17.3 Migration
No backward-compat needed (personal project). On load, if a plan still has inline bodies, the
daemon migrates them out to .md files once and clears the inline field. New plans write
externalized from the start.

### 17.4 Scope
Applies to **section bodies** (the spec/plan prose — the large ones) and **task bodies**.
Small fields (title, status, model, ids, milestone text, refs) stay inline in JSON — they're
not the editing pain. Milestone text stays inline (short, machine-appended by reports).

## 18. Node VIEW MODES + manual controls (supersedes continuous counter-scale §16.2)

<!-- revision: 3 -->

Continuous text counter-scale (§16.2) is REJECTED by the user — stretching text with zoom
feels bad. Replace it with **discrete, designed view modes** per node + **manual controls**.

### 18.1 Discrete view modes (each DESIGNED to look good at its distance)
Every doc/task node renders in one of a few fixed, hand-designed modes — NOT scaled text:
- **Document** — full markdown body (mermaid/code), per-section badges. The reading view.
- **Card** — title + ready/count badges + a 1–2 line digest. Compact, legible, no tiny text.
- **Tile** — title + status glyph + counts only. The far/overview view; reads cleanly at distance.
Switching modes swaps the LAYOUT/representation; text inside each mode is at a FIXED, readable
size (no continuous scaling). This is the "looks better at large distances" the user wants.

### 18.2 Manual controls (the toggles)
- **Scale controls:** explicit zoom in / out / fit / reset buttons (zoom is not pinch-only).
- **View-mode switch:** a control (global, and optionally per-node) to pick Document / Card /
  Tile. The user drives it.
- **Text on/off:** a toggle to hide bodies (declutter to Card/Tile) — DELIBERATE, user-driven;
  never an accidental empty box (§16.2's no-empty rule still holds for the chosen mode).
- **Auto-by-distance (optional, overridable):** view mode MAY auto-pick by zoom band
  (far→Tile, mid→Card, near→Document) as discrete designed views — but the user can lock a mode.

### 18.3 No-overflow discipline + Playwright verification
Each mode is bounded and must never overflow/clip. Verify with Playwright across modes ×
zoom bands (screenshot + bbox asserts: no overflow, no overlap, every mode legible) — the same
visual review loop the orchestrator runs. Agents may be launched to iterate the modes' look.

> Net change from §16.2/§16.8: keep "never hide info accidentally", wider nodes, badges, and the
> doc↔index intent — but deliver it via DISCRETE designed view modes + manual controls, NOT
> continuous text scaling.

### 17.5 File-map manifest + read CLIs (so editing the .md files is convenient)
Opaque `sec_<uuid>.md` / `task_<uuid>.md` filenames make hand-editing hard ("which file is
this section?"). Fix with an auto-generated map + read commands:
- **`<plan_id>/INDEX.md`** — the daemon regenerates this on every write: the plan title, then a
  list mapping each section/task TITLE → its relative `.md` path (a clickable markdown link).
  Open the folder, read INDEX.md, jump to the right file. Always current (single-writer rewrites
  it alongside the body write).
- **`codeterm plan get <id>`** — already assembles the full plan markdown (hydrated). 
- **`codeterm plan spec <id>`** — prints just the sections (the spec prose) assembled, for
  reading the full spec without the task/meta chrome.
- **`codeterm plan files <id>`** — lists section/task title → `.md` path (for scripting / quick
  "which file do I edit").
INDEX.md is generated, gitignored with the rest of `.codeterm/plans/`, never hand-edited.

## 19. Tag system (synthesized: opus two-tier + codex typed-payload)

<!-- revision: 3 -->

Tags give every node a purposeful payload at every zoom band, readable task/file names, and
future grouping/filtering. Synthesized from 2 brainstorms (opus + codex).

### 19.1 Two-tier model (THE load-bearing decision — opus)
A flat folksonomy rots at scale (synonym drift, unstable colors, n² edges). Split:
- **FACETS — declared, controlled, COLORED, structural.** A small per-plan vocabulary of
  *dimensions* with value sets (`area`, `phase`, `risk`). ONLY facets drive color/group/filter.
  This reuses the existing `milestone_kinds`/`task_groups` "declared color lane" idiom (§2.4/§13.5).
- **TOPICS — free kebab keywords, UNCOLORED, the long tail** (`mermaid`, `bin-pack`, `debounce`).
  Power search + handles; never colored; allowed to be messy.

### 19.2 Schema
String form, parsed on `:`. Faceted `"area:layout"` / `"phase:v3"`; topic `"semantic-zoom"`.
```
Plan.tag_facets: [ { key:"area", label:"Area", color_lane:true,
                     values:[{v:"layout",color},{v:"nav",color},{v:"storage",color},…] },
                   { key:"phase", values:[{v:"v2"},{v:"v3"}] } ]
Section.tags: ["area:layout","semantic-zoom","bin-pack"]   // first faceted = PRIMARY
Task.tags:    ["area:nav","pan-zoom"]                       // inherits section tags + execution tags
```
- **PRIMARY** = first faceted value = the headline essence: biggest at distance, tints the node
  (left-border accent), seeds the handle. One node = one dominant color + neutral secondary chips.
  Topics never color → no rainbow. Coverage tint (§13.1, row background) and facet accent
  (left border) are two non-clashing channels.

### 19.3 Handles — readable names for tasks AND .md files (kills T10 + opaque filenames)
- **Handle = `<primary-facet-value>-<most-distinctive-topic>`** → `nav-pan-zoom`,
  `storage-externalize`, `playwright-zoom-matrix`. Daemon-generated, unique per plan (collision →
  short suffix), author-overridable via explicit `handle`. `task_<uuid>` stays the real key;
  handle is a display alias + CLI selector (`codeterm task show nav-pan-zoom`).
- **Externalized .md named by handle** (supersedes §17 opaque names): `sections/layout-binpack.md`,
  `tasks/nav-pan-zoom.md`. INDEX.md lists handle → path. One mechanism fixes "what is T10?" AND
  "which .md is this?".

### 19.4 Per-distance payload — tags INVERT role by §18 view mode (substitute→companion→ornament)
- **TILE (far):** tags SUBSTITUTE for body — Title · ●primary-facet (colored) · 1–2 top topic
  chips · ready glyph · `[n]`. The far board becomes a readable CONCEPT MAP, not gray micro-text.
  THE biggest payoff (fixes the gray far-view). Tasks: handle · primary color · status · model chip.
- **CARD (mid):** tags ACCOMPANY a 1-line digest (authored `summary`, fallback first sentence) +
  all topic chips + ready/`[n]` badges + strongest linked task handles.
- **DOCUMENT (near):** tags RECEDE to a clickable chip strip in the header; PRIMARY tints the
  header bar; full markdown body dominates. Extends §16.8's meta band; every band = designed payload.

### 19.5 Tag Map = HUD legend+filter (not a word cloud)
A pinned panel (with §18.2 controls): each facet → colored values + counts (sections/tasks/
active/done/uncovered). Click a value = filter/highlight (dim others, optional convex-hull lasso —
NOT n² edges; if edges wanted, route through a per-facet HUB star = n edges). Multi-select within
facet = OR, across facets = AND; topic search box substring-filters. Optional small on-canvas
Legend node near origin so one screenshot tells the whole story (dogfood §11).

### 19.6 Authoring (manual wins; auto only suggests)
- **Front-matter in the .md** (best fit with §17 watcher): YAML `tags: [area:layout, semantic-zoom]`
  at the top of `sections/<handle>.md` → save → live tag update, no CLI.
- **CLI:** `plan section update <id> --tag area:layout --primary area:layout`,
  `plan task update <id> --tag area:nav`, `plan tags <plan>` (facet vocab + counts + fuzzy-dup
  warning = vocab governance), `task show <handle>`, `canvas filter --facet area:layout`.
- **Auto-suggest:** a tagger pass (reuse mem extraction-worker pattern) writes `suggested_tags`
  (ghosted, distinct from confirmed); author promotes with one action; auto NEVER colors/groups
  until confirmed → keeps the controlled vocab clean.

## 20. Worlds model + Banded Multi-Column task layout (synthesized: opus + cursor converged)

<!-- revision: 3 -->

The organizing frame that ties §16/§18/§19 together and kills the портянки. Both brainstorms
converged: "worlds" is VOCABULARY + an LOD contract, not a heavy nested abstraction.

### 20.1 Worlds = vocabulary + LOD contract (two containment levels, not infinite)
- A **world** = a containing region with a level-of-detail contract. World = the unit;
  §18 view mode = its discrete LOD state; §19 tags = its per-distance payload/identity.
- **Bounded to TWO levels:** `plan-region` (soft group, §2.5) ⊃ leaf worlds `{spec, plan, task}`.
  A task's **subtasks + steps are BOUNDED INTERIOR DETAIL** of the task-world (expand IN PLACE),
  NOT separately-navigable worlds. This line stops over-abstraction.
- **Invariant (non-negotiable):** ONE flat, connected, auto-packed map; never hide info; NEVER a
  modal drill-stack (that's the §16.3 accordion under a new name — it severs the cross-world edges
  that make the board worth anything).

### 20.2 Zoom = SELECTOR, not stretcher
Crossing a zoom threshold SWAPS a world to the next hand-designed discrete §18 mode
(Tile→Card→Document), each FIXED readable text. The "fly-in" feeling = camera zoom-to-fit +
threshold crossings, never glyph scaling. §18 modes ARE a world's discrete LOD states.

### 20.3 Banded Multi-Column Sweep (the task-портянка fix — opus algorithm)
Replaces the single tall task column with a bounded-width grid anchored to sections.
1. **Band** = task's PRIMARY covered section (first/topmost `section_id`). Standalone → synthetic
   band under taker group. Band never changes unless coverage changes → stable.
2. **Stable intra-band order** by creation/id (NOT size) — realtime insert appends, no reshuffle.
3. **Bounded column grid** right of plan: `colX[c]=planRight+margin+c*(taskColWidth+colGap)`,
   `c∈0..maxTaskCols-1` (≈3–4) → caps horizontal sprawl.
4. **Lay band left→right, wrap to sub-rows** (7 tasks @3 cols → 3·3·1).
5. **Top-down non-overlap sweep:** process bands by ascending `rowY`; `idealTop=rowY[b]-blockH/2`;
   `actualTop=max(idealTop,cursorY)`; `cursorY=actualTop+blockH+bandPad`. Classic non-overlapping
   interval label-placement: each block hugs its section row, pushed down only when crowded.
   Deterministic; inserts recompute only the changed band DOWN (no global jitter, §16.5).
6. **Edges:** card bezier from its covered section-row anchor → its left edge (short — cluster sits
   in the row's Y-band). Multi-section: solid to primary + thinner to others. Route after packing.
7. **Subtasks = "super-cell":** parent + subtask stack = one cell (w=parentW+subtaskColW). Packer
   treats it like any cell → subtasks nest right of parent, in-band, bounded.
8. **Steps = interior detail** of the task-world (§16.9), part of the cell's measured height — no
   own grid cells. Tile→folded constellation; Document→step cards grow cell height → sweep absorbs.
9. Result: 19 tasks over ~8 bands ×≤3 cols ≈ a few hundred px tall, width hard-capped.
- Escape hatch (if still tall): TRANSPOSE — horizontal section ribbon on top, kanban task columns
  under each section header. Bigger rewrite; keep as fallback.

### 20.4 Spec & Plan портянка
- **Spec-world = PAGINATED document-world + section RAIL** (§16.3 + §18-Document): Tile = a "book
  cover" (title · primary-facet border · `[16 sections]` · 1-line abstract · facet dot; tags
  SUBSTITUTE). Document = fixed A4 frame + greedy block→page packing (keep-with-next; never split
  a diagram) + on-canvas pager (◂ ●●○○ ▸) + a persistent left **section RAIL (in-world TOC/mini-
  map)** → click a title to page to it. The rail is what stops it "falling apart". EXTREME spec →
  child `SectionChunk` worlds (one per top-level section), each paged + edge-connected to its plan
  row → the портянка decomposes into a constellation on the map.
- **Plan-world = INDEX (tall-thin OK, ≈16·36px):** at Tile/Card collapse subsections to "[k subs]"
  and collapse phase groups (§13.5) to one band row; full row tree only in Document. Deep reading
  lives in the spec-world's rail, not the plan node.

### 20.5 Drill-down / back-out / "where am I" (non-modal)
- **In-place semantic zoom (default):** pan/zoom the ONE shared canvas; each world auto-picks mode
  by zoom band; approaching a task expands its interior in place; neighbors stay visible.
- **Focus = camera convenience, not a mode:** double-click a world (or `canvas focus`, §6) → smooth
  zoom-to-fit + LOCK its mode to Document + DIM (never remove) the rest. Cross-world edges still
  drawn to dimmed neighbors. **Back out** = Esc / "fit all" / zoom-out → un-dim, release lock,
  restore saved viewport (§3).
- **Breadcrumb HUD:** `Workspace ▸ Plan:<handle> ▸ <world-handle>` (focused world, or viewport-
  center dominant world); focused world wears a highlight ring. No modal stack to get lost in.

### 20.6 Per-distance payload matrix (world type × §18 mode × §19 tag role)
- **PLAN** — Tile: title·primary dot·done/total·thin coverage strip·1–2 topics (tags substitute).
  Card: +facet chips·top-level rows (subs folded)·1-line digest. Document: full row tree·per-row
  meta band (§16.8)·tags recede to header.
- **SPEC** — Tile: book-cover. Card: rail preview (~5 titles)·digest·topics. Document: paged A4 +
  rail + pager·interactive mermaid/code·tags in header; extreme→chunk-worlds.
- **TASK** — Tile: handle·primary left border·status·model·folded step-constellation (tags
  substitute). Card: handle·status+label·model+collapsed rationale·digest·topics·step thread·
  subtask count. Document: full body·rationale·intent·step cards·subtask cluster (right)·Tab link.
- **STEP-CLUSTER** (interior, LOD=its task's): far constellation / mid thread+coil / near step cards
  (§16.9).
- **PLAN-REGION** (§2.5): always a soft labeled band; orchestrator title = the "territory name",
  dominant at far zoom; never collapses its children (containment ≠ hiding).

## 21. Project plan-memory (mem layer) — RESEARCH verdict (2 brainstorms converged)

<!-- revision: 3 -->

Question: should plan tasks/sections/discussions flow into a project-isolated mem (vectorized)
for recall/dedup/authoring-assist? **Verdict: WORTH DOING, NARROWLY — an embed-only sidecar
INDEX, NOT the extraction/graph/dreaming stack** (the infra is ~80% there; the only expensive
parts are exactly the ones we don't need). NOT a Canvas-v2 blocker; do it after.

- **THE ONE THING TO GET RIGHT (opus):** derive a STABLE scope from the canonicalized PROJECT
  ROOT — `scope = "workspace:plan_<hash(project_root)>"` — NEVER the ephemeral workspace id
  (else isolation breaks on workspace churn and diverges from §4 path-scoping). mem scope is an
  opaque string, so this reuses every existing scope query.
- **Embed-only:** POST mem items with `skip_extract=true` → local/free fastembed (384-dim) on the
  async bg queue, OFF the write path. No LLM extraction worker, no dreaming, no graph for plan items.
- **Staleness free:** save with `--source-path` → the §17 externalized .md + Librarian re-vectorize
  on mtime change automatically. Tags map 1:1 onto §19 facets+topics (the spec already anticipated
  reusing the mem extraction pattern, §19.6).
- **One-way DERIVED read-model:** plans stay JSON (§2 #1); mem is an INDEX only. Plan write →
  idempotent upsert (id=`plantask_<uuid>`, delete+insert); archive/delete → mem delete. Mem is
  NEVER authoritative for task state/status/edges.
- **Use cases (ranked):** HIGH = semantic+keyword search over plan history / cross-plan (killer
  feature, reuses RRF fusion); HIGH = authoring assist (similar past tasks, up-weight cancelled/
  stalled); MED = cross-plan dedup; LOW = drawn "related" edges (n² clutter — use a panel); SKIP =
  graph/dreaming over plan items.
- **Smallest first step:** one-way mirror on plan write + a thin `codeterm plan search` reusing
  `/api/mem/search`. NO UI yet; validate recall with `evals/mem-recall`. Then add the authoring
  "Related" strip. Features: F1 Plan Recall Index (sidecar), F2 "Related past work" strip,
  F3 Stalled-work recall ("ghosts" — abandoned tasks semantically near active work).

## 22. FOUNDATIONAL RULES (override conflicting earlier sections)

<!-- revision: 4 -->

Two non-negotiable base rules from Andrey. Where earlier sections conflict, THESE win.

### 22.1 Zoom = camera; node BOUNDS fixed, but content MAY adapt inside them
Clarified by Andrey: a node's OUTER SIZE/BOUNDS stay FIXED at any zoom (zoom never changes a
node's footprint → layout never reshuffles). The CONTENT INSIDE the fixed frame MAY adapt detail
to zoom (semantic content LOD is fine and wanted — e.g. more text on approach) — but it must
ALWAYS FIT the fixed bounds, NEVER overflow. So: the FRAME is fixed; the CONTENT is allowed to
change. Forbidden = zoom changing the node's bounds/footprint. This KILLS:
- continuous counter-scale typography (§16.2) — already removed.
- AUTO-by-distance view-mode switching (§18.2, §20.2 "zoom = selector") — REMOVED. View mode
  (Document/Card/Tile) is a MANUAL choice (per-node or global toggle), never triggered by zoom.
- semantic-zoom-as-LOD (§16, §20 drill-by-zoom) — reinterpreted: "drill in" = pan/zoom the
  CAMERA to a fixed-size node; the node itself never morphs because of zoom.
So: pick a view mode manually → the node renders at that fixed size → zoom only moves the camera.

### 22.2 Plan + Spec = ONE sectioned node with per-section OUTPUTS (not two nodes)
There is ONE node holding the plan/spec, NOT a separate plan-node + spec-node:
- Header: plan title, owner, coverage.
- Body: a stack of SECTION blocks; each block = section title + (markdown body, per view mode) +
  a per-section OUTPUT ANCHOR (вых) on its edge.
- TASKS connect to those section outputs (banded multi-column, §20.3 still applies to tasks).
- The spec prose IS the section bodies (no separate spec node; §2.3/§16.1 spec-node is folded in).
This is ONE coherent sectioned document node with connection points, fixed-size, navigated by
camera. The портянka concern is handled by: it's ONE structured node (not scattered) + a section
rail/TOC for in-node navigation (§20.4) + manual Tile/Card mode to compact it when desired.

### 22.3 Open question → brainstorm
Given 22.1 + 22.2, the WHOLE node-layout system needs (re)designing: how the one sectioned
node + its task outputs + milestone interiors + standalone tasks + multiple plans + the
orchestrator region all lay out as fixed-size nodes on a camera-zoomed canvas without overlap,
without портянki, readable. Brainstormed next (§23).

## 23. The node-layout system (synthesized: opus + cursor converged) — honors §22

<!-- revision: 4 -->

Both brainstorms converged. Full long-form: `2026-06-24-canvas-v3-layout-proposal.md`. Decisions:

### 23.1 ONE SectionedPlanNode (fixed-size per MANUAL mode)
- **HEADER:** title · owner · coverage · manual mode switch (Doc|Card|Tile) · fit-node button.
- **RIGHT-EDGE RAIL (always-rendered chrome):** one row per section — ready glyph · title · `[n]` ·
  **OUTPUT dot (вых)**. The rail is BOTH the in-node TOC (click → scroll body to section) AND the
  task connection anchors (all sections always connectable, independent of body scroll). Rail row
  world-Y is STABLE → edge anchors stable.
- **BODY in a FIXED frame, per manual mode:**
  - **Document** = full RichMarkdown (mermaid/code) with **INTERNAL SCROLL** inside the fixed frame
    (Alt B — full fidelity, minimal machinery; the frame bounds it, §22).
  - **Card** = section titles + 1-line digests.
  - **Tile** = title + coverage + counts (rail collapses to an outputs stub).
- Fixed bbox per mode (settings.ts): e.g. Tile 480×280 / Card 480×400 / Document 480×640. The MODE
  sets the size (manual); zoom NEVER does (§22.1). Spec prose = the section bodies (one node, §22.2).
- Manual per-section `collapsed` (persisted) skips its body in the scroll but keeps its rail row+anchor.

### 23.2 Tasks (Banded Multi-Column §20.3, anchored to rail outputs)
Band = primary covered section; tasks pack in a bounded 3–4-col grid right of the node; bezier from
the section's OUTPUT dot → task left edge. Band assignment is mode-independent; only the edge
endpoint moves when mode changes. Task cell fixed-size per its own manual mode; steps (§16.9 Thread
& Coil) are INTERIOR to the fixed task cell, representation by the task's manual mode (not zoom).

### 23.3 Multi-plan + orchestrator (3 nested deterministic packers)
task-sweep ⊂ plan-region (node + its task field, plans L→R, wrap) ⊂ orchestrator group (top-down,
stable by owner id). Inbox/standalone = synthetic-band region. No overlap; stable ids keep slots;
incremental remeasure on insert (downward sweep); mode change = full remeasure, monotonic slots.

### 23.4 Readability WITHOUT zoom-resize (the crux — 5 manual/structural mechanisms, NOT zoom-LOD)
1. **Discrete manual view modes** — global **Tile = legible concept-map overview** (fixes the 13%
   gray board; reads tags/coverage, §19.4).
2. **In-node rail/TOC** navigation at any camera zoom.
3. **Bounded fixed frame + internal scroll** → fit-all lands ~50–70%, not 13%.
4. **Camera-focus reading gesture** — double-click / `canvas focus` → zoom-to-fit the fixed
   Document node (PDF-at-100% readable) + dim (not hide) neighbors + breadcrumb. Esc = back.
5. **Minimap HUD** (frame + task dots + viewport rect).
- **Default mount = focused on the active plan (~90%), NOT fit-all.** Presets: Overview (Tile+fit) /
  Read (Document+focus) are manual buttons, never zoom-triggered.
- **Extreme plans (>30 sections) → child SectionChunk nodes** (flat connected constellation, each a
  fixed Document node + own outputs), never an accordion/km-strip.

### 23.5 Implementation locks (FE)
KILL the Auto-by-zoom view mode + any content-driven node height in layout. Lock the settings
constants (planTileH/planDocH/railW/maxTaskCols). Emit rail-row OUTPUT anchors BEFORE the task
sweep. Change default mount fit-all → focused-plan@90%. Playwright mode×camera-zoom visual loop
asserts FIXED node sizes + no overflow (content scrolls inside frame) + anchors on rail rows.

### 23.6 Distance-gated internal scroll (wheel = pan FAR / scroll-content NEAR)
The fixed-frame internal scroll (Document body, §23.1) must NOT fight canvas panning:
- **FAR / not-focused:** wheel over a node = PAN/zoom the CANVAS (no scroll interception). The
  node does not capture the wheel.
- **NEAR / focused (zoomed into / camera-focused a node above a threshold scale):** wheel over the
  node's body = SCROLL its internal content; at the scroll boundary, release back to canvas pan
  (no trap). Threshold = the same camera scale that enters Document reading (~>1.0).
- Never annoy: a gentle hand-off, momentum respected; horizontal pan unaffected; a subtle scroll
  affordance only appears when scroll is active (near). This resolves opus's scroll-vs-pan gate.

### 23.7 Content-LOD on zoom INSIDE the fixed frame (size fixed, content adapts)
Clarifies §22.1: zoom must DO something to CONTENT (not size). Within a node's FIXED frame, the
content density adapts to camera zoom:
- **FAR (zoomed out):** compact — section INDEX only (title · ready · [n] · output dot).
- **NEAR (zoomed in past a threshold):** the section BODIES (the spec prose / markdown / mermaid)
  fade in INSIDE the same fixed frame, with §23.6 internal scroll for overflow. The node footprint
  does NOT change — only what's painted inside.
This reveals the spec on approach and makes zoom responsive, while honoring "bounds fixed" (§22.1).
The MANUAL Doc/Card/Tile toggle still overrides the frame SIZE/base; content-LOD layers on top
(an "Auto" content behavior). FE killed auto entirely — bring back AUTO CONTENT (not auto size).
Playwright must verify content-LOD at far/mid/near camera zoom (index→bodies) with FIXED node size.

## 24. ANDREY OVERRIDES (supersede §22.2 merge, §23 manual modes, §23.6 release)

<!-- revision: 5 -->

Direct corrections from Andrey — these WIN.

### 24.1 ONE mode — no Doc/Card/Tile buttons; it's just a canvas
REMOVE the Doc/Card/Tile mode switcher from the toolbar. There is ONE behavior: every node
ADAPTS ITS CONTENT to camera zoom (far = compact section index/title; near = full bodies/prose/
mermaid) inside a FIXED frame (§23.7 content-LOD, now the ONLY behavior, no manual toggle). Zoom
in/out/fit + minimap + Overview/Read camera presets may remain (they're camera, not node-mode).

### 24.2 Scroll = node-only when hovering it; NEVER hand off to canvas
When the mouse is OVER a node's body, the wheel scrolls ONLY that node — never releases to canvas
pan, even at the scroll boundary (§23.6's boundary-release is REMOVED). To pan/zoom the canvas,
the user moves the mouse OFF the node. Simple, no hijack, no "throw up to canvas".

### 24.3 SEPARATE Spec node connected to the Plan (un-merge §22.2)
Spec is its OWN sectioned node again, NOT merged into the plan node:
- **SPEC node** (left): one block per section showing the section's TITLE + BODY (the prose) —
  content-LOD per §24.1 (far=titles, near=full prose).
- **PLAN node** (middle): one row per section (title · coverage/ready · [n] · OUTPUT dot).
- **1:1 edges SPEC-section-block ↔ PLAN-section-row** (same section id) — shows which plan section
  maps to which spec section. Both reflect coverage.
- PLAN row OUTPUT dots → tasks (banded, as now). So: SPEC | PLAN | tasks, three connected columns.
(The §22.2/§23.1 "one merged SectionedPlanNode" is reverted to two connected nodes; the rail/
content-LOD/banded mechanics carry over to BOTH the spec and plan nodes.)

### 24.4 Zoom-drill INTO tasks (fall in by zoom, not click)
A user zooming the camera INTO a task progressively reveals its INTERIOR — no click required
(content-LOD §23.7 applied to task nodes): far = handle + status + step dots; closer = step
titles + subtask handles; near = full step CARDS (milestone text/iteration) + subtask bodies +
task body. Fixed task footprint; only the painted content deepens. "Coming back out" = zoom out.
Brainstormed (§25) for the exact thresholds, what reveals when, the feel, and overlap handling.

### 24.5 Status emphasis (in-progress pops, done gently dims)
Task (and step) visual weight by status: **in_progress = HIGHLIGHTED** (accent ring / subtle glow
/ brighter) so live work pops; **done = slightly DIMMED** (~0.7 opacity, NOT heavily — still
legible); pending/blocked = normal (blocked maybe a warning tint). Applies at every zoom; the
freshest/in-progress is the eye magnet on a live board.

### 24.6 Subtask VOLUME at bird's-eye, detail on drill (don't let subtasks eat space)
Subtasks must NOT take much space at overview. From bird's-eye (far zoom) a task shows its
VOLUME/scale compactly — that it's BIG, e.g. an "N subtasks" badge / a stacked-cards depth cue /
a density glyph / a small filled-vs-total bar — WITHOUT rendering all subtasks. On zoom-drill
(§24.4) into the task, the subtasks expand to their cards. So: far = "how big is this task" in a
glance; near = the actual subtasks. Use the canvas spatially (zoom = level-of-detail), not by
flattening everything always-visible. Part of the §25 zoom-drill brainstorm.

## 25. Zoom-drill into tasks + subtask volume (synthesized: opus + cursor converged)

<!-- revision: 6 -->

Resolves §24.4 / §24.6. Two brainstorms (opus 5f5ac58f, cursor 9376b50e) converged on the same
ladder; opus contributed the two load-bearing mechanics below. This is the implementation spec.

### 25.1 LOD driver = APPARENT on-screen size, not raw camera scale
The band a node is in is driven by its **apparent width in screen px**: `appW = cellWidth_world ×
cameraScale`, NOT by raw `cameraScale` alone. Each node crosses its OWN bands when it is physically
big enough to READ — so a wide SPEC node and a small TASK card reveal their interiors at the right
moment instead of a global scale forcing a task to show body text while still unreadable. (Cursor's
raw-scale thresholds were the rejected fallback for exactly this reason; opus's appW is the pick.)

### 25.2 RESERVE-AT-DEEPEST packing → overlap structurally impossible (honors §22.1)
The packer sizes every task cell at its **deepest-LOD bbox** (task body + ~6 step cards + a subtask
super-cell), generous-but-bounded, with a node-only internal-scroll tail for the rare overflow.
Content-LOD then only changes WHAT IS PAINTED inside the always-reserved box. Consequences: footprint
never changes (§22.1 satisfied), zero reflow/jitter while zooming, neighbors never repack on drill →
overlap is structurally impossible, not policed after the fact.

### 25.3 Two orthogonal mechanisms — BAND (paint) ⊥ DOMINANCE (focus-assist)
- **BAND** = what is painted; driven ONLY by this node's appW (§25.1). Pure zoom reveal, one
  behavior (§24.1), no modes, fully reversible with ±10% hysteresis (no flicker at thresholds).
- **DOMINANCE** = focus-assist for the task you "fell into"; viewport-relative (fires when one task
  fills ≳60% of viewport). Effect = **elevate** (drop shadow + paint above neighbors, NO footprint
  change) + **soft-dim neighbors** (~0.4). Lockless, no auto-camera-move (disorienting). Double-click
  (§20.5) stays the COMMITTED/locked twin; dominance is the transient hover/zoom one.

### 25.4 Content-LOD ladder (appW px @ ~280px-wide task cell)
- **L0 GLYPH** `appW < 150`: handle + status ring; steps = constellation (6px kind dots, live
  pulses); subtasks = **VOLUME CUE only** (see §25.5) — no per-subtask render. Far board = readable
  concept map that still shows scale.
- **L1 THREAD** `150–320`: + status label + model chip; steps = vertical thread (12px + kind glyph +
  1-line step TITLES + loop coil/iteration badge); subtasks = one-line roster chips (cap ~5, `+k`).
  Titles only, no bodies.
- **L2 CARDS** `320–560`: step mini-cards (~160×72) on the spine; subtasks **fan out** to mini-cards
  in the super-cell; task body fades in CLAMPED; node-only scroll if overflow (§24.2).
- **L3 READING** `> 560`: unclamped task body + full step cards (milestone text/iteration) + all
  subtask bodies + model_rationale + Tab link chip. Dominance focus-assist fires here.
- Crossfade between bands = opacity + a 4px surface-rise only; **NO geometry reflow** ever.

### 25.5 Subtask volume cue — "card-deck thickness" that unfolds
Never flatten subtasks onto the board. The cue is ONE object that unfolds as you drill:
- **Far (L0):** stacked-card depth **silhouette** (depth encodes magnitude, capped ~3 ghost layers,
  3px offset each) + `⊞N` badge + a done/total mini-bar. Reads as physical volume pre-attentively;
  **0-subtask tasks stay flat** so heavy tasks pop.
- **Mid (L1):** the stack peels into a one-line roster (≤2–5 handle chips + `+k`).
- **Near (L2):** fans out to subtask mini-cards inside the reserved super-cell — the SAME object
  unfolding ("fell INTO the stack", not "a panel opened").
- **Reading (L3):** full subtask bodies + each subtask's own step thread (fractal).

### 25.6 Status emphasis (§24.5) — orthogonal, multiplies at every band
in_progress = brighter + accent ring; at L0 the ring **pulses** = eye-magnet = the "auto-suggest"
(ATTRACT, never hijack the wheel / never auto-pan). done ≈ 0.7 dim (restores toward 1.0 at L3 so
history reads comfortably). blocked = warning left border, normal opacity. Fractal: live task among
tasks → live step among steps. One-key "jump to active" fly-to + ping ripple offered; auto-follow
tail-mode default OFF.

### 25.7 Coming back out / not getting lost
ONE flat canvas, no drill-stack: zoom out reverses the ladder (hysteresis = no flicker) and the same
spatial map re-reveals. Breadcrumb appends the task handle when it's dominant. Minimap = viewport
rect + drilled-cell highlight. Esc / a corner "fit board" chip = one-action escape. Composes with
SPEC|PLAN|tasks (§24.3): same ladder on all nodes; coverage edges stay drawn to dimmed neighbors
(§20.1 never-modal).

### 25.8 Playwright verification matrix
Assert across the band boundaries (drive camera so a task hits each appW): fixed task footprint
(±2px across all bands), content-LOD actually changes per band, volume cue present at L0 with NO
subtask text, subtask cards present at L2+, in_progress weight > done weight at EVERY band, no
overflow (overflow scrolls node-only, never canvas), no overlap, breadcrumb + Esc escape work.

## 26. WORLDS zoom model — generalize §25 to every node + fix body-reveal (opus+cursor+codex converged)

<!-- revision: 7 -->

Resolves Andrey's "worlds" direction. Three brainstorms (opus 56c1e70f, cursor 09d9bc85, codex
c64b684b) converged on ONE unified appW-driven content-LOD contract spanning SPEC, PLAN, TASK,
subtasks, milestones. This SUPERSEDES the task-only §25 ladder by generalizing it to all node types,
and FIXES the live body-invisible bug. Zoom is the only navigation; nothing hidden behind clicks.

### 26.1 Root bug (located in code by opus)
- `CanvasView.tsx:273` — `contentNear = scale >= 1.3` is ONE global raw-camera-scale flag for all
  nodes (the §25.1 anti-pattern: a wide SPEC and a small TASK cross "near" at the same camera scale,
  so neither reveals at the right physical size).
- `SectionNode.tsx:133` — plan bodies hard-gated via `!isPlan`, so the PLAN node NEVER renders body.
- Net: PLAN is index-only at every zoom; SPEC body threshold not reached even at 173%; task body /
  model_rationale only behind a click-toggle. Markdown (`RichMarkdown`) IS wired and bodies ARE in
  data — this is a reveal/threshold bug, not a missing renderer.
FIX: delete the global flag + the `!isPlan` gate; drive a per-node band off `appW`.

### 26.2 The four worlds (appW-driven, per node, ±10% hysteresis)
`appW = nodeFootprintWorldWidth × cameraScale`. Footprint is FIXED while zooming; only PAINTED
content changes inside it. Thresholds are wider for the SPEC node (it's physically wider). Nominal
appW px (normal node / SPEC):
- **W0 MASS** `<160 / <220`: board reads as territories. Plan-region label; plan/spec/task masses;
  coverage strips; status rings; task **volume silhouettes** (§25.5 deck). No body text, but no empty
  cards. Section anchors still present so edges stay continuous.
- **W1 STRUCTURE** `160–340 / 220–440`: SPEC = section title rail + first-line abstracts; PLAN = all
  section rows (title · ready/count · output dot); TASK = handle, status, model, step-thread titles,
  subtask roster chips. Edges SPEC-section → PLAN-row → tasks fully visible.
- **W2 CONTENT** `340–620 / 440–760`: **the body-visible band — the bug fix.** SPEC section bodies
  render `RichMarkdown` in chunked/paged blocks; PLAN rows expand to 2–4-line markdown excerpts
  (NO MORE index-only); TASK body appears clamped, steps become mini-cards, subtasks become
  mini-cards (title + body excerpt). model_rationale is zoom-revealed here, not click-only.
- **W3 DEEP** `>620 / >760`, or dominance (node fills ~60% viewport): full readable content. SPEC full
  section bodies w/ pager+rail; PLAN full section bodies/digests + coverage rationale (node-internal
  scroll); TASK unclamped body + rationale + Tab link + full milestone/step text + subtask bodies +
  subtask step threads. Dominance may elevate/dim but NEVER changes footprint (§25.3).

Per-node: SUBTASKS = deck(W0) → roster chips(W1) → mini-cards(W2) → full cards + own step threads(W3),
bounded interior of the task-world (§20.1, §25.5). MILESTONES/STEPS = constellation(W0) → readable
thread titles(W1) → step cards short text(W2) → full text + iteration/links(W3). No click anywhere.

### 26.3 Markdown treatment
`RichMarkdown` with visible block structure: headings, paragraphs, lists, code/quote containers
separated by spacing/borders — never flattened to a text blob. W2 = density-clamped by a per-block
line budget; W3 = unclamped with node-local scroll. mermaid/code = compact preview chip at W2 →
full/interactive block at W3.

### 26.4 Data-driven task footprint (constraint B — size = work volume, fixed across zoom)
Footprint recomputed ONLY on data mutation, never on zoom (refines §22.1: fixed across ZOOM, varies
by DATA). The reserved bbox IS the deepest-LOD reserve (§25.2). Synthesized formula (codex base,
opus fixed-column width):
```
bodyUnits    = clamp(ceil(bodyChars/240), 0, 10)
stepUnits    = clamp(stepCount, 0, 12)
subtaskUnits = clamp(Σ(1 + ceil(sub.bodyChars/320) + ceil(sub.stepCount/3)), 0, 18)
volume       = bodyUnits*1.0 + stepUnits*1.4 + subtaskUnits*2.0 + (hasRationale?1:0)
height       = clamp(180 + 34*sqrt(volume)*sqrt(1+stepCount/4) + 38*min(subtaskCount,6), 180, 760)
width        = band-column slot (FIXED per band for stable columns); giants (volume in top decile)
               may span 2 columns. (Height carries the magnitude; width stays column-stable.)
```
So heavy tasks are physically BIGGER (mass visible at W0 before text is readable); zoom never resizes
them; overflow beyond the reserved height is handled by node-internal scroll at W3, never by growing.

### 26.5 Packing stability (no portyanka, no overlap, deterministic)
- Band = primary section id (§20.3 banded sweep); stable until coverage data changes.
- Stable order = (planId, sectionBand, createdAt, id) — NOT size, so cards don't reshuffle on edit.
- On data growth: recompute only the changed card's bbox → local band re-pack + monotonic downstream
  push (DOWN only), never a global reshuffle; edges reroute after packing. Same data → same layout.
- PLAN node height is NOT content-driven: fixed body viewport (e.g. ~560×680, bounded by section
  count). W2 = per-row excerpts; W3 = node-internal vertical scroll + section rail + sticky current
  header + 3–5-section page chunks. Wheel over body scrolls node only (§24.2). SPEC owns full prose.

### 26.6 Reconcile with §25 / supersession
KEEP: appW driver (§25.1), reserve-at-deepest (§25.2), BAND⊥DOMINANCE (§25.3), ladder semantics
(§25.4), card-deck subtask cue (§25.5), status emphasis (§25.6), flat-canvas escape (§25.7). GENERALIZE:
L0–L3 → W0–W3 applied to EVERY node type (SPEC + PLAN + TASK), so body text is reachable at every
hierarchy level. §22.2 merged-node = obsolete (§24.3 separate SPEC stands). §22.1 refined as 26.4.

### 26.7 Playwright matrix (extends §25.8)
1. Body visibility by band: seed long SPEC/PLAN/task/subtask/step text; drive camera so each node's
   appW enters W2; assert SPEC body, PLAN row excerpt, TASK body, subtask body, step text all visible
   + non-zero-size; assert `RichMarkdown` emits distinct block/list/code elements.
2. Threshold reachability: reproduce the 173% case; assert PLAN is NOT index-only once appW crosses W2.
3. Footprint fixed across zoom: snapshot SPEC/PLAN/task bboxes at W0–W3; assert Δ ≤ 2px world.
4. Size↔volume: small/medium/large-volume tasks; assert bbox area monotonic with computed volume +
   W0 deck/scale cue on heavy ones.
5. No overlap on data growth: mutate a task (more body/steps/subtasks); settle; assert no bbox
   intersections among SPEC/PLAN/tasks + edges still attached; only changed/downstream bands move.
6. Scroll rule: wheel over node body scrolls node only; off-node wheel pans canvas.

## 27. Orchestrator HANDOFF system (opus+codex converged) — full doc: docs/specs/2026-06-24-orchestrator-handoff-design.md

<!-- revision: 8 -->

Andrey's direction: an orchestrator composes a DIRECTED handoff aimed at the active plan/goal (sharper
than a generic /compact), attached to the plan as provenance, rendered on the canvas, and used to
continue in a FRESH context. Two brainstorms (opus 290a0087, codex 63761d90) converged. Build it; do
NOT trigger a real handoff in the building session (separate testing).

### 27.1 Storage — reuse infra, NO new entity
Handoff = `CanvasElement{kind:"handoff"}` whose `content` holds structured fields (goal bullets,
plan_snapshot, in_flight agents+Tabs+branches+sessions, next_actions [first 3 executable w/o chat
context], foundational_rules, open_decisions, links, resume_prompt, reseed_state, integrity = git
HEAD/dirty + plan updated_at). The large narrative body externalizes to a sidecar `.codeterm/plans/
<plan>/handoffs/<id>.md` (§17 pattern). A linked `Milestone{kind:"handoff"}` puts it in the task/plan
timeline; a `TransferRecord` appends to the existing `Plan.owner_history` audit. Auto-composed by the
daemon from the live plan index (coverage/tasks/branches from `agent report` provenance) + orchestrator
free-text via `--from-file`/stdin. Add `handoff` to `milestone_kinds`.

### 27.2 CLI
`codeterm plan handoff create|show|list <plan_id>` (+ `--latest`, `--markdown`, `--json`, `--from-file|-`,
`--mode record|new|compact`, `--dry-run`); `codeterm orchestrator handoff [--reseed]` = convenience that
resolves caller Tab / owned active plan / active task / session. Capability-shaped daemon RPC →
native+JS parity is structural (`parity_test` asserts identical element + identical reseed send-sequence).

### 27.3 Reseed flow — mode "new" (RECOMMENDED over compact)
1. Daemon composes + attaches the handoff (marker pulses on the board), in ONE plan transaction.
2. Daemon records a `handoff_reseed` pending row keyed `{pane_id, handoff_id, source_session_id}`.
3. Daemon sends the provider fresh-context command to the SAME Tab — `/new` for Claude-like providers,
   via the provider CAPABILITY (same path `agent compact` uses), never a hardcoded string in core.
4. On `SessionStart` with a new session id ≠ source, daemon sends EXACTLY ONE lean seed via
   `codeterm send --tab <self>`: "run `plan handoff show --latest` FIRST, load the orchestrator role
   (docs/contributing/agents.md + AGENTS.md), `plan get`, resume from next_actions. Do not re-handoff."
5. Guaranteed-read = it's the first and only instruction; the fresh agent's handoff-ack report flips
   `reseed_state`. Monotonic states created→new_sent→session_seen→resume_sent→acked; per-tab lock +
   cooldown + `handoff_resume_of` loop-guard; never auto-triggered; `--force` overrides. `--mode compact`
   (seed handoff then /compact) kept as a lossy fallback (spends tokens on stale context).

### 27.4 Canvas render
Handoff "coins" on an orchestrator-region TIMELINE LANE, chronologically linked `[n-1]→[n]` (work →
reset → resumed → reset). Content-LOD W0–W3 (§26): coin → goal → next_actions+agents → full .md body.
Latest-pending pulses; acked dims to ~0.7. Unmissable at overview, reads on zoom, never dominates the
SPEC/PLAN columns. Provenance chips: old tab/session → new tab/session, mode, timestamp. (Rejected:
milestone-on-synthetic-task = buries it; full doc column = too dominant.)

## 28. Dogfood-recording ergonomics — batch write path (opus+codex converged)

<!-- revision: 8 -->

Recording provenance today costs 4–7 HTTP round-trips (task create + assign + body PATCH + N milestone
POSTs + link + focus), each re-loading/writing the plan JSON and forcing manual id juggling. Embed an
ergonomic path into BOTH the CLI AND the app/daemon so the in-app UI and every agent write the same way.

### 28.1 Batch endpoint + `plan record` sugar
`POST /api/plans/:id/batch` = ordered, typed ops with client ids + `$ref` resolution, executed in ONE
daemon transaction with a SINGLE `plan-changed` emit, returning all created/updated ids:
```
[ {op:"upsert_task", client_id:"t", handle:"handoff-cli", name:"…", sections:["cli"]},
  {op:"set_task_body", task:"$t", body_md:"…"},
  {op:"add_milestone", task:"$t", kind:"decision", text:"…"},
  {op:"add_element", kind:"handoff", content:{…}, link:"$t"} ]
```
CLI sugar: `codeterm plan record <plan> --task <id|handle> [--upsert-task <handle> --name … --sections …]
--status … --body-file f.md --step decision:"…" --step testing:@file.md --link <path> --json`. Idempotency
via `--key <stable>` or implicit `{pane_id, command_hash, minute_bucket}`; task upsert by handle; milestone
de-dupe by `(task, kind, key)`. 6 calls → 1; structured provenance preserved.

### 28.2 Everything folds onto the batch path
`agent report` becomes a preset over the batch op; `plan handoff create` is an `add_handoff`/`add_element`
op on the same path. One write primitive for all plan provenance — CLI, in-app UI, and drop-in agents.
Recommendation: ship the batch service first (it's the foundation), then handoff on top of it.

## 29. EFFORT model — Spec ⊥ Plan as separate objects (Andrey decision; opus+codex converged)

<!-- revision: 9 -->

Andrey decided spec and plan are TWO DIFFERENT OBJECTS, not two views of one section set.
Standalone designs: docs/specs/2026-06-24-spec-plan-split-design.md (opus). Sharpened semantics +
final vocabulary in §30. NO legacy/migration code — the dogfood is reshaped MANUALLY (reuse opus's
deterministic recipe as the one-time steps).

### 29.1 Semantics
- **Spec** = WHAT we want (intent/requirements), prose sections. Links ONLY to Plan sections, never to tasks.
- **Plan** = HOW we'll do it (approach/breakdown), sections that bear tasks.
- **Task** = concrete granular work on a Plan section (+granular description); groupable; subtasks.
- **Stride** (renamed from milestone/step, §30) = a work-point: what was done + status delta, NOT a log;
  attaches to Spec, Plan AND Tasks; groupable like tasks (group/phase reused).
- **Effort** = Spec + Plan + Tasks + Strides together = one complete unit of work. Efforts LINK to each
  other (**Lineage**, `--follows`/`--supersedes`) — one Effort flows out of another (survives archive).

### 29.2 Storage + links (codex+opus converged)
- Spec = first-class entity reusing the `Section` type, stored `.codeterm/specs/<id>.json` +
  `specs/<id>/sections/<sid>.md` (§17 externalization, single-writer, watched, INDEX.md). Own dir is what
  makes predate/persist/share-by->1-plan structurally true.
- Links: the dependent (Plan) owns the mapping so a Spec stays write-free when linked — `Plan.spec_refs`
  (coarse object edge) + `Plan.spec_links[{spec_id, spec_section_id, plan_section_id}]` (fine grid),
  mem-indexed by spec_id for reverse lookup. n:m, explicit (NO id-equality magic). Plan needs 0 specs;
  Spec can have 0..n plans.
- Coverage = TWO-HOP: spec_section →(spec_link)→ plan_section →(Task.section_ids)→ tasks, rolled daemon-side.
  NEW state: an UNLINKED spec section = "a requirement nobody planned" (red/dashed) — the headline the
  merged model couldn't show. Plan coverage (plan_section→tasks) unchanged.

### 29.3 CLI + canvas
- New nouns: `codeterm spec create|show|list|section|files|link`, `effort` (bundle ops + `effort … --follows`),
  `stride` (rename of milestone authoring). Spec/link ops fold onto the §28 batch path (`upsert_spec`,
  `upsert_spec_section`, `link_spec_section`, `create_plan_from_spec`); batch target generalizes to {plan|spec}.
  Native + JS parity test per capability.
- Canvas: SPEC node renders the Spec ENTITY (delete the SectionNode plan/spec variant fork); edges
  spec-block ↔ plan-row come from `spec_links` (not same-id); coarse plan↔spec edge joins headers; all
  obey §26 worlds W0–W3. Dangling links (deleted spec/plan/section) → GHOST block (dashed/dimmed, anchor
  kept, never auto-pruned) — same ghost pattern as §27 handoff + plan-lineage.
- Plugin-extensible nodes (separate task): the existing open kind→renderer registry (`registerNodeRenderer`,
  fallback) + `CanvasElement{kind}` are extended so PLUGINS register node kinds/renderers, create external
  nodes (e.g. a GitHub node), and agents read them via `codeterm canvas nodes list|show`.

### 29.4 Build order
Entity+storage → batch spec ops → MANUAL dogfood reshape (one Spec per current plan, 25 sections copied,
plan bodies cleared, 25 spec_links) → canvas un-fork (SPEC node = entity) → parity + Playwright. The
canvas un-fork lands AFTER §26 worlds stabilizes (it reshapes the SPEC node).

## 30. Vocabulary (Andrey's pick — canonical names)

<!-- revision: 9 -->

| Concept | Name | CLI noun | Notes |
|---|---|---|---|
| Complete-work bundle | **Effort** | `effort` | Spec+Plan+Tasks+Strides; links via Lineage |
| WHAT (intent) | **Spec** | `spec` | prose sections; links to Plan sections only |
| HOW (approach) | **Plan** | `plan` | kept; sections bear tasks |
| Granular unit | **Task** | `task` | kept; groupable; subtasks |
| Work-point | **Stride** | `stride` | renamed from milestone/step; on spec/plan/task; groupable; not a log |
| Effort↔Effort link | **Lineage** | `lineage` | `--follows` / `--supersedes`; survives archive |
| Spec↔Plan link | structural | (in batch) | both live in one Effort; `spec_links` grid |

Minimal rename: only NEW words are **Effort** and **Stride**; Spec/Plan/Task kept. "group/phase" is reused
for both task-groups and stride-groups. `milestone_kinds` → `stride_kinds`.

## 31. FOUNDATIONAL UX — live changes NEVER disrupt the camera (Andrey)

<!-- revision: 9 -->

A hard rule, on the level of §22. The board is live (workers add tasks/strides, plans mutate, other Tabs
act). **None of that may EVER move the user's camera, zoom, or focus.** The user's viewport changes ONLY
in response to THIS user's own direct interaction.

### 31.1 The rule
- **Programmatic / remote change → zero camera movement.** A `plan-changed` event, a new task/stride/handoff,
  an agent action, another Tab's activity — the node updates IN PLACE; the camera does NOT pan, zoom, recenter,
  refit, or steal focus. No surprise motion, ever.
- **User-initiated motion is fine** (it's expected): the user clicking a minimap dot, pressing the Plan/"show me"
  button, a `canvas focus` they triggered, double-click drill, zoom/fit controls. The test: did THIS user, in
  THIS session, just act? If not, the camera stays put.
- **Agent/CLI `canvas focus` does NOT auto-move the user's camera.** It raises an off-screen change signal
  (31.2), not a pan — unless the user themselves invoked the focus. (Refines the earlier `canvas focus`
  emit-pan behavior: pan only on local user action; remote/agent focus = signal only.)

### 31.2 Off-screen change signal (instead of moving the camera)
When something changes outside the current viewport, show a subtle, non-stealing indicator: an edge marker
(a dot / soft glow) on the side of the screen toward the change, optionally with a small count. Clicking it
(user-initiated) flies there; ignoring it does nothing. In-viewport changes may use a gentle in-place pulse
(§24.5 / handoff §27.4) — still no camera move. The minimap may tint the region that changed. The signal
informs; it never grabs.

### 31.3 Applies to everything live
Dominance focus-assist (§25.3), handoff markers (§27.4), status emphasis (§25.6), worlds LOD (§26), new
nodes from auto-pack — all update in place. fit-to-view runs ONLY on explicit mount/fit, never on data change.
Auto-pack may re-slot nodes, but the camera transform is untouched. Playwright must assert: after a live data
mutation, the camera transform (pan x/y + zoom) is byte-identical to before.

## 32. WORLDS v2 — fix the live board (opus+codex converged; Andrey live review)

<!-- revision: 10 -->

Andrey reviewed the shipped §26 board and it's wrong: nodes read as empty giants at birds-eye, content goes
transparent on approach, "done = dimmed" was misread as opacity (he meant LOWER HIERARCHY), "size" was misread
as "huge" (he meant bounded growth by content), strides render as squares, text isn't selectable, sections
don't navigate, no go-to affordances, handoff falls back. Two brainstorms (opus 0d781608, codex f4b0f9cc)
converged. This SUPERSEDES the §24.5/§25.6 opacity reading and the §26.4 size feel.

### 32.1 The spine — orthogonalize four channels (never overload one)
- **SPATIAL-Y position = HIERARCHY** (status/importance). in_progress rises, done sinks. NOT opacity.
- **PAINT band (appW W0–W3) = DETAIL** (how much content is painted). §26 keeps.
- **COLOR = IDENTITY** (kind/facet). 
- **OPACITY = TRANSIENT FOCUS ONLY** (hover haze, off-screen ghost, focus-assist neighbor dim). NEVER a status
  or hierarchy signal. Primary content text is ALWAYS opacity 1.0.

### 32.2 Birds-eye W0 is COMPRESSED content, not less content (fixes "empty giants")
W0 ≠ hollow. PLAN/SPEC W0 = title · owner · coverage bar · section count · top ~3 section rows (glyph+count) ·
uncovered/in-progress counts · edge-anchor rail. TASK W0 = name/handle · solid status chip · 2-line digest
(from intent/body) · body/stride/subtask counters · progress mini-bar · latest-stride bead · stride-snake
preview. Fits the fixed footprint via line budgets + overflow-omission order. Because size tracks content
(§32.4), a node is never a big hollow box.

### 32.3 Transparency banned as a signal; done SINKS (fixes #2, #3)
Body/content surfaces stay opacity 1.0 at all bands — remove `chrome.dim` + secondary-opacity on PRIMARY text.
Status/hierarchy = SPATIAL: each band has three stable shelves — **active/top** (in_progress, slight forward z),
**normal/mid** (pending, blocked w/ warning rail), **done/bottom** (done/cancelled, recessed surface, smaller
header emphasis, FULL opacity). Sort key `(shelfRank, status_changed_at bucket, createdAt, id)`; shelf change
only on `status_changed_at` then monotonic local repack (~300ms slot animation), no jitter while typing.

### 32.4 Two-axis bounded, content-driven size (fixes "huge"/"hollow")
Footprint fixed across zoom, recomputed only on data mutation. Grows in BOTH width and height, bounded:
```
textUnits    = sqrt(clamp(bodyChars + intentChars + 0.6*rationaleChars, 0, 3600)/180)
strideUnits  = 1.35*sqrt(clamp(strideCount,0,24))
subtaskUnits = 1.6*sqrt(clamp(subtaskCount,0,16)) + sqrt(childBodyChars/600)
volume       = 1 + textUnits + strideUnits + subtaskUnits + 0.15*linkCount
width  = clamp(260 + 56*floor(min(3, volume/3.2)), 260, 428)  // snap to 1 / 1.5 / 2 col slots; only top-decile → 2col
height = clamp(156 + 38*volume + 16*min(strideCount,8) + 22*min(subtaskCount,5), 176, 680)
```
**aspectFloor = 0.62·width** (opus) — height never below it, so cards can't be hollow letterboxes. W3 overflow =
node-local scroll. Heavy work = visibly bigger MASS at W0; deterministic packing (snapped slots).

### 32.5 Strides = serpentine snake of circles (fixes #5, #6) — replaces square step blocks
Inside a task, reserve a rail; draw ONE continuous 2px path through kind-colored circular beads (Thread & Coil).
Wrap as a **boustrophedon snake** (semicircular elbow turns, not separate lines); loops coil. Freshest bead =
head (pulses); done strides coil to the tail. Per band: W0 = 7px dots (cap ~10 +N) on a compact line; W1 = 12px
beads + kind ring + 1-line labels alternating above/below; W2 = 18px beads, near bead sprouts a callout on the
line; W3 = beads bloom into full stride cards, path stays visible as provenance spine. **Each stride is a leaf
MICRO-WORLD** with its own appW bands (s0 bead → s1 title → s2 body excerpt+links → s3 structured body +
links + related task/spec chips + author/tab/session/timestamp). Zoom reveals structure; click only for
explicit actions (copy-link, focus, open source). Bloom space is reserved in the task bbox (no reflow).

### 32.6 Selectable text (fixes #7) — CSS pointer/select split, no overlay
Root cause: `body{user-select:none}` + container `userSelect:none`. Fix: text islands get
`.ct-canvas-text { user-select:text; cursor:text; pointer-events:auto }` + `data-canvas-selectable`. Pan
`onMouseDown` skips starting a drag when `target.closest('[data-canvas-selectable]')` AND a selection is
forming (drag only past threshold with no selection). Buttons `stopPropagation`. Empty padding/header chrome
stays pan. Overlay approach rejected (only a fallback if global/xterm listeners steal selection).

### 32.7 Navigation + affordances (fixes #8, #9) — all §31-compliant (user-initiated only)
Plan/spec section rows are interactive: single-click a PLAN section → highlight its outgoing task edges +
connected task nodes; double-click / row action → frame those tasks (fly+blink). SPEC section click → highlight
spec→plan mapping (via spec_links) then tasks. Empty section click → frame + open composer with that section
context. At W2/W3, rows + task cards show icon buttons: go-to-tasks, go-to-spec, open-tab, copy-id/copy-link,
focus-stride. ALL camera motion is user-click / toolbar / minimap only; agent/CLI `canvas focus` = off-screen
signal (§31), never auto-fit.

### 32.8 Handoff render (fixes #10) — register kind=handoff
Replace the fallback with a real renderer: an **orchestrator-region timeline lane** (above/left of owned plan
frames, never inside task clutter). Each handoff = a reset coin on a chronological spine `[n-1]→[n]`; W0 coin →
W1 goal+timestamp → W2 next_actions + in-flight agents + old→new session chips → W3 expanded markdown body + ack
state. Latest/pending pulses; acked recedes spatially (smaller/behind), NOT content-faded. Creation never moves
the camera; an off-screen marker points to the lane. (Checkpoint `el_e8ab8c97` exists to render against.)

### 32.9 Verification
Playwright: W0 nodes show digest+counters+snake (not hollow); content opacity == 1.0 at all bands; done sits
below in_progress in its band (position assert, not opacity); card area tracks volume with aspectFloor honored;
strides render as a connected snake of circles, bloom on zoom; canvas text is selectable + copyable; section
click highlights/ frames the right tasks; handoff renders on the timeline lane; camera transform byte-identical
after any live data mutation.

### 32.10 Task CLOUD to the right of the plan (fixes the task портянka) + smaller tasks

<!-- revision: 10 -->

Andrey live review: tasks stack into a kilometer-tall single COLUMN again (a портянka). Because we work
in WORLDS (zoom-drill into a task for detail), tasks DON'T need to be big up front.

- **Tasks are SMALLER than SPEC/PLAN nodes** — base footprint ≈ ½ the section-node size (at least 2× smaller),
  bounded growth per §32.4 still applies (a heavy task is bigger, but the BASE is small). Detail comes from
  zoom-drill (§26/§32.5), not from a large default card. So birds-eye shows many small task tiles, readable
  as a constellation, not a wall of big boxes.
- **Layout = a CLOUD to the RIGHT of the PLAN node**, not a tall single column. Tasks pack as a 2D cluster
  (multi-column flow filling a bounded region right of the plan), edges from plan section OUTPUT dots into the
  cloud. The cloud's VERTICAL axis encodes priority/status (composes with §32.3): **top of the cloud = highest
  priority + in_progress/active; bottom = done/cancelled** (done sinks to the bottom of the cloud). So the eye
  lands on live high-priority work at the top-right, history settles at the bottom.
- Deterministic + stable: cloud packing keyed by (statusShelf, priority, status_changed_at, createdAt, id);
  a task changing status animates to its new vertical slot (~300ms), no global reshuffle, no jitter. Bounded
  region height; overflow flows into additional columns rightward, never an infinite vertical strip.
- Composes with §20 worlds/banded layout (this IS the banded cloud done right), §32.4 size, §32.3 hierarchy,
  §31 (camera never moves on live repack).

## 33. Mergeable git-native storage (opus+codex converged research)

<!-- revision: 11 -->

Andrey's question: sqlite is binary → can't git-merge; how do mergeable plan/memory systems work; can
this travel with git? Two research agents converged. This resolves central-project (§ git-common-dir)
+ git-versioning + merge in one architecture. RESEARCH verdict — decide before building.

### 33.1 Verdict — hybrid file-per-entity + append-only event log; sqlite ALWAYS derived
The ONLY non-mergeable artifact today is the monolithic `.codeterm/plans/<id>.json` (its `tasks[]` with
embedded stride arrays = one big conflict surface). The bones already exist: §17 externalizes section/task
BODIES to `.md` (file-per-entity), §21 makes the sqlite index DERIVED. **Finish the decomposition** and the
data becomes git-mergeable with ZERO custom machinery.

### 33.2 On-disk layout
- `.codeterm/plans/<id>/meta.yaml` — slim SCALAR metadata only (title, status, owner, section order, refs,
  facets, links pointers). No arrays of content.
- `.codeterm/plans/<id>/sections/<handle>/section.md` — YAML frontmatter (id, title, handle, tags, order,
  canvas) + Markdown body. (extends §17)
- `.codeterm/plans/<id>/tasks/<handle>/task.md` — frontmatter (id, status, priority, model, section_ids,
  parent, group, assigned Tab) + body.
- `.codeterm/plans/<id>/tasks/<handle>/strides.jsonl` — **APPEND-ONLY**, one ULID-keyed event per line
  (stride / status-change / assignment / edit). (codex variant: `events/<ulid>-<actor>.json` one-file-per-event.)
- `specs/<id>/…` same pattern; `links/` + `canvas/` one-file-per-entity; `INDEX.md` generated.
- `plan-index.sqlite`, vectors, FTS, layout caches — DERIVED, gitignored, never committed.

### 33.3 Merge mechanism — git's BUILTINS, no custom driver
- Entity/body files: git DEFAULT 3-way per-file merge. Two branches adding different tasks = two different
  files = auto-merge, no conflict. Editing the SAME body on two branches = a normal TEXT conflict — which is
  the correct human-visible conflict (resolve in a PR), NOT a silent overwrite.
- Append-only logs (`*.jsonl`): git BUILT-IN `merge=union` via `.gitattributes` — concatenates both sides.
  Immutable ULID events → concurrent strides across worktrees/branches NEVER conflict. Fold to current state =
  parse → dedup by id → sort by (ts, id). **NO custom merge driver, NO CRDT, NO Dolt.**
- Scalar field conflicts (status/priority on the same task from two branches): model as explicit events with a
  Lamport tuple (logical_clock, actor_id, event_id) → deterministic LWW ONLY for genuinely scalar fields;
  body/text edits stay git conflicts (never silent LWW).

### 33.4 Concurrency, vectors, mem, migration
- Single-writer per-worktree daemon (existing serialized writer, now cheaper — one entity file + append).
  GIT is the cross-worktree/branch merge transport; no cross-process lock. On merge/pull/rebase the watcher
  folds + reindexes; derived indexes are discarded/rebuilt (sub-second at plan scale via content-hash skip).
- Vectors/sqlite ALWAYS derived (binary, model-version-dependent, cheap to regen from source_path/mtime).
- Mem: workspace facts = one `.md` per fact under `.codeterm/mem/<area>/` (travels with git); global / identity
  / peer facts stay in the home sqlite, never git.
- Migration: idempotent deterministic "explode" on load (no Date::now/random; mirrors §17.3 migrated-flag).

### 33.5 Rejected + build order
REJECTED: CRDT/Automerge (binary, no git-diff/PR-review, QuickJS parity break, redundant given append-only);
Dolt (no Go runtime, data lives outside project git so won't travel with branches, no PR diffs); pure
event-sourcing for everything (wrong for big prose bodies — hence the hybrid). Build order: explode migration →
per-entity write path → strides.jsonl + merge=union → post-merge reindex → mem workspace-fact files. SPIKE
FIRST: verify git's built-in `merge=union` behaves on EOF-append hunks before choosing jsonl over
one-file-per-event. Composes with §32-central-project (git-common-dir) — together: one central, git-mergeable,
branch-travelling plan/memory store.

### 33.6 Compaction + scale (Andrey: don't let logs bloat; must hold ~10k tasks)
- **Log compaction** (append-only must not grow unbounded): `task.md` frontmatter IS the materialized snapshot,
  so the `strides.jsonl` can be folded + truncated to a recent window. Mechanism = a deterministic MAINTENANCE
  op (like `git gc`): fold old events into the snapshot, keep the last N strides (+ a total count), archive the
  rest. Run it on the CANONICAL branch at a controlled checkpoint, NEVER ad-hoc per-worktree (compaction is a
  non-append rewrite → would break `merge=union` if two branches compact differently). Deterministic fold =
  idempotent. Simpler fallback: keep the full log but the daemon only LOADS/renders the last N + count (file
  compacted offline, live view never reads the whole log).
- **Scale target ~10k tasks + ~10k embeddings** (stored, not concurrent): holds. Git handles 10k+ small files;
  the daemon loads PER-PLAN lazily (never all 10k at once); the canvas renders via worlds-LOD + per-plan scope
  (never 10k cards). sqlite-vec kNN over 10k–100k vectors = sub-ms–ms; FTS+RRF trivial; derived-index rebuild
  of 10k units = seconds, incremental via content-hash skip. Bottleneck is UI render (already bounded), not
  storage. VALIDATE with a benchmark: seed ~10k tasks/strides across N plans → assert search latency, reindex
  time, canvas fit-all + per-plan load times stay bounded.

## 34. GOAL construct + orchestration postulates + handoff VERIFY-LOOP (Andrey direction; design)

<!-- revision: 1 -->

Andrey's direction: a plan needs a **GOAL** = the north-star VECTOR + general info, ALWAYS visible,
PERIODICALLY re-injected to the orchestrator, NEVER decomposed into tasks. Today it is MISSING
(verified: `Plan` has no `goal` field). Orchestration **POSTULATES** (heartbeat, re-read plan usage,
always review, real visual testing, group-and-drive, periodic audit, single build-gate, provider-
balance) are standing axioms that hang off the GOAL. And a handoff **VERIFY-LOOP**: compose → read
back → verify 3× vs the spec (nothing forgotten) → only then compact → fold into the skill.
Composes with §27 (handoff), §28 (batch), §29/§30 (Effort/Spec vocab), §31 (no camera move),
§32/§35 (worlds). DESIGN ONLY.

### 34.1 GOAL data model — first-class plan meta, never work
`Plan.goal: Option<Goal>` (one per plan/effort, the north-star — not a Vec, not a section, not a task):
```
Goal {
  vector: String,            // ONE-LINE north-star headline (the direction the whole effort serves)
  vision: String,            // markdown prose: the WHAT/WHY general info (externalized body)
  constraints: Vec<String>,  // key HARD constraints (e.g. "fixed footprint", "no camera move on live change")
  postulates: Vec<Postulate>,// standing orchestration axioms ACTIVE for this plan (§34.4)
  updated_at: u64,
}
Postulate { id: String, text: String, rationale: Option<String> }   // id mirrors the skill axiom id
```
- **How it differs from sections/tasks:** GOAL is the north-star, NOT work. It is **never decomposed**,
  bears **no tasks/strides**, has **no coverage**, is **never archived independently** (it rides the
  plan/effort). Sections answer WHAT/HOW and bear tasks; the GOAL answers WHY-the-whole-thing-exists and
  bears nothing. It is plan META, peer to `title`/`owner`, not a node in the section tree.
- **Effort-level (§29):** GOAL belongs to the **Effort**, surfaced through its Plan. Lineage (`--follows`/
  `--supersedes`) carries the vector forward — a successor effort inherits/refines the predecessor's goal.

**Fork — where it lives:**
- **Alt A — inline `Plan.goal` field + externalized `goal.md` body (PICK).** Scalars (vector, constraints,
  postulate ids+text) in `meta.yaml`; long `vision` prose in `.codeterm/plans/<id>/goal.md` (frontmatter +
  body, §17/§33 — hand-editable, watched, git-mergeable). 1:1 with the plan, tiny, ALWAYS loaded with the
  plan → cheap to inject everywhere. Reuses all plan machinery, zero new entity.
- Alt B — separate `Goal` entity (own dir, like Spec). Rejected: goal is 1:1 with the effort and trivially
  small; a full entity buys predate/share which the goal never needs.
- Alt C — `CanvasElement{kind:"goal"}`. Rejected: elements are canvas decorations; goal must be queryable
  plan meta that `plan get`/heartbeat read without walking elements.

### 34.2 ALWAYS-VISIBLE on canvas — a pinned screen-space GOAL HUD (not a world node)
"Always visible at every zoom" is structurally incompatible with a world node (§26): world nodes live in
world-space and pan/scroll away. So the GOAL is **screen-space HUD chrome**, pinned to the viewport like the
breadcrumb (§20.5) and minimap (§23.4) — it never scrolls, never auto-packs, never participates in LOD.
- **Collapsed (default):** a thin pinned bar at the top of the canvas (~40px), accent-tinted by the plan,
  showing the **vector** (1 line, prominent, ellipsis) + a `◆ postulates` pill + a `⌄` expander. Prominent,
  not dominating — one bar, not a panel.
- **Expanded (user click):** drops a panel with full `vision` markdown + `constraints` list + the active
  `postulates` (id · text · rationale on hover). Click again / Esc collapses. Pure user-initiated (§31).
- **Multi-plan:** the HUD shows the **focused/dominant plan's** goal (breadcrumb-driven); workspace overview
  with many plans → the bar shows the workspace name + a `▾ N goals` switcher, no single vector forced.
- **Readability:** because it is screen-space it is legible at ANY camera zoom by construction (§22/§31 — it
  is not a node, so "fixed footprint / no camera move" apply trivially). Optional faint world-space "goal
  monument" above the plan node for spatial grounding at far zoom is a nice-to-have, NOT the load-bearing
  surface — the HUD is.
- **Fork:** Alt A world node (scrolls away — rejected), **Alt B screen-space HUD (PICK)**, Alt C hybrid
  (HUD + monument) — adopt B now, C's monument later if the board feels ungrounded.

### 34.3 PERIODIC REINJECTION — heartbeat is the carrier, plan-get is the pull
The orchestrator must never lose the north-star. Three channels, the heartbeat being the periodic one:
1. **Heartbeat (periodic, the key one):** the plan-owned orchestrator heartbeat is **goal-anchored** and
   auto-injected by daemon nudge/delivery paths. New sugar `codeterm plan goal heartbeat <plan>` composes the
   standard reminder text — `GOAL: <vector>\nPOSTULATES: <id list>\nNow: reports first, then …` — for that
   plan-owned heartbeat. Delayed self-sends are optional backup state, not the current primary mechanism.
2. **On every `plan get` (pull / always-present):** `plan get` emits a **GOAL block at the very top** of the
   assembled markdown (vector + constraints + active postulates, vision link). Missing today — add it.
3. **On reseed (§27.3 step 4):** the lean fresh-context seed gains "load the goal: `plan goal show`" so a
   reseeded orchestrator picks up the north-star FIRST, before plan get / next_actions.
- **Explicit pull:** `codeterm plan goal show <plan>` prints vector · constraints · postulates · vision.
- **Fork — push vs pull:** Alt A orchestrator re-reads manually (weak — it IS the forgetting problem we are
  fixing). **Alt B heartbeat-carried + always-on-plan-get (PICK)** — reinjection rides infra that already
  fires periodically; no new timer, no context-grabbing daemon push. Alt C daemon timer-push into the Tab
  (rejected: interrupts mid-turn, the §31 "never grab" spirit applied to context, not just camera).

### 34.4 ORCHESTRATION POSTULATES — declared in the skill, pinned per-plan in the goal
The standing axioms (Andrey's list), each an id the heartbeat/HUD cite:
| id | axiom |
|---|---|
| `heartbeat` | keep the plan-owned heartbeat current so daemon reinjection carries focus and active lanes |
| `re-read-plan-usage` | periodically re-read HOW to use the plan system (don't drift from the tooling) |
| `always-review` | every worker report is evidence, not proof — review the diff/source yourself |
| `real-visual-test` | verify via TestFlight / real visual (Playwright screenshot) testing, never unit-only for UI |
| `group-and-drive` | group related work and drive lanes as bands, not ad-hoc one-offs |
| `plan-audit` | run a periodic plan audit (coverage, stalled tasks, stranded branches) |
| `single-build-gate` | one canonical build-gate before claiming readiness, not N divergent local builds |
| `provider-balance` | balance work across providers/accounts (anti-ban, §project_cron_antidetect) |
| `handoff-verify-before-compact` | never compact/reseed without the §34.5 verify-loop passing |

**Two-tier (mirrors §19 facets/topics governance):**
- **Canonical home = the skill.** `.agents/skills/codeterm-orchestrator/SKILL.md` holds the full doctrine
  (axiom text + rationale + procedure). Single source of truth, version-controlled, loaded every orchestrator
  session. (Add a "Postulates" section — currently the skill has Heartbeat/Verification/Review as prose but
  no enumerated, id'd axiom set.)
- **Per-plan pin = `Goal.postulates`.** A plan REFERENCES the skill axioms by id and may extend/override with
  plan-specific ones. The goal pins the ACTIVE set; the HUD (§34.2) and heartbeat (§34.3) surface exactly that
  set. Skill = library; goal = the checked-out, plan-active subset.
- **Kept top-of-mind by:** (a) goal HUD expander, (b) every heartbeat (by id), (c) `plan get` goal block,
  (d) skill loaded each session, (e) reseed seed. Five reinforcing surfaces, one source.

### 34.5 HANDOFF VERIFY-LOOP — compose → read-back → 3× verify vs spec → compact → fold
Layers on §27 handoff. The destructive context reset (§27.3 reseed) is GATED behind a completeness proof.
**Procedure:**
1. **COMPOSE** — daemon auto-composes the handoff from the live plan index (§27.1) + orchestrator free-text:
   `codeterm plan handoff create <plan> --verify` (folds onto the §28 batch `add_handoff` op).
2. **READ BACK** — `codeterm plan handoff show --latest --markdown`. The orchestrator (or verifier) reads the
   composed handoff as it will actually be consumed.
3. **VERIFY 3× vs the SPEC** — three passes against the GOAL+SPEC+live plan, each scoring a fixed checklist;
   gaps amend the handoff and re-verify; only **3 consecutive clean** passes proceed. What each pass checks
   ("nothing forgotten"):
   - **GOAL** present — vector + active postulates carried into the handoff.
   - **STATE** complete — coverage, what's done, plan snapshot, integrity (git HEAD/dirty + plan `updated_at`).
   - **IN-FLIGHT** complete — every active agent/tab/branch/session listed; cross-checked vs
     `task list --active` + `agent reports` (a worker present there but absent from the handoff = FAIL).
   - **NEXT-ACTIONS** executable — first 3 runnable with zero chat context.
   - **RULES** present — foundational rules (§22/§31) + the active postulates (§34.4).
   - **NO SPEC ITEM ORPHANED** — nothing in-flight against the spec/goal that the handoff omits.
   3× = three independent passes; best as three **distinct lenses** (completeness / accuracy / executability),
   the adversarial-verify pattern (default to "incomplete" when unsure).
4. **COMPACT / RESEED** — ONLY when context is large AND the verify gate is green: run §27.3 reseed
   (mode `new` preferred over lossy `compact`). `--gate` makes reseed refuse until clean.
5. **FOLD INTO THE SKILL** — durable run-learnings (a corrected procedure, a new postulate) fold into
   `.agents/skills/codeterm-orchestrator/SKILL.md` — but per §35.7, **only after a live pass proves the change
   worked** (review-before-trust applied to the skill itself).

**Safety — scratch verifier, not the live orchestrator (the load-bearing decision):**
- **Fork — who runs the 3× verify?** Alt A live orchestrator self-verifies (cheap but biased, and burns the
  very context it is trying to preserve). **Alt B scratch verifier sub-agent in a SEPARATE Tab (PICK)** —
  spawned worker reads the handoff `.md` + spec + `plan get`, returns a structured gap report; independent
  (skill Review Discipline: "evidence, not proof") and leaves the live orchestrator's context untouched.
  Alt C daemon structural-only check (fast, but can't judge semantic "nothing forgotten").
- **Recommendation: B for the 3× semantic passes + C as an automatic structural precheck.** The daemon's
  `--verify` does the cheap structural gate (required fields present; in-flight set == `task list --active`;
  integrity stamp matches) FIRST; then the scratch verifier does the 3× semantic completeness pass. Compact
  only after BOTH are green. The live orchestrator composes and decides; it does not grade its own handoff.
- **CLI:** `codeterm plan handoff verify <plan> [--passes 3] [--against <spec_id>] [--SCRATCH-tab] [--gate]`
  → composes-or-takes-latest, runs structural precheck, drives the scratch verifier, returns the gap report;
  `--gate` blocks reseed until 3 clean passes.

### 34.6 What's MISSING to execute the GOAL (concrete gaps)
- **Data:** `Plan.goal: Option<Goal>` + `Goal`/`Postulate` structs in `plan/types.rs` (absent today — verified).
- **Storage:** goal scalars in `meta.yaml`; `vision` body externalized to `.codeterm/plans/<id>/goal.md`
  (frontmatter + body, watched/mergeable, §17/§33). INDEX.md lists it.
- **CLI:** `plan goal set` (vector/constraints/postulate ids), `plan goal show`, `plan goal edit` (opens the
  `.md`), `plan goal heartbeat <plan>` (compose reminder text), `plan handoff verify <plan>` (the verify-loop).
- **`plan get`:** prepend a GOAL block (vector + constraints + active postulates) — missing.
- **Heartbeat:** the orchestrator-skill heartbeat template is generic — make the plan-owned heartbeat
  goal-anchored (carry vector + postulate ids) and refresh it after each reconciliation cycle.
- **Canvas:** pinned screen-space GOAL HUD banner (new component; NOT a world node; §34.2). §31-compliant.
- **Batch (§28):** `set_goal` / `set_goal_body` ops so goal writes use the one write primitive (CLI + UI + drop-ins).
- **Reseed (§27.3 step 4):** add "load the goal (`plan goal show`)" to the lean seed.
- **Skill:** `codeterm-orchestrator/SKILL.md` needs (a) an enumerated **Postulates** section (§34.4 ids), (b) a
  **Handoff Verify-Loop** section (§34.5 procedure + scratch-verifier safety), (c) the fold-after-live-pass rule.
- **Parity:** every new capability (goal RPC, handoff verify) gets a native+JS `parity_test` (§architecture #2).

### 34.7 Recommendation summary
GOAL = inline `Plan.goal` field + externalized `goal.md` (§34.1 Alt A). Always-visible = pinned screen-space
HUD banner, not a world node (§34.2 Alt B). Reinjection = goal-anchored heartbeat + GOAL block on every
`plan get` + reseed seed (§34.3 Alt B). Postulates = enumerated in the skill (canonical) + pinned per-plan in
`Goal.postulates` (§34.4). Verify-loop = daemon structural precheck + independent SCRATCH-tab 3× semantic
verify, gating reseed; fold into the skill only after a live pass (§34.5 Alt B+C). All §31-compliant; all new
capability native+JS parity-tested.

<!-- revision: 2 -->

## 35. WORLDS v3 — plan-centric composition (Andrey live review; brainstorm in flight)
<!-- revision: 1 -->
> Status: DIRECTION captured; design synthesis pending (brainstorm opus cbeec826 + codex 9309d30a).
> Loop/pass on §26/§32 worlds. Plan task `worlds-v3` (task_99dfa6cd), loop-from worlds-v2.

1. **Plan-centric composition.** The PLAN's own world is the central, LARGEST object — bigger than all
   else, occupying the center. LEFT of the plan = the SPEC it was built from. RIGHT = the TASKS. Size
   hierarchy: plan > spec ≈ tasks; a task can NEVER exceed plan/spec size ("they're just tasks").
2. **Task cloud = circle pack, NOT a line.** Tasks today form a line (was vertical, now horizontal —
   both wrong). Want cubic task blocks INSCRIBED in a CIRCLE, non-overlapping, a round cloud right of
   the plan. Needs a concrete square-into-disk packing algorithm (phyllotaxis / circle-pack relaxation /
   concentric rings), stable, no jitter on add/remove (§31).
3. **Task zoom-in, STABLE layout.** Fixed slots that never reflow: top=TITLE, center=DESCRIPTION,
   bottom=WORK-POINTS row. All three visible even far; only CONTENT detail changes per LOD band (far:
   title+tags; closer: short desc; closest: full markdown, scroll only at the closest band). Blocks stay
   put — content swaps inside them.
4. **Work-points (strides).** Circles confirmed right. Zoom into one → tags → text → full internal
   structure. FIX: text renders AROUND the point, must be INSCRIBED inside (scale issue). Deepest band =
   the work-point fills the screen for comfortable reading. No flying — fixed slot at every zoom.
5. **Sections show FULL content, not truncated.** Progressive reveal like tasks: far=title+tags;
   closer=body; closest=rendered markdown paragraphs.
6. **Spec vs plan correctness.** Plan currently REPEATS spec titles and is UNDER-FILLED (sections have no
   names — confirmed live). Plan must be filled DISTINCT from + more detailed than the spec (plan=HOW/
   tasks/strides, spec=WHAT). Fill the plan-system from the orchestrator's working markdown.
7. **Emergence / loops.** The whole planning feature supports PASSES/LOOPS: every object (spec/plan/
   section/task/stride) is re-enterable and loopable; plan execution itself loops. Data model + canvas
   must express "pass N / loop" on any object (lineage/iteration count, re-open semantics, visual
   signal). Ties to `codeterm plan loop`. Same review-before-trust paradigm applies to the orchestrator
   skill: only fold a change into the skill once a live pass proves it worked.

## 36. Agent PRESENCE — live "who is working on what" (Andrey direction)
<!-- revision: 2 -->
> Status: DIRECTION captured. Plan task `agent-presence` (p1) — DB creation currently eaten by the
> live lost-update bug (§36 is partly motivated BY that bug being visible). Durable record here.

Andrey: I must ALWAYS see, in the canvas UX, what the **orchestrator** is doing AND which **agent**
is working on which **task right now**. Every agent (orchestrator included) continuously surfaces its
current task + current work-point, and reminders enforce it so nobody silently drifts.

### 36.1 Model — reuse existing primitives, derive presence
- **Claim = take.** Working on a task CLAIMS it (`task take` / `plan task assign`) — ownership explicit,
  transferable ("you take it from whoever held it").
- **Work-point authorship = the activity record.** Each work-point (stride) stamps WHO + WHAT. The
  task's current worker = author of the **latest** work-point. "Who is on this task now" is *derived*,
  not separately tracked. Verify every `plan record` stamps the author (tab/agent id).
- **Ownership history = the work-point author chain.** The ordered authors over time ARE the task's
  hand-off / transition history, viewable later. Reuse §27 `owner_history` + strides.
- **Current-focus.** Each agent publishes a one-line "current focus" (task id + what); the orchestrator
  too. Either a field or derived from the last work-point.

### 36.2 Reminders — the enforcement (multi-trigger, not one)
Everyone is nudged to keep "what I'm working on" current via SEVERAL triggers:
- **Heartbeat / time-based** — the plan-owned heartbeat/nudge cadence carries "state your current task"
  and composes with §34 goal-anchored heartbeat. Delayed messages are a backup carrier, not the primary
  state mechanism.
- **CLI-use-triggered** — when an agent calls the CLI, a guard-rail hint reminds it to record a
  work-point / refresh focus if stale (extends the cli-hints guard-rail already shipped).
- **Message-count / cadence-based** — after N turns without a work-point, prompt to report.

### 36.3 Canvas UX
- Live per-agent activity ON the task: the task an agent owns shows that agent (color/name/avatar) +
  its latest work-point pulsing. The orchestrator's own current focus is visible too (ties to the §34
  GOAL HUD — a presence strip beside it).
- "Who's on what" is glanceable: scan the board → every active agent on its task. §31-compliant
  (presence updates pulse / off-screen-signal, never move the camera).

### 36.4 Gaps to build
Author stamping on every `plan record`; a current-focus field or derivation + CLI (`agent focus
set/show` or fold into `plan record`); reminder hooks (heartbeat line, cli-hint on stale focus,
cadence trigger); canvas presence render; ownership-history view (`task history` over work-point
authors). Loops (§35.7) compose: a re-opened task shows the new pass's worker taking it.

### 36.5 Meta — why now
The orchestrator itself was NOT surfacing its current task, so Andrey couldn't see what it was doing.
This makes every agent's current work visible *by construction* + reminded, so the whole swarm is
legible. (Note: filing this very task repeatedly hit the live lost-update/lock bug — presence depends
on reliable plan writes landing first → the plan-concurrency deadlock must be fixed first.)

## 37. BOXINESS + worlds-granularity NAVIGATION (Andrey direction)
<!-- revision: 1 -->

### 37.1 Boxiness — fixed slot-boxes that never shift
Every card (task, section/spec, plan) is composed of FIXED inner BOXES — a box for the title, a box
for the description, a box for results/strides, etc. The boxes DO NOT move when content or sizes
change inside them. Zooming changes the CONTENT *inside* a box (more detail, more text, swapped
representation) but the box layout — and the card's outer footprint (§22.1) — stays put. "Everything
together does not slide." The current bug (card resizes/shifts on zoom) is exactly this being
violated; see §35.3 (3 fixed task slots) + card-footprint task. Fixed boxes are the precondition for
the immersion model below — you dive INTO a box, not into a reflowing blob.

### 37.2 Worlds granularity — recursive zoom regulates what you see
Zoom IS the granularity dial. Diving in/in/in reveals progressively more; pulling out shows less but
broader. The bands, top → deep:
- **Top view (W0):** on the PLAN you see only what's IN PROGRESS — current tasks, sorted. A glanceable
  "what's being worked on right now".
- **Closer:** plan SECTIONS become legible; inside each section you see HOW MANY tasks are thrown on
  this plan, as task BUBBLES *inside the plan* ("this many tasks on this plan"). Spec prose lives in
  the SPEC node, not the plan (§29/§36 split).
- **Into a task:** click/zoom a bubble → fly to that task → its boxes (title/desc/work-points) reveal
  per §35.3/§37.1. From the task you fly back to the plan.
- **Cross-links:** on any plan SECTION you see WHICH SPEC SECTION it belongs to, and can fly there;
  from a spec section you can fly into the plan's section. Section↔task and section↔spec are the
  navigable edges (all camera moves user-initiated, §31).

### 37.3 This must survive memory loss
All of the above (boxiness, granularity nav, the §38 acceptance model below) lives in: the orchestrator
SKILL, the heartbeat/delay message, the plan tasks, and CLI hints — so a fresh orchestrator (or a
brand-new worker who knows nothing) can recall it from zero. Verify the rendering matches THIS spec
via Playwright at each pass; judge each task against the plan's GOAL/vector before accepting `done`.

## 38. Acceptance model — orchestrator decides `done` (in_review → done)
<!-- revision: 1 -->
A worker reports `in_review`, never `done`. The ORCHESTRATOR verifies (gate tests + judge against the
GOAL/vector + spec, optionally via a fast branch-local Playwright verifier e.g. a `cursor` worker) and
DECIDES `done`. Match verifier cost to the claim; don't take the worker's word; don't always need a
heavy agent. `in_review` → (orchestrator verification) → `done` or bounce (loop/pass). In the live
orchestrator SKILL (Acceptance section) + worker preamble (workers report in_review).

## 39. Canvas IMPROVEMENT LOOP (Andrey: the mechanism that polishes the board to "красиво")
<!-- revision: 1 -->
The canvas is iterated to quality through a LOOP, not one-off fixes. Each iteration:
1. **Render + capture** the live board across the FULL zoom range (fit → 1000% / maxScale 10) at many
   steps — screenshots at each band boundary, not 3 arbitrary scales.
2. **Verify with GOOD tests + a frontend-designer eye.** Run the Playwright criterion
   (`@canvas-worlds-v3`) AND a `claude frontend designer`-style visual assessment of the screenshots
   against the design vector (§22.1 fixed footprint, §26/§35 worlds-dive, §37 boxiness, legible titles).
3. **Enumerate concrete defects** the pass found (bad titles, node reflow on zoom, no deep-dive detail,
   text garbling, etc.).
4. **Each defect → a CRITERIA-bearing task** (§ acceptance_criteria + test_plan) → fix → re-verify.
5. **Repeat ~15–20×** until the criteria pass AND the visual matches the vector.

**HARD DEPENDENCY — the tests must be GOOD or the loop is blind.** A criterion that samples WITHIN a
single LOD band false-greens while the node visibly reflows across band boundaries (this happened: a
within-band footprint test passed while the sectioned node grew on zoom). Good criteria: span the LOD
band transitions (W0→W3), span the full zoom range (fit→1000%), check title legibility, assert
no-reflow, and the frontend-designer eye covers taste/vector-alignment tests can't express.

**The worlds VECTOR (the dive):** zooming deeper must reveal progressively MORE — title → tags →
body → strides → into a stride. Deep zoom (to 1000%) is the granularity dial (§37.2). A node that
just gets bigger with no new detail (or that reflows/garbles) fails the vector. This is the loop's
north-star: zoom in and in and keep finding more, with the frame fixed and content adapting.

<!-- revision: 2 -->
