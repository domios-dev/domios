# Memory Visuals — Explorable Graph + Graph-Worker Agents

**Status:** design doc, pre-implementation
**Scope:** full visual memory system for CodeTerm — canvas-based explorable graph of `mem_items` with rich interactive nodes, a dedicated class of "graph-worker" agents that live inside the memory view, and scale architecture from 50 to 50k+ items.
**Author:** Claude session, working on `feat/memory` branch
**Parent:** `docs/plans/mem-architecture.md`

## Problem

Phase 3.x shipped a working mem store. It has a CLI, FTS5, vectors, a rerank pass, graph relations with temporal validity, dreaming cron. **None of it is visible.** The user can see what's inside mem only via `mem search` / `mem get` in a terminal. For a system whose whole premise is "this is where everything the agents know about me lives," this is a massive UX gap:

- The user can't browse mem visually to understand what agents have learned
- The user can't watch a dreaming run consolidate items
- The user can't see which agents are currently reading/writing mem
- The user can't click a mem_item to inspect its body, graph connections, edit tags, or promote a learning
- When something goes wrong (wrong wake-up result, stale fact, forgotten skill), there's nowhere to *look* at mem to find the bad row

Fixing this is not just adding a table view. The user's requirement is much stronger: **a full explorable canvas of the entire mem graph, where nodes are rich interactive panels, agents are visible working on the graph in real time, and the whole system scales**.

## Goals

1. **Full-map canvas view**, not subgraph neighborhoods. User wants to see everything at once, pan/zoom like Google Maps.
2. **Rich node content, not just dots.** Nodes can contain markdown bodies, images, forms, buttons, mini-editors. Expandable.
3. **Graph-worker agents** — a new class of agent that lives *inside* the memory view, not in the main pane tree. Spawned with a graph-specific intent ("dedup area=db", "consolidate person_* entities", "find all skills related to auth"). Visible as bubbles in the memory panel, not as regular tabs.
4. **Live activity overlay** — when any agent (graph-worker or regular) reads/writes mem, the touched nodes pulse in the agent's color. User can watch what's happening.
5. **Scale architecture** — 50 items today, 5-10k in 1-2 years, 50k+ possible. Must not require a rewrite at each scale tier.
6. **Jump-to-terminal and back** — clicking a graph-worker opens its pane; a floating "← Memory" button returns.

## Non-goals (explicitly out of scope for v1)

- Collaborative multi-user editing (tldraw-style multiplayer)
- 3D graph rendering
- Hand-drawn annotations on top of the canvas (whiteboard mode)
- Export the graph to an image / PDF
- Mobile touch support — desktop only for now
- Editing relations visually (drag an edge between two nodes) — v2
- Time-travel / diff between two mem snapshots — v2

## Reference projects studied

Cloned to `/references/` (gitignored) for code study:

| Project | URL | License | Why we study it |
|---|---|---|---|
| **xyflow** | github.com/xyflow/xyflow | MIT | Reference implementation for custom React-component nodes, minimap, viewport culling, example library |
| **langflow** | github.com/langflow-ai/langflow | MIT | `src/frontend/src/CustomNodes/GenericNode/` — how to render an agent node with embedded forms, inline status, dynamic sockets. Closest UX analog to "memory_item as interactive panel" |
| **sigma.js** | github.com/jacomyal/sigma.js | MIT | WebGL graph rendering at 10-100k scale. Reference custom node programs in `packages/node-image/`, `packages/node-piechart/`, `packages/node-border/`, `packages/node-square/` — each is a self-contained example of how to build a shader-based node renderer. Core program base in `packages/sigma/src/rendering/program.ts`. |
| **cosmos** | github.com/cosmograph-org/cosmos | CC-BY-NC-4.0 | GPU force simulation study only — non-commercial license, we can't vendor code. Modules in `src/modules/` (Points, Clusters, ForceManyBody, Drag, Zoom, FPSMonitor) show the architecture of a WebGL graph engine. |
| **tldraw** | — | tldraw license (non-OSI) | Not cloned (license) — studied online for ShapeUtil pattern (React components as shapes with hit-testing/resize) |

Key insights from research:

- **No single library covers both "rich React component nodes" AND "10k+ nodes at 60fps".** DOM-based libs (xyflow, reactflow, tldraw) top out around 1-2k rich nodes before DOM reconciliation kills frame rate. WebGL libs (sigma, cosmos, reagraph) scale but render points/circles, not React content.
- **The only realistic architecture for both is a dual-renderer hybrid:** WebGL point cloud when zoomed out (overview, 50k+ fine), DOM-React custom nodes when zoomed in (rich content, never more than ~200 visible at once).
- **Langflow's `src/frontend/src/CustomNodes/GenericNode/` is the closest existing pattern** to what we want — an agent node that:
  - Has a title bar with icon + name + status badge
  - Has a collapsible body with inline form fields
  - Shows dynamic input/output sockets generated from a schema
  - Updates in place when the agent is running (streaming text, progress indicators)
  - Has `components/` and `hooks/` subdirs showing how they split concerns
  - Related: `CustomNodes/NoteNode/` is a great example of a smaller rich node (note with toolbar), perfect analog to our `ExpandedPanelRenderer`
