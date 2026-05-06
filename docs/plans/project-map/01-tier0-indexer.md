# Phase 1 — Tier-0 indexer + SQLite store

**Status**: plan
**Owner**: Mac-Air
**Branch**: `feat/project-map`

## Goal

Stand up the indexing floor: tree-sitter + aider's `tags.scm` queries, persisting a SourceTrail-shaped graph to per-workspace SQLite. Expose the result through three CLI verbs (`index`, `status`, `search`). No UI, no watcher (Phase 2), no embeddings (Phase 6), no SCIP (Phase 9). All code in the CLI crate; backend (`src-tauri/`) untouched in this phase.

## Why CLI-first

The CLI already vendors `rusqlite` and is the most-isolated place to add a new dependency tree (tree-sitter parsers blow up the `src-tauri` build noticeably). Phase 2 will add a thin Tauri command that *invokes* the CLI binary (or factors the lib portion to a shared crate); whichever shape we land on then is cheaper to evolve from a self-contained CLI module than from `src-tauri/` business logic.

## Files

```
cli/Cargo.toml                                  # add tree-sitter parsers + globwalk
cli/src/args/map.rs                             # NEW — clap subcommands for `map`
cli/src/cmd/map/mod.rs                          # NEW — facade (<300 lines)
cli/src/cmd/map/store.rs                        # NEW — SQLite schema + insert/query helpers
cli/src/cmd/map/tags.rs                         # NEW — tree-sitter tag extraction (per file)
cli/src/cmd/map/lang.rs                         # NEW — extension → language registry
cli/src/cmd/map/walker.rs                       # NEW — workspace walk + ignore patterns
cli/src/cmd/map/cache.rs                        # NEW — mtime-keyed SQLite cache
cli/src/cmd/map/index.rs                        # NEW — `index` command driver
cli/src/cmd/map/search.rs                       # NEW — `search` command driver
cli/src/cmd/map/status.rs                       # NEW — `status` command driver
cli/src/cmd/map/workspace.rs                    # NEW — resolve workspace from cwd / --workspace
cli/src/cmd/map/queries/ATTRIBUTION.md          # NEW — Apache-2.0 notice for aider tags.scm
cli/src/cmd/map/queries/<lang>-tags.scm         # vendored from references/aider/aider/queries/tree-sitter-language-pack/
cli/src/cmd/mod.rs                              # EDIT — `pub mod map;`
cli/src/args/cli.rs                             # EDIT — add `Commands::Map { action }`
cli/src/args/mod.rs                             # EDIT — `pub mod map;`
cli/src/main.rs                                 # EDIT — dispatch `Commands::Map`
```

Every new file kept under 400 lines; `mod.rs` ≤ 200. No file added to anything currently >800.

## Languages shipped in Phase 1

Vendor exactly the seven that cover this repo + the user's daily mix:

| Language | Source path | Notes |
|---|---|---|
| Rust | `references/aider/aider/queries/tree-sitter-language-pack/rust-tags.scm` | Primary CodeTerm code |
| TypeScript / TSX | `typescript-tags.scm` (+ `tsx-tags.scm` if present) | Primary CodeTerm UI |
| JavaScript | `javascript-tags.scm` | TS fallback |
| Python | `python-tags.scm` | Andrey's Rekap / FastAPI work |
| Go | `go-tags.scm` | Coverage |
| Swift | `swift-tags.scm` | Binaura (lifeline reference) |
| Java | `java-tags.scm` | Coverage |

Phase 9 (SCIP loader) and a follow-up worker fan-out can extend the registry; we don't need 40 languages on day one. The registry is a single `match ext { ... }` so adding a language is one line + one `.scm` + one parser dep.

Tree-sitter parser crates pulled per language: `tree-sitter` core (0.24+) + `tree-sitter-rust`, `-typescript`, `-javascript`, `-python`, `-go`, `-swift`, `-java` (or use `tree-sitter-language-pack` if a single crate covers all — verify with `cargo search`; prefer per-grammar crates for smaller compile time when feasible).

`.scm` files **vendored verbatim**, with attribution at `cli/src/cmd/map/queries/ATTRIBUTION.md`:

```markdown
The `*-tags.scm` files in this directory are derived from
https://github.com/Aider-AI/aider (Apache-2.0).
Specifically, they are copied unchanged from
references/aider/aider/queries/tree-sitter-language-pack/.
The original LICENSE is preserved at: <link>.
```

