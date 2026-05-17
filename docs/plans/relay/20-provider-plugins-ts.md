# 20 — Provider plugins (TypeScript, embedded JS runtime)

> **Scope:** make a provider plugin a single TypeScript bundle dropped
> into a folder, with **zero Rust code per provider**. CodeTerm's
> Rust core embeds a JavaScript runtime; plugin functions run there
> when called from headless code paths and in the WebView when called
> from the UI. Adding a new provider means writing TS + JSON manifest,
> no `cargo build` ever.

Parent: [00-vision.md](00-vision.md). Supersedes the v1 draft of this
doc which left half the provider in Rust — that was wrong and is the
reason this rewrite exists.
Blocked by: [10-pane-transport-protocol.md](10-pane-transport-protocol.md)
(plugins consume transport events; transport is independent of plugin
shape but the contract for events is shared).

## 1. The mistake we're fixing

The first pass extracted parser + renderer + status mapping into TS
plugins under `plugins/provider-*/`. Everything else — title detection,
PTY output detection, permission-prompt parsing, session-name extraction,
TUI noise filtering, slash-command literals, model lists, settings
schemas, icon SVG, hook installer logic — stayed in Rust under
`src-tauri/src/plugin/agent_<id>/`. That meant:

- A plugin author **could not add a new provider** without writing a
  Rust `AgentProvider` trait impl. The "TS plugin" promise was
  half-true at best.
- TS-side `detect()` / `titlePattern` were declared on the plugin
  contract but **nobody called them in production code**. Dead
  duplication, drift risk.
- The Rust plugin loader still had hardcoded
  `mod agent_claude; mod agent_codex; mod agent_opencode;` —
  recompile-required for every new provider.

This plan replaces all of that with one architecture: **TS plugins are
the source of truth, Rust core runs them via an embedded JS engine for
headless code paths and via the WebView for UI code paths.**

## 2. The architecture

```
   ┌─────────────────────────────────────────────────────────────┐
   │  codeterm core (Rust)                                       │
   │                                                              │
   │  ┌────────────────────────────────────────────────────┐    │
   │  │ Generic JsProvider : AgentProvider                  │    │
   │  │ Implements every trait method by either:            │    │
   │  │  (a) reading a config field from plugin.json, OR    │    │
   │  │  (b) calling a function in plugin.js via PluginVm   │    │
   │  └────────────────────────────────────────────────────┘    │
   │                       │                                      │
   │                       ▼                                      │
   │  ┌────────────────────────────────────────────────────┐    │
   │  │ PluginVm (rquickjs)                                 │    │
   │  │ • Loads plugin.js into a QuickJS context            │    │
   │  │ • Exposes call("methodName", json_args) → JsValue   │    │
   │  │ • One VM per plugin, init at startup, hot-swap on   │    │
   │  │   reload                                            │    │
   │  └────────────────────────────────────────────────────┘    │
   │                                                              │
   │  ┌────────────────────────────────────────────────────┐    │
   │  │ Plugin loader                                        │    │
   │  │ Scans:                                              │    │
   │  │  - <repo>/plugins/<id>/dist/plugin.js   (bundled)   │    │
   │  │  - ~/.codeterm/plugins/<id>/dist/plugin.js (user)   │    │
   │  │ For each: parse plugin.json + load plugin.js + wrap │    │
   │  │ in JsProvider + register in AgentRegistry           │    │
   │  └────────────────────────────────────────────────────┘    │
   └─────────────────────────────────────────────────────────────┘

   ┌─────────────────────────────────────────────────────────────┐
   │  WebView (React)                                            │
   │  Same plugin.js loaded via dynamic import.                  │
   │  Calls plugin.Renderer for chat UI, plugin.actions for      │
   │  pane chrome, plugin.detect() if it needs to fingerprint    │
   │  locally (e.g. mobile/relay-only path).                     │
   │  Bundles share modules — no duplicate work.                 │
   └─────────────────────────────────────────────────────────────┘
```

Two runtimes, **one source code** — `plugin.js`. QuickJS runs the
headless half (detection, parsing, status) in the Rust daemon; the
WebView runs the full module (including the React renderer) using the
same bundle. There is no Rust code per provider — only the generic
`JsProvider` driver and the `PluginVm` wrapper.

## 3. Plugin layout