- **xyflow's `onlyRenderVisibleElements` viewport culling** is the baseline scale strategy below the WebGL tier — render only nodes whose bounding box intersects the visible viewport, which keeps us smooth through ~2k rich DOM nodes.
- **Sigma.js custom node programs** (shader-based renderers with texture atlases for images) are the escape hatch at 10k+, but only for the overview renderer.

## Architecture

### Dual-renderer hybrid

The canvas has **two rendering layers** that switch based on zoom level:

```
┌─────────────────────────────────────────────────────────┐
│  Viewport                                               │
│                                                         │
│  Zoom < 0.5 (overview)                                  │
│    └─ WebGL renderer (sigma.js programs)                │
│       - All nodes as colored dots, size by importance   │
│       - Edges as lines, alpha by relation strength      │
│       - No DOM, handles 50k+                            │
│                                                         │
│  0.5 <= Zoom < 1.5 (transition)                         │
│    └─ WebGL + sparse DOM labels                         │
│       - Dots + text labels for nodes in viewport        │
│                                                         │
│  Zoom >= 1.5 (rich mode)                                │
│    └─ DOM nodes (React components, xyflow-style)        │
│       - Only nodes intersecting viewport are rendered   │
│       - Full custom node components, editable, images   │
│       - Max ~200 simultaneously visible                 │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

Transition happens smoothly: at zoom 1.0, overview renderer fades out while DOM nodes fade in for the viewport region. Both share the **same data model and the same force-simulated positions**, so the only thing changing is the renderer, not layout.

Concrete libraries:
- **Overview renderer:** sigma.js with graphology data model (both MIT)
- **Rich renderer:** custom React components positioned absolutely via a small viewport transform layer (we do NOT use xyflow directly because xyflow owns its own zoom/pan and fights with sigma — we build the transform layer ourselves, 150 lines of React)

### Data model

Shared graph state lives in a single `graphology` Graph instance. Both renderers read from it, mutations propagate both ways.

```ts
interface MemNode {
  id: string;              // mem_item.id: "skill_home_assistant"
  kind: MemKind;           // for color + icon
  title: string;
  summary: string | null;
  area: string | null;
  importance: number;      // for size
  tags: string[];
  hasVector: boolean;      // dot has a ring if vectorized
  hasBody: boolean;        // lazy-loaded on zoom-in
  x: number;               // persisted layout (mem_items.graph_x)
  y: number;
  nodeKind: 'basic' | 'expanded' | 'form' | 'image' | 'agent';  // node renderer variant
}

interface MemEdge {
  id: string;              // "skill_home_assistant::has_script::script_ha_api_py"
  from: string;
  rel: string;             // relation_type
  to: string;
  confidence: number;      // 0..1, mapped to edge alpha
  valid_from: string;
  valid_to: string | null;
  source_mem_id: string | null;  // which mem_item this relation came from
}

interface GraphWorker {
  worker_id: string;       // "gw_abc123"
  pane_id: string;         // underlying hidden pane
  intent: string;          // "dedup area=db"
  spawned_by: string;      // "cron:mem-dreaming-auto" or "claude:main-pane"
  spawned_at: string;      // ISO
  status: 'starting' | 'reading' | 'analyzing' | 'writing' | 'done' | 'error';
  progress_text: string | null;  // "scanned 42/128 items"
  touched_nodes: Set<string>;    // mem_ids this worker has read/written
  color: string;           // stable color from palette
}
```

### Node kinds (rich rendering variants)

Not every mem_item wants the same UI. The `nodeKind` column (derived from `kind` + heuristics, not stored in DB) determines how the DOM renderer draws it:

1. **`basic` (default, dot)** — small colored circle + label. What you see at zoom 1.5-2.5. For most rules, facts, short preferences.

2. **`expanded` (panel)** — expandable card with title bar, summary, tags, importance stars. Click to toggle collapsed/expanded. Shows first 200 chars of body in expanded state. Default for all skills and long notes.

3. **`form` (editable)** — inline edit form. Click to enter edit mode, tags become chip inputs, importance becomes a slider, body becomes a textarea. Save button commits via `PATCH /api/mem/items/{id}`.

4. **`image` (with preview)** — if a note/skill has `tags: [image]` or the body contains a markdown image reference, render the image inline (thumbnail when collapsed, full when expanded). Loaded lazily via blob URL.

5. **`agent` (graph-worker)** — completely different visual. Avatar + name + status + progress bar + "touched: N nodes" counter. Clicking opens the worker's pane terminal.

All five kinds are standalone React components implementing a common interface:

```ts
interface NodeRendererProps {
  node: MemNode;
  selected: boolean;
  activePulse: { color: string; age: number } | null;
  onClick: (id: string) => void;
  onEdit: (id: string, patch: Partial<MemNode>) => void;
  onExpand: (id: string, expanded: boolean) => void;
}
```

New node kinds get added by implementing this interface and registering in a map. Zero-lock-in on the initial set.

### Graph-worker agents — the key new concept

This is the novel architectural piece. Most AI IDE graph views show static data. We want **agents living inside the graph**, working on it in parallel with the user.

#### Lifecycle

A graph-worker is spawned with an explicit intent and lives as a hidden pane in the CodeTerm mux. It does not show up in the main tab bar, the pane tree sidebar, or any other regular pane list. It only shows up in the Memory panel's "Active workers" bubble area.

```
Spawn:
  codeterm mem worker spawn \
    --intent "dedup area=db" \
    --agent claude \
    --parent "cron:mem-dreaming-auto" \
    --task "Review all mem_items with area=db, find duplicates, merge them via mem update --replace, report back."

