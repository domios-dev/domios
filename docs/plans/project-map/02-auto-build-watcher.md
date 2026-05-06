# Phase 2 — Per-workspace auto-build + watcher

**Status**: plan
**Owner**: Mac-Air
**Branch**: `feat/project-map`
**Depends on**: Phase 1 (Tier-0 indexer is in place; CLI binary `codeterm map index` works against any workspace cwd).

## Goal

Make the project map build itself, automatically and incrementally, for every eligible workspace — without UI work and without ballooning into a large daemon refactor. The user opens a workspace, the indexer runs in the background, and edits trigger debounced re-indexes. `General` and workspaces without a `cwd` stay excluded.

## Hard-scope cuts (vs. a "complete" Phase 2)

To avoid spreading across UI + persistence + REST in one phase:

- **No frontend Settings panel.** The toggle is a JSON file written by CLI; UI lands in Phase 3 alongside the canvas.
- **No new dispatch::Command variants.** Map config is a per-workspace JSON file (`<workspace>/.codeterm/map/v1/config.json`) read directly by both daemon and CLI. No daemon RPC needed for the toggle.
- **No new HTTP / REST endpoints.** Same reason.
- **Indexer runs as a CLI subprocess.** No shared lib refactor. Daemon spawns `codeterm map index --workspace <id>` and watches stdout/exit. Subprocess startup cost (~50–80ms on this hardware) is dwarfed by the index itself.

These cuts are deliberate; Phase 3+ can revisit any of them when the canvas needs richer info.

## Files

```
src-tauri/Cargo.toml                                # confirm notify + notify-debouncer-mini already pinned (yes)
src-tauri/src/service/project_map/mod.rs            # NEW — facade (<200 lines)
src-tauri/src/service/project_map/manager.rs       # NEW — ProjectMapManager: per-workspace state + lifecycle
src-tauri/src/service/project_map/watcher.rs       # NEW — debounced fs-event → re-index trigger
src-tauri/src/service/project_map/runner.rs        # NEW — spawn `codeterm map index` subprocess
src-tauri/src/service/project_map/config.rs        # NEW — read/write per-workspace config.json
src-tauri/src/service/mod.rs                       # EDIT — add `pub mod project_map;`
src-tauri/src/service/workspace.rs                 # EDIT — hook create/close into manager
src-tauri/src/service/mod.rs (ServiceContext)      # EDIT — `pub project_map: Arc<ProjectMapManager>`
src-tauri/src/lib.rs                               # EDIT — bootstrap: spawn manager, replay existing workspaces

cli/src/cmd/map/config_io.rs                       # NEW — reusable read/write of map config.json
cli/src/cmd/map/mod.rs                             # EDIT — add Config subcommand handler
cli/src/args/map.rs                                # EDIT — add `Config { Get, Set }` subcommands
```

All new files ≤ 250 lines. `service/project_map/mod.rs` is a facade.

## Per-workspace map config

`<workspace_cwd>/.codeterm/map/v1/config.json`:

```json
{
  "indexing_enabled": true,
  "watch_debounce_ms": 1500,
  "max_files": 50000
}
```

Defaults loaded if absent. **Live read on every event** — no caching tier, no daemon-side struct holding it. Tiny file, hot path is debounce-flush (every 1.5s+); fs read cost is invisible.

`max_files` is a guard: if a workspace's cwd contains >`max_files` walkable source files, refuse to index and log an error to `error` table (prevents accidentally pointing the indexer at `/`). Tunable.

## ProjectMapManager

```rust
pub struct ProjectMapManager {
    inner: Arc<parking_lot::Mutex<Inner>>,
    cli_path: PathBuf,                         // resolved `codeterm` binary path
    handle: tokio::runtime::Handle,
}

struct Inner {
    workspaces: HashMap<WorkspaceId, WorkspaceSlot>,
}

struct WorkspaceSlot {
    cwd: PathBuf,
    debouncer: Option<DebouncerHandle>,        // notify-debouncer-mini
    in_flight: Arc<AtomicBool>,                 // true while a subprocess runs
    pending: Arc<AtomicBool>,                   // request to re-run after current finish
    last_run: Option<RunSummary>,
}

impl ProjectMapManager {
    pub fn start_for_workspace(&self, ws_id: &str, cwd: &Path);
    pub fn stop_for_workspace(&self, ws_id: &str);
    pub fn trigger_index(&self, ws_id: &str);   // fire-and-forget subprocess
    pub fn bootstrap(&self, workspaces: &[(WorkspaceId, Option<PathBuf>, String)]);
    pub fn shutdown(&self);                      // stop all watchers + wait briefly
}
```

