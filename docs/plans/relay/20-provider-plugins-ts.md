# 20 — Provider plugins (TypeScript)

> **Scope:** make provider-specific knowledge (Claude, Codex, Opencode,
> future ones) shippable as standalone TypeScript packages — parser,
> detection, renderer, actions — that load into the WebView and (later)
> into a server-side JS runtime. Hot-reloadable in dev, replaceable in
> prod without rebuilding the Rust binary.

Parent: [00-vision.md](00-vision.md). Amends:
[plugin-architecture.md](../plugin-architecture.md).
Blocked by: [10-pane-transport-protocol.md](10-pane-transport-protocol.md)
(plugins consume the transport's events stream).

## 1. Why this exists

The current state (read by inspection on `docs/relay-plan` branch):

- **Rust side** has a real plugin skeleton in `src-tauri/src/plugin/`
  with `Plugin` trait, `PluginManifest`, `PluginManager`, three tiers
  (Native / Process / Wasm). All three current providers
  (`agent_claude`, `agent_codex`, `agent_opencode`) are Native — i.e.
  compiled into the binary. Process and Wasm are stubs.
- **Frontend side** has zero plugin abstraction.
  `src/components/ChatView/parser.ts` is a hardcoded Claude JSONL parser
  (`parseJsonlSession`, `STATUS_MARKER = "✱"`, `parseClaudeOutput`).
  Codex and Opencode panes get **no fancy chat at all** today, just raw
  xterm.

The gap: the Rust trait is fat (~70 methods, file paths, lifetimes,
locks) and cannot be exposed as-is to a TS plugin author. Meanwhile the
chat UI is single-provider and stuck.

This plan **does not throw out the Rust skeleton.** It splits the
"provider" concept into two halves:

- **Backend half (stays in Rust):** PTY spawn, file I/O on session
  files, OS-level process introspection. Owned by the existing plugin
  tiers; very few people will ever write this.
- **Behaviour half (moves to TS):** PTY-byte detection patterns,
  JSONL parsing, renderer component, action handlers, status mapping.
  This is what changes often, what plugin authors actually want to
  customise, and what must run in the WebView anyway.

## 2. What a TS provider plugin is

A directory shipped as an npm-style package:

```
@codeterm/provider-claude/
├── package.json          name, version, codeterm engine range
├── plugin.json           CodeTerm-specific manifest (capabilities, schemas)
├── src/
│   ├── detect.ts         pty fingerprint → boolean
│   ├── parser.ts         jsonl line → ProviderEvent
│   ├── renderer.tsx      React component, consumes ProviderEvent[]
│   ├── actions.ts        { id, label, run } — buttons in pane chrome
│   └── status.ts         ProviderEvent → PaneStatus mapping
├── tests/
│   └── snapshots/        captured real-Claude JSONL fixtures
└── dist/
    └── plugin.js         single esbuild bundle, what gets shipped
```

`plugin.json` (extends, doesn't replace, the existing Rust manifest
shape — see §6):

```jsonc
{
  "id": "provider-claude",
  "kind": "provider",
  "provider_id": "claude",         // the agent_type it claims
  "displayName": "Claude Code",
  "engine": ">=1.3.0",             // codeterm version range
  "entry": "dist/plugin.js",
  "permissions": {
    "transport_events": ["pty", "events"],
    "send_text": true
  }
}
```

## 3. Runtime model

### MVP (M3): WebView only

- The plugin bundle is loaded at app start with `import(pluginUrl)`.
- It exports a default object satisfying the `ProviderPlugin` interface
  (§4).
- The frontend's `<PaneView pane={p}>` looks up
  `getPlugin(p.agent_type)`. If found → renders
  `<plugin.Renderer events={…} />`. If not → falls back to xterm
  passthrough (always available, never broken).
- Detection in the WebView: when a new pane shows up without
  `agent_type`, the frontend feeds early PTY bytes through every
  registered plugin's `detect()` and pins the first match. Cheap
  because we only do it for ambiguous panes in the first few seconds.

### Later (M4+): server-side via embedded QuickJS

Some plugin code must run when no WebView is open — e.g. mobile relay
case: a phone backgrounded for hours wakes up, asks for a snapshot,
needs `status` populated correctly even though no desktop UI has been
running. For that we host a tiny **QuickJS** runtime inside the Rust
daemon and load the *same* plugin bundle's `detect`, `parser`, and
`status` exports (not `renderer` — no React on the server).

This is deliberately deferred. M3 (WebView-only) is enough for desktop
and for the relay's hosted SPA (it's a WebView too). Mobile native apps
can call the peer's REST endpoints that internally invoke the plugin —
i.e. the peer hosting the pane runs the plugin server-side. That's
exactly what QuickJS enables.