## Data model — SourceTrail-shaped, RepoGraph-typed

Schema lives in `cli/src/cmd/map/store.rs`. Path: `<workspace_cwd>/.codeterm/map/v1/index.sqlite`.

```sql
CREATE TABLE map_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- keys: schema_version, last_index_started_at, last_index_finished_at,
--       last_indexer_version, workspace_cwd

CREATE TABLE file (
  id        INTEGER PRIMARY KEY,
  path      TEXT NOT NULL UNIQUE,        -- workspace-relative, forward-slashed
  language  TEXT,                         -- canonical lang name ("rust", "typescript", …)
  mtime_ns  INTEGER NOT NULL,             -- file mtime as nanoseconds since epoch
  size      INTEGER NOT NULL,
  indexed   INTEGER NOT NULL DEFAULT 0,   -- 0=stale, 1=fresh
  last_error TEXT
);

CREATE TABLE node (
  id              INTEGER PRIMARY KEY,
  kind            TEXT NOT NULL,                -- file | class | interface | function | method | module | macro | constant | variable
  serialized_name TEXT NOT NULL,                -- "rust:src/cmd/map/store.rs::open_index" — see "name format" below
  display_name    TEXT NOT NULL,                -- short form, "open_index"
  file_id         INTEGER REFERENCES file(id),  -- null for synthetic nodes
  start_line      INTEGER,                      -- 0-based, matches tree-sitter
  start_col       INTEGER,
  end_line        INTEGER,
  end_col         INTEGER,
  metadata        TEXT,                         -- JSON; signature, visibility, doc — Famix-style traits
  UNIQUE(serialized_name)
);
CREATE INDEX node_file_idx ON node(file_id);
CREATE INDEX node_kind_idx ON node(kind);
CREATE INDEX node_display_idx ON node(display_name);

CREATE TABLE edge (
  id             INTEGER PRIMARY KEY,
  kind           TEXT NOT NULL,                 -- invoke | contain | type_use | inherit | import | bridge
  source_node_id INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  target_node_id INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  weight         REAL NOT NULL DEFAULT 1.0,
  metadata       TEXT,
  UNIQUE(kind, source_node_id, target_node_id)
);
CREATE INDEX edge_source_idx ON edge(source_node_id, kind);
CREATE INDEX edge_target_idx ON edge(target_node_id, kind);

-- Raw tree-sitter capture occurrences. Consumed by ranking later (PageRank weight)
-- and by the canvas (highlight occurrences when a node is focused).
CREATE TABLE occurrence (
  id          INTEGER PRIMARY KEY,
  file_id     INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  node_id     INTEGER REFERENCES node(id) ON DELETE SET NULL,
  symbol_name TEXT NOT NULL,
  role        TEXT NOT NULL,                    -- def | ref
  line        INTEGER NOT NULL,
  col         INTEGER NOT NULL
);
CREATE INDEX occurrence_file_idx  ON occurrence(file_id);
CREATE INDEX occurrence_name_idx  ON occurrence(symbol_name);

-- Index health log; surfaced by `codeterm map status`.
CREATE TABLE error (
  id        INTEGER PRIMARY KEY,
  file_id   INTEGER REFERENCES file(id) ON DELETE CASCADE,
  message   TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
```

`PRAGMA user_version = 1;` for the v1 schema. **No migrations.** Future schema bump → write under `v2/`, blow away `v1/` (project rule).

### Symbol name format

```
<lang>:<rel-path>::<container>::...::<name>
```

Examples:

- `rust:cli/src/cmd/map/store.rs::open_index`
- `rust:cli/src/cmd/map/store.rs::Index::open`
- `typescript:src/components/Settings/Panel.tsx::Panel`

Phase 1 produces flat (file-only) containers because tree-sitter `tags.scm` doesn't surface enclosing scopes reliably across all grammars. Phase 7+ may upgrade by using `name.definition.<kind>` outer captures' parent ranges — out of scope here.

## tags.scm extraction

Mirrors `references/aider/aider/repomap.py:279-336`:

1. Look up parser + query for the file's language.
2. Parse to a `tree_sitter::Tree`.
3. Run query, walk captures.
4. For each capture name `name.definition.<kind>` → emit `Tag { role: Def, kind: <kind>, name, line, col }`.
   For each capture name `name.reference.<kind>` → emit `Tag { role: Ref, kind: <kind>, name, line, col }`.
