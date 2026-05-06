# Phase 3 — Sigma.js canvas + focus mode + right rail

**Status**: plan
**Owner**: Mac-Air (orchestration), worker(s) for sub-pieces
**Depends on**: Phase 1 (SQLite store), Phase 2 (auto-build).

## Goal

Render the project-map graph on a polished Sigma.js canvas inside CodeTerm, with the SourceTrail-style focus mode and a right-rail details panel. Open via the workspace overflow menu's "Show Project Map" action.

The frontend already vendors `sigma`, `graphology`, `@sigma/edge-curve` (see `package.json`). No new heavy deps.

## Hard-scope cuts

- **No selection / multi-select / send-to-pane.** That's Phase 4.
- **No PageRank-driven sizing, no Leiden clusters, no edge bundling.** Phase 7 / 8.
- **No dedicated "Project Map" workspace tree child node.** v1 affordance: "Show Map" item under workspace overflow menu (matches §1 of the spec — "v1 acceptable temp affordance"). v1.1 polishes to native tree child.
- **Search rail postponed** — substring search lives only in CLI for now; canvas left rail has filter chips + back/forward; the search overlay lands later in Phase 3.5.

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│ ProjectMapPanel (Browser-pane content, 100% width/height)          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ TopBar: workspace name • [refresh] [layout: force | DSM…]    │  │
│  └──────────────────────────────────────────────────────────────┘  │
│  ┌────────────┐  ┌──────────────────────────────┐  ┌────────────┐  │
│  │ LeftRail   │  │ Sigma canvas                 │  │ RightRail  │  │
│  │ filter     │  │  - force layout              │  │ NodeDetail │  │
│  │ chips      │  │  - focus mode (1-hop)        │  │ - kind     │  │
│  │ back/fwd   │  │  - hover tooltip             │  │ - signature│  │
│  │ history    │  │  - click → focus             │  │ - source   │  │
│  │ bookmarks  │  │  - bundled edges (count chip)│  │ - edges    │  │
│  └────────────┘  └──────────────────────────────┘  └────────────┘  │
│  Bottom strip: minimap • zoom % • status                            │
└────────────────────────────────────────────────────────────────────┘
```

All overlays mount inside the canvas pane, **collapsible**. The canvas itself is the primary surface.

## Backend additions (Rust, in CLI's existing map module + new daemon endpoints)

### 1. Graph query in `cli/src/cmd/map/graph.rs`

A pure-Rust query layer over the Phase-1 SQLite, callable from both CLI (export) and the daemon's REST API.

```rust
pub struct GraphQuery<'a> {
    pub workspace_root: &'a Path,
}

#[derive(Serialize)]
pub struct GraphPayload {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphStats,
}

#[derive(Serialize)]
pub struct GraphNode {
    pub id: i64,
    pub kind: String,
    pub display_name: String,
    pub serialized_name: String,
    pub path: Option<String>,
    pub start_line: Option<i64>,
    pub start_col: Option<i64>,
    pub end_line: Option<i64>,
    pub end_col: Option<i64>,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub source: i64,
    pub target: i64,
    pub kind: String,            // contain | invoke | bridge (later)
    pub weight: f64,
}

#[derive(Serialize)]
pub struct GraphStats { pub files: i64, pub nodes: i64, pub edges: i64 }

