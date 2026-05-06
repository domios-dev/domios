# Project Map — v1.2.0 Spec

**Status**: spec, May 2026
**Supersedes**: the earlier `codeterm-token-tools-v1.2.0.md` (snip + hook layers were dropped — see commit log for that pivot).
**Companion**: `project-map-research.md` in this directory holds the prior-art survey, deep-dives, viz benchmarks, and references-cloned table. **Read it first**. This spec assumes its conclusions.

## Pitch (one sentence)

> Every CodeTerm workspace except `General` quietly builds a **bird's-eye architectural graph** of its project — files, classes, functions, and the data-flow edges between them — persisted to `<workspace>/.codeterm/map/v1/`, queryable from CLI, and explorable on a polished React canvas where the user (or an agent) can select nodes, ask "what is this?", and **dispatch the selection straight into a chosen pane** as a piece of work.

This is **not** a codeviewer. It is a **block-level operator surface**. You think and act in modules and connections, not lines of code.

## Hard scope rules

- **Per-workspace, automatic.** Opens a new workspace → indexer kicks in in the background. No "press build."
- **`General` workspace excluded.** It is a meta-workspace, not a project. Skip.
- **User-disable per workspace.** Settings toggle: "Build project map for this workspace." Default: on. Stored in workspace config.
- **No backward-compat.** Schema versioned via dir suffix (`.codeterm/map/v1/`) — bump to v2 = blow it away, no migrations. Project rule.
- **No General-style global indexing.** Each workspace's map lives inside its own folder. Mesh peers can request maps over the wire, but persistence is per-workspace.
- **All artifacts in English** (codebase rule).

---

## Architecture summary (from research doc)

```
                ┌──────────────────────────────────────────┐
                │  Tier-0 indexer  (always on)             │
                │   tree-sitter + aider tags.scm           │
                │   Pygments fallback                      │
                ├──────────────────────────────────────────┤
                │  Tier-1 indexer  (opt-in, slow)          │
                │   SCIP loaders (TS / Py / Rust / Java…)  │
                │   LSP queries when server is alive       │
                ├──────────────────────────────────────────┤
                │  Bridge extractors  (handcrafted)        │
                │   Tauri command pairs   (FLAGSHIP)       │
                │   HTTP routes (FastAPI/Express/Axum/…)   │
                │   Codegen detection (tRPC/GraphQL/OAS)   │
                └─────┬────────────────────────────────────┘
                      ▼
                ┌─────────────────────────────────┐
                │  SQLite store                   │
                │  .codeterm/map/v1/index.sqlite  │
                │  SourceTrail-shaped schema      │
                │  + RepoGraph-typed edges        │
                └─────┬───────────────────────────┘
                      ▼
       ┌──────────────┴─────────────┐
       ▼                            ▼
┌─────────────┐            ┌──────────────────┐
│ LanceDB     │            │  Query layer     │
│ chunk       │            │  (Rust + REST)   │
│ embeddings  │            │   focus subgraph │
│ (parallel)  │            │   k-hop expand   │
└─────────────┘            │   semantic seed  │
                           │   reflexion check│
                           └────────┬─────────┘
                                    ▼
                    ┌────────────────────────────┐
                    │  React canvas              │
                    │  Sigma.js + Graphology     │
                    │  ELK in WebWorker (DSM)    │
                    │  focus mode (SourceTrail)  │
                    │  selection → send-to-pane  │
                    └────────────────────────────┘
```

Justifications for every choice are in `project-map-research.md` §7.2. Don't second-guess them without re-reading that section first.

---

## UI vision (the heart of this spec)

The user demands a **polished, deliberate** UI. This is the part where we don't cut corners. Use the **frontend-designer skill** if available; if not, study `references/codecharta/` (live Three.js viewer with thoughtful tooling) and `references/aider/` for inspiration, then design it cleanly.

### 1. Where the map lives in the workspace tree

The workspace folder in CodeTerm's left panel gets a **child node** when a map exists for that workspace:

```
▼ workspace foo
   ▸ tab: Terminal
   ▸ tab: Editor
   ▸ Project Map  ←  click here
```

