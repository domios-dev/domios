# CodeTerm Plugins — Authoring & Maintenance Guide

CodeTerm's features live in **plugins**. The host (Tauri/Rust + React) keeps only
the *capability seams* — traits, registries, the JS runtime, and the iframe-view
host/bridge. Everything feature-specific — Git, Bitwarden, the Transcriber, and the
agent providers (Claude/Codex/OpenCode) including their UI — is a plugin.

This guide reflects **what exists in the code today**.

## Anatomy of a plugin

```
plugins/<id>/
  plugin.json          # manifest: id, capabilities, permissions, icon, settings schema
  plugin.js            # logic — a QuickJS module: module.exports.default = { ...capability methods }
  settings.schema.json # optional — JSON-schema for the settings panel
  ui/                  # optional — the plugin's own UI (for the `view` capability)
    src/main.tsx       # React entry, mounts on #ct-root
    index.html         # BUILT bundle (esbuild) — committed; do not hand-edit
  tests/ or *.test.cjs # plugin tests (run via tsx / scripts/plugin-tests.sh)
```

A plugin has up to **two halves**:

1. **Logic** — `plugin.js`, run in a confined **QuickJS** VM. No DOM, no network of
   its own, synchronous. It talks to the host only through the injected `host.*` API.
2. **UI** (optional) — a React app in `ui/`, rendered in a **sandboxed iframe**
   (origin-null, CSP-locked). It talks only to its own logic half, via a
   `window.ct.invoke()` postMessage bridge the host relays to `viewCall`.

This mirrors the VS Code / Figma model: untrusted plugin code in a sandbox, a narrow
audited channel to the host, and a separately-sandboxed UI.

## `plugin.json` manifest

```jsonc
{
  "id": "git",                 // stable id; matches the plugins/<id>/ dir
  "version": "0.1.0",
  "minCodeterm": "1.0.0",      // compatibility gate (see "Maintenance")
  "hostApi": 2,                // host-API contract version the plugin targets
  "displayName": "Git",
  "description": "One-line summary shown in the Extensions detail pane.",
  "homepage": "https://…",     // optional project/docs link
  "readme": "README.md",       // optional relative markdown path; loaded into the snapshot (bounded) and rendered safely (no raw HTML, remote images not fetched)
  "iconColor": "#f05133",
  "iconHint": "✳",             // text fallback when no svg
  "iconSvg": "<svg …>",        // inline icon shown in the Extensions UI + bubbles
  "entry": "plugin.js",
  "capabilities": {
    "agentProvider": false,
    "secretBackend": false,
    "transcriber": false,
    "glanceView": true,        // a popover summary (ViewNode tree)
    "statusBubble": true,      // a pane-footer pill
    "view": true               // a full sandboxed-iframe UI
  },
  "permissions": {
    "subprocess": { "allow": ["git"] },     // exact binaries host.exec may run
    "network":    { "allow": ["vault.bitwarden.com"] },  // hosts host.fetch may reach
    "secrets":    true                       // may use host.secretGet/Set
  },
  "settingsSchema": "settings.schema.json"
}
```

Capabilities are **booleans the host reads at registration**. Each true flag wires the
plugin into the matching registry. Permissions are the confinement contract for the
host API (subprocess allow-list, network allow-list, secrets access).

## Capabilities and the methods they expect

Every capability is one Rust trait + a `CapabilityRegistry<dyn Trait>` + a `Js*` adapter
that calls the corresponding `plugin.js` export. The exports a plugin provides:

| Capability     | `plugin.js` export(s) | Returns |
|----------------|-----------------------|---------|
| `statusBubble` | `statusBubble(ctx)`   | `{ label, tone, icon?, tooltip?, action? }` or `null` |
| `glanceView`   | `renderGlance(ctx)`   | `{ title, nodes: ViewNode[] }` |
| `view`         | `viewCall(method,args)` | any JSON (the iframe UI calls this via `window.ct.invoke`) |
| `secretBackend`| `secretStatus`, `secretUnlock`, `secretGet`, `secretSet`, … | provider-specific |
| `transcriber`  | `transcribe(opts)`    | `{ text }` |
| `agentProvider`| `detect`, `parseSessionDelta`, `buildLaunchCommand`, `checkIntegration`, `installIntegration`, … | see `plugins/provider-*/` |

`ctx` for bubble/glance carries the pane's `cwd`. `action: "openPanel:view:<id>"` opens
the plugin's own iframe view; `"openPanel:<name>"` opens a built-in panel.

