# Plugins Rethink — Capability Plugins, Transcriber & Bitwarden

**Status:** Design approved (brainstorm 2026-06-15/16). Build target.
**Branch:** `plugins-rethink`.
**Scope this cycle:** the plugin **capability system**, the **Transcriber** plugin,
the **Bitwarden** plugin, and the **Extensions** UX surface. The external
**connector** path (e.g. `dread-delusion-ai`) is **out of scope this cycle** — but
every abstraction here must leave a clean seam for it (see §12).

---

## 1. Problem

CodeTerm already has the bones of a plugin system — a `PluginManager`, a TS/QuickJS
runtime (`JsProvider` implementing `AgentProvider`), a manifest, channels, bundle
hashing, and a `system_plugins` concept. But the only thing a plugin can *be* today
is an **agent provider**. Two features we want are not agent providers:

- **Transcriber** — speech→text. Today this is hardcoded: `voice_transcribe.rs`
  reads `config.integrations.voice_transcribe.daemon_url` and POSTs to it. There is
  no way to ship it as an installable, configurable, mesh-aware plugin.
- **Bitwarden** — a secret backend. Today `secrets::factory` has a *static* registry
  of three compiled-in backends (`local`/`bitwarden`/`mock`). Bitwarden is good Rust
  behind a clean `SecretStore` trait, but it is welded into the binary and cannot be
  a channel plugin.

Both reveal the same missing abstraction: **a plugin should be able to contribute a
*capability* other than "agent provider", and a third-party (non-system) plugin loaded
from a channel must be able to actually *do the work* that capability requires** —
which today it cannot, because the JS host API has **no subprocess and no network**
(confirmed: `host_api.rs` exposes fs/shell-quote/sqlite/crypto/logging only).

This spec generalizes "plugin = agent provider" into "plugin = one or more
**capabilities**", each backed by a core trait and a dynamic registry, and extends the
host API just enough — under explicit, granted permissions — for real plugins to run.

## 2. The core abstraction (this is the whole thing)

```
            core defines a TRAIT          a DYNAMIC REGISTRY        a JS ADAPTER wraps plugin.js
capability  ───────────────────────       ──────────────────       ────────────────────────────
agent       AgentProvider (exists)        agent::registry          JsProvider          (exists)
secret      SecretStore   (exists)        secrets::registry  ◄new   JsSecretBackend     ◄new
transcribe  Transcriber   ◄new            transcribe::registry ◄new JsTranscriber       ◄new
            (future: pane_type, tool_provider, hook, …)
```

**Invariants (non-negotiable — these are the load-bearing rules):**

1. **Core asks capabilities, never identities.** Core code branches on
   `registry.get(capability)` / trait methods — never on a hardcoded plugin-id list or
   display string. (Same rule the architecture doc already enforces for providers.)
2. **One trait per capability, owned by core.** A plugin *implements* a capability; it
   never invents the contract. New capability = new trait + new registry + new JS adapter,
   added to core deliberately, with a parity test.
