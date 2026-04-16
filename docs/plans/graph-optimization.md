# Memory graph optimization plan

> **Status (2026-04-13):** ~50% done. Phase 1 (physics off main thread) + Phase 2 (Sigma renderer with zoom switching) shipped. Phase 3 (pinned positions persistence + save-on-idle semantics) still pending.

The `MemoryCanvas` mem-graph view becomes sluggish past ~500 nodes because every node is a React DOM component, every edge is an SVG element, and the force simulation runs on the main thread inside React state (`usePhysics`). Positions are persisted per-node in SQLite (`graph_x`, `graph_y` columns on `mem_items`), which adds write amplification on every tick and makes graph layout fragile — a single new edge visibly jumbles existing positions.

This doc plans the migration to a Sigma-rendered canvas with client-side layout and optional pinning.

## Current shape

**Backend:**
- `mem_items.graph_x DOUBLE, graph_y DOUBLE` persisted per node.
- Writes land via `mem update` / background physics sync (`bulkUpdatePositions` → REST `PATCH /api/mem/positions`).

**Frontend:**
- `src/components/MemoryPanel/MemoryCanvas.tsx` — main view, DOM/SVG, ~265 lines.
- `src/components/MemoryPanel/usePhysics.ts` — force simulation in React state, `requestAnimationFrame` loop.
- `src/components/MemoryPanel/MemoryCanvasOverview.tsx` — mini-map, ALREADY uses Sigma + graphology (91 lines, simple proof that we can).
- Node renderers: `nodes/BasicNode.tsx` (collapsed), `nodes/ExpandedNode.tsx` (expanded card with body preview), `nodes/ImageNode.tsx` (image thumbnail).

**Deps already installed:** `sigma ^3.0.0`, `graphology ^0.25.4`. No new packages needed for Phase 2.

## Why it's slow

1. **DOM nodes, not canvas.** Each node is a React component with its own state, refs, event handlers. At 500 nodes, reconciliation + style recalc per physics tick costs ms that add up.
2. **SVG edges.** 500+ `<line>` elements in one `<svg>` — browsers don't batch this well during transforms.
3. **Main-thread physics.** `usePhysics` runs the integrator in React, blocks interactions while simulating.
4. **Per-tick position persistence.** `bulkUpdatePositions` hits REST → SQLite inside the physics loop. Throttled but still chatty.
5. **DB-persisted positions.** Layout for 500 nodes should NOT live in storage — deterministic from the graph topology.

## Phased plan

### Phase 1 — unblock the main thread, stop writing positions (small, 1 session)

**Scope:** no rendering change. Just move the slow bits off the UI thread and stop persisting layout.

- Move `usePhysics` body into a Web Worker. Main thread posts (nodes, edges, pins); worker posts back `[{id, x, y}]` per tick.
- Drop `graph_x`/`graph_y` writes on save/update. Columns stay for legacy read compatibility but new saves leave them NULL.
- Drop the `PATCH /api/mem/positions` endpoint from client, ignore from server (keep endpoint as no-op for one release, delete in the next).
- On first load of a session, derive positions client-side via `graphology-layout-forceatlas2` (non-interactive placement) OR the existing force sim seeded from a deterministic hash of node id.
- Cache computed positions in `sessionStorage` keyed by workspace so a reload inside one session reuses them.

**Outcome:** interactions stay responsive while physics runs. No DB churn. Layout determinism improves (same graph → same positions).

### Phase 2 — Sigma as the main renderer (bigger, 1-2 sessions)

**Scope:** replace `MemoryCanvas` DOM/SVG rendering with Sigma while preserving node-kind UX.

- Wrap `MemoryCanvas` in a Sigma instance (mirror the pattern in `MemoryCanvasOverview`).
- Node sizing/color from `nodeKind.ts` — same mapping, rendered through Sigma's `defaultNodeType: "circle"` + custom node programs for kind-specific shapes if needed.
- Drag / pin: Sigma supports `mousedown` on nodes, the handler pins that node and the worker's force sim respects pinned IDs.
- Pulse animations: Sigma has a reducer hook for per-frame node attribute overrides — use `pulses` map to scale/tint the matching nodes.
- **Expanded nodes + image nodes as HTML overlay.** Sigma renders uniform circles; when a node is selected we position an absolute `<div>` (BasicNode / ExpandedNode / ImageNode) over its screen coords, following the camera via Sigma's `camera.coordinatesUpdated` event. Only the currently-expanded nodes (≤ a handful) exist as DOM — the other 499 are canvas pixels.
- `MemoryCanvasOverview` already proves the pattern: reuse its init/cleanup shape verbatim.

**Trade-offs:**
- Expanded-card-per-node visual style goes away for the default view. If the user wants every node as a card, they get worse perf back — this is deliberate.
- Sigma label rendering is uniform; richly styled titles need a custom node program or accept the default.

### Phase 3 — pinned positions, graceful saves (small, optional)

- New table `graph_pinned_positions (mem_id TEXT PK, x DOUBLE, y DOUBLE)`. Only user-drag-pinned nodes land here — not every node.
- Worker reads pins as boundary conditions for force sim.
- User can "unpin" to reflow — row deletes.
- Migration: drop `mem_items.graph_x`, `mem_items.graph_y` columns entirely after Phase 2 has shipped one release.

## Execution order

Do Phase 1 first and ship. It's the 80/20 — most of the perceived lag is main-thread blocking, not the renderer. Sigma in Phase 2 is the cherry on top for 1k+ nodes.

Phase 3 only if users complain about drag positions not sticking. Optional.

## Non-goals

- No 3D, no VR, no cosmic visualizations. Keep it a 2D force-directed graph.
- No server-side layout. Mem is per-machine, the cost of layout belongs on the client.
- No physics engine swap (matter-js, cannon, etc.) — ForceAtlas2 / Sigma's own forceSupervisor are fine.

## Open questions

- Do image nodes need to be rendered in canvas directly (node program with texture) or is HTML overlay enough at the rare cardinalities where images are shown?
- Is `graphology-layout-forceatlas2` deterministic enough that seeded positions stay stable across reloads without explicit storage? If yes, drop `sessionStorage` caching from Phase 1.
- Do we keep the overview mini-map (`MemoryCanvasOverview`) or fold it into the main Sigma instance as a second camera / viewport?
