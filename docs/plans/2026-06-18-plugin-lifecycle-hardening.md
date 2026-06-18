# Plugin Lifecycle Hardening — Implementation Plan

> **For workers (Codex):** Each track = ONE worker on its own branch off
> `plugins-rethink`, STRICT TDD (red→green→commit; write the test if missing).
> You MUST `git add -A && git commit` BEFORE reporting. There is no role skill in
> mem (404) — your role hint is inline below. Report `DONE <track>: <branch>,
> <gate>, <sha>` or `BLOCKED <track>: <q>`. No backticks in shell strings. Never
> spawn agents. Merge target: `plugins-rethink` directly (one mainline).

**Goal:** Fix three install/uninstall lifecycle gaps found in live verify so the
plugin flow "just works" without manual reload or leftover data.

**Findings → fixes:**
1. **L1 hot-activate:** installing a *capability* plugin (transcriber /
   secretBackend / chatBackend / glance / statusBubble) into a running daemon does
   NOT register its capability — the watcher logs *"capability plugin (not an
   agent) — skipped agent registration"* and only hot-registers AGENT plugins.
   Result: install → still inactive until a full reload. Fix: the watcher must
   hot-(re)register capability plugins too.
2. **L2 channel freshness:** `plugin install <id>` from a channel serves the
   channel's CACHED clone — stale until `channel refresh`. Live verify installed
   the OLD transcriber from a stale cache. Fix: refresh the channel before a
   channel install (or detect drift and refresh/​warn).
3. **L3 clean uninstall:** uninstall removes only the plugin dir — it leaves the
   plugin's downloaded data (e.g. `~/.codeterm/transcriber` = 487 MB model) AND
   config (`[plugins.<id>]` + `integrations.*` pointing at the id). Fix: uninstall
   also removes plugin data + config.

**Decisions:**
- L3 data discovery: **convention** `~/.codeterm/<plugin-id>/` (transcriber
  already uses this) **plus** an optional manifest `dataPaths: string[]` override
  for non-standard paths. Config cleanup (`[plugins.<id>]` + integrations that
  reference the id) is done by core regardless.
- L3 confirm: **ask the user** before deleting data — a checkbox in the uninstall
  flow (default checked: "also remove downloaded data (NNN MB)"). Config cleanup
  always runs (no prompt).

**Repos/branches:** mainline `plugins-rethink`. L1 `pl-hotactivate`, L2
`pl-channel-refresh`, L3 `pl-clean-uninstall`. Independent — parallel-safe (L1
watcher, L2 channel cli, L3 uninstall+config+frontend; only minor overlap in
cli/src/cmd/plugin — orchestrator merges sequentially + gates).

**Gates — codeterm-alpha:** `export CARGO_TARGET_DIR=/Users/kandrey/Developer/.psetup-target` then `cd src-tauri && cargo fmt && cargo build && cargo test <module> && cargo clippy --lib -- -D warnings` (clean for YOUR files; the ~145 baseline errors are pre-existing rustc-1.95 noise — ignore those not in your files) · `./node_modules/.bin/tsc --noEmit` for any TS · for the cli crate: `cd cli && cargo test <module>`.

---

## Track L1 — hot-activate capability plugins (watcher)

**Role hint:** Rust plugin-runtime engineer (src-tauri/src/plugin/js_runtime).

**Files:**
- Modify: `src-tauri/src/plugin/js_runtime/hot_reload.rs` (the plugin-watcher path
  that currently logs "capability plugin … skipped agent registration")
- Reference: `src-tauri/src/plugin/js_runtime/loader.rs` (how startup builds
  `JsTranscriber`/`JsSecretBackend`/`JsChatBackend`/glance/bubble and registers
  them into the capability registries on `ServiceContext`)
- Test: hot_reload tests (or a focused registry-registration test)

