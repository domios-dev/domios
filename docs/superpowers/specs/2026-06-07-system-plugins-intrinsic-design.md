# System Plugins as Intrinsics + Compatibility Gate — Design

<!-- revision: 1 -->

## Problem

The Plugins settings panel shows nothing on a fresh setup, and never shows the
system provider plugins (`claude`, `codex`, `opencode`) or the system
marketplace (`codeterm-core`). Three independent causes:

1. **Native plugins are invisible by construction.** `agent-claude`,
   `agent-codex`, `agent-opencode` are compiled into the binary and activated at
   startup (`lib.rs:327`). They never live under `~/.codeterm/plugins/`, and
   `plugin list` only scans that directory (`cli/.../plugin.rs:454`).
2. **Provider bundles are not extracted in dev.** First-run extraction
   (`embedded::bootstrap_into`) is skipped when `CODETERM_DEV=1` (`lib.rs:359`),
   so `~/.codeterm-dev/plugins/` stays empty.
3. **The system channel is registered lazily.** `ensure_system_channel` runs
   only inside `install`/`update`, never on `channel list`. Until you install
   something, `channels.json` does not exist and the UI shows zero channels.

There is also no compatibility gate: a plugin version that needs a newer
CodeTerm than the one running can still be installed/updated, and there is no
way for the UI to explain "update blocked, CodeTerm too old".

## Goals

- System provider plugins and the system marketplace **always appear** in the
  Plugins UI — identically in dev and prod, **without network access** just to
  list them.
- They are **non-removable** from the UI (full lock; CLI `--force` stays as a
  developer escape hatch).
- Two update paths both work:
  1. **Per-plugin from the system market** — `update`/`update-system` git-pulls
     the `codeterm-core` subtree and updates in place, no CodeTerm reinstall.
  2. **CodeTerm app upgrade** — newer bundled provider versions are pulled in on
     daemon startup (upgrade-aware bootstrap), without clobbering a channel
     update that is already newer.
- A **`minCodeterm` compatibility gate**: each plugin version declares the
  minimum CodeTerm it needs; install/update refuses incompatible versions with a
  clear reason, and channel diff marks them so the UI shows "needs CodeTerm ≥ X"
  instead of an Update button.

## Non-goals

- No third-party marketplace UX changes beyond the lock/compat flags.
- No change to native `agent-*` plugins — they stay an implementation detail and
  are not surfaced as rows (the user-facing system plugins are the provider
  bundles `claude`/`codex`/`opencode`).
- No DB schema changes, no migrations. Everything is file/registry + additive
  JSON fields.

## Design

### Source of truth: the CLI

`plugin list` / `channel list` / `channel diff` already run in the CLI, and the
Tauri layer shells out to them (`commands/plugins.rs`). To keep one source of
truth (DRY), the CLI — not the Tauri layer — owns "what is a system intrinsic".
The CLI already hardcodes the identity of the system set:
`is_reserved_system_id` = `claude|codex|opencode`, `SYSTEM_CHANNEL_NAME =
codeterm-core`, `DEFAULT_SYSTEM_CHANNEL_URL`. We build on those constants; no
duplicate list is introduced.

### Virtual intrinsics (visibility)

**`list_installed`** — after scanning the plugins dir, ensure every system id is
represented:

- Installed on disk → keep the real row, set `system = true`, `removable =
  false`.
- Not on disk (the dev case, or a fresh prod before bootstrap) → synthesize a
  **virtual row**: `state = "bundled"`, `system = true`, `removable = false`,
  `source = SystemChannel{codeterm-core}`, `version` resolved best-effort from
  the system channel cache manifest if present, else `None`. `verified = true`.

**`list_channels`** — always include `codeterm-core`:

- Registered → real record, annotated `system = true`, `removable = false`.
- Not registered → a virtual record built from the constant name/URL
  (`DEFAULT_SYSTEM_CHANNEL_URL` or `CODETERM_SYSTEM_PLUGIN_CHANNEL_URL`
  override), `kind = git`, `system = true`, `removable = false`. No clone — the
  list stays offline and fast. Actual fetch happens lazily on refresh/diff/
  install, exactly as today.

New serialized fields (`system`, `removable`) are additive and default to
`false` for non-system entries, so existing consumers keep working.

### Compatibility gate (`minCodeterm`)

**Manifest fields (additive):**

- `plugins/provider-*/plugin.json` gains `"minCodeterm": "1.0.0"`.
- `plugins/channel.json` plugin entries gain `"minCodeterm": "1.0.0"`.

Both parsed via new optional fields on `PluginManifest` / `ChannelPlugin`
(serde-default `None`, so older manifests still parse).