Result:
  { "worker_id": "gw_abc123", "pane_id": "...", "intent": "..." }
```

Behind the scenes:
1. Creates a new pane in the mux with `hidden: true` and `tags: ["graph-worker", "worker_id:gw_abc123"]`
2. Registers the worker in a `GraphWorkerRegistry` keyed by `worker_id`
3. Emits event `graph-worker-spawned` with full metadata
4. Starts the agent in that pane with the task prompt prefixed by graph-worker context (role, intent, parent pane, reporting instructions)

The worker task prompt automatically includes:

```
You are a Graph Worker for the CodeTerm memory system. Your intent is:
<intent string>

You are NOT visible as a regular tab — you exist in the memory panel only.
Your touched mem items will show as pulses in the user's memory graph.
Report progress via `codeterm mem worker progress "<status text>"` — this
updates your bubble status. When done, report via
`codeterm mem worker report "<summary>"` and exit.

Your parent is: <parent id>. When you report, your parent will be notified.
```

#### Hidden pane — what "hidden" means

The pane is fully real: it has a PTY, runs an agent, accepts input, writes output. What "hidden" means:

- It does NOT appear in `mux.list_panes()` by default — only when `include_hidden = true` is passed
- The main React `TabBar` component filters out `pane.hidden === true` panes
- The `PaneTree` sidebar (when implemented) filters them out
- They still exist in the mux snapshot, still persist on restart, still accept `focus_pane` calls
- The Memory panel has its own list that specifically queries for `tags: ["graph-worker"]` panes and shows them

When you click the worker bubble in the memory panel:
1. Navigation state saves `returnTo: "memory-panel"` with the current zoom/pan/selected state
2. Memory panel closes
3. `focus_pane(worker.pane_id)` is called — this temporarily **unhides** the pane for display
4. The pane becomes the active pane in the mux (but stays filtered from TabBar via a different mechanism — see below)
5. A floating back bar appears top-center: `← Memory Graph`
6. Click back → unfocus the worker pane, re-hide it, reopen Memory panel with restored state

The "stays filtered from TabBar" part uses a separate flag `show_in_tabbar: bool` that defaults to `false` for graph-workers even when they're focused. The main window still shows their terminal content, but the surrounding UI chrome treats them as non-tab panes (like a modal view). This avoids the worker accidentally becoming a "real" tab that the user has to close.

#### Intent and progress

The user wants to know *why* a worker exists and *what it's doing*. Workers have two string fields updated via CLI:

- `intent` — set at spawn time, never changes. "Dedup area=db", "Find all skills related to auth", "Consolidate person_* entities from last 7 days".
- `progress_text` — updated by the worker as it runs, short status. "Scanning 42/128", "Found 3 dup clusters", "Merging set 1/3".

Backend commands:

```bash
# Called by the worker agent itself
codeterm mem worker progress "Scanned 42/128 items, 3 clusters found"

# Get current state of all workers
codeterm mem worker list
codeterm mem worker get gw_abc123

# Report completion (like regular agent report)
codeterm mem worker report "Merged 7 duplicate pairs in area=db. Saved 4 facts."
# This auto-emits a report back to the parent pane (same mechanism as agent report)
```

#### Touched-nodes tracking

When a graph-worker runs `codeterm mem get X`, the REST handler reads the `X-CodeTerm-Worker-Id` header (CLI auto-injects it from `$CODETERM_WORKER_ID` env var set at spawn) and records X in the worker's `touched_nodes` set. Backend emits `mem-activity` event with `source.worker_id` populated.

The frontend uses this for two things:
1. Pulse animation on the node in the worker's color
2. The worker's bubble shows "touched: 42" counter, updated live

### Activity pulses

Every mem read/write operation emits a `mem-activity` Tauri event:

```ts
interface MemActivityEvent {
  op: 'read' | 'search' | 'write' | 'update' | 'delete' | 'dream';
  mem_ids: string[];           // could be multiple (search returns N)
  source: {
    kind: 'cli' | 'rest' | 'dream' | 'worker';
    pane_id?: string;
    worker_id?: string;
    agent_provider?: string;
  };
  ts_ms: number;
}
```

Backend adds event emission to every `MemManager::get/search/recall/save/update/delete/...` call after successful completion. Header `X-CodeTerm-Pane-Id` / `X-CodeTerm-Worker-Id` identifies the source.

Frontend:

```ts
const pulses = useRef<Map<string, Pulse>>(new Map());