## 4. The plugin interface

The contract a TS plugin satisfies. Stable across minor versions.

```ts
import type { ReactNode } from "react";

/**
 * One provider plugin's surface. Pure data + functions — no
 * stateful classes, no host references. Anything the plugin needs
 * from the host is passed in arguments.
 */
export interface ProviderPlugin {
  meta: {
    id: string;                    // matches plugin.json provider_id
    displayName: string;
  };

  /** Cheap structural check on PTY bytes. Called repeatedly on fresh
      panes; must be fast. No I/O. */
  detect(sample: { bytes: string; title?: string }): boolean;

  /** Parse one JSONL session line into structured events. May emit
      zero or many. The host calls this per line received over
      transport. */
  parseLine(line: string): ProviderEvent[];

  /** Map a parsed event to a coarse pane status. Returning `null`
      means "no status change". */
  statusOf(event: ProviderEvent): PaneStatus | null;

  /** React component rendering the chat view. Consumes the event
      stream and the live PTY (for fallback / raw view toggle). */
  Renderer: (props: RendererProps) => ReactNode;

  /** Buttons/menu items the plugin contributes to pane chrome.
      Each action's `run` returns text to send via transport
      `send_text`, or null to cancel. */
  actions: ProviderAction[];
}

export interface ProviderEvent {
  kind: string;                     // "user_message" | "assistant" | "tool_call" | ...
  ts?: number;
  payload: unknown;                 // plugin-defined
}

export interface RendererProps {
  events: ProviderEvent[];
  raw: () => ReactNode;             // mount xterm.js on the raw pty
  send: (text: string) => Promise<void>;
  status: PaneStatus;
}

export interface ProviderAction {
  id: string;
  label: string;
  run: (ctx: { events: ProviderEvent[] }) => string | null;
}

export type PaneStatus =
  | { kind: "idle" }
  | { kind: "agent_working" }
  | { kind: "agent_waiting_input"; question?: string }
  | { kind: "agent_done" }
  | { kind: "agent_error"; message?: string }
  | { kind: "exited"; code: number };
```

Two things to notice:

- The plugin **does not own state**. The host keeps the event list and
  pane status; the plugin is a set of pure transforms + a React
  component. This is what makes hot-reload trivial — replace the
  module, re-render, done.
- The plugin **does not see PTY directly except via `raw()`**. The
  fancy renderer consumes structured events. If the plugin's parser
  hasn't seen events yet (e.g. a brand new pane), the renderer simply
  mounts `raw()` and the user sees terminal output until parser data
  flows in.

## 5. Migration path — extract Claude first

Concrete first delivery for M3:

1. Create `plugins/provider-claude/` in the repo (top-level, sibling to
   `src/` and `src-tauri/`).
2. Move:
   - `src/components/ChatView/parser.ts` → `provider-claude/src/parser.ts`
   - `src/components/ChatView/MessageBubble.tsx` parts that are
     Claude-specific → `provider-claude/src/renderer.tsx`
   - Hardcoded constants (`STATUS_MARKER`, slash command list) into
     the plugin
3. Leave `src/components/ChatView/ChatView.tsx` as the **host
   container** that picks a plugin by `pane.agent_type` and mounts
   `<plugin.Renderer />`. Falls back to xterm if no plugin matches.
4. Write `tests/snapshots/*` from real recent Claude sessions on dev
   machines. Run them in plugin CI.
5. Ship as workspace package; the build step bundles it into the
   app's static dir for now (later: load from
   `~/.codeterm/plugins/<id>/dist/plugin.js`).

This is also the dogfood for the plugin API — if any current Claude
behaviour can't be expressed via the interface above, the interface is
wrong and we adjust before publishing.

## 6. Relationship to existing Rust plugin system

The existing `src-tauri/src/plugin/` system stays. The relationship:

- A **bundled provider** like Claude currently registers a Rust
  `Plugin` that contributes an `AgentProvider` (process spawn, session
  file paths, etc.). That stays for the **backend half**.
- The new TS plugin bundles the **behaviour half** and ships as part of
  the same logical "Claude provider". For bundled providers (the three
  we ship) the TS plugin lives in the repo and is loaded from a
  built-in URL. For third-party providers, both halves can be shipped
  by the same author — the TS plugin in npm, the Rust plugin via the
  existing Process or Wasm tier when those land.
