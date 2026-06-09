# System Plugins Intrinsic + Compat Gate — Implementation Plan

<!-- revision: 1 -->

Spec: `docs/superpowers/specs/2026-06-07-system-plugins-intrinsic-design.md`
Branch: `feat/system-plugins-intrinsic`

Order is bottom-up: manifests → CLI data model → CLI behaviour → backend env +
bootstrap → UI. Each stage compiles and tests green before the next.

## Stage 1 — Manifests (`minCodeterm`)

Files: `plugins/provider-claude/plugin.json`, `provider-codex/plugin.json`,
`provider-opencode/plugin.json`, `plugins/channel.json`.

- Add `"minCodeterm": "1.0.0"` to each provider `plugin.json` (top level).
- Add `"minCodeterm": "1.0.0"` to each entry in `channel.json.plugins`.

`1.0.0` ≤ current `1.3.2`, so everything stays installable; the field exercises
the gate without blocking anything.

## Stage 2 — CLI data model

File: `cli/src/cmd/plugin.rs`.

- `PluginManifest`: add `#[serde(default, rename = "minCodeterm")] min_codeterm:
  Option<String>`.
- `ChannelPlugin`: add `#[serde(default, rename = "minCodeterm")] min_codeterm:
  Option<String>`.
- `InstalledPluginRow`: add `system: bool`, `removable: bool`.
- `ChannelDiffRow`: add `compatible: bool`, `blocked_reason: Option<String>`.
- New `#[derive(Serialize)] struct ChannelListRow` wrapping `ChannelRecord`
  fields + `system: bool`, `removable: bool` (avoids mutating the persisted
  `ChannelRecord` shape).
- Helpers:
  - `fn app_version() -> Option<String>` — reads `CODETERM_VERSION`.
  - `fn compat_check(min: Option<&str>) -> Result<(), String>` — Ok when no min,
    no app version (fail-open), or `app >= min` via `version_cmp`; else Err with
    the reason string. Plus `fn compat_reason(min, app) -> String` for diff.
  - `fn manifest_min_codeterm(dir) -> Option<String>`.

## Stage 3 — CLI behaviour

File: `cli/src/cmd/plugin.rs`.

- `list_installed`: after collecting FS rows, call a new
  `merge_system_rows(rows)` that:
  - marks any FS row whose id `is_reserved_system_id` → `system=true,
    removable=false`;
  - for each missing system id, pushes a virtual row (`state="bundled"`,
    `verified=true`, version from `system_channel_version(id)` best-effort,
    `source=SystemChannel`).
  - keeps a stable sort (system ids first, then alpha) for deterministic tests.
- `list_channels`: map records through `ChannelListRow`, force
  `system/removable` for `codeterm-core`; if absent, prepend a virtual
  `codeterm-core` row from `system_channel_url()`.
- `channel_diff`: compute `compatible`/`blocked_reason` from the remote plugin's
  `min_codeterm` vs `app_version()`; only meaningful when `drift` implies an
  install/update is offered.
- `install_command` (channel branch) and `update_one` (Channel + SystemChannel
  branches): call `compat_check(manifest_min_codeterm(&source))` before
  `install_from_dir`; on Err, skip the copy and return it.
- Leave `remove_*` soft guards as-is (UI enforces the hard lock).
- `print_human`: show a `system`/`bundled` hint and skip nothing — minor.

Tests (append to existing `#[cfg(test)] mod tests`): empty-dir virtual rows;
installed system row non-removable; virtual channel; compat gate
allow/block/fail-open; diff compatible flag. Use `CODETERM_DATA_DIR` temp dirs
(pattern already in the file) and set/clear `CODETERM_VERSION` with the existing
env restore helper style.

## Stage 4 — Backend: version env + upgrade bootstrap

Files: `src-tauri/src/commands/plugins.rs`, `src-tauri/src/commands/pty.rs`,
`src-tauri/src/plugin/embedded.rs`.

- `run_plugin_cli`: `.env("CODETERM_VERSION", env!("CARGO_PKG_VERSION"))`.
- `pty.rs` env block: insert `CODETERM_VERSION = env!("CARGO_PKG_VERSION")`.
- `embedded.rs`:
  - `extract_to` gains an upgrade path, or add `fn upgrade_to(id, dst_root) ->
    io::Result<Option<&'static str>>` returning `Some(action)` when written.
    Cleanest: refactor to `fn sync_to(id, dst_root, mode)` where mode handles
    "create only" vs "create-or-upgrade". Keep `extract_to` as the create-only
    wrapper for callers/tests that rely on no-overwrite.
  - Compare embedded vs installed `version` from each `plugin.json`; re-extract
    only when embedded `>` installed (semver via `semver::Version`, fall back to
    string `!=` ⇒ treat unparsable as "upgrade" only if embedded parses and is
    greater — be conservative: if either side unparsable, skip upgrade).
  - `bootstrap_into` uses the upgrade-aware path; return value distinguishes
    written vs upgraded for the startup log.
- Update `embedded.rs` tests: existing `bootstrap_writes_all_then_idle` stays;
  add `bootstrap_upgrades_when_embedded_newer` and
  `bootstrap_skips_when_installed_newer` using temp dirs with hand-written
  `plugin.json` of differing versions.

## Stage 5 — UI

File: `src/components/Settings/PluginsTab.tsx` (+ types it imports).

- Extend the snapshot TS types: `system?: boolean`, `removable?: boolean` on
  plugin rows; `compatible?: boolean`, `blocked_reason?: string | null` on diff
  rows; `system?`/`removable?` on channel rows.
- Installed table: `system` → render a "system" pill in State; hide Remove when
  `removable === false`.
- Update affordance: if the diff row is `update_available` but `compatible ===
  false`, render a disabled pill "needs CodeTerm ≥ X" (title = `blocked_reason`)
  instead of the Update IconButton.
- Channels list: render a lock pill for `codeterm-core` and hide its Remove
  button when `removable === false`.
- Boy-scout: extract `PluginRow`/`ChannelRow` subcomponents and a typed
  `Snapshot` interface; collapse duplicated action wiring. Keep file < 1000
  lines (split a `pluginTypes.ts` if needed).

## Stage 6 — Verify + docs

- `cargo test plugin` (CLI crate) + `embedded` tests (src-tauri) via PowerShell/
  foreground equivalent; `./build.sh check`; `tsc` (or `pnpm -s tsc --noEmit`).
- `./build.sh dev --wait`, open Settings → Plugins, confirm 3 system plugins +
  codeterm-core appear locked in dev.
- Docs: `docs/API.md` plugin section (new fields, env, gate) + revision bump;
  note system-intrinsic + compat in `docs/CONFIG.md` if plugins are documented
  there.

## Rollback

Pure-additive; revert the branch. No persisted-state migration to undo (registry
and install records are unchanged in shape; new JSON fields are ignored by old
binaries).