**App version into the CLI:** the CLI is a separate crate (its own
`CARGO_PKG_VERSION` is unrelated to the app's `1.3.2`). The running CodeTerm
version reaches the CLI via a `CODETERM_VERSION` env var:

- `run_plugin_cli` (`commands/plugins.rs`) injects it from
  `env!("CARGO_PKG_VERSION")` (the app/src-tauri version).
- Pane env (`commands/pty.rs`) injects it too, so a terminal
  `codeterm plugin update` is gated the same way.
- If unset (standalone CLI, no daemon), the gate **fails open** (treated as
  compatible) and logs a debug note — we never block a developer running the raw
  CLI, but the app path always has it.

**Gate semantics:**

- `install`/`update`: before copying a channel/dir source into place, read the
  source's `minCodeterm`. If `app_version < minCodeterm` → return a clear error
  (`"plugin '<id>' v<X> requires CodeTerm ≥ <min>, but this is <app>"`). The
  in-place bundle is left untouched.
- `channel diff`: each row gains `compatible: bool` and `blocked_reason:
  Option<String>`. An `update_available` row whose remote `minCodeterm` exceeds
  the app version is reported as `update_available` **and** `compatible = false`,
  so the UI shows a blocked badge instead of an active Update button.

Version comparison reuses the existing `version_cmp`/`semverish` helpers (no new
dependency in the CLI).

### Upgrade-aware bootstrap (app-upgrade path)

`embedded::bootstrap_into` currently extracts only when the target dir is
absent. Change it to be version-aware:

- Absent → extract (unchanged).
- Present → compare embedded manifest version vs installed manifest version
  (semver). Embedded **strictly newer** → re-extract (upgrade). Installed
  `>=` embedded → skip (a channel update may already be ahead; never downgrade).
- The compatibility gate is moot here (bundled ships with the app, so it is
  always satisfied) but is still asserted defensively.

`bootstrap_into` stays skipped under `CODETERM_DEV=1` — dev keeps workspace
sources authoritative and relies on the virtual rows for visibility. Semver
compare uses the `semver` crate already present in `src-tauri`.

### UI (`Settings/PluginsTab.tsx`)

- Row type gains `system`/`removable`; diff row gains `compatible`/
  `blocked_reason`.
- A `system` plugin/channel renders a lock pill ("system") and **no Remove
  button**.
- An incompatible update renders a disabled "needs CodeTerm ≥ X" pill instead of
  the Update button (tooltip = `blocked_reason`).
- Boy-scout: lift the inline row/action markup into small typed helpers, type
  the snapshot shape instead of `any`, remove duplication between the installed
  table and the "available from channels" table.

## Data flow

```
daemon startup ──▶ embedded::bootstrap_into (prod only, upgrade-aware)
                       │ writes/upgrades ~/.codeterm/plugins/<id>
UI open ──▶ plugin_manager_snapshot
   └─ run_plugin_cli(list)     ──▶ FS rows + virtual system rows (system/removable)
   └─ run_plugin_cli(channel list) ─▶ registry + virtual codeterm-core
   └─ run_plugin_cli(channel diff)  ─▶ drift + compatible/blocked_reason
        (CODETERM_VERSION injected on every CLI call)
UI update click ──▶ run_plugin_cli(update <id>)
   └─ refresh codeterm-core ──▶ minCodeterm gate ──▶ copy in place
```

## Error handling

- Channel fetch failure during diff/update → surfaced as today (diff returns its
  error field; update returns Err). Listing never fetches, so it cannot fail on
  network.
- Incompatible version → typed Err with the reason string; nothing is written.
- Missing `CODETERM_VERSION` → gate fails open with a debug log.
- Removal of a system entry from the CLI without `--force` → existing soft guard
  error; the UI simply never offers it.

## Testing

CLI (`cli/src/cmd/plugin.rs` tests):
- `list_installed` on an empty plugins dir returns the three system rows, all
  `system && !removable`, `state = "bundled"`.
- An installed system plugin is `system && !removable` (real row, not
  duplicated).
- `list_channels` on an empty registry returns a virtual `codeterm-core`,
  `system && !removable`.
- `minCodeterm` gate: `app < min` → install/update Err with reason; `app >= min`
  → ok; unset env → ok (fail-open).
- `channel diff` marks an incompatible `update_available` row `compatible =
  false` with a reason.

Backend (`src-tauri`):
- `embedded::bootstrap_into` upgrades when embedded version is newer; skips when
  installed `>=` embedded; still no-ops when equal.
- `plugin_action_args` unchanged; add a test that `run_plugin_cli` sets
  `CODETERM_VERSION` (via a small seam).

Frontend: type-check only (no runtime test harness for this panel); manual
Playwright pass is covered by the existing plugin-store drill.

## Risks / mitigations

- **Downgrade on app upgrade** if a channel pushed a newer version than the
  bundle — mitigated by the strict `>` comparison (never overwrite when
  installed `>=` embedded).
- **Breaking existing snapshots** — all new fields are additive with serde
  defaults; the UI tolerates missing flags (treated as non-system/compatible).
- **Blocking the raw CLI** — gate fails open when `CODETERM_VERSION` is absent.
