# Canvas Redesign Brief — "Figma for plan review"

<!-- revision: 1 -->

**Goal:** the Canvas must look and feel like Figma, but purpose-built for reviewing
agent orchestration plans. The current rendering is broken: sections scatter as
loose cards, tasks float disconnected, milestones fly off to the right with **zero
edges**. Throw that layout model out and rebuild it around the model below.

This is a **frontend-design** task. Invoke the `frontend-design` skill before
writing any UI and follow it. Quality bar: it must look polished and be genuinely
usable for reviewing a plan at a glance.

## The target picture (what the user asked for, verbatim intent)

1. **One plan node, divided by sections.** The main plan document is a single,
   compact node — NOT a giant frame that contains everything. Inside it: a header
   (plan title, owner, `done/total tasks`, coverage % + bar) and a vertical list of
   **section rows**. Each section row shows the section title, a task-count badge
   (how many tasks cover it) and a covered/uncovered state. **Uncovered sections
   (no task points at them) must be visually obvious** (warning tint / dashed) —
   this is the whole point of coverage: a section with no task = we skipped it.
   Subsections are indented rows. Show overall coverage (%) and number of completed
   tasks on this node.

2. **Tasks are separate nodes connected by EDGES.** Each task is its own card,
   positioned around the plan node, and connected by a smooth edge **to the precise
   section row(s) it covers** (`task.section_ids`) — not to the plan node's center.
   The edge must anchor at the right edge of that specific section row. A task that
   covers two sections gets two edges (many-to-many).