impl<'a> GraphQuery<'a> {
    pub fn full_graph(&self) -> rusqlite::Result<GraphPayload>;
    pub fn focus_subgraph(&self, node_id: i64, k_hops: u32) -> rusqlite::Result<GraphPayload>;
    pub fn neighbours(&self, node_id: i64) -> rusqlite::Result<NeighbourSummary>;
}
```

`full_graph()` returns the entire workspace graph as a flat list. Realistic ceiling for Phase 3: ≤5k nodes, well within Sigma.js. If the count exceeds a soft cap (default 5k), the API returns a partial payload with `truncated: true` flag — frontend treats it as "you must focus a node first."

`focus_subgraph(id, k_hops)` collects the node + its k-hop neighbourhood via BFS over the `edge` table.

### 2. CLI: `codeterm map export`

```
codeterm map export [--format json|graphology] [--workspace WS_ID] [--node N] [--k-hops N]
```

`json` format = flat `{nodes, edges, stats}` (the GraphPayload above).
`graphology` format = the Sigma-ready Graphology serialised JSON.

This makes the data layer testable from shell + scriptable for the user.

### 3. Daemon REST: `/api/map/graph`

```
GET  /api/map/graph?workspace=WS_ID                  # full graph
GET  /api/map/graph?workspace=WS_ID&node=NODE_ID&k=1 # focus subgraph
GET  /api/map/node/{node_id}?workspace=WS_ID         # full metadata for one node
```

Implementation: `src-tauri/src/api/map_routes.rs` (new), 3 handlers, each opens the workspace's SQLite and runs the queries above. Workspace path resolved from `workspace_id` via mux.

The daemon does NOT cache — SQLite reads are fast enough on these volumes (5k nodes / 10k edges = single-digit-ms).

### 4. Tauri command alias

For frontend, add a Tauri command `map_graph_get` in `src-tauri/src/commands.rs` that wraps the same logic — so React can call `invoke()` without reaching for the REST API. Cleaner than relying on the REST inside the same process.

## Frontend additions

### Files

```
src/components/ProjectMap/
  ProjectMapPanel.tsx              # entry point, renders TopBar + 3-pane layout
  ProjectMapCanvas.tsx             # Sigma + Graphology, focus mode, hover, click
  RightRail.tsx                    # node detail panel + "Ask LLM" button (stub)
  LeftRail.tsx                     # filter chips, back/forward, history, bookmarks
  TopBar.tsx                       # workspace name, refresh, layout switcher
  BottomStrip.tsx                  # zoom %, status pill
  hooks/useProjectMapGraph.ts      # fetches /api/map/graph, returns Graphology instance
  hooks/useFocusMode.ts            # focus history (Z / Shift+Z), 1-hop subgraph
  hooks/useNodeDetail.ts           # fetch detailed metadata for selected node
  types.ts                         # GraphNode, GraphEdge, NodeDetail TS interfaces
  styles.ts                        # color tokens (re-use existing CodeTerm theme vars)
```

All files ≤ 350 lines. Pattern-copy from `src/components/MemoryPanel/MemoryCanvas.tsx` (419 lines — established Sigma+Graphology recipe in this codebase).

### State management

- Panel-local `useState` + `useRef` (mirrors MemoryPanel; no new global store).
- Focus history: a small ring buffer of `nodeId` + `timestamp`.
- Filter chips: kind set, language set — stored in a `useState<{ kinds: Set<string>; langs: Set<string> }>`.

### Visual style

Re-use existing CodeTerm theme tokens (`--ct-bg`, `--ct-fg`, `--ct-accent`, `--ct-border`, `--ct-active`, `--ct-hover`, `--ct-yellow`, etc — confirmed in `WorkspaceItem.tsx`). No new colors. Follow the established compact / mono aesthetic — no big shadows, no rounded-3xl cards, dense spacing.

Inspired by `references/codecharta/visualization/app/` for layout chrome (rail panels, detail pane).

### Interaction states

| State | Visual |
|---|---|
| Hover node | Tooltip with name + kind, no recenter |
| Click node | Becomes focus; canvas animates; 1-hop neighbours stay full opacity, others fade to 5% |
| Focus + filter chips active | Filter applies on top of focus — only matching kinds visible |
| Empty graph | "Indexing..." with progress, or "Index this workspace now" CTA |
| Indexing in progress | Status pill in BottomStrip: "Indexing 312/432..." (data via SSE, deferred) — Phase 3.5 |
| Truncation | Banner: "Graph too large; pick a starting node to focus." |
| Error | Red status pill; click to copy error |

### Wiring

1. **WorkspaceItem.tsx** gets a new menu item: "Show Project Map" — only visible when `workspace.cwd` is set and the workspace name is not "General". Action: dispatch a frontend event `OPEN_PROJECT_MAP` carrying `{ workspaceId }`.
2. **App-level handler** (likely in `App.tsx` or wherever browser-pane mount lives) listens for that event and creates a `Browser` pane whose URL is `app://project-map/<workspaceId>` (or, since we don't have a router on Browser, creates the pane and slots ProjectMapPanel directly).

   The cleanest approach: a new `PaneType::ProjectMap { workspace_id: String }` in the multiplexer, with frontend matching on it to render the panel. Skip routing complexity.
3. **CLI deeplink:** `codeterm map show [--workspace WS_ID] [--node NODE_ID]` — invokes a new daemon endpoint `/api/map/show` that creates the pane via service layer. Same Tauri command also fires from the frontend menu.

### `PaneType::ProjectMap` addition

Adding a third pane type (currently `Terminal`, `Browser`) is a small but cross-cutting change:

```rust
pub enum PaneType {
    Terminal,
    Browser,
    ProjectMap,
}
```