### Eligibility (called inside `start_for_workspace`)

```text
not eligible if any of:
  - workspace name == "General"
  - cwd is None
  - cwd does not exist
  - config.indexing_enabled == false
otherwise eligible
```

### Subprocess invocation

```rust
let mut cmd = std::process::Command::new(&self.cli_path);
cmd.arg("map").arg("index").arg("--workspace").arg(ws_id);
// inherit env so CLI re-discovers daemon socket; its `discover_socket`
// already prefers $CODETERM_SOCKET_PATH which the daemon sets on every
// PTY, but for a daemon-spawned subprocess we set it explicitly so we
// don't rely on PTY env propagation
cmd.env("CODETERM_SOCKET_PATH", &daemon_socket);
cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
let child = cmd.spawn()?;
// drain stdout for the IndexReport JSON (we'll pass --json), capture stderr
// into the error log; exit-on-first-line-of-stderr is fine for Phase 2.
```

`map index` already supports `--json` → daemon parses `IndexReport` and emits a `mux-state-changed` style event so future UI can react. Phase 2 just emits, no UI binding.

### Watcher

`notify-debouncer-mini` (`Debouncer::new(Duration::from_millis(cfg.watch_debounce_ms), ...)`) per workspace cwd. Recursive. On flush:

```rust
let interesting = events.iter().any(|ev| {
    ev.path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| Lang::from_path_ext(ext).is_some())
        .unwrap_or(false)
});
if interesting && !slot.in_flight.swap(true, Ordering::Relaxed) {
    self.trigger_index(ws_id);
} else if interesting {
    slot.pending.store(true, Ordering::Relaxed);
}
```

