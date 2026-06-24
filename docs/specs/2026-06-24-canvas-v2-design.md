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
  subtask count. Document: full body·rationale·intent·step cards·subtask cluster (right)·pane link.
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