3. **Spec node.** A separate, larger node renders the **full specification the plan
   was built from** — the prose. In our model the spec lives in the section bodies:
   render the plan as a readable document (plan title, then each section heading +
   `section.body` as markdown, subsections nested), scrollable inside the node.
   The compact plan node = structural overview; the spec node = the full prose.
   Connect the spec node to the plan node with an edge (it's the source).

4. **Milestone bubbles, grouped.** Milestones render as **circular bubbles**
   (not rectangles), clustered near their owning task, **grouped by kind** — each
   declared `milestone_kind` is a color/lane. Bubbles of the same kind group
   together; iterations (loops) read as a progression. Thin edges connect the task
   to its milestone cluster and milestone→milestone links (`m.links`) draw within
   the cluster. Use the vertical axis for iteration depth (loops stack downward),
   as in the existing chain logic — but as bubbles, visually grouped, not a bare grid.

5. **Tasks show all saved data, nicely & current.** name, type, `model` badge,
   `model_rationale` (expandable "why this model?"), `status` + color, `status_label`,
   `intent`, `body` (markdown). Keep it live — re-renders on `plan-changed`.

## Navigation — Figma-grade trackpad (rewrite `useCanvasViewport`)

Current viewport only does mouse-drag pan + wheel-to-zoom. Replace with the
Figma/Excalidraw wheel model:

- **Two-finger trackpad scroll → PAN** both axes. macOS trackpad scroll arrives as
  `wheel` with `deltaX`/`deltaY` and `ctrlKey === false`. Pan by `(-deltaX, -deltaY)`.
- **Cmd/Ctrl + scroll → ZOOM** around the cursor. Browsers synthesize `ctrlKey === true`
  on trackpad **pinch**, so `e.ctrlKey || e.metaKey` ⇒ zoom-around-pointer (this also
  makes pinch-to-zoom work for free). Keep the existing zoom-around-cursor math.
- Keep **middle-button drag** (and add space+drag) for pan as a fallback.
- Must be **smooth** — keep RAF batching; respect `deltaMode` (lines vs pixels).
- `e.preventDefault()` so the page never rubber-bands; attach the wheel listener as
  **non-passive** (React's onWheel is passive on some setups — use a ref + native
  `addEventListener("wheel", h, { passive: false })`).
- Sensible `minScale`/`maxScale`; smooth zoom step. Fit-to-view on first mount stays.

## Current code map (what exists, what to change)

- `src/components/Canvas/useCanvasViewport.ts` — pan/zoom hook. **Rewrite nav** per above.
- `src/components/Canvas/layout/intraPlan.ts` — `layoutPlanIntra` produces the 3-band
  scatter. **Rewrite** into the plan-node-with-section-rows + satellite-tasks +
  milestone-clusters model. Must emit **section-row anchor points** (world coords of
  each section row's right-edge midpoint) so `CanvasView` can draw task→row edges.
- `src/components/Canvas/layout/types.ts` — `LaidOutNode`/`LaidOutEdge`/`PlanFrame`.
  Extend: edges need explicit anchor endpoints (not just node-center). Add per-row
  anchors. Consider an `anchor?: {x,y}` on edge endpoints, or section-row sub-nodes.
- `src/components/Canvas/CanvasView.tsx` — `edgeEndpoints()` currently uses node
  centers; change to use the new anchors (task left-edge ← section row right-edge;
  task ← milestone cluster). Edges should be **smooth bezier curves**, not straight
  lines, with subtle styling. Keep virtualization (visibleNodeIds/visibleFrameIds).
- `src/components/Canvas/cards/PlanFrame.tsx` — becomes the **compact sectioned plan
  card** (header + section rows + coverage), NOT a giant container. Reuse
  `planCoverageFraction` (cards/coverage.ts) and add per-section coverage counts.
- `src/components/Canvas/cards/SectionCard.tsx` — likely folds INTO the plan node as
  a row sub-component; or repurpose. Don't leave loose section cards on the board.
- `src/components/Canvas/cards/TaskCard.tsx` — keep as satellite node; already shows
  model/why/status/body. Tighten visuals per frontend-design.
- `src/components/Canvas/cards/MilestoneNode.tsx` — make it a **circular bubble**,
  colored by kind; clusters handled by the layout.
- **New:** a `SpecNode` card (full-spec markdown document). Register it in
  `nodeRegistry.tsx` as kind `"spec"` and emit one per plan in the layout.
- `src/components/Canvas/settings.ts` — all spacing/scale constants live here. Add
  what you need (row height, anchor inset, bubble radius, cluster spacing); no inline
  magic numbers.
- `nodeRegistry.tsx` — kind→renderer. Add `"spec"`. Keep the open-kind fallback.

## Data model (already in `src/components/Canvas/types.ts`)

`Plan { id, title, owner, status, sections: Section[], tasks: Task[],
milestone_kinds: {key,label,color}[], elements, task_groups, links, updated_at, canvas }`.
`Section { id, title, body, subsections: Section[], canvas{x,y} }`.
`Task { id, name, type, model, model_rationale, status, status_label, section_ids: string[],
group, intent, body, milestones: Milestone[], canvas{x,y} }`.
`Milestone { id, from, kind, text, ts, links: string[], loop_id, iteration, canvas{x,y} }`.
Backend just added `task_groups` and `links` (plan-level) — render task_groups as
loose visual grouping if cheap, but coverage + edges are the priority; groups/links
can be a follow-up if time-boxed.

## Constraints

- Deterministic layout (the layout engine is pure; manual `canvas{x,y}` overrides win
  and persist via drag → PATCH set_layout — keep that working).
- Files < 1000 lines; split by responsibility. No inline magic numbers (use settings).
- Keep existing tests green; add tests for: new viewport pan-vs-zoom branching,
  section-row anchors, per-section coverage counts, spec-node emission.
- Run via the project test runner: `npx tsx src/components/Canvas/<file>.test.tsx`
  (happy-dom). Typecheck: `npx tsc -p tsconfig.json --noEmit`.
- English only. Terse comments (≤2 lines, WHY not WHAT).

## Definition of done

- The board reads like Figma: one sectioned plan node, a connected spec node, task
  cards joined by curved edges to their section rows, milestone bubbles clustered by
  kind around their task. **Nothing floats disconnected.** Uncovered sections pop.
- Trackpad pan (both axes), cmd/ctrl+scroll zoom around cursor, pinch zoom — smooth.
- `tsc` clean; canvas tests green; new tests added. Commit AND push to your branch,
  then `codeterm agent report` with the exact commit hash + a summary + any deviations.
