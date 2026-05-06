# Phase 5 — Tauri-command bridge extractor (flagship)

**Status**: plan
**Owner**: Mac-Air (parallel with frontend-dev worker on Phase 3 Step B)
**Depends on**: Phase 1 (extraction pipeline + SQLite store), Phase 2 (auto-build watcher).

## Goal

Connect the two halves of CodeTerm itself — Rust `#[tauri::command]` declarations and TypeScript `invoke('cmd_name', …)` call sites — by recognising them at index time and emitting cross-language **bridge** edges. This dogfoods the project-map vision in the very repo the user is editing: open the map of CodeTerm, see Rust handlers and TS callers physically wired together.

## Scope rules

- Only Tauri-command bridges. HTTP-route bridges (FastAPI / Express / Axum) are Phase 10.
- Match by **command name** string. No type-level inference, no LLM step. The Tauri convention guarantees the string is identical on both sides — we exploit that.
- Edge kind is `bridge` (a third edge kind, alongside `contain` and `invoke`).

## How Tauri commands work in this repo

Rust side:

```rust
#[tauri::command]
pub fn mux_create_workspace(...) { ... }
```

…and registered in `src-tauri/src/lib.rs::tauri::Builder::invoke_handler` (exists already).

TS side:

```ts
import { invoke } from "@tauri-apps/api/core";
invoke("mux_create_workspace", { name, cwd });
```

Bridge edge: TS call site in some file → Rust function definition. Both sides reference the same `mux_create_workspace` string. Use that.

## Indexer changes

### 1. Capture annotations during tag extraction

Today `tags.rs` emits `Tag { role, kind, name, line, col }`. Extend it to optionally carry a **container hint** that tells the bridge extractor "this Rust function has a `#[tauri::command]` attribute attached".

Approach: a second pass over the parse tree that, **for Rust files only**, walks function definitions and checks whether the parent's previous sibling chain contains an `attribute_item` whose path matches `tauri::command` or `command` under a `tauri` use.

The simplest reliable pattern: regex-string-match `#[tauri::command]` on the source within the function definition's start byte minus a 200-byte lookbehind window. False positives on commented-out code are negligible (and acceptable).

Output: a side-table `tauri_handlers: HashMap<file_id, Vec<(node_id, name)>>` written into the store.

### 2. Capture invoke() call sites

For TS / TSX / JS files, walk the parse tree for `call_expression` whose function is `invoke` (identifier or member with name `invoke`) and whose first argument is a `string_literal`. Emit those literals into a side-table `tauri_invokes: HashMap<file_id, Vec<(node_id_of_enclosing_function, command_name, line, col)>>`.

Similar simple pattern — a second pass after the standard tag extraction.

### 3. Persist to SQLite

New table `tauri_bridge`:

```sql
CREATE TABLE IF NOT EXISTS tauri_bridge (
  id           INTEGER PRIMARY KEY,
  command_name TEXT NOT NULL,
  side         TEXT NOT NULL,    -- 'handler' | 'invoke'
  file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  node_id      INTEGER REFERENCES node(id) ON DELETE SET NULL,
  line         INTEGER NOT NULL,
  col          INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tauri_bridge_name_idx ON tauri_bridge(command_name);
```

Actually the cleanest form is: skip a separate side-table; just write `bridge` edges directly into `edge` and add a `metadata` JSON `{ "via": "tauri", "command": "<name>" }`. We already have a `metadata` column. Simpler — fewer tables.

### 4. Edge derivation

After all files indexed, group `tauri_handlers` and `tauri_invokes` by command name. For each name with at least one handler and at least one invoke, write an edge:

```text
edge(kind='bridge',
     source_node_id=<invoke's enclosing function node_id>,
     target_node_id=<handler node_id>,
     weight=1.0,
     metadata={"via":"tauri","command":"<name>"})
```

