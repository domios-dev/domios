# CodeTerm Plugins

TypeScript packages that extend CodeTerm without rebuilding the Rust
binary. The default workflow ships three bundled provider plugins:

| Package | What it covers |
|---|---|
| `provider-claude` | Claude Code JSONL parser, MessageBubble renderer, slash commands |
| `provider-codex` | Codex CLI detection + raw fallback |
| `provider-opencode` | sst/opencode CLI detection + raw fallback |

User-installed plugins are planned for later — for now plugins live in
this repo's `plugins/` workspace and ship with the app.

## Architecture in one diagram

```
        codeterm-daemon (Rust, thin)            React UI (Vite, WebView)
        ┌───────────────────────┐               ┌──────────────────────┐
        │ PTY spawn / file I/O  │               │ ChatView / Sidebar / │
        │ AgentRegistry trait   │   transport   │ Settings / ...       │
        │  (process spawn,      │ ◀─────────▶  │                      │
        │   session file paths) │               │  plugin registry  ───┼─→  ProviderPlugin
        └───────────────────────┘               └──────────────────────┘    (TS, hot-reload)
                                                                              ↑
                                                                              │
                                                                            plugins/<id>/
                                                                              src/index.ts
```

Two halves of "a provider":

- **Backend half** in `src-tauri/src/plugin/agent_<id>/` — process spawn,
  session file paths, OS introspection. Rust. Only changes when a
  new provider's CLI behaves fundamentally differently from the three
  we already support.
- **Behaviour half** in `plugins/provider-<id>/` — JSONL parser,
  detection patterns, renderer, actions, status mapping. TypeScript.
  Where almost all real work happens — Claude changes their JSONL?
  Edit the parser here, hot-reload, done. No Rust rebuild.

## Creating a new provider plugin

### 1. Scaffold the package

```bash
mkdir -p plugins/provider-foo/src plugins/provider-foo/tests
cd plugins/provider-foo
```

`plugins/provider-foo/package.json`:

```json
{
  "name": "@codeterm/provider-foo",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "main": "src/index.ts",
  "types": "src/index.ts",
  "exports": {
    ".": "./src/index.ts"
  },
  "peerDependencies": {
    "react": "^19.0.0"
  }
}
```

`plugins/provider-foo/plugin.json` (manifest — read by the host loader):

```json
{
  "id": "provider-foo",
  "kind": "provider",
  "provider_id": "foo",
  "displayName": "Foo CLI",
  "engine": ">=1.3.0",
  "entry": "src/index.ts",
  "permissions": {
    "transport_events": ["pty"],
    "send_text": true
  }
}
```

`plugins/provider-foo/src/index.ts`:

```ts
import type {
  ProviderPlugin,
  RendererProps,
} from "../../../src/lib/plugins/types";

function Renderer({ raw }: RendererProps) {
  return raw();  // start with xterm fallback; build a fancy renderer later
}

const plugin: ProviderPlugin = {
  meta: { id: "foo", displayName: "Foo CLI" },
  detect: (sample) => sample.bytes.includes("foo>") || /foo/i.test(sample.title ?? ""),
  parseLine: () => [],         // no JSONL parser yet
  statusOf: () => null,
  Renderer,
  actions: [],
  titlePattern: /foo/i,
  noisePatterns: [/^[\s─━═]+$/],
};

export default plugin;
```

### 2. Register in root workspace

Add to root `package.json` → `dependencies`:

```json
"@codeterm/provider-foo": "workspace:*"
```

Then `pnpm install` — symlink appears in `node_modules/@codeterm/`.

### 3. Loader picks it up automatically

The plugin loader at `src/lib/plugins/loader.ts` discovers everything
matching `plugins/*/src/index.ts` via Vite's `import.meta.glob`. No
loader code changes needed when adding a new plugin.

Restart `./build.sh dev` once (Rust needs to recompile only because of
manifest changes); after that all plugin edits hot-reload.

### 4. Add the Rust backend-half (only if the new provider needs custom
spawn / session file logic)

If your provider just launches a CLI and lets CodeTerm observe the PTY,
you don't need a Rust plugin — the existing generic process spawning
covers it. You only need a Rust-side `AgentProvider` impl if your
provider has:

- A non-trivial launch command (env tweaks, wrapper scripts)
- On-disk session files CodeTerm should index for search
- Provider-specific resume flags
- Custom "skill" delivery to provider's config dir

For those, copy `src-tauri/src/plugin/agent_codex/` as a template and
register in `src-tauri/src/plugin/mod.rs`. Backend changes do require
Rust rebuild.

## The ProviderPlugin contract

Defined in `src/lib/plugins/types.ts`. Stable API surface — additions
are non-breaking, removals/semantic changes bump the host API version.

```ts
interface ProviderPlugin {
  meta: { id: string; displayName: string };

  detect(sample: { bytes: string; title?: string }): boolean;
  parseLine(line: string): ProviderEvent[];
  statusOf(event: ProviderEvent): PaneStatus | null;
  Renderer: (props: RendererProps) => ReactNode;
  actions: ProviderAction[];

  noisePatterns?: RegExp[];
  titlePattern?: RegExp;
  imageSend?: { method: "clipboard-paste" | "path"; pasteKeystroke?: string };
}
```

### Per-method contract