5. Skip files whose tags produced **only defs and zero refs** with a warning written to `error` table — Pygments fallback (aider repomap.py:340-363) is **deferred** to Phase 6 (we'd otherwise pull a Pygments-equivalent Rust crate now for marginal gain). Phase-1 acceptance only requires Rust/TS/Python/Go to round-trip cleanly; their `.scm` queries already include refs.

`Kind` mapping (capture suffix → schema kind):

| tags.scm capture | schema `node.kind` |
|---|---|
| `class`, `struct`, `union`, `enum`, `trait` | `class` |
| `interface` | `interface` |
| `function` | `function` |
| `method`   | `method` |
| `module`, `package` | `module` |
| `macro`    | `macro` |
| `constant` | `constant` |
| `variable` | `variable` |

## Edge derivation

Phase 1 produces only `contain` and `invoke` edges (RepoGraph's pair):

- **`contain`** — `file → def_node` for every definition; later phases enrich with class→method when scope info reliable.
- **`invoke`** — for every `(name, ref)` in file `B` and `(name, def)` in file `A`, write `edge(invoke, file_A_node, file_B_node, weight=1)`. Weight bumped if (a) def is `well_named` (snake_case / CamelCase, len>3), (b) name is private (underscore-prefix, lower weight), mirroring aider's heuristic without the chat-mention layer (none in CLI context).

This mirrors aider's file-level digraph (`repomap.py:399-466`) but stored as proper rows so we can later re-rank without a full rebuild.

## mtime cache

`cache.rs` exposes:

```rust
pub struct CacheCheck { pub fresh: bool, pub last_mtime_ns: i64 }
pub fn check_file(conn: &Connection, path: &str) -> Result<CacheCheck>;
pub fn mark_indexed(conn: &Connection, path: &str, mtime_ns: i64, lang: Option<&str>) -> Result<i64>;
```

Cache hit = `file.mtime_ns == fs_mtime_ns AND file.indexed = 1`. Miss → re-extract; old `node`/`edge`/`occurrence` rows for that file deleted via `ON DELETE CASCADE`. The `error` table behaves like aider's `tags_cache_error` graceful-degradation log.

## Workspace resolution

`workspace.rs::resolve_workspace_root(cli_arg: Option<&str>) -> Result<PathBuf>`:

1. If `--workspace WS_ID` passed → call `client.call("workspace.list", None)` → match by `id` → use `cwd`. Error if no `cwd`.
2. Else walk up from `std::env::current_dir()` looking for any of:
   - `.codeterm/map/` (already-indexed sentinel)
   - `.git/` (typical project root)
   - explicit `.codeterm/workspace.json` if we add one later
3. If neither found and CLI is invoked from a CodeTerm pane (`$CODETERM_WORKSPACE_ID` env var), use that to look up cwd.
4. Otherwise: `Err("not in a workspace; pass --workspace WS_ID")`.

This is path-aware as required by the spec.

## CLI surface (Phase 1 subset)

```rust
// cli/src/args/map.rs
#[derive(Subcommand)]
pub enum MapAction {
    /// Build or refresh the project map for a workspace.
    Index {
        #[arg(long)] workspace: Option<String>,
        #[arg(long)] full: bool,            // ignore mtime cache
        #[arg(long)] verbose: bool,
    },
    /// Print map metadata: last index time, node/edge count, errors.
    Status {
        #[arg(long)] workspace: Option<String>,
    },
    /// Search nodes by display_name (Phase 1: substring + kind filter).
    Search {
        query: String,
        #[arg(long)] workspace: Option<String>,
        #[arg(long, default_value = "20")] limit: u32,
        #[arg(long)] kind: Option<String>,
    },
}
```

`map config get|set indexing.enabled` and `map show` come in Phase 2 / Phase 3 respectively. `map export`, `map fitness` ship later.

Output respects the global `--json` flag (mirror existing CLI commands at `cli/src/main.rs:320-360`).

## Acceptance criteria

**Functional**
- `cd <repo>; codeterm map index` finishes on this repo without panicking, writes `.codeterm/map/v1/index.sqlite`.
- `codeterm map status` shows non-zero `node` and `edge` counts and a `last_indexer_version` field.
- `codeterm map search send_text --kind function` returns at least one row pointing at `cli/src/main.rs` (well-known function in this repo).
- `codeterm map index` re-run with **no file changes** completes in `<= 25%` of cold-run time (mtime cache works).
- `codeterm map index --full` ignores cache.
- `--workspace WS_ID` resolves a non-cwd workspace correctly.
- `--json` output validated: `index` returns `{ "files": N, "nodes": N, "edges": N, "errors": N, "elapsed_ms": N }`.

**Structural**
- All new files ≤ 400 lines except where vendored `.scm` content (excluded by file-size rule).
- `cli/src/cmd/map/mod.rs` is a facade only (≤ 200 lines).
- `./build.sh check` clean.
- `cd cli && cargo test` green; new tests live in `cli/src/cmd/map/<file>.rs` under `#[cfg(test)] mod tests`.

**Tests**
- `tags.rs`: parses a tiny inline Rust + Python + TS string each, asserts the expected def names & kinds.
- `store.rs`: open → schema present → insert node/edge → query back.
- `cache.rs`: write file w/ mtime X → second pass with same mtime returns `fresh=true`.
- `walker.rs`: respects `.gitignore` (use `ignore` crate, same one CodeTerm already pulls transitively).
- `index.rs`: end-to-end against a fixture dir under `cli/tests/fixtures/sample-project/` containing one Rust + one TS file with a known cross-reference; assert one `invoke` edge appears.

**Performance**
- Cold index of this repo (`~30k LOC`) under 10s on Mac-Air.
- Warm (cache-hit) re-index under 1.5s.
- SQLite file under 5MB on this repo.

## Patterns borrowed from references

| Pattern | Source | Where reused |
|---|---|---|
| Mtime-keyed cache, version-suffixed dir | `references/aider/aider/repomap.py:43,233-264` | `cli/src/cmd/map/cache.rs` + `<workspace>/.codeterm/map/v1/` |
| Tag extraction via `name.definition.<kind>` / `name.reference.<kind>` | `references/aider/aider/repomap.py:279-336` | `cli/src/cmd/map/tags.rs` |
| File-level digraph w/ identifier-name join | `references/aider/aider/repomap.py:399-466` | `cli/src/cmd/map/index.rs` (edge derivation) |
| `tags.scm` queries | `references/aider/aider/queries/tree-sitter-language-pack/*-tags.scm` | `cli/src/cmd/map/queries/*-tags.scm` (vendored) |
| Typed-edge schema (`invoke`, `contain`) | RepoGraph (research §2.2) | `node.kind` / `edge.kind` enums |
| SQLite schema shape (`node`, `edge`, `file`, `occurrence`) | SourceTrail (research §2.3) | `cli/src/cmd/map/store.rs` |

## Risks / open questions

1. **Tree-sitter dep cost.** Each grammar crate is ~200–500KB compiled. Seven grammars push the CLI binary by ~2–5MB. Acceptable; revisit if it becomes painful.
   - *Open*: should we feature-flag grammars (`--features rust,ts,py`)? Decision: **no, ship them all**. Single binary, predictable behaviour.
2. **TSX vs TypeScript.** `tree-sitter-typescript` ships two grammars (`tsx`, `typescript`). Lang registry must distinguish by extension.
3. **Pygments fallback skipped.** Languages with def-only `tags.scm` (e.g. C++) currently produce zero `invoke` edges. Phase-1 doesn't ship C++; revisit when needed.
4. **Cross-platform paths.** Internal storage forward-slashed, OS-native at filesystem boundary (CodeTerm rule). Add `to_forward_slashes()` helper in `walker.rs`.
5. **Symbol-name uniqueness across overloads.** Two `fn open(...)` in the same file would collide on serialized name. Mitigation: append a 4-char hash of the byte range when collision detected; documented in `node` doctest.
6. **`General` workspace exclusion.** Enforced in Phase 2 (auto-build watcher). Phase 1 CLI commands run on whatever workspace user points at — including `General` if they explicitly do — that's fine for `index` testing.

## Out of scope (Phase 1)

- Watcher / incremental on save → Phase 2.
- Sigma.js canvas → Phase 3.
- Selection → Phase 4.
- Tauri-command bridge → Phase 5.
- Embeddings → Phase 6.
- PageRank scoring → Phase 7.
- SCIP loader → Phase 9.

## Done definition

- All acceptance criteria above met.
- Plan doc + code committed on `feat/project-map`.
- Ack report up via `codeterm agent report "phase 1 tier-0-indexer done: …"`.