**Contract:** When the watcher detects a newly-installed or changed plugin that
declares capabilities (manifest `capabilities.transcriber/secretBackend/
chatBackend/glanceView/statusBubble/...`), it must build the matching `Js*`
adapter from the plugin VM and (re)register it into the corresponding capability
registry on `ServiceContext` — mirroring what `loader.rs` does at startup — INSTEAD
of skipping with "not an agent". Removal/disable must deregister. Keep the agent
path unchanged. This makes hot-install/hot-update of a capability plugin take
effect live (no reload), matching `requiresReload: false`.

- [ ] Step 1: read loader.rs to see exactly how each capability registry is
  populated at startup (the registries live on ServiceContext: `transcribe`,
  secret backend, chat backend, glance, bubble…). Identify the registry handles.
- [ ] Step 2: failing test — simulate the watcher registering a capability plugin
  and assert the capability registry contains it afterward (use whatever test
  seam exists; if ServiceContext can't be built in a unit test, extract the
  "build Js adapter + register" into a pure-ish helper and test that the right
  registry insert is called). RED.
- [ ] Step 3: implement: in the watcher's capability branch, call the same
  register path loader.rs uses (per declared capability), and deregister on
  removal. GREEN.
- [ ] Step 4: gate + commit `fix(plugins): hot-register capability plugins on install/change (L1)`.

---

## Track L2 — refresh channel before channel install

**Role hint:** Rust CLI engineer (cli/src/cmd/plugin).

**Files:**
- Modify: `cli/src/cmd/plugin/install.rs` (channel-install resolution path)
- Reference: `cli/src/cmd/plugin/update.rs` (existing `refresh` / channel fetch)
- Test: cli plugin tests

**Contract:** When `plugin install <id>` resolves the plugin from a registered
channel (not `--from-dir`), refresh that channel's cached clone first (the same
fetch/pull `channel refresh` does) so the install uses the latest published
version. If a refresh fails (offline), fall back to the cache with a clear
note. Keep `--from-dir` installs unchanged.

- [ ] Step 1: failing test — a channel install triggers a channel refresh
  (assert the refresh/fetch path is invoked; stub the git fetch). RED.
- [ ] Step 2: implement the refresh-before-resolve. GREEN.
- [ ] Step 3: gate + commit `fix(plugins): refresh channel before channel install (L2)`.

---

## Track L3 — clean uninstall (data + config)

**Role hint:** Rust (cli + commands + config) with a small React/TS frontend bit.

**Files:**
- Modify: `cli/src/cmd/plugin/install.rs` `remove_installed` (data + config cleanup)
- Modify: `src-tauri/src/commands/plugins.rs` `run_action` remove path (pass the
  `removeData` flag through; emit the existing PluginActivity uninstall frame)
- Modify: the config module (`crate::config`) — helper to drop `[plugins.<id>]`
  and reset any `integrations.*` field equal to the removed id (e.g.
  `voice_transcribe.active`)
- Modify: manifest type (`src-tauri/src/plugin/js_runtime/manifest.rs` + CLI
  manifest reader if separate) — add optional `data_paths: Vec<String>`
  (`#[serde(rename="dataPaths")]`)
- Modify (frontend): the Extensions uninstall flow
  (`src/components/Extensions/ExtensionsView.tsx` / `PluginDetailPanel.tsx`) —
  a confirm with a checkbox "also remove downloaded data (NNN MB)" default ON,
  pass `removeData` to `plugin_manager_action`
- Test: cli remove test (data dir + config removed); config helper test

**Contract:**
- Data discovery: default path `~/.codeterm/<plugin-id>/`; PLUS any manifest
  `dataPaths` (expanded, must stay under the user's home — reject absolute paths
  outside home as a safety check). Only delete when `removeData` is true.