- **Practical v1:** all three current providers keep their Rust half as
  Native. Only the TS half becomes pluggable. We are not asking anyone
  to write Rust to add Claude renderer improvements.

## 7. Hot reload (dev)

In `./build.sh dev` mode:

- Vite watches `plugins/*/src/**`.
- On change, Vite re-bundles the affected plugin and HMR-pushes the new
  module to the WebView.
- The host's plugin loader unregisters the old plugin, registers the
  new one, and forces a re-render of any pane currently using it.
- **Plugin state survives** because state lives in the host (events,
  status) — only the function references change.

In prod, plugins are versioned bundles fetched from a CDN or the
peer's local plugins dir; updates happen on app launch by checking
`engine` compat and the published version.

## 8. Snapshot testing against real CLIs

Each plugin owns its own drift defence:

- `tests/snapshots/` holds JSONL fixtures captured from real provider
  runs (`tests/snapshots/claude-2026-05-17/session.jsonl`).
- Nightly CI:
  1. Spawn a fresh container with the latest provider CLI installed.
  2. Run a scripted scenario (ask a question, run a tool, cancel, ...).
  3. Capture the resulting JSONL.
  4. Run plugin against it, diff against last-known-good event stream.
  5. On diff: open a PR with the new fixture + the failing diff.
- A plugin only publishes a new version when its own CI is green.
  CodeTerm pulls the latest *compatible* (per `engine`) version.

This is the contract-test pattern. It moves the burden of "Claude
changed their format" from the codeterm release cycle to the plugin's
own cycle, which is a much faster loop.

## 9. Distribution and discovery

- **Bundled plugins** (Claude/Codex/Opencode for now): shipped in-tree,
  loaded from app static files. No user action.
- **User plugins**: dropped into `~/.codeterm/plugins/<id>/` (manually
  or via a `codeterm plugin install <name>` CLI). The daemon discovers
  on startup, validates `plugin.json`, makes them available.
- **Auto-update for bundled**: a tiny manifest at a known URL lists
  newest plugin versions compatible with the current codeterm build.
  On app start, codeterm fetches, compares, downloads if newer. No
  app rebuild. This is the win the user cares about — Claude changes
  their format Tuesday, a new plugin ships Wednesday, every codeterm
  user has it by Thursday without a release.

## 10. Acceptance criteria for M3

- [ ] `plugins/provider-claude/` package builds standalone (`pnpm
      build` inside the dir produces `dist/plugin.js`).
- [ ] Desktop CodeTerm loads it at startup; Claude panes render via
      the plugin renderer (the same fancy chat as today).
- [ ] Hot reload: editing `provider-claude/src/parser.ts` updates a
      live Claude pane in under a second, no daemon restart.
- [ ] Snapshot tests pass against ≥3 real Claude session captures.
- [ ] Codex pane still works (raw xterm fallback — no Codex plugin
      yet, but nothing should break).
- [ ] No reference to "claude" / "STATUS_MARKER" / Claude-specific
      strings remains in `src/components/ChatView/` outside the
      plugin host shell.

## 11. Open questions

1. **QuickJS or Boa or rquickjs for server-side runtime?** Decide at
   M4 when actually building it. rquickjs has the nicest Rust API.
2. **Plugin sandboxing in WebView.** WebView already isolates from
   filesystem. We expose a narrow host API
   (`host.transport.send(...)`, `host.config.get(...)`) and forbid
   arbitrary `fetch()` to non-tailnet origins. Enforcement: CSP +
   reviewed host API surface.
3. **Settings UI per plugin.** Reuse the existing declarative
   `SettingField` schema from `agent/provider.rs`? Or let TS plugins
   ship a React settings component? Lean: schema-driven for
   consistency, escape hatch later.
4. **Versioning the `ProviderPlugin` interface.** Use a `host_api`
   version field; plugins declare which versions they support; host
   refuses to load if mismatched. Same pattern as protocol version
   in transport.

## 12. First step

Inside `plugins/provider-claude/`, set up the package skeleton and
move `parser.ts` and the `STATUS_MARKER` constant. Get
`pnpm build` producing `dist/plugin.js`. Don't wire into the host
yet — just prove the extraction shape. Then in a follow-up:
write the host loader in `src/lib/plugins/` and switch `ChatView` to
use it.

<!-- revision: 1 -->
