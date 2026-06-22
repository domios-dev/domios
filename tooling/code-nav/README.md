# code-nav — dev-only structural code navigator

A small tree-sitter-based CLI for asking **structural** questions about the
CodeTerm source tree: *where is X defined*, *who implements trait Y*, *what is
this file's symbol outline*. Built for agents (and humans) exploring the repo.

**This is a dev tool. It is NOT part of the product.** It is a standalone Cargo
project, not a member of any workspace, and `build.sh` never touches it (every
product cargo call is pinned to `src-tauri/Cargo.toml` / `cli/Cargo.toml`). It
never links into the shipped binary.

## Index model: parse-on-demand, no index

code-nav keeps **no persistent index or cache**. Every invocation parses the
files it needs from disk, fresh. Consequences:

- Results are **always current** — there is nothing to rebuild, refresh, or go
  stale. Edit a file, re-run, get the new answer.
- There is **nothing to gitignore** except the Rust `target/` dir (handled by
  `tooling/code-nav/.gitignore`). No `.cache`, no `.index`, no DB.
- It's fast enough to not need one: a full `src-tauri/src` scan (≈900 `.rs`
  files) parses in well under a second; a single file parses in ~1–1.5 ms.

If the repo ever grows to where a per-invocation full-tree scan feels slow, the
honest upgrade is a content-hash-keyed cache under `target/` — but that's not
needed today, and is deliberately not built.

## Build

```bash
cargo build --release --manifest-path tooling/code-nav/Cargo.toml
# binary: tooling/code-nav/target/release/code-nav
```

First build pulls 3 grammar crates and compiles their C parsers via `cc`
(~8 s, ~21 crates). No new toolchain: the CodeTerm build already compiles C
deps and already wires MSVC on Windows.

## Run

All paths are relative to the repo root. Convenience alias:

```bash
alias cn='tooling/code-nav/target/release/code-nav'
```

### `outline <file> [--json]` — one file's symbols

```bash
cn outline src-tauri/src/service/orchestrator.rs
#    10  struct     BatchOp
#    17  impl       ServiceContext
#    19  fn         batch
#   ...  (25 symbols)

cn outline src/components/ChatView/ChatView.tsx
cn outline src/store/multiplexer/reducer.ts --json
# {"file":"...","symbols":[{"kind":"type","name":"Action","line":15},...]}
```

TS/TSX outlines are module-level only (local `const`s inside function bodies are
intentionally excluded).

### `def <dir> <name>` — where a symbol is DEFINED (no call sites)

```bash
cn def src-tauri/src spawn_agent
# api/agent_routes.rs:51       fn spawn_agent
# service/orchestrator.rs:308  fn spawn_agent
```

Scans `.rs`, `.ts`, `.tsx`. Returns only definitions — ignores call sites,
comments and strings (where `grep -rn spawn_agent` returns ~68 mixed hits).

### `impls <dir> <Trait>` — every impl + its methods (Rust)

```bash
cn impls src-tauri/src AgentProvider
# plugin/js_runtime/provider.rs:279  impl AgentProvider for JsProvider
#         fn id  (L280)
#         fn display_name  (L284)
#   ...
# (11 impl blocks across the tree)
```

Useful for the native+JS provider parity rule: diff the trait surface against a
given impl to see which methods rely on defaults.

### `query <file> <scm>` — arbitrary tree-sitter query (escape hatch)

```bash
# every function name in a file
cn query src-tauri/src/service/orchestrator.rs '(function_item name: (identifier) @fn) @def'

# every #[tauri::command]-style fn signature, trait required methods, etc.
cn query src-tauri/src/agent/provider.rs '(function_signature_item name: (identifier) @required) @def'
```

Capture a node as `@def` to report its start line; any other capture name is
used as the symbol "kind" label. Grammar node names differ between Rust
(`function_item`, `type_identifier`) and TS (`function_declaration`,
`identifier`) — dump a tree first if unsure.

## When to use this vs ripgrep

- **Use code-nav** for structure: definitions, trait impls, symbol outlines,
  "X of kind Y named Z". It ignores comments/strings/call-sites.
- **Use ripgrep** for flat text: does this string occur, count occurrences,
  find a literal. It's faster and the structural precision buys nothing there.

See the AGENTS.md section *"Dev tooling: tree-sitter code navigation"* for the
agent-facing decision rule.

## Supported languages

`.rs` (tree-sitter-rust), `.ts` / `.tsx` (tree-sitter-typescript). Other
extensions are skipped. `target/`, `node_modules/`, `.git/`, `dist/`, `gen/`
directories are never descended.