For v1, **acceptable temp affordance**: a `Show Map` action under the workspace's `…` overflow menu. Polish in v1.1. The architectural seam is: opening the map is a UI command that mounts a full-canvas pane in the active tab area (or a dedicated sliding panel — UX-decide based on what looks best with the existing CodeTerm chrome).

### 2. The canvas itself

Layout: full-pane Sigma.js canvas, side panels overlay style:
- **Left rail (collapsible)**: search box (keyword + semantic), filter chips (kind: file / class / function; language; cluster), bookmarks, history (back/forward). SourceTrail's three-pane is the inspiration; condense to overlays so the canvas feels primary.
- **Bottom strip**: minimap (Graphology has one), zoom level, layout switcher (force / DSM-layered / 3D-city later), quick-actions (refresh, refocus root).
- **Right rail (collapsible)**: details panel for the currently focused / selected node — kind, signature, docstring (from tree-sitter or LSP hover), source range link (opens the file at that line in the right pane), incoming / outgoing edges grouped by type, "ask LLM" button.
- **Empty state**: clean illustration + two CTAs ("Indexing…" with progress, or "Index this workspace now" if user disabled it).

### 3. Focus mode (cribbed from SourceTrail)

Default canvas behaviour at any time: **one node is in focus**. The graph rerenders so only the focused node + 1-hop neighbours (or 2-hop on a slider) are visible. Other nodes faded to 5% opacity.

- Click any node → it becomes the new focus, animates to centre, neighbours fan out.
- Hover any node → tooltip with name + kind, no recenter.
- `Z` / `Shift+Z` → browser-style back / forward through focus history.
- Bookmark current focus with `B`.
- Edges colored by RepoGraph kind (`invoke` solid, `contain` dotted, plus future `bridge` highlighted).
- Bundled edges between two parent nodes when child-edge count > 5 — single thick edge with a count chip; click to expand.
- Striped/hatched node = referenced but not defined in this workspace.

### 4. Selection and sending to a pane (the operator surface)

This is where CodeTerm becomes more than a viewer. **Selection model**:
- Click node = focus (single-select for navigation).
- Cmd/Ctrl-click = add to **multi-selection** (separate from focus).
- Marquee drag = box-select.
- Selected nodes get a thick highlighted ring, count badge floats near the cursor.

**When selection is non-empty**, a floating action bar slides up from the bottom of the canvas:

```
┌────────────────────────────────────────────────────────────────┐
│  3 nodes selected                                              │
│                                                                │
│  Action: [ Send to ▼ Orchestrator (pane abc1234) ]            │
│  Message: ┌──────────────────────────────────────────────┐    │
│           │ optimize this — too much coupling here       │    │
│           └──────────────────────────────────────────────┘    │
│                                  [Cancel]   [Send  ⇧⌘↵ ]      │
└────────────────────────────────────────────────────────────────┘
```