- `detect(sample)` — cheap structural fingerprint. Called on every fresh
  pane during warm-up. **No I/O, no allocations beyond regex matching.**
- `parseLine(line)` — one JSONL line in, zero or many `ProviderEvent`s
  out. Stateless — receives raw text, returns structured events. Carry
  enough context in `payload` so `statusOf` and `Renderer` don't need
  external state.
- `statusOf(event)` — map ONE event to a coarse pane status, or `null`
  to leave the previous status untouched. Used for tab badges + push
  notifications.
- `Renderer(props)` — React component. Receives the event list, the
  pane status, a `send()` to write back to the PTY, and a `raw()`
  helper that mounts xterm.js if you want a "raw view" toggle.
- `actions[]` — slash commands / buttons in pane chrome. `run()` is
  pure: returns the text to send, host calls `send()` for you.

## Testing

### Smoke / unit tests

Each plugin should have `tests/*.test.ts` with parser fixtures from
real provider sessions. Run via:

```bash
npx tsx plugins/provider-<id>/tests/parser.test.ts
```

The current test runner is a tiny `assert`/`eq` shim inside each test
file (legacy from before the workspace). Migrating to `vitest` is
planned — when done, `pnpm -F @codeterm/provider-<id> test` will work.

### Capturing real fixtures

The cheapest way to get a snapshot to test against:

```bash
# Run the provider in a CodeTerm dev pane, capture the JSONL it produces
cat ~/.<provider-config-dir>/sessions/<session-id>.jsonl > plugins/provider-<id>/tests/snapshots/<scenario>.jsonl
```

Add a test that runs `parseJsonlSession()` against the fixture and
compares the resulting event count + shapes. Real Claude/Codex/Opencode
session files are the source of truth for "does our parser still work?"

### Drift defence (planned)

Nightly CI will run real CLIs in a container, capture JSONL, and diff
against last-known-good snapshots. Plugin can ship a new version only
when its own CI is green. Plan: `docs/plans/relay/20-provider-plugins-ts.md` §8.

## Hot reload in dev

`./build.sh dev` runs Vite. Vite watches `plugins/*/src/**` along
with the main `src/`. On edit:

1. Vite re-bundles the affected plugin module (~10ms).
2. `import.meta.hot.accept()` in `src/lib/plugins/loader.ts` re-imports.
3. `register()` overwrites the old plugin reference.
4. Components subscribed via `useRegistryVersion()` re-render with the
   new functions.
5. **Pane state (events, status) is preserved** because it lives in the
   host, not the plugin.

If you don't see hot reload working: open browser devtools, look for
`[plugins] registered <id> (v<N>)` in the console — the version should
bump on each edit.

## Other plugin kinds (future)

The current contract is `ProviderPlugin` only. The plugin system is
designed to be extended with more kinds (theme plugins, command
plugins, transport adapters, etc) by:

1. Adding a new contract interface in `src/lib/plugins/types.ts`
   alongside `ProviderPlugin`.
2. Updating `plugin.json` to discriminate via `kind: "provider" | "theme" | ...`.
3. Adding a kind-aware route in `src/lib/plugins/loader.ts`.

The registry, the workspace, and the loader are kind-agnostic — they
just register objects with a `meta.id`. The only thing tied to
"provider" today is the call sites that consume `ProviderPlugin`-shaped
data (ChatView, agentProviders adapter, Sidebar).

## File layout reference

```
plugins/
├── README.md                           ← this file
└── provider-claude/                    ← one example plugin
    ├── package.json                    ← workspace package
    ├── plugin.json                     ← manifest
    ├── src/
    │   ├── index.ts                    ← ProviderPlugin default export
    │   ├── detect.ts                   ← title regex + PTY fingerprint
    │   ├── parser.ts                   ← JSONL → ChatMessage parser
    │   ├── status.ts                   ← event → PaneStatus mapping
    │   ├── renderer.tsx                ← React renderer
    │   └── MessageBubble.tsx           ← per-message component
    └── tests/
        ├── parser.test.ts              ← unit tests
        └── snapshots/                  ← real session fixtures
```

## What's intentionally NOT in plugins

To keep the plugin contract honest:

- **Process spawning** — stays in Rust. Generic per-OS, not provider-
  specific.
- **Tauri commands** — stay in Rust. Plugins can call them via the
  host's exposed API (`invoke()`), not redefine them.
- **PTY rendering primitives** — `xterm.js` is mounted by the host
  via `raw()`. Plugins consume the rendered view, not the byte stream
  directly (you're free to subscribe to events though).
- **Mesh transport** — stays in Rust. Plugins are local to the WebView
  for now; the transport layer routes events but doesn't know about
  plugins.

## Status

| Capability | Status |
|---|---|
| TS workspace + Vite HMR | ✅ shipped |
| Plugin registry + loader | ✅ shipped |
| Three bundled providers extracted | ✅ shipped |
| Per-plugin tests via `npx tsx` | ✅ shipped |
| Per-plugin vitest config | ⏳ planned |
| `codeterm plugin install` CLI | ⏳ planned |
| Hot-reload from `~/.codeterm/plugins/` | ⏳ planned |
| Server-side plugin code (QuickJS) | ⏳ planned (M4) |
| Snapshot CI for JSONL drift | ⏳ planned |
| Non-provider plugin kinds | 🟡 contract-only (extensible by design) |

<!-- revision: 1 -->