`Lang::from_path_ext(ext)` is a small new helper added to `cli/src/cmd/map/lang.rs` so daemon and CLI share extension semantics (compile-time copy is fine; we don't share the crate).

### Lifecycle hooks

- **Bootstrap (lib.rs `setup`):** after constructing `ServiceContext`, call `service.project_map.bootstrap(&service.list_workspaces())`. Indexes every eligible workspace once at startup, then arms watchers.
- **`workspace_create` in `service/workspace.rs`:** after the call returns the new `WorkspaceInfo`, if it has a `cwd`, call `service.project_map.start_for_workspace(&info.id, cwd)`.
- **`workspace_close`:** before returning, `service.project_map.stop_for_workspace(ws_id)`.
- **Daemon shutdown:** `service.project_map.shutdown()` — currently the daemon doesn't have a clean shutdown hook for the service layer, but the watchers' `Drop` impls handle native fs-watch cleanup. Add a shutdown call into the existing graceful-shutdown path if one exists; otherwise rely on Drop.

## CLI: map config

```rust
// cli/src/args/map.rs
#[derive(Subcommand)]
pub enum MapAction {
    // existing: Index, Status, Search
    /// Read or write per-workspace map configuration.
    Config {
        #[command(subcommand)]
        action: MapConfigAction,
    },
}

#[derive(Subcommand)]
pub enum MapConfigAction {
    Get {
        #[arg(long)] workspace: Option<String>,
        /// Restrict output to a single key (e.g. `indexing.enabled`).
        #[arg(long)] key: Option<String>,
    },
    Set {
        /// Key, e.g. `indexing.enabled`.
        key: String,
        /// Value as JSON literal: `true`, `false`, `1500`.
        value: String,
        #[arg(long)] workspace: Option<String>,
    },
}
```

Implementation in `cli/src/cmd/map/config_io.rs`:

```rust
pub fn read_config(workspace_root: &Path) -> MapConfig;          // never fails — defaults
pub fn write_config(workspace_root: &Path, cfg: &MapConfig) -> std::io::Result<()>;
pub fn get_value(cfg: &MapConfig, key: &str) -> Option<serde_json::Value>;
pub fn set_value(cfg: &mut MapConfig, key: &str, value: &str) -> Result<(), String>;
```

Keys recognised in Phase 2: `indexing.enabled`, `watch.debounce_ms`, `index.max_files`. Unknown key → error.

Daemon's `ProjectMapManager` reads the same JSON live, so `codeterm map config set indexing.enabled false` immediately stops the watcher on the next debounce flush (which sees `enabled=false` and unregisters). Eventual consistency, ~debounce period — that's fine.

## Acceptance criteria

**Functional**
- After daemon starts with a non-General workspace, `<workspace_cwd>/.codeterm/map/v1/index.sqlite` exists within ~30s with non-zero `node` count.
- Editing a `.rs` file in that workspace triggers a re-index within `debounce_ms + ~3s`. Verified by `codeterm map status` showing a fresh `last_index_finished_at`.
- `codeterm map config set indexing.enabled false` stops the watcher; subsequent file edits do **not** trigger a re-index. Re-enabling resumes.
- A workspace named `General` does NOT get a `.codeterm/map/` directory created — even if the user manually invokes `codeterm map index --workspace <general_id>` the resolver rejects it.
- A workspace with `cwd: None` is silently skipped (no error, no map dir).
- Closing a workspace stops its watcher (verified by lsof / no further index runs after edits).

**Structural**
- Lock ordering preserved: `ProjectMapManager` holds its own internal mutex (separate from mux/output_buf). Never acquires `mux` while holding its own lock.
- All new files under 250 lines.
- `./build.sh check` clean.

**Tests** (`#[cfg(test)] mod tests` in each new module)
- `config.rs`: read defaults from missing file, write+read round-trip, set/get key.
- `manager.rs`: eligibility decision matrix (General / no-cwd / disabled / OK).
- `runner.rs`: spawn returns NonZero on missing CLI, parses JSON `IndexReport` from stdout. Use a stub binary in `tests/fixtures/`.
- `watcher.rs`: simulate fs event with a `tempfile` workspace, assert debounced trigger callback fires once with batched events. Use `tokio::time::pause()` to compress the debounce window.

**Performance**
- Daemon startup overhead from Phase 2: under 1s before bootstrap returns. Per-workspace initial index runs async, doesn't block startup.
- Debounce flush + subprocess spawn under 200ms (subprocess actual index time is governed by Phase 1).

## Patterns borrowed

| Pattern | Source | Where reused |
|---|---|---|
| Per-workspace fs-watch with debouncer | `notify-debouncer-mini` (already a dep) | `service/project_map/watcher.rs` |
| Subprocess-driven worker for heavy work | `cli/src/cmd/worker.rs` (`mem run-worker` pattern) | `service/project_map/runner.rs` |
| JSON config sidecar in `.codeterm/<feature>/v1/` | aider repomap cache layout | `<workspace>/.codeterm/map/v1/config.json` |

## Risks / open questions

1. **CLI binary discovery.** The daemon needs the path of `~/.codeterm[-dev]/bin/codeterm` (or `target/debug/codeterm` in dev). Use `std::env::current_exe()` and check for sibling `bin/codeterm`, fall back to `which codeterm`. Document the resolver in `runner.rs`.
2. **fsevents on macOS coalesces editor saves into multiple events.** `notify-debouncer-mini` already handles this; just verify with a real Vim save (`:w`) during manual test.
3. **Symlink loops.** `walker.rs` already uses the `ignore` crate which respects loops. Watcher recursive-mode could still loop on bad symlink config — `notify` itself protects via `set_max_recursion`. Default depth is unlimited; Phase 2 caps recursion depth at 32.
4. **Watcher resource cap.** macOS limits open file handles. For huge monorepos with many workspaces, we might exhaust. `max_files` config + an aggregate-cap "max watchers running" guard. Phase 2 ships with the per-workspace `max_files` only; multi-workspace cap deferred.
5. **Indexer subprocess race.** If a watcher fires while a subprocess is mid-run, queue a single pending re-run (`pending: AtomicBool`). When the running subprocess completes, check `pending` and re-spawn if set. Avoids unbounded queue.
6. **Daemon-spawned subprocess' socket access.** The CLI uses `discover_socket` which expects `$CODETERM_SOCKET_PATH` in env. Daemon must export this when spawning, otherwise the CLI walks `~/.codeterm-dev` vs `~/.codeterm` heuristically. Tested explicitly.

## Out of scope (Phase 2)

- UI for the toggle / progress / errors → Phase 3 (canvas).
- Mesh-aware `--mesh PEER` for `map config` → Phase 9+ (requires REST endpoint).
- Multi-workspace watcher cap → backlog.
- Smart "only re-index changed file" path that avoids full repo rebuild → Phase 6 / Merkle layer.

## Done definition

- All acceptance criteria above met.
- Plan doc + code committed on `feat/project-map`.
- Manual e2e: open a fresh workspace pointed at a small Rust crate, see map appear; edit a file, see it refresh; flip the toggle, see watcher idle; flip back, see it resume.
- `codeterm agent report "phase 2 auto-build watcher done: …"`.