useEventListener('mem-activity', (event) => {
  const color = colorFor(event.source);  // agent/worker color
  event.mem_ids.forEach((id) => {
    pulses.current.set(id, {
      color,
      op: event.op,
      startTime: performance.now(),
      duration: 3000,
    });
  });
});

// In the render loop, for each pulse:
// - Age = (now - startTime) / duration
// - If age > 1, remove from map
// - Otherwise, draw a ring around the node with radius = baseRadius * (1 + (1 - age) * 1.5)
//   and alpha = 1 - age, color = pulse.color
```

Both renderers (WebGL and DOM) consult the pulse map. In WebGL we pass pulse data as a uniform buffer; in DOM we add a CSS animation class.

### Clustering and level-of-detail

At 5000+ items we need more than viewport culling. The strategy:

1. **Area-based clustering** — items with the same `area` tag are grouped into a "super-node" at zoom < 0.5. The super-node shows the area name, item count, and dominant kind color. Click to expand (zoom to the cluster region automatically).

2. **Importance-first culling** — when a lot of nodes fall in the same pixel, we render the highest-importance one on top and drop the rest. Edge culling similarly favors high-confidence relations.

3. **Saved layout positions** — after the initial force simulation converges, we batch-PATCH positions to `mem_items.graph_x / graph_y`. On the next load, we skip force simulation entirely and use persisted positions. New items coming in trigger only local reshuffles.

4. **Progressive load** — on first open, we fetch the first 1000 items sorted by importance via `/api/mem/graph/full?limit=1000&order=importance`. The user sees immediately. In the background, the next chunks load and fade in. For 50k+ we never load all at once — we stream by clusters, loading the cluster contents only when the user zooms in on that area.

### Scale tiers

| Items | Strategy | DOM nodes visible | Cold load time |
|---|---|---|---|
| < 500 | DOM-only renderer, no culling | all | < 100ms |
| 500-2000 | DOM renderer with viewport culling | ~200 | < 300ms |
| 2000-5000 | Dual renderer: WebGL for zoom < 1.5 | ~200 | < 500ms |
| 5000-20000 | Dual + persisted layout | ~200 | < 1s |
| 20000-50000 | Dual + area clustering + progressive load | ~200 | < 2s (first 1000), rest streams |
| 50000+ | Dual + clustering + server-side node pagination by spatial region | ~200 | < 2s (visible region only) |

The key invariant: **number of simultaneously rendered DOM nodes never exceeds ~200**, regardless of total item count. That's what makes this architecture scale.

### Data flow

```
┌────────────────────────────────────────────────────────────┐
│  Frontend: MemoryPanel                                      │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ GraphologyStore (shared state)                      │   │
│  │  - nodes, edges, positions, pulses, workers         │   │
│  └─────────────────────────────────────────────────────┘   │
│         │                          ▲                        │
│         │                          │                        │
│   ┌─────▼─────┐             ┌──────┴──────┐                 │
│   │ Viewport  │             │ Mem events   │                 │
│   │ renderer  │             │ subscription │                 │
│   │ (sigma)   │             │              │                 │
│   └───────────┘             │ mem-activity │                 │
│   ┌───────────┐             │ mem-changed  │                 │
│   │ Rich DOM  │             │ graph-worker │                 │
│   │ renderer  │             │  -spawned    │                 │
│   │ (custom)  │             │  -progress   │                 │
│   └───────────┘             │  -reported   │                 │
│                             └──────────────┘                 │
└────────────────────────────────────────────────────────────┘
                │                         ▲
                │                         │
                ▼                         │
┌────────────────────────────────────────────────────────────┐
│  Backend: mem_routes.rs + new graph routes                  │
│  ─────────────────────────────────────────────────────────  │
│  GET  /api/mem/graph/full       ← initial load              │
│  GET  /api/mem/graph/region     ← spatial pagination        │
│  GET  /api/mem/graph/stats      ← counts                    │
│  PATCH /api/mem/positions       ← persist layout            │
│  GET  /api/mem/workers          ← list graph-workers        │
│  POST /api/mem/workers          ← spawn worker              │
│  POST /api/mem/workers/{id}/progress  ← update status       │
│  POST /api/mem/workers/{id}/report    ← complete            │
│  DELETE /api/mem/workers/{id}   ← stop / kill               │
│                                                             │
│  Events emitted (Tauri app_handle.emit):                    │
│    mem-activity      (every read/write)                     │
│    mem-changed       (topology changed — add/delete/update) │
│    graph-worker-spawned                                     │
│    graph-worker-progress                                    │
│    graph-worker-reported                                    │
│    graph-worker-done                                        │
└────────────────────────────────────────────────────────────┘
                │
                ▼