3. **Registries are dynamic.** A capability registry maps `id → impl` and is populated
   at runtime by the `PluginManager` as plugins activate/deactivate — not a compile-time
   `OnceLock` table. (`agent::registry` already works this way; `secrets::factory` does
   not yet — that's the gap.)
4. **A JS plugin is confined by its manifest + the host-API sandbox**, not by an API
   token. Tokens are for out-of-process actors (connectors), which we are not building
   this cycle (§12).
5. **Native + JS parity in lockstep.** Any capability a JS drop-in can implement gets a
   `parity_test.rs` case proving native and JS agree on shared fixtures.

### 2.1 Capability traits

`AgentProvider` and `SecretStore` exist unchanged. We add:

```rust
// src-tauri/src/transcribe/mod.rs  (new module — promotes bridge/voice_transcribe.rs)
#[async_trait]
pub trait Transcriber: Send + Sync {
    fn id(&self) -> &str;
    async fn status(&self) -> TranscriberStatus;        // Ready | Loading | Unloaded | Unavailable{reason}
    /// Transcribe an audio file already on disk. `language` is a hint (may be "").
    async fn transcribe(&self, audio_path: &Path, language: &str)
        -> Result<Transcript, TranscribeError>;
}
pub struct Transcript { pub text: String }
```

`TranscribeError` is the existing enum, moved into this module. The trait is engine-
agnostic: a local-whisper implementation, a mesh-delegating implementation, and the JS
adapter all satisfy it identically.

### 2.2 Dynamic registries

A capability registry is a thin, lock-guarded map shaped like the existing agent
registry:

```rust
pub struct CapabilityRegistry<T: ?Sized> { entries: RwLock<HashMap<String, Arc<T>>> }
impl<T> CapabilityRegistry<T> {
    fn register(&self, id: impl Into<String>, imp: Arc<T>);
    fn unregister(&self, id: &str);
    fn get(&self, id: &str) -> Option<Arc<T>>;
    fn active(&self) -> Vec<Arc<T>>;
}
```

- `secrets`: replace the static `OnceLock` factory table with a `CapabilityRegistry<dyn SecretStore>`-style dynamic registry. **The three built-ins (`local`/`keychain`/`mock`) register themselves at startup; Bitwarden registers when its plugin activates.** Backend selection (`config.integrations.secrets.backend`) resolves through the registry; unknown id → fall back to `local` (today's behavior preserved).
- `transcribe`: new registry. The active transcriber is chosen by config
  (`integrations.transcribe.active = "<plugin-id>"`, default none). `voice_transcribe::format_attachment_line` resolves the active transcriber from the registry instead of reading `daemon_url` directly.
- `agent`: already dynamic; no change beyond consistency review.

### 2.3 JS capability adapters

Just as `JsProvider` implements `AgentProvider` by calling exported `plugin.js`
functions, we add two adapters that wrap the *same* `PluginVm`:

- `JsSecretBackend` implements `SecretStore` — dispatches `status/unlock/lock/save/get/list/delete/...` to `plugin.js` exports (`secretStatus`, `secretGet`, …). Returns structured JSON validated against the trait's types.
- `JsTranscriber` implements `Transcriber` — dispatches `status/transcribe` to exports (`transcribeStatus`, `transcribe`).

The manifest's `capabilities` field declares which adapter(s) to build for a plugin
(`capabilities.secret_backend = true`, `capabilities.transcriber = true`,
`capabilities.agent_provider = true`). The loader builds the adapter, the `PluginManager`
registers it into the matching registry on activate and unregisters on deactivate.

## 3. Confinement model

Two models; **we build only the in-process one this cycle.**

| | In-process (JS / native) | Out-of-process (connector, process-tier) |
|---|---|---|
| Confined by | manifest permissions + host-API sandbox | scoped token via `check_access` |
| Token | none | `CallerIdentity::Plugin/Connector` (deferred §12) |
| Examples | Transcriber, Bitwarden | Dread Delusion AI (later) |

In-process plugins never receive `CODETERM_API_TOKEN`; they call the host API directly
and are gated by the permissions the user granted at install (Screen 02 of the design).

## 4. Host API v2 — the flexibility the plugins actually need

The JS host API gains three permissioned function groups. **Every new function is gated
by a manifest permission and the existing `PluginSandbox`; bundled/system plugins get the
unrestricted sandbox, third-party plugins get the restricted one.** `HOST_API_VERSION`
bumps `1 → 2`; third-party plugins declaring `host_api: 2` get the new surface, older
plugins keep v1 behavior (conformance gate already exists).

> **Sync-blocking (load-bearing — gaps G1 + G8a):** QuickJS plugin methods are
> **synchronous** — `vm.rs::call_json` rejects any returned Promise. So `host.exec` and
> `host.fetch` are **synchronous host functions** (`std::process::Command`,
> `reqwest::blocking`) that block the calling thread until done, then return — consistent
> with the existing sync `readFile`/`sqlite` host calls.
>
> **The VM is NOT `Send`** (rquickjs `Runtime`/`Context` are not `Send`; `PluginVm` mutex-
> guards them). So `spawn_blocking` is **impossible**. The async `Transcriber`/`SecretStore`
> adapter methods call the sync `call_json` **directly on the calling tokio thread —
> exactly as `JsProvider` already bridges its trait methods to the VM**. The brief I/O
> block (subprocess / HTTP) on that thread is acceptable. `JsTranscriber`/`JsSecretBackend`
> MUST mirror `JsProvider`'s exact call mechanism — do not invent a threading model.

### 4.1 Subprocess — `host.exec` (permission: `subprocess`)

```ts
host.exec({ bin: "bw", args: ["list","items","--nointeraction","--response"],
            env?: {BW_SESSION:"…"}, stdin?: string, timeoutMs?: number })
  : { code: number, stdout: string, stderr: string }
```

- Gated by `permissions.subprocess.allow = ["bw", …]` — an **allowlist of binaries**.
  No allowlist entry → `exec` throws. Restricted sandbox additionally resolves the
  binary via the same `which` + platform-fallback logic `bitwarden/subprocess.rs` uses
  today (reused, not reinvented).
- Secrets travel via `env`/`stdin`, never argv (matching today's Bitwarden discipline).

### 4.2 Network — `host.fetch` (permission: `network`)

```ts
host.fetch({ url, method?, headers?, body?, timeoutMs? })
  : { status: number, headers: object, body: string }
```

- Gated by `permissions.network.allow = ["127.0.0.1:7891","vault.bitwarden.com", …]` —
  a **host/origin allowlist**. The bare `network: bool` flag that exists today is
  promoted to this structured form (a `true` legacy value = allow-all, logged as a
  warning so we can tighten later).
- This is what lets the Transcriber POST to its engine and lets a mesh-delegating
  transcriber reach a peer.

### 4.3 Per-plugin secret bucket — `host.secret*` (permission: `secrets`)

```ts
host.secretGet(name): string | null
host.secretSet(name, value): void
host.secretList(): string[]
```

- Backed by the existing `Keychain` (`secrets/keychain.rs`) under a **plugin-namespaced
  service** (`codeterm.plugin.<plugin_id>.<name>`). A plugin sees only its own bucket;
  it has **no access** to CodeTerm's global secret store or other plugins' buckets.
- This is how the Bitwarden plugin persists its `bw` session token / master password
  (it owns the same keys it owns today, just namespaced under its plugin id), and how a
  future connector stores its own API keys.

### 4.4 Capability registration

A plugin does not "register" imperatively; the manifest *declares* capabilities and the
loader builds the adapters (§2.3). The plugin only needs to **export the functions** the
adapter calls. This keeps registration declarative and statically inspectable from the
manifest alone (you can answer "what could this plugin be" without running it).

## 5. Manifest schema additions

`plugin.json` (the JS-runtime manifest) gains:

```jsonc
{
  "id": "transcriber",
  "host_api": 2,
  "capabilities": {                 // NEW — what this plugin contributes
    "transcriber": true             // agent_provider | secret_backend | transcriber
  },
  "permissions": {                  // EXTENDED
    "network":   { "allow": ["127.0.0.1:7891"] },
    "subprocess":{ "allow": [] },
    "secrets":   true,
    "filesystem":{ "read": ["$AUDIO_TMP"], "write": [] }
  },
  "settings_schema": "settings.schema.json",  // rendered by SchemaRenderer
  "ui": { "icon_hint": "mic", "pinnable": true }
}
```

- `capabilities` is the contract: a plugin can only register adapters for capabilities it
  declares. The install dialog (§9) renders `permissions` as the human-readable grant.
- `settings_schema` reuses the existing `SettingField[]` schema and `SchemaRenderer`.
- Native/system plugins use the equivalent Rust `PluginManifest` fields; the
  `PluginCapabilities` struct (already has `agent_providers`, `tools`, etc.) gains
  `secret_backends: Vec<String>` and `transcribers: Vec<String>` for symmetry.

## 6. Plugin lifecycle + event bus

- **Activate:** loader builds declared capability adapters → `PluginManager` registers
  each into its registry → emits `ContributionsChanged{kind}`.
- **Deactivate/remove:** unregister from every registry → if the removed plugin was the
  *active* secret backend or transcriber, fall back (secrets→`local`; transcribe→none,
  voice notes degrade to "no transcriber" per §7) → emit `ContributionsChanged`.
- **Event bus wiring (fix dead code):** add the missing subscriber loop so
  `PluginManager::dispatch_event` actually runs. In `lib.rs`, alongside the existing
  `ContributionsChanged → frontend` subscriber, add
  `while let Ok(ev) = sub.recv().await { plugin_manager.dispatch_event(&ev); }`. This
  unblocks `Plugin::on_event` for the watchdog (the Transcriber plugin listens for
  focus/idle/system events to decide load/unload). Today `dispatch_event` has no
  production caller.

## 7. Transcriber plugin

**Capability:** `transcriber`. **Tier:** JS (host_api 2). **Channel:** `codeterm-plugins`.

### 7.1 Engine modes (the defining choice)

Settings lead with **where the engine runs**:

- **Local** — the plugin manages a local transcribe engine. It reuses the existing
  daemon contract (`POST {url}/transcribe`, `GET {url}/healthz`) via `host.fetch`. The
  watchdog (below) decides when the engine is loaded.
- **Mesh peer** — the plugin delegates to a peer that runs the engine. It calls the
  peer's transcribe endpoint over the mesh. (Your real topology: Blackstar runs the
  engine; MacBook Air delegates to it.)

`model` is selectable wherever the engine runs (passed to the engine; the daemon already
accepts a model/language). No model ships in the binary (invariant from issue #125).

### 7.2 Watchdog

Plugin-owned state machine (persisted via `host.secret*`/plugin sqlite). Unload the local
engine when: GPU memory in use > N GB, or a GPU-heavy app/game runs (>80% for 30s);
optional idle timeout. Reload on the next transcribe request. The watchdog reacts to
`SystemEvent`s via `on_event` (now that the bus is wired, §6).

### 7.3 Consumption (no behavior regression)

`voice_transcribe::format_attachment_line` (the hinge shared by the chat composer and the
Telegram bridge) changes from "read `daemon_url`, POST" to "resolve the active
`Transcriber` from the registry, call `.transcribe()`":

- active transcriber + ok → `mk::voice(path, Some(text), None)` (unchanged shape).
- active transcriber + err → `mk::voice(path, None, Some("transcription unavailable: …"))`.
- **no active transcriber** → `mk::voice(path, None, Some("install the Transcriber plugin from <channel> to transcribe voice notes"))` — replaces today's `codeterm docs TRANSCRIBE` hint with the install offer (Screen 06). The audio is never lost.

**Zero-regression migration (gap G5):** at startup the `transcribe` registry is seeded
with a built-in `HttpEngine` (id `builtin-http`) constructed from the existing
`VoiceTranscribeConfig`, and `integrations.transcribe.active` defaults to `builtin-http`
when `voice_transcribe.enabled`. So voice transcription keeps working with no plugin
installed; installing the Transcriber plugin lets the user switch `active` to it (and gain
mesh + watchdog). `VoiceTranscribeConfig` is retained for the built-in engine; the plugin
carries its own settings.

### 7.4 Capabilities granted at install

`network` (its engine / mesh peer), `secrets` (watchdog state). It does **not** request
pane access — CodeTerm records (mic is internal) and inserts the returned text. No
`write_panes`.

## 8. Bitwarden plugin

**Capability:** `secret_backend`. **Tier:** JS (host_api 2). **Channel:** `codeterm-plugins`.

### 8.1 Make the secrets registry dynamic

Today `secrets::factory` is a static table. Change:

- `secrets::factory` → a dynamic `CapabilityRegistry<dyn SecretStore>` populated at
  startup with `local`/`keychain`/`mock`, and at runtime by the `PluginManager`.
- `build_active_store(cfg)` resolves `cfg.backend` through the registry; unknown →
  `local` (unchanged fallback). The atomic-swap mechanism (`switch_backend` →
  `Arc<RwLock<Box<dyn SecretStore>>>`) is preserved.

### 8.2 Bitwarden moves out of the binary, into the plugin

The Rust `bitwarden/` implementation is **ported to TS** in the plugin, using the host
API: `host.exec({bin:"bw", …})` for every `bw` invocation, `host.secret*` for the
session/master-password keychain (same keys, plugin-namespaced), `host.fetch` only if a
direct API path is ever needed (CLI-only for v1). The cipher JSON envelope parsing
(`cipher.rs`) becomes TS. The plugin exports `secretStatus/unlock/lock/logout/save/get/
list/search/delete/organizations/collections/sync` → `JsSecretBackend` maps them onto the
`SecretStore` trait.

- **Permissions granted at install:** `subprocess.allow=["bw"]`, `secrets`, and
  `network.allow=["vault.bitwarden.com", <self-hosted host>]` for a self-hosted server URL.
- The native `bitwarden/` module is **deleted** (no compiled-in backend; it lives in the
  channel now). `local`/`keychain`/`mock` stay native (they are the safety floor and have
  no external deps).
- Risk control: a `parity_test.rs` case runs the same fixtures through a reference and the
  JS backend (mocked `bw` responses via the existing `BwExec`-style injection ported to a
  fake `host.exec`) to prove the port matches the old behavior before we delete the Rust.

### 8.3 SecretsTab → plugin settings

The current `SecretsTab.tsx` (backend picker, server URL, unlock, org/collection pickers)
is re-expressed as the Bitwarden plugin's schema-rendered settings panel + the generic
backend picker stays in core (it chooses among *registered* backends, which now includes
plugin-contributed ones). No capability is lost; it moves surface.

## 9. Extensions UX surface

Promote the buried plugin UI to a first-class surface. **Use the frontend-design language
already in `tokens.css` — indigo `#6366f1`, Inter + JetBrains Mono, hairline borders,
`#1e1e2e` elevated dropdown surfaces, the real `MeshPresenceTray` dropdown chrome. No
ostentation; restraint. The one signature element is the capability chip-row**, identical
across install dialog, card, and settings.

- **Extensions tray** (titlebar): the entry point next to presence; rows = icon · name ·
  kind · status dot · gear-on-hover · pin. Groups: Pinned / Plugins / **Agents** (was
  Providers) / System. Keep-Awake is a `system` plugin (green-tinted, uninstall-blocked).
- **Top-level Extensions view** (`App.tsx` `mainView` gains `"extensions"`).
- **Install grant dialog** — renders `manifest.permissions` as the capability chip-row;
  default-deny, grant a subset; install pulls from the channel.
- **Plugin settings** — `SchemaRenderer` over `settings_schema` (Transcriber: engine /
  model / watchdog; Bitwarden: backend status / server URL / unlock / org+collection).
- **Settings → Plugins** — VS Code-style: channels (your repo is a channel), list,
  description, permissions, changelog. Reachable from the tray.
- **Pin** — `pinnable` plugins get a pin toggle; pinned ones appear as titlebar icons for
  quick access.
- **Sidebar presence** — replaces the dead "Panes" label with the real `MeshPresenceTray`
  dot; on **web UI** a second dot beside it is the **machine picker** (which daemon you're
  driving), reusing the exact dropdown chrome.
- **Web parity** — add `plugin_manager_*` (and the new capability/secret commands) to
  `bridge.ts` `HTTP_MAP` so the surface works in the web mirror, not just Tauri.

## 10. The plugin channel repo

Use the existing **`rollacode/codeterm-thirdparty-channel-demo`** → rename to
**`rollacode/codeterm-plugins`** (it is empty/demo, public, last pushed 2026-06-06). It
houses the **Transcriber** and **Bitwarden** plugins as the user's own (non-system)
channel. The existing channel machinery (`PluginsTab` add-channel, `plugin_manager_action`
refresh/install, bundle hashing) consumes it unchanged. For local dev before publishing,
plugins load from a local dir (`scan_roots`), so this is testable without the remote.

## 11. Security model

- **Default-deny, explicit grant.** A plugin gets exactly the permissions checked at
  install. Widening requires re-approval.
- **Allowlists, not booleans.** `subprocess` and `network` are allowlists; `secrets` is a
  per-plugin bucket. No ambient access.
- **No master token to in-process plugins.** Confinement is the sandbox, not a token.
- **System plugins** stay hash-verified + uninstall-blocked (existing `system_plugins`).
- **Bitwarden port is parity-gated** before the native code is deleted.

## 12. Out of scope this cycle (but seams left open)

- **Connectors** (`CallerIdentity::{Plugin,Connector}`, scoped tokens via `check_access`,
  connect/approval endpoints, origin tagging, the Dread Delusion AI game). The capability
  traits/registries here are exactly what a connector or process-tier plugin will register
  into later; nothing here blocks it.
- **Model tiers** (strong/medium/weak → id mapping). Only needed by the game connector;
  deferred. The `Transcriber` `model` field is a plain id for now.
- **Process/WASM tiers** for these two plugins — JS is sufficient and reuses the most.

## 13. Acceptance tests (definition of done)

1. **Transcriber, local:** install from local channel → settings → engine=local,
   model set, watchdog configured → record a voice note → transcript appears in the
   `-=-codeterm:voice-=-` marker exactly as before. Uninstall → voice note shows the
   install-offer hint, audio preserved.
2. **Transcriber, mesh:** on a second daemon set engine=mesh-peer → audio is transcribed
   by the peer; text returns.
3. **Bitwarden:** install plugin → backend picker shows it → unlock via `bw` through
   `host.exec` → list/get/save/delete a secret → telegram-bridge token fetch still works.
   `parity_test` green vs the pre-port reference. Native `bitwarden/` deleted, build green.
4. **Permissions:** a plugin without `subprocess.allow=["bw"]` cannot exec `bw`
   (throws); without `network.allow` cannot fetch its host.
5. **Event bus:** `dispatch_event` reaches `on_event` (watchdog reacts to a synthetic
   `AgentIdle`/system event).
6. **UX:** Extensions tray lists both plugins + Agents + Keep-Awake(system); install
   dialog shows the capability chips; pin puts an icon in the titlebar; sidebar presence
   + web machine-picker render in the real dropdown style.
7. **Cross-platform + file-size discipline** (`scripts/check-file-sizes.sh`), `./build.sh
   check` + `test` green, dev runs (port 7686).

## 14. Appendix — reuse map (file : symbol)

| Need | Reuse / change | Location |
|---|---|---|
| Plugin manager, manifest, lifecycle | extend `PluginCapabilities`; wire `dispatch_event` | `src-tauri/src/plugin/mod.rs` |
| JS runtime, manifest, loader | add `host_api:2`, `capabilities`, build adapters | `src-tauri/src/plugin/js_runtime/{manifest,loader,vm}.rs` |
| JS adapter precedent | model `JsSecretBackend`/`JsTranscriber` on it | `src-tauri/src/plugin/js_runtime/provider.rs` (`JsProvider`) |
| Host API (add exec/fetch/secrets) | extend; bump `HOST_API_VERSION` | `src-tauri/src/plugin/js_runtime/host_api.rs` |
| Sandbox gating | reuse `PluginSandbox` for new perms | `host_api.rs` (`PluginSandbox`) |
| Secret trait + factory→registry | dynamic registry; register built-ins | `src-tauri/src/secrets/{mod,factory}.rs` |
| Bitwarden (port to TS, then delete) | `bw` detection logic reused in `host.exec` | `src-tauri/src/secrets/bitwarden/*` |
| Keychain (per-plugin bucket) | namespace by plugin id | `src-tauri/src/secrets/keychain.rs` |
| Transcribe (trait + registry + consume) | promote module; registry; `format_attachment_line` | `src-tauri/src/bridge/voice_transcribe.rs` → `src-tauri/src/transcribe/` |
| Event bus | add subscriber → `dispatch_event` | `src-tauri/src/api/events.rs`, `lib.rs` |
| Config | `integrations.transcribe.active`; keep secrets backend id | `src-tauri/src/config/schema/mod.rs` |
| Settings rendering | `SchemaRenderer` + `SettingField[]` | `src/components/Settings/SchemaRenderer.tsx` |
| Plugin UI to promote | `PluginsTab` → Extensions surface | `src/components/Settings/PluginsTab.tsx` |
| Secrets UI to re-surface | `SecretsTab` → Bitwarden plugin settings + core backend picker | `src/components/Settings/SecretsTab.tsx` |
| Tray dropdown chrome to match | `MeshPresenceTray` (220px, `#1e1e2e`, active tint + ✓) | `src/components/Titlebar/MeshPresenceTray.tsx` |
| Main view routing | add `"extensions"` | `src/App.tsx` |
| Web parity | add `plugin_manager_*` + new cmds to `HTTP_MAP` | `src/lib/bridge.ts` |
| Channel repo | rename demo → `codeterm-plugins`; house both plugins | `rollacode/codeterm-thirdparty-channel-demo` |

<!-- revision: 1 -->