### Host API (`host.*`, synchronous, inside QuickJS)

- `host.exec(json)` → run an allow-listed subprocess (`{bin, args}`) → `{stdout, stderr, code}`.
- `host.fetch(json)` → HTTP to an allow-listed host.
- `host.fs.{fileExists, readFile, readJson, writeFile, writeJson}`, `host.makeDirs`.
- `host.secretGet(key)` / `host.secretSet(key, value)` (needs `permissions.secrets`).
- `host.settingsJson()` → the plugin's settings bag (`config.plugins.<id>`).
- `host.platform()`, `host.homeDir()`, `host.expandHome(path)`.
- `host.shell.quote(s)` / `host.shell.quoteFor(s, platform)`.
- `host.log(level, msg)`.

QuickJS is **not** a browser: there is no `console` binding by default, no `fetch`, no
timers. Anything not in `host.*` does not exist — a stray `console.warn` once aborted
parsing with a silent `ReferenceError`. Keep `plugin.js` to the host surface.

## The `view` capability (sandboxed-iframe UI)

The richest capability. Flow:

1. `plugin.json` declares `"view": true` and ships a `ui/` bundle.
2. The host serves `ui/index.html` from `/api/plugins/<id>/ui/...` (query-token-gated,
   exempt from Bearer auth because an iframe can't send headers).
3. On load the host injects, per request: a fresh **nonce**, a strict **CSP**
   (`default-src 'none'; script-src 'nonce-N'; connect-src 'none'; …`), and the
   **bridge shim** that defines `window.ct`.
4. The iframe is `sandbox="allow-scripts"` (origin null) — so it has **no** same-origin
   storage, no cookies, no network of its own. Its only channel is
   `parent.postMessage`.
5. `window.ct.invoke(method, args)` → postMessage `{__ctInvoke}` → `PluginView` (host)
   → `invoke("plugin_view_call", {pluginId, method, args})` → the plugin's
   `viewCall(method, args)` → result posted back as `{__ctReply}`.
6. `window.ct.close()` → `{__ctClose}` → the host dismisses the view. (Esc also closes.)
7. The host pushes theme CSS vars into the iframe on `{__ctReady}`.

**The plugin owns its own chrome** (header, close button). The host renders no
"back" bar — the view is just the iframe.

### Where the iframe loads from (all access modes)

`PluginView` resolves the daemon origin via `daemonHttpBase()`/`daemonAuthToken()`
(`src/lib/webApi.ts`) — never from the daemon's bind address. This is the single source
of truth and must stay so:

- **Web** → `apiHost()` + `currentToken()`: the exact origin the SPA is talking to —
  works for `localhost`, a LAN/**Tailscale IP** (which can change), **Tailscale serve**
  (`https://host.ts.net`, no port), and a peer-switched session.
- **Desktop (Tauri)** → the local daemon (`127.0.0.1:<config.api.port>`) + config token,
  because the app shell runs at a `tauri://` / Vite origin and only the daemon serves
  plugin routes.

## Creating a plugin

1. `mkdir plugins/<id>` and write `plugin.json` (id, capabilities, permissions, icon).
2. Write `plugin.js`:
   ```js
   function git(cwd, args) { return JSON.parse(host.exec(JSON.stringify({ bin: "git", args: ["-C", cwd].concat(args) }))); }
   function statusBubble(ctx) {
     if (!ctx || !ctx.cwd) return null;
     // … shell git, build the pill …
     return { label: branch, tone: "default", icon: "git", action: "openPanel:view:git" };
   }
   module.exports.default = { statusBubble: statusBubble /*, renderGlance, viewCall, … */ };
   ```
3. If it has a UI: `plugins/<id>/ui/src/main.tsx` (React, `createRoot(document.getElementById("ct-root"))`),
   talk to the logic half via `window.ct.invoke("<method>", args)`, then build:
   ```
   node scripts/build-plugin-ui.mjs <id>     # esbuild → plugins/<id>/ui/index.html
   ```
   The bundle is self-contained (React inlined); the iframe cannot fetch app code.
4. Tests: a `*.test.cjs` for pure parsers (run with `npx tsx`), and a `tests/*.test.ts`
   for contract/behavior. Wire them into `scripts/plugin-tests.sh`.

### UI gotchas (the sandbox is strict)

- **No `localStorage`/`sessionStorage`/`document.cookie`.** Touching them throws
  `SecurityError` in the origin-null iframe and crashes the React render → blank panel.
  Guard every access with `try/catch` and degrade to in-memory.
- **No app imports.** The iframe bundle can't import from `src/`. Inline the types,
  icons, and CSS it needs (no Tailwind compiler runs in the bundle — ship the CSS).
- **Theme** comes via the `{__ctTheme}` push as `--ct-*` CSS vars; use those for color.

## Maintenance

- **Versioning.** Provider plugins (`claude`/`codex`/`opencode`) are *intrinsic*: bundled
  into the binary (`build.rs` → `embedded::bootstrap_into`) and updatable without
  reinstalling via the `codeterm-core` channel. While they're core, their versions mirror
  the app version: after bumping the root `package.json`, run `npm run sync:versions` and
  verify with `npm run check:versions` (covers `plugins/provider-*/plugin.json`,
  `packages/*`, and `plugins/channel.json`).
- **Compatibility gate.** `minCodeterm` in `plugin.json`/`channel.json` is checked on
  install/update against `CODETERM_VERSION`; incompatible versions are refused with a
  reason (the UI shows "needs CodeTerm ≥ X").
- **Hot-reload caveat.** The plugin watcher reloads `plugin.js` and re-registers **agent
  providers** live, but the other capability registries (`statusBubble`, `glanceView`,
  `view`, `secretBackend`, `transcriber`) are populated at startup and are **not**
  refreshed on reload — a new/changed capability needs a daemon restart to take effect.
- **Asset caching.** The plugin-UI route sends `Cache-Control: no-store` so the embedding
  WebView (WKWebView caches aggressively) never serves a stale bundle after a rebuild.
- **No `X-Frame-Options`** on the plugin-UI route: the app embeds the iframe *cross-origin*
  (desktop shell ≠ daemon origin), so `SAMEORIGIN` would blank it. Framing is safe anyway
  — sandboxed, CSP-locked, token-gated; a foreign framer gets an inert document.

## Testing

- `npx tsx plugins/<id>/plugin.test.cjs` — pure parser/logic tests (mock `host`).
- `bash scripts/plugin-tests.sh` — aggregate runner for all plugin suites.
- `src-tauri/src/plugin/js_runtime/parity_test.rs` — for any `AgentProvider` method a JS
  drop-in implements, asserts the native and JS paths agree on shared fixtures.
- Live: `curl --noproxy '*' -H "Authorization: Bearer <token>" \
  http://127.0.0.1:7686/api/plugins/snapshot` lists plugins + capabilities + load errors;
  `POST /api/plugins/{bubble,glance,view-call}` exercises the capabilities.

## Development approach — the load-bearing principles

The provider/plugin boundary stays clean only if every change crossing it pays the same
tax (see also `docs/contributing/architecture.md`):

1. **Core asks capabilities, never identities.** Branch on `supports_chat_ui()` /
   `available_models()` / a new capability fn — never on a hardcoded provider-id list or
   display string. `if id == "codex"` in core code is a leak.
2. **Native + JS parity in lockstep.** Every trait method a JS drop-in can implement gains
   a `parity_test.rs` case, or it silently drifts on drop-ins.
3. **Detection keys off the pane's OWN session.** Read only the record for this pane's
   claimed session id — never "newest for cwd". Unknown → return null (honest) over a
   guess (contaminating).
4. **Audit the host-API surface when it grows.** Every new `host.*` function is a new
   confinement hole — justify it against the FS/subprocess/network sandbox.
5. **One source of truth for the daemon origin** (`daemonHttpBase`), reused by every
   iframe and fetch, so all access modes (localhost / Tailscale IP / serve / peer /
   desktop) work without per-call host strings.

### Lessons paid for in this migration (don't relearn them)

- A sandboxed iframe that touches `localStorage` crashes on mount — guard browser APIs.
- WKWebView serves a stale iframe without `no-store`; a plugin-UI fix won't reach the
  desktop until the route forbids caching.
- `X-Frame-Options: SAMEORIGIN` blanks every cross-origin embed (i.e. always, on desktop).
- The iframe must load from the *connected* origin, not the daemon's `0.0.0.0` bind addr,
  or remote web / Tailscale loads the client's own `127.0.0.1` and fails.
- Capability registries don't hot-reload — restart to register a new capability.
- A plugin-launched agent's hooks inherit the daemon's **process env**; if the daemon
  inherited a parent's `CODETERM_API_PORT` (e.g. prod's 7685 when launched from prod),
  the hook reports to the wrong daemon. Inject env from **config**, not `std::env`.

<!-- revision: 5 -->