┌────────────────────────────────────────────────────────────┐
│  mem/ (core)                                                │
│    mem_items + relations tables                             │
│    GraphWorkerRegistry (new: src/mem/workers.rs)            │
│    Activity event emission on every MemManager method      │
└────────────────────────────────────────────────────────────┘
```

### New backend modules

1. **`src-tauri/src/mem/workers.rs`** — GraphWorkerRegistry
   - In-memory map `worker_id → GraphWorkerInfo`
   - Persisted to `workers.json` in mem dir (so restart preserves active workers — though they may not actually be running anymore, so we mark them `status: orphaned`)
   - Spawn/get/list/update-progress/report/kill methods
   - Emit events on every state change

2. **`src-tauri/src/mem/activity.rs`** — ring buffer of recent mem-activity events
   - Capped at 500 events
   - `record(event)`, `recent(since_ms) -> Vec<MemActivityEvent>`
   - Used by `GET /api/mem/active-agents` endpoint
   - Also drives the "Recent activity" feed in the sidebar

3. **`src-tauri/src/api/mem_graph_routes.rs`** — new route file for graph-specific endpoints
   - `/api/mem/graph/full` — initial dump
   - `/api/mem/graph/region` — spatial pagination (given a viewport rect, return nodes inside)
   - `/api/mem/graph/stats` — counts by kind, by area
   - `/api/mem/positions` — batch layout persist
   - `/api/mem/workers` — worker CRUD

4. **`src-tauri/src/mem/schema.rs` addition** — `graph_x REAL, graph_y REAL` columns on `mem_items`
   - Migration via ALTER TABLE (rusqlite's `ALTER TABLE ADD COLUMN` is fine; we handle the "column already exists" error by catching and ignoring)

5. **`src-tauri/src/multiplexer/state.rs` additions** — `hidden: bool` and `show_in_tabbar: bool` flags on PaneInfo
   - Default to `false` / `true` for normal panes
   - Graph-workers set to `true` / `false`
   - All pane-list APIs gain `include_hidden: bool` parameter

6. **`cli/src/main.rs` additions** — `codeterm mem worker` subcommand
   - `spawn --intent --agent --parent --task`
   - `list`
   - `get <id>`
   - `progress <text>` (uses `$CODETERM_WORKER_ID` env var, self-identifies)
   - `report <summary>` (same)
   - `kill <id>`
   - Also: inject `X-CodeTerm-Worker-Id` and `X-CodeTerm-Pane-Id` headers on all mem_* HTTP calls when env vars are set

### Frontend component tree

```
src/components/MemoryPanel/
├── MemoryPanel.tsx                 # Modal root, opens via command palette
├── MemoryContext.tsx               # Graphology store + subscriptions
├── MemoryCanvas.tsx                # Main canvas, switches renderers by zoom
├── MemoryCanvasOverview.tsx        # WebGL overview renderer (sigma wrapper)
├── MemoryCanvasRich.tsx            # DOM rich renderer (viewport-culled custom nodes)
├── MemoryViewport.ts               # Shared zoom/pan transform + viewport math
├── nodes/
│   ├── BasicNodeRenderer.tsx       # Dot with label
│   ├── ExpandedPanelRenderer.tsx   # Collapsible card with body preview
│   ├── EditableFormRenderer.tsx    # Inline edit form (tags, importance, body)
│   ├── ImageNodeRenderer.tsx       # Image preview with metadata
│   └── AgentBubbleRenderer.tsx     # Graph-worker bubble (status, progress, click → open)
├── panels/
│   ├── MemoryTopBar.tsx            # Search, filters, layout control
│   ├── MemoryActivityFeed.tsx      # Left: recent mem activity log
│   ├── MemoryWorkerPanel.tsx       # Right: active graph-workers
│   ├── MemoryStatsBar.tsx          # Bottom: item counts, vector coverage, dream status
│   ├── MemoryNodeDetail.tsx        # Floating detail card when node clicked
│   └── MemorySaveDialog.tsx        # Add new mem_item
├── hooks/
│   ├── useGraphData.ts             # Fetch /api/mem/graph/full, subscribe mem-changed
│   ├── useActivityPulses.ts        # mem-activity subscription + pulse decay
│   ├── useGraphWorkers.ts          # Worker CRUD + event subscription
│   ├── useViewport.ts              # Zoom/pan state + viewport rect
│   └── usePersistedLayout.ts       # Throttled position batch-PATCH
├── renderers/
│   ├── sigma-setup.ts              # Create sigma instance, custom programs
│   ├── webgl-pulse-program.ts      # Custom sigma program for pulse rings
│   └── node-color-palette.ts       # kind → color, importance → size
├── lib/
│   ├── memory-api.ts               # All REST calls
│   ├── spatial-index.ts            # Quadtree for hit testing + viewport culling
│   └── memory-types.ts             # Shared TS types

src/components/Navigation/
├── NavigationBackBar.tsx           # "← Memory Graph" floating pill
└── navigationStore.ts              # Return-to state

