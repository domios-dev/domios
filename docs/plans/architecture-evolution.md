# Architecture Evolution Plan

**Status:** In progress (Phase B/C/D partially landed)
**Branch:** `feat/plugin-prep`
**Goal:** Evolve codebase toward Hexagonal + Command Bus architecture, covered by tests, ready for plugin system.

## Status as of 2026-04-06

- Phase A: not tracked here; see test coverage reports.
- Phase B1 — unified dispatch infrastructure: **DONE** (commit `2a5ef09`). `src-tauri/src/dispatch/mod.rs` exists with `Command` enum and `dispatch()`.
- Phase B2 — migrate first 5 ops across REST/Socket/Tauri: **DONE** for `health`, `pane.list`, `send_text`, `get_output`, and the Tauri `get_pane_output` / `get_pane_screen` / `get_pane_screen_formatted` commands (`cac958e` series: `cac954`, `65ce514`, `a8a7f4e`).
- Phase B3 — remaining operations: **IN PROGRESS**. Batch 1 pane ops (`PaneSplit`/`PaneClose`/`PaneFocus`/`PaneRename`) wired through REST and Socket (`36b5065`). Remaining: workspace, pty, agent, mesh operations.
- Phase C — EventBus: **DONE** structurally (`88c7afe`). Typed `SystemEvent` bus lives in `src-tauri/src/api/events.rs`; `ServiceContext` publishes `MuxStateChanged` and `PaneTitlePinned` via `emit_state_changed` / `emit_pane_title_pinned`. Migration of output-pipeline notifications and agent status events to typed variants is still outstanding.
- Phase D — Plugin trait + first plugin: **SKELETON DONE** (`f371699`). `Plugin` trait, `PluginManager`, and manifest parser live in `src-tauri/src/plugin/mod.rs`. `AgentProvider` trait has been expanded with `auto_approve_flags`, `compact_command`, `model_switch_command`, `exit_command`, `install_skills`, `uninstall_skills`, `supports_lazy_skills`. **Not yet:** `agent-claude` has not been extracted — it still lives in `src-tauri/src/agent/claude.rs`. No process host, no WASM host.
- Supporting refactor: ANSI helpers extracted to `pty::ansi` (`4cbfd77`); `get_pane_screen` unified through the backend VT100 path (`a7c2a5a`).


## Current State → Target

```
CURRENT                                  TARGET (Hexagonal + Command Bus)
─────────────────────────────            ──────────────────────────────────
REST routes.rs (2225 LOC) ─┐             REST adapter (thin) ─┐
Socket server.rs (974 LOC) ┼─ duplicate  Socket adapter (thin) ┼─ Command enum
Tauri commands.rs (1547 LOC)┘            Tauri adapter (thin) ─┘     │
         │                                        │                   │
    ServiceContext (god object)              dispatch(Command, ctx)
         │                                        │
    All modules see all state              ┌──────┴──────┐
                                     ServiceContext    EventBus
                                           │               │
                                    ┌──────┼──────┐    Subscribers
                                    Core   │   Plugins
                                           │
                              ┌────────────┼────────────┐
                          Multiplexer    PTY         Mesh/Config
```

## Architecture: Hexagonal + Command Bus

**Why this pattern:**
- VS Code, Zed, Home Assistant, Neovim all converge on: typed command dispatch + event bus + adapter-per-transport
- Solves 3-API duplication (~2700 LOC eliminated)
- Plugins naturally register commands + subscribe to events
- Each operation testable in isolation (mock ServiceContext)
- Incremental migration — one command at a time

**Layers:**

| Layer | Contents | Depends on |
|-------|----------|------------|
| **Core** | Multiplexer, PTY, Mesh, Config, EventBus, PluginManager | Nothing external |
| **Commands** | `Command` enum, `dispatch()`, validation | Core |
| **Services** | Orchestrator, GA, Output pipeline | Core + Commands |
| **Adapters** | REST, Socket, Tauri IPC, CLI | Commands only |
| **Plugins** | Agent providers, Bridges, Memory | Core + Commands + EventBus |

**Dependency rule:** Adapters → Commands → Core. Never the reverse.

## Phase Plan

### Phase A: Test Foundation (this branch)
Cover critical untested code BEFORE restructuring. Tests are the safety net.

**A1. Clean up existing tests**
- [ ] Delete 6 useless tests (test_capitalize, test_tracker_set_dead, test_get_reports_empty, test_identity_has_keypair, duplicate registry detect tests)
- [ ] Fix 1 failing test (test_pty_kill in tests/pty_test.rs)

**A2. Cover zero-coverage critical modules**
- [ ] `service/output.rs` (429 LOC, 0%) — core output pipeline, ~20 tests
- [ ] `pty/screen_state.rs` — new get_screen_formatted, ~5 tests
- [ ] `service/pane.rs` (208 LOC, 0%) — pane CRUD, ~10 tests
- [ ] `service/workspace.rs` (128 LOC, 0%) — workspace ops, ~8 tests