Pane gets a new field on the existing `Pane` struct: `project_map_workspace_id: Option<String>` (#[serde(skip_serializing_if = "Option::is_none", default)]).

Frontend's pane renderer (search the existing `Browser` rendering site) gains a third branch that mounts `<ProjectMapPanel workspaceId={…} />`.

## Acceptance criteria

**Functional**
- "Show Project Map" appears under the workspace's overflow menu when it has a cwd and is not General.
- Clicking opens a new pane in the active tab; pane shows the workspace's graph rendered with Sigma.
- Hovering shows tooltip; clicking a node animates focus to it; only 1-hop neighbours visible at full opacity.
- Right rail populates with node metadata: kind, name, file path + line, list of incoming + outgoing edges.
- `Z` / `Shift+Z` walks the focus history.
- `codeterm map export --format json` produces valid `GraphPayload` JSON parseable by the frontend.
- Pressing the refresh button on the top bar re-fetches the graph (after the daemon has run an index).

**Structural**
- All new TS files ≤ 350 lines.
- All new Rust files ≤ 350 lines.
- `./build.sh check` clean.
- `cd src && npx tsc --noEmit` clean.

**Tests**
- Rust: `graph.rs` unit tests with a fixture SQLite (build a tiny graph, query, assert shapes).
- Rust: `map_routes.rs` integration test or at least a query-roundtrip test.
- TS: lightweight manual smoke (no new test runner setup) — typecheck + build green = pass.

**Performance**
- Initial graph fetch + render under 600ms on this repo (5k nodes / 10k edges).
- Focus animation steady at 60fps when zooming.

## Patterns borrowed

| Pattern | Source | Where reused |
|---|---|---|
| Sigma.js + Graphology canvas | `src/components/MemoryPanel/MemoryCanvas.tsx` | `src/components/ProjectMap/ProjectMapCanvas.tsx` |
| 3-pane chrome (Search / Graph / Code) | SourceTrail (research §2.3) | TopBar + LeftRail + Canvas + RightRail layout |
| Per-symbol focus, 1-hop neighbours | SourceTrail (research §2.3, item 1) | `useFocusMode` hook |
| Workspace overflow menu integration | `src/components/Sidebar/WorkspaceItem.tsx:71-107` | new menu item there |
| REST handler shape | `src-tauri/src/api/*_routes.rs` | new `map_routes.rs` |

## Risks / open questions

1. **`PaneType::ProjectMap` ripple.** Every match site on `PaneType` needs a new arm. Estimated 6–10 sites in `multiplexer/state/`, `dispatch/mod.rs`, `service/pane.rs`, `service/output.rs` (skip), session snapshot. Cataloged before coding; if cost balloons, fall back to PaneType::Browser with a special URL prefix and route in `BrowserPane.tsx`.
2. **Sigma performance.** This repo is ~5k nodes/10k edges. Sigma's WebGL renderer should handle this comfortably. If we hit issues, reduce edge density via initial focus-mode-only render.
3. **No TS test runner registered for this module.** We rely on tsc + manual smoke. Acceptable for v1.
4. **Graphology layout cost.** Force-Atlas-2 on 5k nodes is ~1–2s in JS. Pre-compute layout server-side (random + tweaks) for the first paint, refine in-browser. Phase 3 ships with synchronous layout, Web Worker offload deferred.
5. **The "Project Map" tree-child node** is acknowledged in spec as a Phase 3.x polish item — handled by overflow menu in Phase 3.

## Out of scope (Phase 3)

- Selection model / multi-select → Phase 4.
- Send-to-pane → Phase 4.
- Tauri-command bridge edges rendering → Phase 5.
- Edge bundling, PageRank sizing, Leiden coloring → Phase 7+.
- Layered DSM / 3D city → Phase 8 / 12.
- Settings UI for the toggle → backlog.

## Done definition

- All acceptance criteria met.
- Plan + code committed on `feat/project-map`.
- Manual e2e: open this very repo's workspace, click "Show Project Map", click on `service/project_map/manager.rs` node, see right rail populate with kind=file + file_path; press Z to go back.
- `codeterm agent report "phase 3 sigma-canvas done: …"`.

## Execution plan

**Step A — Backend graph API (Mac-Air does this directly).**
1. `cli/src/cmd/map/graph.rs` — `GraphQuery` + `full_graph()` + `focus_subgraph()`.
2. CLI `map export` subcommand.
3. Daemon REST endpoint at `src-tauri/src/api/map_routes.rs`.
4. Tauri command alias.
5. Tests + build green.

**Step B — Frontend canvas (delegated to one frontend-dev Worker).**
1. Read `references/codecharta/visualization/app/` and `src/components/MemoryPanel/MemoryCanvas.tsx` for patterns.
2. Build the components listed above.
3. Wire workspace menu item and pane mounting.

**Step C — Stitch + smoke.**
- Build dev, manually verify on this repo, commit, push.