src/hooks/
└── useMemoryEvents.ts              # Centralized Tauri listen subscriptions
```

Approximately 30 files, 4000-5000 lines of frontend code.

### Dependencies

```json
"sigma": "^3.x",                    // WebGL graph (MIT, ~300KB)
"graphology": "^0.25.x",            // Graph data model (MIT, ~60KB)
"graphology-layout-forceatlas2": "^0.10.x",  // Force layout (MIT, ~20KB)
"react-window": "^1.8.x",           // Virtualized lists in side panels (MIT, ~8KB)
```

Plus maybe `d3-quadtree` for the spatial index (MIT, ~8KB) if graphology's built-in isn't enough.

**No tldraw (license), no cosmograph (license), no xyflow (not compatible with our zoom-layer architecture, though we steal patterns).**

Total bundle addition: ~400KB gzipped. Code-split so it only loads when the Memory panel opens.

## UX flows

### Opening the Memory panel

1. User hits `Cmd+Shift+M` or runs `codeterm mem open` (future CLI command)
2. MemoryPanel modal opens full-screen
3. Loading spinner while `/api/mem/graph/full?limit=1000` is fetched + force simulation runs OR persisted positions load
4. Canvas appears: if < 500 items, everything in rich DOM mode at zoom ~1.5. If > 500, overview mode at zoom 0.3 with "zoom in to see details" hint

### Browsing

- Mouse wheel / pinch = zoom (smoothly transitions between renderers)
- Click-drag on empty space = pan
- Click a node = select + show detail card floating next to it
- Click the same node again = toggle expanded mode (if node supports it)
- Hover shows a tooltip with summary + tags
- `Esc` deselects + closes any open detail card

### Search and filter

- Top search bar: debounced 300ms, calls `/api/mem/search?q=...` and marks matching node ids
- Matching nodes stay fully opaque, non-matching fade to 20% opacity
- Zoom auto-fits to show all matching nodes when search is submitted
- Filters (kind, area, scope): same effect, applied locally on the graph

### Editing a mem_item

- Click a node to select
- Click `Edit` in the detail card OR double-click the node
- Node morphs to `form` renderer in-place (if it wasn't already)
- Tags become chip inputs, importance becomes a slider, body becomes a textarea
- `Save` button → `PATCH /api/mem/items/{id}` → mem-changed event → node re-renders with new data
- `Cancel` reverts

### Spawning a graph-worker

Two ways:

1. **From the Memory panel UI** — "Spawn worker" button in the top bar opens a modal:
   - Intent text input
   - Task prompt textarea
   - Agent provider selector (claude/codex)
   - Parent: auto-filled to "user:memory-panel"
   - On submit → `POST /api/mem/workers` → worker bubble appears in the right panel

2. **From a running agent** — any agent in any pane can spawn a graph-worker by running:
   ```bash
   codeterm mem worker spawn --intent "..." --task "..."
   ```
   The worker's `spawned_by` becomes the calling agent's pane_id. When the worker reports, the report is auto-injected into the spawning pane (like `codeterm agent report`).

3. **From a cron** — the dreaming cron already spawns an agent. We change it to spawn a graph-worker instead (via a config flag), so the nightly dreaming run shows up in the memory panel as a visible worker bubble. User can watch it consolidate in real time.

### Worker bubble UX

Right panel shows active workers:

```
┌─ Active workers (2) ───────────────┐
│                                    │
│  ● gw_abc123                       │
│  Intent: dedup area=db             │
│  Status: ▰▰▰▱▱ analyzing            │
│  Progress: 42/128 scanned          │
│  Spawned by: cron:mem-dreaming     │
│  Touched: 42 nodes                 │
│  [→ Open terminal]                 │
│                                    │
│  ● gw_def456                       │
│  Intent: find all auth skills      │
│  Status: ▰▰▰▰▱ writing              │
│  Progress: Found 7 candidates      │
│  Spawned by: claude:main-pane      │
│  Touched: 15 nodes                 │
│  [→ Open terminal]                 │
│                                    │
└────────────────────────────────────┘
```

Color dot left of the worker ID is the worker's assigned color — matches the pulse color on the graph. User can visually correlate "this worker is touching these nodes" by color.

Click `→ Open terminal`:
1. Navigation state saves current memory panel state
2. Memory panel closes
3. The worker's hidden pane becomes visible + focused (temporarily overriding `show_in_tabbar = false` for this session)
4. Floating back bar appears: `← Memory Graph`
5. User watches the worker's terminal output live
6. Click back → return to memory panel with restored state

### Worker completion

When the worker calls `codeterm mem worker report "summary"`:

1. Backend updates worker status to `done`, stores the report text
2. Emits `graph-worker-reported` event with payload `{worker_id, report, touched_nodes}`
3. Frontend bubble shows a green check + the report text
4. After 30 seconds, the bubble auto-fades + the worker pane auto-closes (PTY killed, pane removed from mux)
5. If the worker had a parent pane (spawned from an agent), the report is ALSO delivered to that parent's pane via the existing `codeterm agent report` mechanism — so the spawning agent naturally gets notified that its subordinate finished

### Jumping to agent + back

```
User in MemoryPanel
    │
    │ click "→ Open terminal" on worker bubble
    │
    ▼
navigationStore.setReturnTo({
  view: 'memory-panel',
  state: { zoom: 1.2, panX: 120, panY: -50, selectedNodeId: 'skill_x' }
})
    │
    │
    ▼
MemoryPanel.close()
mux.focus_pane(worker.pane_id)  // pane temporarily shown
    │
    │
    ▼
