# CodeTerm Plugins — Authoring Guide

This document describes how to author a CodeTerm plugin **given what exists in
the code today**.

## What a plugin is

A plugin is a unit of functionality described by a `plugin.toml` manifest and,
optionally, a Rust implementation of the `Plugin` trait. CodeTerm's manifest
distinguishes three **tiers**:

| Tier      | Runtime                                           | Status today |
|-----------|---------------------------------------------------|--------------|
| `native`  | Compiled into the CodeTerm binary, full access    | **Usable**   |
| `process` | Long-running external process over JSON-RPC stdio | Not implemented — manifest parses, but no host spawns anything |
| `wasm`    | Sandboxed `wasm32-wasip2` module                  | Not implemented — manifest parses, but no host runs WASM |

In practice, **only `native` plugins are functional right now**. You can still
ship a `plugin.toml` for the other tiers — `PluginManager::discover` will
register the manifest and flip it to `Active` on `activate()` — but no external
process is spawned and no WASM is loaded. Treat that as a dry run.

## `plugin.toml` format

The authoritative source is `src-tauri/src/plugin/mod.rs`. Everything below
reflects the fields the parser actually accepts; fields used by plan documents
but missing from the Rust structs (`[mesh]`, `settings` panels beyond a bool,
etc.) are **not** wired up and should not be declared.

```toml
[plugin]
name         = "my-plugin"         # required, must match Plugin::id()
version      = "0.1.0"             # required
description  = "One-line blurb"    # optional
tier         = "native"            # required: "native" | "process" | "wasm"
entrypoint   = "python3 main.py"   # optional — used by the (not-yet-implemented) process host
wasm_module  = "plugin.wasm"       # optional — used by the (not-yet-implemented) WASM host
platforms    = ["windows", "macos", "linux"]  # optional, omit = universal
min_codeterm = "0.10.0"            # optional, currently not enforced

[capabilities]
commands        = ["my-plugin"]    # CLI subcommands (declared only; no CLI host yet)
tools           = []               # MCP-style tool ids (declared only)
pane_types      = []               # custom pane renderers (declared only)
agent_providers = []               # agent provider ids this plugin registers (declared only)
hooks           = ["mux-state-changed", "pane-title-pinned"]  # SystemEvent type_name() strings
settings        = false            # bool — plugin has a settings UI panel (declared only)

[permissions]
filesystem = ["~/.codeterm/plugins/my-plugin/"]   # declared only, not enforced
network    = false                                 # declared only, not enforced
mesh_sync  = false                                 # declared only, not enforced

[activation]
triggers = ["on_startup"]          # declared only; native plugins activate when you call PluginManager::activate

[prompt]
ga_section    = "prompts/ga.md"    # optional path to GA-prompt section (declared only)
skill_section = "prompts/skill.md" # optional path to skill markdown (declared only)
```

`platforms` is the one declarative field with runtime effect: if the current
`std::env::consts::OS` is not in the list, `PluginManager::discover` skips the
plugin. An empty list (or omitted field) means universal.

## The `Plugin` trait

From `src-tauri/src/plugin/mod.rs`:

```rust
pub trait Plugin: Send + Sync {
    /// Stable plugin id, must match manifest.plugin.name.
    fn id(&self) -> &str;

    fn activate(&mut self) -> Result<(), String> { Ok(()) }
    fn deactivate(&mut self) -> Result<(), String> { Ok(()) }

    /// Called for every SystemEvent this plugin is subscribed to.
    fn on_event(&mut self, _event: &SystemEvent) {}
}
```

Minimal native plugin:

```rust
use codeterm::api::events::SystemEvent;
use codeterm::plugin::Plugin;

pub struct HelloPlugin {
    id: String,
    seen: usize,
}

impl HelloPlugin {
    pub fn new() -> Self {
        Self { id: "hello".into(), seen: 0 }
    }
}

impl Plugin for HelloPlugin {
    fn id(&self) -> &str { &self.id }

    fn activate(&mut self) -> Result<(), String> {
        log::info!("hello plugin up");
        Ok(())
    }

    fn on_event(&mut self, event: &SystemEvent) {
        self.seen += 1;
        log::debug!("hello saw {} ({} total)", event.type_name(), self.seen);
    }
}
```

## Lifecycle: discovery, register, attach, activate

`PluginManager` is the single entry point for the plugin registry.