**Behaviour**:
- The "Send to" picker defaults to the **last pane this workspace's map sent to**. Persisted per-workspace in workspace config: `last_dispatch_pane: PANE_ID`. If that pane no longer exists (closed or workspace doesn't contain it anymore), silently fall back to picker open.
- If the workspace has exactly one Orchestrator pane, the picker auto-fills it on first selection — no extra click. "Send" is one click away.
- If multiple eligible panes (Orchestrator and active Leads — workers are excluded as send targets, they don't take new tasks), show as a dropdown.
- "Send" composes a message with: (a) the user's typed prompt, (b) a structured block listing the selected nodes (`path:line | signature`), (c) optionally the focused subgraph as a small dot/ascii rendering for context.
- The composed message goes to the chosen pane via `codeterm send-text --pane <id>` — exactly like a normal chat input. The agent receives it as user input and acts.
- After send: selection clears, action bar collapses. A small toast: `Sent 3 nodes to pane abc1234`.

**From-the-other-direction flow** (chat → map): when the user is typing in a chat pane and references the map, allow paste-in shortcut. v1.0 ergonomics: user clicks "Copy as block" on the action bar instead of "Send"; clipboard receives the same structured payload. v1.1 polish: `@map` mention in chat input opens a mini map picker.

### 5. The "what is this?" affordance

Right rail shows the selected node's metadata. The "Ask LLM" button on the right rail composes a question from:
- node identity (path + symbol)
- signature + docstring
- 1-hop neighbours summarised
- relevant source range (read on demand)

…and routes the question into the workspace's default LLM (the orchestrator). This is *separate* from the multi-selection send-to-pane flow; this is single-node lookup.

### 6. CLI surface

```bash
codeterm map index                                  # index current workspace
codeterm map index --workspace WS_ID                # index specific workspace
codeterm map status [--workspace WS_ID]             # show last index time, node/edge count, errors
codeterm map search "auth flow" [--limit 20]        # hybrid (graph + vectors) for this workspace
codeterm map show [--workspace WS_ID] [--node ID]   # opens canvas in CodeTerm UI, optionally focused on node
codeterm map export --format json|dot|graphology    # for piping to scripts / external viz
codeterm map fitness                                # run reflexion check vs architecture.yml (later phase)
codeterm map config get|set indexing.enabled        # per-workspace toggle
```

The CLI must be **path-aware**: when no `--workspace` is given, CLI determines the workspace by walking up the cwd until it finds `.codeterm/map/` or a workspace marker. If outside any workspace and no flag → friendly error pointing the user at `--workspace`.

Mesh: every `codeterm map *` command accepts `--mesh PEER` to operate on a remote peer's workspace, same convention as the rest of the CLI.

### 7. Agent integration

- `Project Map` skill installed to `~/.claude/skills/codeterm-project-map/` (auto-via mem skill add). It documents:
  - Map exists per workspace (except `General`).
  - How to query: `codeterm map search`, `codeterm map status`, `codeterm map show --node ID`.
  - When to use: before going on a Read-spree across many files; for "where is X used?"; for "give me an overview of this codebase."
- AGENTS.md auto-injects a one-line nudge when the workspace has a built map: `Map available — try \`codeterm map search\` or open via "Show Map" before grepping.`
- The General Agent gains a soft rule: when an agent asks a structural question about a project workspace, prefer hitting the map over recursive Reads.

---

## Implementation phases (Mac-Air, this is your runway)

The order below is from `project-map-research.md` §7.4, refined for our UI emphasis. **Mac-Air must produce a per-phase detailed plan** before writing code for that phase, validated against the cloned references. See "Mac-Air mandate" at the bottom.

| # | Phase | Deliverable | Key references |
|---|-------|-------------|----------------|
| 1 | **Tier-0 indexer + SQLite store** | `cli/src/cmd/map/` Rust crate; tree-sitter + aider's `tags.scm` files vendored; SourceTrail-shaped schema; basic index/status/search CLI; mtime-keyed cache. | `references/aider/aider/repomap.py`, `references/aider/aider/queries/*.scm` |
| 2 | **Per-workspace auto-build + watcher** | New workspace event triggers index. notify crate watcher for incremental updates. Settings toggle. `General` excluded. | none |
| 3 | **Sigma.js canvas + focus mode + right-rail details** | React panel; opens via Show Map; force layout; click→focus→1-hop neighbours; right-rail metadata; back/forward. | `references/codecharta/` (UI patterns), `https://www.sigmajs.org`, `https://graphology.github.io` |
| 4 | **Selection + send-to-pane action bar** | Multi-select model; floating action bar; pane picker w/ persisted default; structured-block composer; clipboard fallback. | none — handcrafted UX |
| 5 | **Tauri-command bridge extractor (flagship)** | Detects `#[tauri::command] fn …` (Rust) and `invoke('…')` (TS); emits cross-edges; renders distinctly on canvas; demos itself in **this** repo. | `cli/src/cmd/map/bridge/tauri.rs` |
| 6 | **LanceDB chunk vector layer** | Tree-sitter AST-aware chunking (~500 tokens); LanceDB persistence at `.codeterm/map/v1/vectors.lance/`; `codeterm map search` becomes hybrid (vectors → k-hop expand). | `references/continue/core/indexing/`, `https://lancedb.github.io/lancedb/` |
| 7 | **PageRank + Leiden community detection** | Personalized PageRank for node sizing; `graphology-communities-louvain` for cluster groups; render as colored backdrop. | aider repomap.py (PageRank), `graphology-communities-louvain` |
| 8 | **Layered DSM view (ELK)** | Alternate layout: Sugiyama via `elkjs` in WebWorker; toggle in canvas chrome. | `https://eclipse.dev/elk` |
| 9 | **Tier-1 SCIP loader** | Detect `index.scip` files; loader translates `Document/Occurrence/SymbolInformation` → our schema; opt-in "deep index" UI. | `references/scip/scip.proto`, scip-typescript / scip-python / rust-analyzer scip CLIs |
| 10 | **HTTP-route bridge extractor** | Generalize Tauri pattern to FastAPI/Express/Axum/Flask routes vs `fetch`/`axios`/`useQuery` URL literals; LLM-fuzzy-match for the long tail. | none — handcrafted, study Respector + RESTSpecIT |
| 11 | **Reflexion-model fitness layer** | `architecture.yml` schema; reflexion check; render confirmed/violation/missing edges; CI exit code. | `https://www.archunit.org/userguide/html/000_Index.html` |
| 12 | **3D code-city secondary mode (optional polish)** | @react-three/fiber view of the same graph as buildings/districts. | `references/codecharta/` |

Phases 1–5 are **must-ship** for v1.2.0. 6–9 are **strong-want**. 10–12 are **post-1.2**.

---

## What we explicitly DO NOT build

- The original `codeterm snip` per-tool truncators. Dropped.
- The universal `codeterm hook <event>` layer. Dropped.
- Per-pane token tracking (`per-pane-token-tracking-v1.3.0.md`). Dropped.
- Per-language semantic indexers in-house (we adopt SCIP indexers as opt-in tier-1).
- Custom DSL for queries (NDepend's CQLinq, Atomist). LLMs replace these.
- Cross-language symbol unification beyond what bridge extractors + LLM-glue achieves. Honest pitch only.
- General-purpose IDE go-to-def. The map is overview + semantic-question, not nav.

---

## Mac-Air mandate

The Mac-Air orchestrator is taking this from here. Its instructions are encoded in the spawn task — but the spec-level expectations are:

1. **Read both plan docs** (`project-map-research.md` and this one) end-to-end. Re-clone `references/` (gitignored) by re-running the commands captured in §8 of the research doc.
2. **For each phase**, before any code, write a per-phase implementation plan into `docs/plans/project-map/<NN>-<phase>.md`:
   - Files to create / edit (with module boundaries — respect the 800-line discipline)
   - Concrete data shapes (Rust structs / SQL DDL / TS interfaces)
   - Acceptance criteria (test scenarios + perf targets where relevant)
   - Patterns being borrowed from references — cite by path:line
   - Risks & open questions
3. Validate each per-phase plan against references — if a pattern can be re-used wholesale (aider's PageRank, Continue's chunking), use it; if not, document why and design from scratch.
4. **Use the frontend-designer skill** for phases 3, 4, 8, 12 (anything visual). If the skill is not present, study `references/codecharta/` and write a design doc before coding.
5. Implement phase-by-phase. After each phase: tests, build green (`./build.sh check`, `./build.sh test`, `cargo test --lib` in cli/), commit on this branch (`feat/project-map`), push.
6. Spawn Workers for parallel sub-tasks where it actually helps (e.g. one Worker per language tags.scm port). Do not spawn for serial work.
7. Update `references/aider/aider/queries/*-tags.scm` files into our crate at `cli/src/cmd/map/queries/` — they are Apache-2.0, attribution in `cli/src/cmd/map/queries/ATTRIBUTION.md`.
8. **Stop and ask** the user (Andrey) before:
   - Adding a new heavy dependency (e.g. switching from SQLite → DuckDB)
   - Materially changing the recommended stack from the research doc
   - Building anything in the "DO NOT build" list above

When v1.2.0 (phases 1–5) is feature-complete and built green, run:
```
codeterm bridge send --to <ANDREY_TELEGRAM_ID> --service telegram "project-map v1.2.0 phases 1-5 ready on feat/project-map @ Mac-Air — please review"
```
…and report up via `codeterm agent report`.

<!-- revision: 1 -->