- Config: ALWAYS remove the `[plugins.<id>]` table and reset any `integrations.*`
  string field whose value == the removed plugin id (so a removed transcriber
  doesn't leave `voice_transcribe.active = "transcriber"`). Do this even when
  `removeData` is false.
- `removeData` flag flows: frontend checkbox → `plugin_manager_action {kind:
  "remove", removeData}` → `run_action` → `remove_installed(id, force, remove_data)`.
- Reuse Track-A/C PluginActivity: emit an "uninstall" activity frame.

- [ ] Step 1: failing test — `remove_installed` with remove_data removes the
  convention data dir + manifest dataPaths and the config helper drops
  `[plugins.<id>]` + resets a matching `integrations.*.active`. RED.
- [ ] Step 2: implement cli + config helper + manifest field. GREEN.
- [ ] Step 3: thread `removeData` through `run_action` + the Tauri/web command;
  add the frontend confirm checkbox; `tsc --noEmit`.
- [ ] Step 4: gate + commit `fix(plugins): clean uninstall — remove plugin data + config (L3)`.

---

## Track V — live verify (orchestrator)
- [ ] macOS `./build.sh dev`: install transcriber from channel → **no reload
  needed**, capability live (L1); install serves the LATEST (L2). Use it, then
  uninstall with "remove data" checked → data dir gone + config clean; with it
  unchecked → data kept but config still clean (L3). Cross-check
  `~/.codeterm/transcriber` + `~/.codeterm-dev/config.toml`.
- [ ] Windows `--mesh ROG` (also clears the still-pending plugin-activity ROG check).

## REVISION (AUTHORITATIVE — supersedes the tracks above where they conflict)

Pre-dispatch review + two product decisions changed the scope. Workers: follow
THIS section.

### L1 — DROPPED. Already implemented; do NOT build.
The watcher already hot-registers capability plugins live: `hot_reload.rs:423`
`self.caps.register_plugin(provider.manifest(), provider.vm())` runs BEFORE the
agent branch, with passing tests (`tick_registers_capability_plugin_live_without_restart`,
`tick_deregisters_capability_on_folder_removal`). The "skipped agent registration"
log (`hot_reload.rs:437`) only means "not added to the AGENT registry" — benign.
The live-verify NoBackend was **L2 (stale channel served the old transcriber)**,
not a missing hot-register. Action: only reword the misleading log line at
`hot_reload.rs:437` to "capability registered live; not an agent provider —
skipped agent registry" (do this inside L2's commit). Re-verify hot-install after
L2 lands.

### L2 — channel refresh on install (refined). Branch `pl-l2-channel`.
As written in the L2 track, PLUS:
- `find_channel_plugin` (`channel.rs:316`) reads only the cached clone — no fetch.
  `refresh_channel(Some(name))` (`channel.rs:252`, `pub(super)`) is the reusable
  fetch; `update.rs:38` already uses it for channel updates.
- Sequencing: if `--channel <name>` given → refresh THAT name, then resolve. If
  omitted → resolve to learn the channel, refresh it, re-resolve (do not refresh
  all channels).
- **Offline = non-fatal:** `refresh_channel` returns `Err` and `update_one` uses
  `?` — do NOT copy that. In `install_command`, on refresh error, push a warning
  into the result JSON and proceed with the cached clone.
- `--from-dir` untouched. Also do the L1 log-line reword here.

### L3 — clean uninstall (RESTRUCTURED, one track). Branch `pl-l3-clean-uninstall`.
Spans CLI + src-tauri + frontend; keep it ONE worker (the `--remove-data` flag is
the spine — don't split mid-flag). Apply ALL of:
- **Config cleanup is src-tauri-side, NOT CLI.** `cli` has no `src-tauri` dep, so
  `remove_installed` cannot reach `crate::config`. Do config cleanup in
  `run_action` (`src-tauri/src/commands/plugins.rs:126`) AFTER the remove CLI
  subprocess returns ok: a new `crate::config` helper drops `config.plugins.remove(id)`
  and, if `integrations.voice_transcribe.active == id`, resets it to
  `default_transcribe_active()`. **Config cleanup ALWAYS runs** (no flag).
  Config writes are lossy serde round-trip (`config/mod.rs:360`) — that's
  consistent with the existing `config set`; accept it, do NOT add `toml_edit`.
- **`removeData` is a CLI flag, threaded across the subprocess boundary:**
  (1) add `#[serde(default)] pub remove_data: bool` to `PluginManagerAction`
  (`plugins.rs:12`) — grep `PluginManagerAction {` and add `remove_data: false` to
  EVERY struct-literal site (28+ in tests; no Default derive);
  (2) `plugin_action_args` remove arm (`plugins.rs:684`) appends `--remove-data`;
  (3) clap `PluginAction::Remove` (`cli/src/args/plugin.rs:27`) gains
  `#[arg(long="remove-data")] remove_data: bool`;
  (4) the dispatch match (`cli/src/cmd/plugin/mod.rs:45`) destructures the new
  field (exhaustive — compile breaks until updated);
  (5) `remove_installed(id, force, remove_data)` — data-dir deletion only when
  `remove_data`. `remove_channel_plugins` (`plugins.rs:261`) stays WITHOUT
  `--remove-data` (bulk channel removal must not nuke data).
- **Data discovery (CLI-side, in `remove_installed`):** delete
  `data_root().join(<id>)` (the convention; transcriber uses `~/.codeterm/transcriber`)
  PLUS manifest `dataPaths`. SAFETY (required tests): reject `id` ∈
  {`plugins`,`channels`,`bin`,`logs`}; for `dataPaths`, expand `~`, `canonicalize`,
  and reject anything not under the user home or containing `..`.
- **Manifest field (both readers):** add `#[serde(default)] data_paths: Vec<String>`
  to `src-tauri/.../manifest.rs:23` (rename_all already → `dataPaths`) AND to the
  CLI reader `cli/src/cmd/plugin/types.rs:60` (`#[serde(default, rename="dataPaths")]`)
  — the CLI does the deletion so it must read it.
- **Frontend:** uninstall confirm with a checkbox "also remove downloaded data"
  (default ON). State lives in `PluginDetailPanel` (`useState`); thread `removeData`
  through `onAction`'s signature → `actionPayload` (`ExtensionsView.tsx:31`) →
  `{kind:"remove", pluginId, removeData}`. DROP the "(NNN MB)" size — no size API
  exists; out of scope (a backend size probe is a future nicety).
- The existing `run_action` already emits the PluginActivity uninstall frame — do
  NOT add a new one.

### L4 — transcriber: delete orphan models on switch (NEW, decision Q1). Branch `pl-l4-orphan` (repo codeterm-plugins).
On `transcribe`/`ensureModel`, after resolving the active model, remove any other
`ggml-*.bin` (+ matching `.done`) in `~/.codeterm/transcriber` that are not the
active model — so switching models keeps only the active one on disk; the new one
lazy-downloads as today. STRICT TDD against the fake-host fs (assert orphan model
files are removed, active kept). Guard so an in-progress `.part`/active file is
never deleted. Rebuild `plugin.js`.

### L5 — warm model + VRAM watchdog (NEW, decision Q2). NOT dispatched — needs its own spec.
This is a real architecture change, designed separately (do it RIGHT, not bolted
on). It requires:
1. A NEW host capability: **multipart/file upload** (e.g. `host.uploadFile` /
   `host.fetchMultipart`) so a plugin can POST audio to a resident server —
   `host.fetch` is text-only today, which is the whole reason we went one-shot.
   This is a new confinement-surface (audit per the architecture rules) + SDK +
   parity considerations.
2. **Warm `whisper-server`**: run it detached with the model resident; transcribe =
   upload audio → `/inference` → parse. Lazy-start on first transcribe; stay warm.
3. **Watchdog**: per-OS GPU/VRAM probe (nvidia-smi on Win/Linux+NVIDIA; macOS
   unified-memory has no discrete VRAM — likely memory-pressure or no-op) + the
   `watchdogGame`/`watchdogVramGb` settings (re-added) → unload (stop server) over
   threshold/while a GPU-heavy app runs → lazy reload on next request.
4. Integrate with `host.progress` ("Loading model…/Transcribing…/Unloading (GPU busy)…").
Owner: gets a dedicated brainstorming → spec → plan → pre-dispatch-review cycle
before any worker. Sequenced AFTER L2/L3/L4 land + re-verify.

### Dispatch order
Now: **L2, L3, L4** (Codex, branches off mainline, merge back, gate each). Then
re-verify hot-install + clean-uninstall live. Then design **L5** properly.