App.tsx renders <NavigationBackBar/>  because returnTo is set
User sees the worker's terminal output + back pill top-center
    │
    │ click back
    │
    ▼
navigationStore.back()
    │
    │
    ▼
mux.unfocus_pane(worker.pane_id)  // pane re-hidden
MemoryPanel.open(returnTo.state)  // opens with restored zoom/pan/selection
<NavigationBackBar/> hides itself
```

## Implementation phases

### Phase A — Foundation (no UI yet)

- Backend event emission for mem-activity (wrap MemManager methods)
- `GraphWorkerRegistry` with persistence
- `hidden` + `show_in_tabbar` pane flags
- Schema: `graph_x / graph_y` columns
- New REST endpoints: `/api/mem/graph/full`, `/api/mem/workers`, `/api/mem/positions`
- CLI: `codeterm mem worker spawn/list/progress/report/kill`
- CLI header injection for `X-CodeTerm-Pane-Id` / `X-CodeTerm-Worker-Id`
- Tauri commands equivalents for all of above (invoke from frontend)
- Inline tests for each

**Deliverable:** full backend API for the graph + workers, no visible change for the user yet. A worker can be spawned from CLI and it runs as a hidden pane, reports progress via CLI. Event stream is live but no one listens.

~8-10 hours of work.

### Phase B — Canvas MVP with DOM-only renderer

- MemoryPanel modal skeleton
- Graphology store + initial data fetch
- DOM renderer: a single `basic` node type (dot + label), viewport transform layer, wheel zoom, drag pan, click select
- Force simulation via graphology-layout-forceatlas2, cool-down after convergence, positions persisted
- `mem-changed` event subscription: incremental add/update/delete
- Simple search box with non-match fade
- Stats bar at bottom

**Deliverable:** full graph of all mem items, zoomable, pannable, with live updates. Nothing clickable yet beyond selecting. Works well up to ~1k items.

~5-6 hours.

### Phase C — Rich node kinds + detail card

- `expanded`, `form`, `image`, `agent` renderers
- Node kind auto-detection (has body → expanded, has image markdown → image, is graph-worker → agent)
- Floating detail card component
- Edit mode via form renderer
- Delete / Copy id actions

**Deliverable:** clicking nodes gives rich info, editing works inline, the UX feels like a whiteboard with smart shapes.

~5-6 hours.

### Phase D — Activity pulses + recent feed

- `useActivityPulses` hook with decay
- Canvas render loop draws pulse rings on both renderers
- Color palette assignment: `agent_provider + pane_id → stable color`
- Recent activity feed in left sidebar
- mem-activity event listener

**Deliverable:** you see live what agents are doing with mem. Pulse when they read, pulse when they write, fading rings around recently-touched nodes.

~3-4 hours.

### Phase E — Graph workers panel + navigation

- `MemoryWorkerPanel` with bubble cards
- Spawn worker dialog
- Click worker → focus pane + NavigationBackBar
- Back bar component
- Worker completion fade-out
- Dreaming cron updated to spawn as graph-worker (config-gated)

**Deliverable:** the full "live agents working on the graph" experience. User can spawn a worker, watch it touch nodes, jump into its terminal, come back. Dreaming cron is visible.

~5-6 hours.

### Phase F — WebGL overview renderer + scale

- sigma.js integration as second renderer
- Renderer swap at zoom threshold
- Area clustering at zoom < 0.3
- Spatial index + viewport culling on both layers
- Persisted layout load path (skip force sim if positions exist)
- Progressive load for > 5000 items

**Deliverable:** the full scale story. 10k items smooth. Switching zoom levels transitions smoothly between overview and rich.

~6-8 hours.

### Phase G — Polish + tests

- Loading states, empty states
- Keyboard shortcuts (arrow nav, Delete, /, Esc, Cmd+F for search focus)
- Error toasts
- Accessibility: focus traps, aria labels, reduced-motion variants
- TS + Rust tests for each module
- Performance profiling against a 10k synthetic dataset

~4-6 hours.

**Total: 35-45 hours of work.** The first usable thing (Phase A + B + C) is ~18-22 hours.

## Open decisions (need user input)

1. **Dreaming cron as graph-worker by default?** Or opt-in via config? Default probably yes — the whole point is visibility.

2. **Graph-worker persistence across restarts.** Workers keep running if they're still alive; their state in `workers.json` is a snapshot. But if CodeTerm restarts, the worker PTY is gone — do we auto-mark all workers as `orphaned` and let the user clean up, or do we try to resume them? Probably orphan them. We don't try to auto-resume.

3. **Worker auto-kill on idle.** If a worker hasn't emitted progress or touched mem in N minutes, should we auto-kill it? Default: no. But expose `codeterm mem worker kill-idle --since 30m` as an explicit tool for cleanup.

4. **Rate limit on mem-activity events.** If dreaming touches 1000 items in 10 seconds, we emit 1000 events. Frontend handles this fine, but if we're running mesh-sync later, this could flood the wire. Solution: batch events when they're from the same source within a 100ms window. Apply at backend.

5. **Color palette size.** 10 colors rotating is enough for 10 concurrent workers. If user is running 20+ workers at once, colors repeat — is that ok? I say yes; it's rare and visually confusing anyway.

6. **Node editing in-place vs dialog.** Form renderer mutates the node in place. Alternative: click edit → open a modal dialog. In-place is slicker but takes more screen real estate when many nodes are being edited. I lean in-place for v1, add a "pin edit dialog" option later.

7. **Graph worker reuse of regular agent infra vs dedicated track.** We reuse the same agent spawn / PTY / session system — a graph-worker is a regular agent with a flag. This is simpler than building a parallel track, but means the graph-worker can technically spawn sub-workers of its own, send text to other panes, etc — things we may not want. We can enforce limits via the worker's task prompt ("you are not allowed to spawn sub-agents or read other panes, only work with mem") and document this rather than locking it in code. I lean toward prompt-based discipline for v1, code-enforced for v2 if it becomes a problem.

## Risks

1. **Force simulation on 10k items is slow first time (~10s).** Mitigation: show a progress bar during initial layout; persist positions so subsequent loads skip it.

2. **Graphology + sigma memory overhead at 50k nodes is ~200MB.** Mitigation: progressive load, only keep in-memory the cluster contents that are visible.

3. **Dual renderer architecture is tricky.** Risk of flicker during renderer swap, desynced state between the two. Mitigation: single graphology store is the source of truth, both renderers are dumb views. Swap is a CSS opacity cross-fade over 200ms.

4. **Hidden panes leak resources.** A stuck graph-worker whose agent crashed but whose PTY is still alive. Mitigation: watchdog checks every 60s whether worker is making progress (progress_text updated in last N minutes). Auto-orphan stale workers after 10 minutes of silence.

5. **Bundle size.** +400KB gzipped is significant. Mitigation: code-split the entire MemoryPanel behind a dynamic import. The main bundle stays lean; the graph viz only loads when the user opens the panel.

6. **tldraw license temptation.** tldraw's shape system is beautiful but the license is non-OSI. I'm NOT vendoring any tldraw code. We study their API design online, re-implement patterns we like. Enforcement: I won't `git clone` tldraw locally.

## Concrete first commit after approval

```
feat(mem-visuals): Phase A foundation