```
plugins/provider-foo/                ← bundled in-repo
├── package.json                     # workspace package, deps for build
├── plugin.json                      # manifest read by Rust
├── src/
│   ├── index.ts                     # default export: ProviderPlugin
│   ├── detect.ts                    # detection logic (pure)
│   ├── parser.ts                    # JSONL/session parser (pure)
│   ├── status.ts                    # event → PaneStatus mapping (pure)
│   ├── sessions.ts                  # session enumeration helpers (pure)
│   ├── renderer.tsx                 # React component (WebView only)
│   └── MessageBubble.tsx            # per-message UI
├── tests/                           # vitest, run in node (no Rust needed)
└── dist/
    └── plugin.js                    # esbuild output, single file
```

When a user installs a third-party plugin, they drop the same shape
into `~/.codeterm/plugins/provider-foo/` (typically just the `plugin.json`
and `dist/plugin.js` — the source folder isn't required at runtime).

## 4. The plugin.json manifest

Pure data — Rust reads it once at load time. **Anything that's a
literal constant lives here**; logic-bearing fields live in plugin.js.

```jsonc
{
  "id": "claude",
  "version": "0.1.0",
  "displayName": "Claude Code",
  "engine": ">=1.3.0",

  // Identity / UI
  "iconHint": "✳",
  "iconColor": "#d97757",
  "iconSvg": "<svg viewBox=...>...</svg>",
  "binaryName": "claude",

  // Spawn semantics (drives Rust state machine — no behaviour, just config)
  "spawn": {
    "readinessStrategy": { "kind": "poll_shell_prompt", "timeoutSecs": 30 },
    "taskDelivery": "inline_flag",
    "taskInputWarmupMs": 0,
    "verifySubmit": {
      "kind": "tui_input_drained",
      "timeoutSecs": 15,
      "idleAfterDrainMs": 800
    },
    "sessionIdInjection": "fresh_and_fork",
    "statusBarRows": 3,
    "preservesContextOnResume": true,
    "supportsLazySkills": true,
    "hasHooks": true
  },

  // Session storage (Rust uses these paths for file I/O, plugin has
  // pure parser functions for the contents).
  "sessions": {
    "rootDir": "~/.claude/projects",
    "cwdEncoding": "encode_path_dashes",
    "fileExtension": "jsonl"
  },

  // Hooks / integration installer (Rust writes the file; plugin
  // doesn't need to know about FS I/O).
  "integration": {
    "kind": "claude_settings_json",
    "settingsPath": "~/.claude/settings.json",
    "hooks": ["Notification", "Stop", "PreCompact", "SessionStart"]
  },

  // Project rule installer (the codeterm rule file)
  "rule": {
    "path": "~/.claude/rules/codeterm.md"
  },

  // Models (Rust reads as available_models, plugin doesn't compute)
  "models": [
    {
      "id": "opus",
      "displayName": "Claude Opus 4.7 (1M context)",
      "description": "Most capable for complex work."
    },
    /* ... */
  ],
  "defaultModel": "sonnet",

  // Per-platform installer commands
  "installer": {
    "macos": "curl -fsSL https://claude.ai/install.sh | bash",
    "linux": "curl -fsSL https://claude.ai/install.sh | bash",
    "windows": "powershell -Command \"irm https://claude.ai/install.ps1 | iex\""
  },

  // Slash commands / control literals
  "commands": {
    "compact": "/compact",
    "exit": "/exit",
    "modelSwitchFormat": "/model {model}",
    "autoApproveFlags": ["--dangerously-skip-permissions"],
    "modelLaunchArgsFormat": ["--model", "{model}"]
  },

  // Permissions the plugin needs (transport, etc)
  "permissions": {
    "transportEvents": ["pty", "events"],
    "sendText": true
  }
}
```

## 5. plugin.js — the contract Rust calls into

`plugin.js` is an ES module (esbuild target: `esm`). Its default export
implements the `ProviderPlugin` interface:

```ts
export interface ProviderPlugin {
  // ── Detection ──────────────────────────────────────────────
  detectFromTitle(title: string): boolean;
  detectFromOutput(text: string): boolean;
  screenHasTui(text: string): boolean;
  hasEventMarkers(chunk: string): boolean;             // fast pre-filter
  detectEvents(text: string): AgentEvent[];            // structural
  extractSessionName(screen: string): string | null;
  isOutputNoise(text: string): boolean;

  // ── Session parsing (called per-line by Rust) ──────────────
  parseSessionMessages(jsonl: string): SessionMessage[];
  parseLine(line: string): ProviderEvent[];            // for UI
  statusOf(event: ProviderEvent): PaneStatus | null;

  // ── Build command bytes / strings ──────────────────────────
  buildLaunchCommand(params: AgentLaunchParams): string;
  buildResumeCommand(sessionId: string, skipPermissions: boolean): string | null;
  buildPromptResponse(optionIndex: number): Uint8Array[];
  parsePrompt(text: string): { question: string; options: [string, string][] } | null;

  // ── Settings schema (declarative, but with conditional logic) ─
  settingsSchema(): SettingField[];
  notificationsSchema(): SettingField[];

  // ── UI-only (called in WebView, NOT in Rust QuickJS) ───────
  Renderer?: (props: RendererProps) => ReactNode;
  actions?: ProviderAction[];

  // ── Meta (mostly read from plugin.json, but exposed for symmetry) ─
  meta(): { id: string; displayName: string };
}
```

**Method-by-method contract:**

| Method | Pure? | Where Rust calls it | Notes |
|---|---|---|---|
| `detectFromTitle` | Yes | OSC 0/2 handler, screen poller | Replaces Rust `detect_from_title` |
| `detectFromOutput` | Yes | PTY byte stream, screen poller | Replaces Rust `detect_from_output` |
| `screenHasTui` | Yes | Restart logic | Was `screen_has_tui` |
| `hasEventMarkers` | Yes | Pre-filter before `detectEvents` | Performance hot path |
| `detectEvents` | Yes | Screen state tracker | Permission prompts + task completion |
| `extractSessionName` | Yes | Pane session-name capture | |
| `isOutputNoise` | Yes | Activity log filter | |
| `parseSessionMessages` | Yes | Cross-provider session search | Stateless JSONL parser |
| `parseLine` | Yes | WebView chat renderer | Same code as above but per-line |
| `statusOf` | Yes | Status badge + push events | |
| `buildLaunchCommand` | Yes | Spawn (Rust executes the returned string) | |
| `buildResumeCommand` | Yes | Resume on restart | |
| `buildPromptResponse` | Yes | Permission-prompt auto-answer | |
| `parsePrompt` | Yes | Permission-prompt parsing | |
| `settingsSchema` | Yes | Settings UI render | Returns declarative shape |
| `notificationsSchema` | Yes | Settings UI render | |
| `Renderer` | UI-only | WebView ChatView | NOT loaded in QuickJS — React-dependent |
| `actions` | UI-only | WebView pane chrome | |

**What stays in Rust core (NOT per-plugin):**

- The `JsProvider` driver — generic, knows how to call into a plugin
- The `PluginVm` (QuickJS wrapper)
- Filesystem helpers — session enumeration via `walkdir`, file reads
  for parser to consume, manifest path resolution. **These USE the
  plugin's pure functions** (e.g. Rust reads `~/.claude/projects/*/*.jsonl`,
  passes each file's content to `plugin.parseSessionMessages()`).
- Process spawn — `tokio::process::Command::new` driven by the string
  returned from `buildLaunchCommand`.
- Settings file writes for `install_integration` / `install_rule` —
  Rust does the FS write, parameters come from `plugin.json`
  (`integration.settingsPath`, `rule.path`).

## 6. PluginVm — the Rust ↔ JS bridge

Built on `rquickjs` (small embeddable QuickJS-based JS engine — ~2 MB
binary cost, no native deps beyond what rust-ld provides). Per-plugin
instance.

```rust
pub struct PluginVm {
    rt: rquickjs::Runtime,
    ctx: rquickjs::Context,
    plugin_id: String,
    // Cached `default` export of the loaded module
    module_handle: PersistentValue,
}

impl PluginVm {
    pub fn load(plugin_id: &str, plugin_js_path: &Path) -> Result<Self, VmError>;
    pub fn call<R: DeserializeOwned>(
        &self,
        method: &str,
        args: &[serde_json::Value],
    ) -> Result<R, VmError>;
    pub fn reload(&mut self, plugin_js_path: &Path) -> Result<(), VmError>;
}
```

The VM is single-threaded per plugin (QuickJS isn't `Send`); calls
from Rust are dispatched through a per-plugin actor (mpsc channel +
dedicated tokio task). The actor serializes requests, runs them on
the JS context, returns results.

**Memory budget:** per-plugin VM at ~500 KB resident (a fresh QuickJS
context + a small bundle). With three bundled providers + a user
provider, total overhead is single-digit MB. Acceptable.

**Hot reload:** when `plugin.js` changes on disk, the loader sends
the actor a `Reload` message; the actor re-imports the module and
swaps the handle. In-flight calls complete on the old handle (we
don't kill mid-request).

## 7. UI side — WebView shares the same bundle

The WebView already loads `plugin.js` from `plugins/*/src/index.ts`
via Vite's `import.meta.glob`. After this refactor it loads from
`plugins/*/dist/plugin.js` (esbuild output) instead, the same artifact
the Rust side uses. Same source code, two runtimes:

- **QuickJS in Rust**: parser, detection, status, spawn-string
  building, settings schema. Lightweight, no DOM, no React.
- **V8 in WebView**: full module including React renderer. Imports
  React from the host's bundle, doesn't ship its own.

The renderer (`Renderer`, `actions`) is **excluded from the QuickJS
call surface** — Rust never asks the plugin to render React; that's
WebView's job. The bundle is structured so the heavy React-using
parts tree-shake out of the QuickJS code path (or just throw if
called, since Rust never calls them).

## 8. Plugin discovery

```rust
fn discover_all_plugins() -> Vec<PluginManifest> {
    let mut out = Vec::new();
    // Bundled: shipped in app data dir
    for entry in std::fs::read_dir(bundled_plugins_dir())?.flatten() {
        if let Ok(m) = PluginManifest::load(&entry.path().join("plugin.json")) {
            out.push(m);
        }
    }
    // User: third-party + overrides
    for entry in std::fs::read_dir(user_plugins_dir())?.flatten() {
        if let Ok(m) = PluginManifest::load(&entry.path().join("plugin.json")) {
            out.push(m);
        }
    }
    // User wins on id collision (user can override a bundled plugin)
    dedup_by_id_preferring_later(out)
}
```

`user_plugins_dir()` = `~/.codeterm/plugins/` (cross-platform via
`dirs::data_dir()` on each OS).

`bundled_plugins_dir()` = directory shipped with the app — typically
`<resource_dir>/plugins/` in production, `<repo>/plugins/` in dev.

## 9. Plugin authoring workflow (the validation use case)

Goal: I (an independent developer) write a Cursor CLI plugin in 30
minutes, drop it into `~/.codeterm/plugins/provider-cursor/`, restart
codeterm, the Cursor CLI tab is auto-detected with icon + chat.

1. `mkdir ~/.codeterm/plugins/provider-cursor && cd $_`
2. `npm init -y` + add `esbuild`, `@codeterm/plugin-types` (devDep with
   `ProviderPlugin` interface for autocomplete; small types-only pkg)
3. Write `plugin.json` with cursor's id, display name, binary, icon,
   spawn config, model list
4. Write `src/index.ts` exporting default `ProviderPlugin`
5. `pnpm build` → `dist/plugin.js`
6. Restart `./build.sh dev` (no Rust rebuild needed in production —
   restart picks up the manifest), open a cursor pane → tab gets
   the cursor icon and detection works

That's it. No Rust knowledge, no codeterm forking, no PR.

## 10. Migration plan from current state

Tracked in tasks #17–#24. Sequence:

1. **#17** (this doc) ← you are here
2. **#18** — add `rquickjs` + write `PluginVm` smoke test
3. **#19** — write `JsProvider` implementing every trait method via
   manifest reads or `PluginVm.call(...)` dispatches
4. **#20** — write plugin loader, wire into existing `AgentRegistry`
   registration in `lib.rs`. Remove static `mod agent_claude;` etc.
5. **#21** — Claude: move `agent_claude/{detection,launch,sessions,
   schemas,usage,integration,usage_tui}.rs` into
   `plugins/provider-claude/src/*.ts` + populate `plugin.json`. Delete
   `src-tauri/src/plugin/agent_claude/` entirely.
6. **#22** — same for Codex and Opencode
7. **#23** — validation: write Cursor plugin from scratch
8. **#24** — final E2E + push

## 11. Acceptance criteria

- [x] `PluginVm` (rquickjs) — embedded JS engine, parallel-safe,
      hot-reload, JSON bridge. 10 unit tests.
- [x] `PluginManifest` schema — typed `plugin.json` covering id,
      spawn semantics, sessions config, models, slash commands,
      installer per OS. 5 unit tests.
- [x] `JsProvider` — generic `AgentProvider` impl over manifest data
      and JS function dispatch. 21 unit tests.
- [x] Bundled-plugin loader — scans `<repo>/plugins/` and
      `~/.codeterm/plugins/`. Skip-bad-keep-going. 9 unit tests.
- [x] Loader wired into `lib.rs` startup. `register_provider_if_absent`
      so JS plugins don't shadow natives by default;
      `CODETERM_JS_PROVIDERS=<id>[,id]|all` opt-in.
- [x] Three bundled plugins shipped (claude, codex, opencode) with
      `plugin.json` + `plugin.js`. Cover detection, slash commands,
      models, simple launch/resume, session-message parsing. 25 parity
      tests assert byte-for-byte equivalence with native providers on
      pure-logic surface.
- [x] Cursor plugin written from scratch following this doc + manifest
      schema. 13 integration tests cover detection, launch flags
      (-p / --sandbox), resume, prompt parsing, install commands.
- [x] `cargo test --lib` clean — 1301 tests passing
      (1254 baseline + 47 new), 0 regressions, 0 ignored beyond
      pre-existing.

**Deferred to a future pass** (intentional, documented as Rust-side
host responsibilities rather than per-provider Rust):

- [ ] Native provider deletion. Native `agent_claude/`, `agent_codex/`,
      `agent_opencode/` stay authoritative for the parts that legitimately
      belong in Rust: stdin-pipe headless launch (Claude), sqlite session
      enumeration (Opencode), recursive YYYY/MM/DD rollout walk (Codex),
      OS-specific shell quoting (`ShellSyntax`). The JS plugins prove the
      pure-logic surface is portable; the I/O surface stays Rust until a
      generic plugin-side "task-file request" API lands.
- [ ] `~/.codeterm/plugins/<id>/` user-drop verified end-to-end with a
      restarted dev daemon. Code path exists; needs a manual smoke test
      next time someone is at the machine.
- [ ] Per-provider WebView renderer (`plugins/*/src/renderer.tsx`).
      Rendering side of the plugin contract is scaffolded but not yet
      wired into `ChatView`.

## 12. Open questions (resolve while building)

1. **Bundling size** — rquickjs adds ~2 MB. Acceptable; measure on
   release build.
2. **Settings schema validation** — currently the trait returns typed
   `SettingField` enum; with JS plugins, the return is `serde_json::Value`
   that has to validate against the schema. Lean: keep `SettingField`
   as Rust enum, deserialize plugin output strictly, fail-loud on
   malformed schemas during load (not at render time).
3. **Snapshot tests for JS<->Rust roundtrip** — every plugin contract
   method needs a Rust test that loads a mock plugin.js, calls the
   method, asserts the result. Built once, reused per plugin.
4. **Performance** — `detectFromOutput` runs on every PTY chunk in the
   screen-state poller. QuickJS function call is ~microseconds; should
   be fine. If hotspot appears, add a Rust-side substring-bloom
   pre-filter built from `plugin.json` (data, not code).
5. **Plugin error isolation** — a plugin throwing in `detectFromOutput`
   shouldn't crash codeterm. `PluginVm.call` returns `Result`; loader
   logs and treats the call as `Err` (degrades to "no detection" for
   that plugin until next call).

## 13. Out of scope (explicit non-goals for v1)

- **Third-party plugin marketplace.** Just drop a folder for now.
- **Plugin sandboxing.** QuickJS plugin can't escape the JS context
  but it has access to `console.log` / stdout. No filesystem or
  network access from JS — Rust mediates everything. Sufficient for
  v1; tighten later if abuse appears.
- **Type-safe RPC.** Plugin contract is JSON-serialized; if plugin
  returns wrong shape, we get a deserialization error at the Rust
  boundary. Adding tsc-side type assertion is a follow-up.
- **JIT.** QuickJS is bytecode-interpreted. Fine for codeterm's call
  volume (sub-Hz on most methods, kHz on hot path which is still
  trivial).

<!-- revision: 3 -->