```rust
use codeterm::plugin::{PluginManager, PluginState};

let mgr = PluginManager::new();

// 1. Discover: scans <dir>/*/plugin.toml and registers each manifest
//    that passes the platform filter. Parse errors are logged + skipped.
let count = mgr.discover("~/.codeterm/plugins")?;

// 2. Attach a native implementation to a previously-registered manifest.
//    Fails if the manifest is missing or tier != "native".
mgr.attach_native(Box::new(HelloPlugin::new()))?;

// 3. Activate by id. Calls Plugin::activate() and transitions
//    Discovered -> Active on success, Failed on error.
mgr.activate("hello")?;

assert_eq!(mgr.state_of("hello"), Some(PluginState::Active));

// Dispatch flows run through here; manual dispatch is also available.
mgr.dispatch_event(&SystemEvent::MuxStateChanged);

// Shut down: runs Plugin::deactivate() and returns the entry to Discovered.
mgr.deactivate("hello")?;
```

Lifecycle states (from `PluginState`):

```
Discovered → Active → Failed
             ↑   ↓
           deactivate()
```

- **Discovered** — manifest registered, no implementation attached yet (or
  attached but not activated).
- **Active** — `activate()` returned `Ok`. The plugin will receive events via
  `dispatch_event`.
- **Failed** — `activate()` returned `Err`; `PluginEntry::error` holds the
  message.

For `process` and `wasm` tiers, `activate()` is currently a no-op that just
flips the state to `Active` — this is a placeholder until the real hosts land.

## Subscribing to events

Events are typed `SystemEvent` variants defined in
`src-tauri/src/api/events.rs`. A plugin subscribes by listing the `type_name()`
strings of the variants it cares about in `[capabilities].hooks`:

```toml
[capabilities]
hooks = ["mux-state-changed", "pane-title-pinned"]
```

Matching is performed by `PluginManager::dispatch_event`. An empty `hooks` list
means "subscribe to everything" — use this for debugging plugins or dev tooling
but not for production plugins.

### Currently-published events

Today, the only publish sites in the codebase are
`ServiceContext::emit_state_changed` and `ServiceContext::emit_pane_title_pinned`
in `src-tauri/src/service/mod.rs`, which publish the following two variants:

| `SystemEvent::type_name()` | Variant                          | Payload fields           |
|----------------------------|----------------------------------|--------------------------|
| `mux-state-changed`        | `SystemEvent::MuxStateChanged`   | — (fires on any workspace / pane / tab state change) |
| `pane-title-pinned`        | `SystemEvent::PaneTitlePinned`   | `pane_id: String`, `title: String` |

Other `SystemEvent` variants (`PaneFocused`, `PaneClosed`,
`AgentStarted`, `AgentIdle`, `ConfigChanged`, `Generic`) are **defined but not
yet emitted** anywhere in core. Subscribing to them today is valid but you will
receive nothing. They will be filled in as the plugin host work progresses.

If you need an event that does not yet exist, publish a
`SystemEvent::Generic { event_type, data }` from core and subscribe to it by
the string `"generic"` — but prefer adding a typed variant.

## Not yet

The following are designed but **do not exist in the code today**. Do not build
plugins that depend on them:

- **`agent-claude` extraction.** Claude detection, prompt parsing, session
  discovery, and skill delivery still live in `src-tauri/src/agent/claude.rs`.
  The `AgentProvider` trait has been expanded with the plugin-facing surface
  (`install_skills`, `supports_lazy_skills`, etc.) but no provider has been
  moved out of core.
- **Process plugin host.** No JSON-RPC stdio runtime, no health checks, no
  restart-on-crash. Manifests with `tier = "process"` parse and register, but
  nothing is spawned.
- **WASM plugin host.** No wasmtime / Extism integration. `tier = "wasm"`
  manifests parse but nothing is loaded.
- **Hot reload.** Neither native nor process plugins can be reloaded at
  runtime.
- **CLI subcommands.** `[capabilities].commands` is recorded in the manifest
  but `codeterm` does not dispatch to plugin-contributed subcommands, and
  there is no `codeterm plugin create|restart|logs|test|validate|dev` command
  group yet.
- **Permission enforcement.** `[permissions]` is parsed but not checked —
  native plugins have full process access by virtue of being compiled in.
- **Prompt composition.** `[prompt].ga_section` / `skill_section` paths are
  parsed but nothing reads them yet.
- **Mesh replication.** `[mesh]` has no struct in the parser at all; the Rust
  side does not know about it.
