# Canvas v3 — Whole Layout System Design (honoring §22)

<!-- revision: 1 -->

**Status:** worker design proposal (no code). Honors the two FOUNDATIONAL rules in
`2026-06-24-canvas-v2-design.md` §22. Where §16/§18/§20 conflict, §22 + this doc win.

## 0. The failure we are fixing (screenshot `/tmp/v3-vm-fixed.png`)

At fit-to-view = 13% the board is two vertical gray strips of micro-text. Root causes,
each killed by §22:
1. The node grew **unbounded vertically** (24 sections × full bodies → a km-tall strip),
   so fit-all is FORCED to 13%.
2. There was effectively **only one representation** (a Document-ish dump). No overview
   mode that is legible-at-small.
3. **No in-node navigation** (no rail/TOC) → a tall node is an unnavigable cliff.
4. Old escape was "zoom shrinks/LOD-swaps the node" — **now forbidden** by §22.1.

So the readability answer must come from MANUAL modes + bounded frames + in-node
navigation + camera-focus — never from zoom resizing anything.

## 1. The TWO base rules I am designing under (§22, absolute)

- **R1 Zoom = camera ONLY.** Nodes have a FIXED world-size. Zoom pans/scales the camera;
  a node's size/content never change with zoom. View mode (Document/Card/Tile) is a
  MANUAL toggle, never zoom-triggered.
- **R2 Plan + Spec = ONE sectioned node** with per-section OUTPUT anchors (вых). Spec
  prose = the section bodies. Tasks connect to those section outputs.

---

## 2. The ONE sectioned plan/spec node — exact structure

A single fixed-size node, three regions:

```
┌─ HEADER (fixed ~56px) ───────────────────────────────────────────┬─ RAIL ─┐
│ title · owner chip · coverage ▓▓▓▓▓░ 72% · 18/24 · [Doc|Card|Tile]│ §1 ✓[4]●│
│ [fit-node ⛶]                                                       │ §2 ●[3]●│  ← per-section
├───────────────────────────────────────────────────────────────────┤ §3 ⚠[0]●│     rail rows,
│ BODY (fixed frame, view-mode-dependent render)                     │ §4 ●[2]●│     each carries
│  Document: full markdown of sections, INTERNAL SCROLL              │ §5 ✓[1]●│     a вых OUTPUT
│  Card:     section titles + 1–2 line digests (no full md)          │ …       │     dot on the
│  Tile:     collapsed — title + coverage + [n sections] + dot       │ §24●[1]●│     outer edge
└───────────────────────────────────────────────────────────────────┴────────┘
```

### 2.1 Header (fixed height)
Plan title, owner/orchestrator chip, coverage bar + %, done/total, the **view-mode
switch** (Doc/Card/Tile, manual), a **"fit this node"** camera button. Header never scrolls.

### 2.2 Right-edge RAIL = TOC + OUTPUT anchors (the load-bearing component)
A thin always-visible vertical strip listing **every** section: `ready-glyph · title(1
line, ellipsis) · [n] task badge · вых output dot` on the outer edge. The rail is CHROME,
not scrolled body, so **all 24 section outputs are always present & connectable** no matter
what the body shows. Two jobs in one component:
- **Navigation (TOC):** click a rail row → body scrolls/pages to that section. This is what
  makes a tall node navigable WITHOUT zoom.
- **Output anchors:** the вых dot at each rail-row's fixed world-Y is the task connection
  point (§3). Rail-row Y is a stable world coordinate (node is fixed-size) → deterministic.

Rail row ~20px. 24 sections = 480px → bounds rail height. >~30 sections → rail gets its own
mini-pager OR the node decomposes into SectionChunk children (§2.5). Coverage tint also
shows on the rail row background (covered=accent, uncovered=warning) so uncovered sections
"pop" even in the rail.

### 2.3 Body — fixed frame, render by view mode (NO zoom scaling)
Outer frame is FIXED (`planWidth ~440`, `docFrameHeight ~640`). Three hand-designed modes,
each at a FIXED readable text size:
- **Document** — full `RichMarkdown` (markdown + mermaid + code highlight) of the section
  stack, **scrolling INTERNALLY inside the fixed frame**. The reading view.
- **Card** — section titles + per-section 1–2 line digest (authored `summary` / first
  sentence) + ready/[n] badges. Compact, legible, no tiny text.
- **Tile** — title + coverage strip + `[n sections]` + primary-facet dot + ready summary.
  The overview view; legible at small camera scale (this is what fixes the 13% gray board).