**A3. Cover secondary modules**
- [ ] `service/notification.rs` (54 LOC, 0%) — ~5 tests
- [ ] `service/hierarchy.rs` (355 LOC, 14%) — role validation, ~10 tests
- [ ] `pty/write_queue.rs` — queue logic, ~5 tests

**Target:** 33% → 50%+ line coverage

### Phase B: Command Dispatch (foundation for plugins)
Extract command logic into unified dispatch. One operation at a time.

**B1. Define Command infrastructure**
- [x] Create `src-tauri/src/dispatch/mod.rs`
- [x] Define `Command` enum with first 5 operations (health, pane_list, pane_split, send_text, get_output)
- [x] Implement `dispatch(cmd: Command, ctx: &ServiceContext) -> Result<Value>`
- [x] Tests for each command in isolation

**B2. Migrate adapters incrementally**
- [x] REST: replace 5 route handlers with `dispatch()` calls
- [x] Socket: replace 5 method handlers with `dispatch()` calls
- [x] Tauri: replace 5 commands with `dispatch()` calls (`get_pane_output`, `get_pane_screen`, `get_pane_screen_formatted`)
- [x] Verify: all 3 adapters produce identical results for same input

**B3. Continue migration (remaining ~35 operations)**
- [x] Pane ops batch 1: `PaneSplit`, `PaneClose`, `PaneFocus`, `PaneRename` wired through REST + Socket (`36b5065`)
- [ ] Workspace ops
- [ ] PTY ops (resize, kill, exec)
- [ ] Agent ops
- [ ] Mesh ops

**Target:** ~4700 LOC → ~2000 LOC. One Command impl per operation.

### Phase C: Event Bus
Inter-module communication without direct coupling.

- [x] Add `EventBus` to ServiceContext (tokio broadcast channel — `EventBroadcaster::system_bus` in `src-tauri/src/api/events.rs`)
- [x] Define `SystemEvent` enum (`MuxStateChanged`, `PaneTitlePinned`, `PaneFocused`, `PaneClosed`, `PaneSplit`, `AgentStarted`, `AgentIdle`, `ConfigChanged`, `Generic`)
- [ ] Migrate `output.rs` notifications → EventBus publish (still call `emit_state_changed` only)
- [ ] Migrate agent detection status updates → typed `AgentStarted` / `AgentIdle` publishes (currently only defined, not emitted)
- [x] Frontend: EventBus → Tauri events (bridge via `app_handle.emit` in `emit_state_changed` / `emit_pane_title_pinned`)

### Phase D: Plugin Trait + First Plugin
From plugin-architecture.md Phase 1-2.

- [x] Define `Plugin` trait (`id`, `activate`, `deactivate`, `on_event`) in `src-tauri/src/plugin/mod.rs`
- [x] `PluginManager` — discover (`<dir>/*/plugin.toml`), register, `attach_native`, activate/deactivate, dispatch events filtered by `capabilities.hooks`
- [x] Expand `AgentProvider` trait with plugin-facing hooks (`install_skills`, `uninstall_skills`, `supports_lazy_skills`, `auto_approve_flags`, `compact_command`, `model_switch_command`, `exit_command`)
- [ ] Extract `agent-claude` into first native plugin (still in `src-tauri/src/agent/claude.rs`)
- [ ] Process plugin host (JSON-RPC stdio)
- [ ] WASM plugin host

## Test Strategy

### Per-phase testing

| Phase | Test approach |
|-------|--------------|
| A (foundation) | Unit tests on existing modules, no behavior changes |
| B (commands) | Each Command gets unit test. Adapter tests verify identical output before/after migration |
| C (event bus) | Integration tests: event published → subscriber receives |
| D (plugins) | Plugin trait mock tests. agent-claude plugin passes same tests as current inline code |

### Test organization

```
src-tauri/
  src/
    module/
      mod.rs          ← code
      (tests inline)  ← unit tests stay inline (standard Rust)
  tests/
    pty_test.rs       ← existing integration
    dispatch_test.rs  ← command dispatch integration tests (Phase B)
    plugin_test.rs    ← plugin lifecycle tests (Phase D)
```

### Coverage targets

| Milestone | Line coverage | When |
|-----------|--------------|------|
| Current | 33% | now |
| After Phase A | 50%+ | next |
| After Phase B | 60%+ | with command tests |
| After Phase D | 65%+ | with plugin tests |

## Key Principles

1. **Incremental.** No big bang rewrites. Each commit is a working build.
2. **Tests first.** Cover existing code BEFORE restructuring it.
3. **One command at a time.** Migrate operations individually, verify parity.
4. **SOLID at module level.** Each module has single responsibility, explicit dependencies.
5. **DRY via dispatch.** Three API surfaces share one implementation, not three copies.
6. **Plugin-ready.** Every architectural decision asks: "how does this work with plugins?"

## References

- `docs/plans/plugin-architecture.md` — full plugin spec
- `VISION.md` — project direction
- `docs/ARCHITECTURE.md` — current architecture + known issues
- VS Code Extension Host, Zed gpui::Action, Home Assistant integrations