- mem/activity.rs — ring buffer + emit API
- mem/workers.rs — GraphWorkerRegistry
- Schema: graph_x / graph_y on mem_items
- REST: /api/mem/graph/full, /api/mem/workers, /api/mem/positions
- CLI: codeterm mem worker spawn/list/progress/report/kill
- CLI: X-CodeTerm-Pane-Id / X-CodeTerm-Worker-Id header injection
- Pane flags: hidden, show_in_tabbar
- Tests for each module
- No UI changes yet
```

Then B, C, D, E, F, G as separate commits with incremental deliverables.

## Explicit not-doing

- **No 3D** — Reagraph does it, looks cool, but adds Three.js (~800KB) and doesn't add real value over 2D for our use case.
- **No collaborative multiplayer** — tldraw has cursors-of-other-users, cool but we're single-user per machine. Mesh-sync lands eventually but not visually in the graph for v1.
- **No animation of the force simulation** — simulation runs silently, user sees the final layout. Animating the settle looks cool but is expensive and distracting.
- **No inline LLM chat on a node** — "ask a question about this skill" via Claude. Cool but scope creep. Use the regular agent spawn flow for now.
- **No mobile / touch** — desktop mouse + keyboard only.

## Summary of what makes this architecture "solid under scale"

1. **Single source of truth** for graph data (graphology store), multiple dumb renderers
2. **Viewport-based rendering** at every scale tier, never "draw all N items at once"
3. **Persisted layout** means the expensive part (force sim) runs once per new item, not every load
4. **Dual renderer switch** at zoom level means the expensive React DOM rendering only happens for ≤ 200 visible items, regardless of total count
5. **Server-side spatial pagination** means the frontend never has to hold 50k items in memory
6. **Event-driven updates** (incremental, targeted) mean topology changes don't trigger full re-renders
7. **Graph-worker agents are plain agents with flags** — we don't build a parallel concurrency system; we reuse the mux and add visibility
8. **Bundle is code-split** — the complex graph UI doesn't bloat the main bundle for users who never open Memory panel
9. **Node kinds are pluggable** — adding new node types (video preview, form with custom fields, embedded CodeMirror editor) is one file each, no changes to the renderer
10. **Data model is wire-compatible with future mesh sync** — `mem_items.graph_x/y` is per-machine local, but `mem_items.id/kind/relations` are sync-friendly; mesh replication lands later without breaking the graph view

---

**This is a 35-45 hour chunk of work. Phase A+B+C (foundation + canvas + rich nodes) is 18-22 hours and gives a usable debug-visible memory graph. Phases D+E (pulses + workers) bring the "live agents working on it" vision. Phase F (scale) is only needed when mem grows past a few thousand items — we can defer until that's a problem.**

**Approve, redirect, or request changes. Nothing is touched in the codebase until you sign off.**