Switching modes swaps the BODY render at a FIXED size; the camera is untouched. **Manual
only** — global toggle and/or per-node override. Never auto-by-zoom (§22.1 kills §18.2/§20.2
auto-switch).

### 2.4 Per-section OUTPUT anchor placement (R2, precise)
Output anchors live on the **rail's outer edge**, one вых dot per section at its rail-row
world-Y. Always rendered (rail is chrome) → every section is connectable in Document/Card.
In **Tile mode** the rail collapses → all section outputs **bundle to one node-level вых
stub** (edges show as "→ 18 tasks" count); switching back to Doc/Card re-fans them out.
Band assignment of tasks NEVER changes with mode — only the edge ENDPOINT adapts (§3).

### 2.5 EXTREME plans (the only real "still tall" case)
If sections > ~30 or bodies are huge, the root node owns child **SectionChunk** nodes (one
per top-level section), each a smaller bounded node, edge-connected to the parent's rail
row. The портянка **decomposes into a connected constellation on the one flat map** —
never a hidden accordion, never a modal drill-stack (§20.1 invariant).

---

## 3. Task attachment + Banded Multi-Column packing (§20.3 reconciled with R1)

Tasks hang off section OUTPUTS (rail вых dots), packed by the Banded Multi-Column sweep:
1. **Band** = task's PRIMARY covered section (first `section_id`). Band Y = that section's
   **rail-row world-Y** (fixed, since node is fixed-size). Standalone → synthetic band.
2. **Stable intra-band order** by creation/id (realtime insert appends, no reshuffle).
3. **Bounded column grid** right of the node: `colX[c] = nodeRight + margin +
   c·(taskColW+colGap)`, `c ∈ 0..maxTaskCols-1` (3–4) → caps horizontal sprawl.
4. Lay each band left→right, **wrap to sub-rows** (7 tasks @3 cols → 3·3·1).
5. **Top-down non-overlap sweep** by ascending band Y: `idealTop = railRowY[b] − blockH/2`;
   `actualTop = max(idealTop, cursorY)`; `cursorY = actualTop + blockH + bandPad`. Classic
   non-overlapping interval label placement: each block hugs its section row, pushed down
   only when crowded. Deterministic; insert recomputes only the changed band downward.
6. **Edges:** bezier from the rail вых dot (fixed world point) → task card left edge. Multi-
   section: solid to primary, thinner to others. Routed AFTER packing, obstacle-avoiding.

**R1 reconciliation:** rail-row Y is a FIXED world coordinate → the whole sweep is computed
in world space and is stable; camera zoom merely scales it on screen. In Tile mode tasks
stay banded (band Y derived from section index even when rail is collapsed); only edges
bundle to the node-level stub. Packing never depends on zoom.

---

## 4. Milestones / Steps — interior to the fixed-size task card (§16.9, no zoom-LOD)

Steps render INSIDE the task card body as the "Thread & Coil" mini-timeline, at the FIXED
representation chosen by the task's MANUAL view mode (not zoom):
- Task=Tile → steps = folded constellation dots in a strip.
- Task=Card → steps = vertical thread (spine) + 1-line step titles + kind lanes; loops coil.
- Task=Document → mini step-cards stacked along the spine; >~6 → internal scroll; click→detail.
Part of the card's fixed measured height / internal scroll; never floats outside; 4px stem,
no own grid cells (§20.3.8). Mode inherits task/global, never zoom.

---

## 5. Multi-plan + orchestrator regions — world arrangement, no overlap

Three nested deterministic packers, all stable-id-keyed (no global reshuffle):
1. **Plan-region** = one sectioned node + its banded task field + step interiors. Reports a
   bounded box (node frame + maxTaskCols·colW + measured task heights).
2. **Orchestrator group** = soft labeled band wrapping its plan-regions; label = orchestrator
   title (territory name), click → opens Tab. Plans laid **left→right, wrap to sub-rows**
   (each region width-bounded), ordered by plan id/creation.
3. **Top-level region packer:** orchestrator groups + "Unowned/claimable" + "Inbox" stack
   **top-down** via the same non-overlap sweep, ordered by orchestrator id/creation.
No overlap because every region reports a bounded box; same data+modes → same layout.

---

## 6. Standalone / Inbox tasks (§5)

Inbox = reserved plan, no sections. Its node has a single synthetic "Inbox" output; tasks
pack in one synthetic band via the same column-grid sweep. Renders as a task cluster under
the taker's orchestrator group, or an "Unassigned" top-level region. Same TaskCard, same
edges (to the synthetic output + tab-link chip).

---

## 7. THE READABILITY / NAVIGATION ANSWER (zoom can't resize — the crux)

Five MANUAL/structural mechanisms replace zoom-driven LOD:

