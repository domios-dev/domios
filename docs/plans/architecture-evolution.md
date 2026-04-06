# Architecture Evolution Plan

**Status:** Planning  
**Branch:** `feat/architecture_evo`  
**Goal:** Evolve codebase toward Hexagonal + Command Bus architecture, covered by tests, ready for plugin system.

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
- [ ] Create `src-tauri/src/dispatch/mod.rs`
- [ ] Define `Command` enum with first 5 operations (health, pane_list, pane_split, send_text, get_output)
- [ ] Implement `dispatch(cmd: Command, ctx: &ServiceContext) -> Result<Value>`
- [ ] Tests for each command in isolation

**B2. Migrate adapters incrementally**
- [ ] REST: replace 5 route handlers with `dispatch()` calls
- [ ] Socket: replace 5 method handlers with `dispatch()` calls  
- [ ] Tauri: replace 5 commands with `dispatch()` calls
- [ ] Verify: all 3 adapters produce identical results for same input

**B3. Continue migration (remaining ~35 operations)**
- [ ] Batch migrate by domain: pane ops → workspace ops → pty ops → agent ops → mesh ops
- [ ] Each batch: define commands → migrate adapters → test → commit

**Target:** ~4700 LOC → ~2000 LOC. One Command impl per operation.

### Phase C: Event Bus
Inter-module communication without direct coupling.

- [ ] Add `EventBus` to ServiceContext (tokio broadcast channel)
- [ ] Define `SystemEvent` enum (PaneCreated, AgentStarted, etc.)
- [ ] Migrate output.rs notifications → EventBus publish
- [ ] Migrate agent detection status updates → EventBus
- [ ] Frontend: EventBus → Tauri events (bridge)

### Phase D: Plugin Trait + First Plugin
From plugin-architecture.md Phase 1-2.

- [ ] Define `Plugin` trait (commands, on_event, activate, deactivate)
- [ ] `PluginManager` — discover, load, lifecycle
- [ ] Extract agent-claude into first native plugin
- [ ] Process plugin host (JSON-RPC stdio)

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
