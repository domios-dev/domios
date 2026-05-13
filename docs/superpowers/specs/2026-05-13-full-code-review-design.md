# Full Project Code Review — Design Spec

## Goal

Produce a comprehensive, ranked findings report covering every source file in the project. Categories: correctness/bugs, architecture/consistency, code quality. Output is a report only — no auto-fixes.

## Scope

All project source files:
- `src-tauri/src/` — ~275 Rust files
- `src/` — ~150 TypeScript/TSX files
- `cli/` — CLI Rust codebase (main.rs + cmd/* + args/*)
- `crates/mem-store/` — standalone Rust crate

Excluded: `references/`, `target/`, `node_modules/`, `dist/`, `evals/`.

## Approach: Multi-Pass Parallel Review

### Pass 1 — Parallel Domain Agents (12 agents)

Each agent owns a subsystem, reads every file in it end-to-end, produces a structured findings report.

| Agent | Domain | Paths |
|-------|--------|-------|
| 1 | Agent & Plugin | `src-tauri/src/agent/`, `src-tauri/src/plugin/` |
| 2 | API Core Routes | `src-tauri/src/api/` (excluding mesh_routes/, mem_routes/) |
| 3 | Memory Subsystem | `src-tauri/src/api/mem_routes/`, `src-tauri/src/mem/` |
| 4 | Mesh Subsystem | `src-tauri/src/api/mesh_routes/`, `src-tauri/src/mesh/` |
| 5 | Bridge & Socket | `src-tauri/src/bridge/`, `src-tauri/src/socket/` |
| 6 | Orchestration Core | `src-tauri/src/multiplexer/`, `src-tauri/src/service/`, `src-tauri/src/dispatch/` |
| 7 | Config & Secrets | `src-tauri/src/config/`, `src-tauri/src/secrets/` |
| 8 | Supporting Services | `src-tauri/src/services/`, `src-tauri/src/session/`, `src-tauri/src/update/`, `src-tauri/src/usage/` |
| 9 | Startup & Commands | `src-tauri/src/startup/`, `src-tauri/src/commands/`, top-level `src-tauri/src/*.rs` |
| 10 | CLI | `cli/src/` |
| 11 | mem-store Crate | `crates/mem-store/src/` |
| 12 | Frontend | `src/` |

### Pass 2 — Cross-System Aggregation (1 agent)

Reads all 12 domain reports. Identifies:
- Patterns that appear across multiple subsystems (e.g., same anti-pattern replicated)
- API contract inconsistencies between backend route handlers and frontend callers
- Naming/convention drift across modules
- Systemic gaps (e.g., error handling style differs wildly between teams)

### Output

- 12 domain reports: `docs/superpowers/review/2026-05-13-domain-<N>-<name>.md`
- 1 consolidated report: `docs/superpowers/review/2026-05-13-consolidated.md`

## Per-Domain Agent Instructions

Each agent must:

1. Read **every file** in its domain using the Read tool
2. For each file, check all three categories:
   - **Correctness**: `unwrap()`/`expect()` in non-test production paths, missing `?` propagation, potential panics, lock ordering violations (per CLAUDE.md ordering: mux → output_buf → screen_state → agent_tracker → pty), race conditions, incorrect error types
   - **Architecture**: file exceeds 1000 lines (hard limit) or mod.rs exceeds 300 lines, SRP violations (file doing two distinct things), abstraction leaks, wrong module ownership
   - **Consistency**: naming drift (snake_case vs camelCase in wrong contexts, inconsistent acronym casing), DRY violations (same logic copy-pasted), inconsistent API response shapes, dead code, TODO/FIXME left in production
3. Produce findings in this format:

```markdown
## <filename>

### CRITICAL
- `file:line` — description

### HIGH
- `file:line` — description

### MEDIUM
- `file:line` — description

### LOW
- `file:line` — description

### OK
(nothing found, file is clean)
```

4. End report with a summary table: total findings by severity and category.

## Severity Definitions

| Severity | Meaning |
|----------|---------|
| CRITICAL | Can panic/deadlock in production, data loss, security issue |
| HIGH | Incorrect behavior, broken abstraction boundary, grows the allowlist |
| MEDIUM | DRY violation, naming inconsistency, responsibility creep |
| LOW | Style nit, minor naming issue, advisory |

## Aggregator Instructions

Read all 12 domain reports. Produce `consolidated.md` with:
1. **Executive summary** — total findings by severity across all domains
2. **Cross-cutting patterns** — issues that appear in 3+ domains (name them)
3. **Per-domain summary** — one paragraph each, top findings
4. **Full ranked findings list** — all CRITICAL + HIGH items from all domains, sorted by severity then domain
5. **Systemic recommendations** — 3-5 concrete structural improvements