Multiple handlers for one name (shouldn't happen, but be safe) → fan out to all.
Multiple call sites → fan in from all.

### 5. Rendering hints

Frontend canvas already gets edges with a `kind` field. Phase 3 colors `contain=dotted`, `invoke=solid`. Phase 5 adds `bridge=dashed-thicker-tinted` (theme color: `--ct-accent`). The Phase 3 worker is told this; if not implemented before Phase 5 ships, the bridge edges still render in Phase 3's existing style and we polish in a follow-up.

## Files

```
cli/src/cmd/map/bridge/mod.rs             # NEW — facade
cli/src/cmd/map/bridge/tauri.rs           # NEW — handler + invoke detection
cli/src/cmd/map/index.rs                  # EDIT — invoke bridge after edge derivation
cli/src/cmd/map/store.rs                  # EDIT — already supports metadata; verify edge insert path takes metadata. If not, extend insert_edge.
```

All new files ≤ 250 lines.

## Acceptance criteria

**Functional**
- Run `codeterm map index` on this very repo. Expect `bridge` edges in the resulting graph linking known pairs:
  - `cmd_send_text` (Rust) ↔ `invoke('cmd_send_text'…)` (TS) — *verify by listing pairs from the actual repo first; the names in tests must come from real call sites.*
  - At least 5 bridge edges emerge across this repo.
- `codeterm map export --workspace-root <repo>` JSON includes `kind: "bridge"` edges with `metadata.command` populated.
- A bridge edge's `metadata` round-trips through the daemon's REST endpoint.

**Structural**
- New files ≤ 250 lines.
- Existing files don't grow past 800.
- `./build.sh check` clean. `cd cli && cargo test` clean.

**Tests**
- `bridge/tauri.rs` unit tests with inline Rust + TS strings, asserting handler / invoke detection picks the right names.
- An `index.rs` test that runs the full pipeline on a tiny fixture with one Rust `#[tauri::command]` + one TS `invoke('foo')`, then queries the SQLite for the resulting `bridge` edge.

**Performance**
- Adds <10% to cold index time on this repo (8.2s → expect ~9s).

## Patterns borrowed

| Pattern | Source | Usage |
|---|---|---|
| Regex-on-source backfill near tree-sitter ranges | aider's Pygments fallback (`repomap.py:340-363`) | Same shape: tree-sitter primary, second-pass enrichment |
| Bridge edge concept | research §3 ("flagship Tauri demo") | Direct implementation |
| `metadata` JSON column for typed traits | Famix / SourceTrail (research §2.3) | Existing column reused |

## Risks / open questions

1. **`#[tauri::command]` macro detection.** Tree-sitter Rust grammar represents attributes as `attribute_item` nodes attached as siblings before the function. Walking that reliably requires careful sibling iteration. Fallback: regex match on source `(?m)^#\[\s*tauri::command\s*\]\s*(?:#\[[^\]]*\]\s*)*pub\s+(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)`. Source-text match is more forgiving and ~2 orders of magnitude simpler than walking the AST. Use regex.
2. **`invoke` may be aliased.** Some files use `import { invoke as call } from '@tauri-apps/api/core'`. Phase 5 ignores aliasing — minor coverage loss, simple fix later. Note in plan.
3. **Cross-call-site enclosing function.** Tying an `invoke()` call to its containing function/method requires walking up the parse tree. Tree-sitter's `node.parent()` chain handles this; if the call site is at module top level, attribute the edge to the file node instead.
4. **Bridge edges show up even when the user hasn't enabled cross-language.** That's the point — they're always-on. No flag.

## Out of scope (Phase 5)

- HTTP route bridges → Phase 10.
- Codegen detection (tRPC, GraphQL Codegen) → Phase 10.
- LLM-fuzzy match for non-literal URLs → Phase 10.
- Frontend rendering polish for bridge edges → Phase 7+.

## Done definition

- All acceptance criteria met.
- Plan + code committed on `feat/project-map`.
- `codeterm agent report "phase 5 tauri-bridge done: …"`.
