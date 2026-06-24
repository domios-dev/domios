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
