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

## Self-review
- L1 fixes hot-activate; L2 fixes freshness; L3 fixes leftover data+config (with
  the agreed convention+manifest discovery + ask-with-checkbox). Manifest gains
  `dataPaths`. `removeData` flag threaded end-to-end. Config helper resets
  integrations referencing the id. All three independent; merged sequentially.