1. **Discrete designed view modes (manual):** Tile (overview-legible), Card (mid), Document
   (reading). Each fixed-size, fixed readable text. **Set globally to Tile → fit-all is a
   readable concept map**, not gray micro-text. This is the direct fix for the screenshot.
2. **In-node RAIL (TOC + outputs):** click a section title to navigate a tall document node
   without touching zoom. Stops the "unnavigable cliff".
3. **Bounded fixed frame + internal scroll:** Document body is confined to a fixed frame;
   long content scrolls INSIDE. Node world-height is bounded (frame + rail), so fit-all is
   ~50–70%, not 13%. (Scroll-vs-pan: scroll the body only when pointer is over an
   active/hovered node body; otherwise wheel pans the canvas.)
4. **Camera-focus reading gesture:** double-click node / click rail entry / `canvas focus`
   → camera smooth-zoom-to-fit that node (or section region). Node is fixed-size in Document
   mode rendering real text, so camera-zoom-until-it-fills = perfectly readable, like zooming
   a PDF to 100%. Neighbors DIM but stay edge-connected (focus = camera convenience, §20.5).
   Esc / "fit all" returns + restores saved viewport.
5. **Minimap + breadcrumb HUD:** corner minimap (region boxes + camera rect) for "where am
   I"; breadcrumb `Workspace ▸ Orchestrator ▸ Plan:<handle> ▸ <section>`.

### Default experience (the two poses)
- **Overview (fit-all):** one-click "Overview" preset = global Tile + fit-all camera. All
  orchestrator regions, plan tiles, task tiles legible (big title, primary-facet dot,
  coverage strip, [n], ready glyph). Uncovered sections still pop. A map, not noise.
- **Reading:** double-click the plan node (or Document + fit-node). Camera fills viewport
  with the fixed node; full markdown; right rail navigates sections; internal scroll moves
  within. One click back to overview.

These presets are MANUAL (a button + camera action), never a zoom threshold — §22.1-compliant.

---

## 8. Determinism + no-overlap + realtime-add stability (§16.5 / §20.3)

- Sizes are FIXED per `(node, view-mode)` — measured once at the mode's render offscreen,
  cached by content hash; layout packs from fixed boxes. View mode is part of the layout
  INPUT (manual + persisted per-node/global), so it's deterministic too.
- Realtime add: new task → recompute only its band downward + its plan-region box (propagate
  upward only if the box grew). New section → fixed frame may not grow at all (scroll/page);
  rail gains a row; only that node's task bands below shift. STABLE slots → no jitter.
- Edge routing after packing, obstacle-avoiding; debounce ~50ms→RAF, cancel stale jobs,
  apply atomically (no intermediate overlap during fast navigation).
- Same data + same modes → identical layout.

---

## 9. Three alternatives for the one-node bounding strategy + my pick

- **Alt A — fixed frame + internal PAGINATION** (block-measure → greedy page pack,
  keep-with-next, on-canvas pager ◂ ●●○○ ▸). Most "designed"/print-like; heavy machinery.
- **Alt B — fixed frame + internal SCROLL** (RichMarkdown flows, sticky right rail
  TOC+outputs, body scrolls inside the fixed frame). Simple, full markdown/mermaid/code
  fidelity, near-zero new machinery; needs careful scroll-vs-pan event gating. **← PICK.**
- **Alt C — unbounded-height node + rail + minimap only** (let it grow, navigate by rail).
  REJECTED: this IS the screenshot — fit-all useless, edges span km, band Ys spread over km.

**Pick = Alt B.** Rationale: under §22, zoom no longer fights us (it doesn't resize), so the
ONLY job is BOUNDING the node — internal scroll bounds it with full RichMarkdown fidelity and
almost no new code. Pagination's only edge (clean designed pages) is a nice-to-have → keep
Alt A as a future upgrade. Pair Alt B with: right-edge rail (TOC+outputs), 3 manual view
modes, Tile-overview default at fit-all, camera-focus reading gesture, minimap+breadcrumb,
and SectionChunk decomposition for extreme plans.

---

## 10. settings.ts additions (no inline magic numbers)
`planWidth 440`, `docFrameHeight 640`, `cardFrameHeight 320`, `tileW 280`/`tileH 160`,
`railWidth 200`, `railRowH 20`, `taskColWidth 260`, `maxTaskCols 3`, `colGap 24`,
`bandPad 16`, `regionPad 24`, `stepBubble 12`, `focusDimOpacity 0.25`, scroll/pan hit-test
threshold. All tuned in the Playwright visual loop (§11.1 / §18.3).
