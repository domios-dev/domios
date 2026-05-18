# Plugin marketplace + delivery

<!-- revision: 1 -->

Closes the gap left by **20-provider-plugins-ts** — provider plugins are
already TS bundles, but they only reach a running daemon if the user
manually clones the repo or drops files into `~/.codeterm/plugins/`. A
shipped CodeTerm has no `provider-claude` at all. This plan adds the
delivery, update, and identity layer that turns plugin folders into a
real ecosystem.

## Goals

- Bundled providers (claude / codex / opencode) reach a fresh install
  with zero manual work and update without rebuilding the app.
- Third-party developers ship plugins through a git-based channel; the
  user adds the channel by URL, lists/installs/updates plugins from it.
- A plugin can ship **more than a provider** — same bundle may
  contribute a provider + a theme + a skill + a hook. Future plugin
  kinds drop in without breaking older plugins.
- "System" plugins (our claude/codex/opencode) cannot be silently
  shadowed by a malicious plugin claiming the same id; verification is
  cryptographically grounded but does not require a private key infra.
- Lazy: a plugin's code does not run until something it contributes is
  actually used.

## Non-goals (this iteration)

- Closed walled-garden marketplace. The default channel is just one
  entry in a user-extendable list.
- Code-signing / publisher accounts. Hash-based verification is enough
  for the trust model we need today.
- Auto-update on every start. User opts in per channel.

## Architecture lessons borrowed

From the research pass over Claude Code, Zed, VSCode, Obsidian:

| Pattern | Where it came from | Why we copy it |
|---|---|---|
| `codeterm plugin marketplace add <git-url>` | Claude Code `/plugin marketplace add` | Multi-source > walled garden. Private channels just work. |
| Multi-type `contributes` block in one manifest | VSCode / Claude Code | Don't lock "1 plugin = 1 type" like Zed did and regretted. |
| Explicit `version` + app-compat range | Obsidian `minAppVersion`, VSCode `engines.vscode` | Stops "every commit is a release" surprise breakage. |
| Lazy activation by contribution point | VSCode (after it burned itself on `activationEvents: ["*"]`) | Startup tax of N plugins must be near zero. |
| Namespaced ids per contribution | Claude Code `/plugin:skill` | Two providers can both ship a `claude` agent type without clashing. |
| Built-in plugins identified by binary-side knowledge, not manifest flag | VSCode + Obsidian core plugins | Manifest flags are spoofable. |

The one we deviate on: instead of code-signing (publisher accounts),
**system plugins are identified by SHA-256 of their bundle**, hardcoded
in the codeterm binary. No key infra, no public-vs-private repo
problem, hash auto-updates on every release build.

## Plugin manifest (v2)

`plugin.json` evolves into a contribution-block model.

```json
{
  "schemaVersion": 2,
  "id": "claude",
  "version": "0.1.0",
  "displayName": "Claude Code",
  "author": "codeterm-core",
  "description": "Anthropic Claude provider with hooks + skills.",
  "engines": { "codeterm": "^1.3.0" },
  "contributes": {
    "providers": [ { "id": "claude", "entry": "plugin.js" } ],
    "themes":    [ /* future */ ],
    "skills":    [ /* future */ ],
    "hooks":     [ /* future */ ]
  },
  "permissions": {
    "fs": ["~/.claude"],
    "sqlite": [],
    "network": false
  }
}
```

Differences from v1:

- `schemaVersion: 2` so the loader can detect old manifests and apply a
  compat shim instead of failing.
- `contributes` moves the per-provider config into a typed list — same
  plugin can declare two providers, or a provider + a theme. Each
  contribution gets a sub-id namespaced by `<plugin-id>:<contrib-id>`,
  same as Claude Code (`/plugin:skill`).
- `engines.codeterm` is a semver range — if the installed codeterm is
  outside, the plugin is not loaded and the UI shows "requires X".
- `permissions` declares the host capabilities the plugin will use.
  Today every plugin gets the full `host` surface; once we have
  third-party plugins, the manifest gates `host.fs.readFile` to declared
  paths, `host.network` to declared origins, etc.

## Channel manifest

A channel is just a git-cloneable directory with a `channel.json` at
the root listing plugins:

```json
{
  "schemaVersion": 1,
  "name": "codeterm-core",
  "displayName": "CodeTerm Core",
  "description": "Official provider plugins shipped by the CodeTerm team.",
  "plugins": [
    { "id": "claude",   "path": "provider-claude",   "version": "0.1.0" },
    { "id": "codex",    "path": "provider-codex",    "version": "0.1.0" },
    { "id": "opencode", "path": "provider-opencode", "version": "0.1.0" }
  ]
}
```

A "marketplace" is just a URL pointing at the directory that contains
`channel.json` — git URL (`https://github.com/codeterm/plugins.git`),
local path (`file:///abs/path/to/dir`), or a tarball URL. Default
channel = our own repo's `plugins/` subtree, added on first launch.

## System-plugin verification (hash, not key)

```rust
// src-tauri/src/plugin/system_plugins.rs — generated by build.rs
pub const SYSTEM_PLUGIN_HASHES: &[(&str, &str)] = &[
    ("claude",   "a1b2c3...sha256_of_provider-claude_bundle..."),
    ("codex",    "d4e5f6..."),
    ("opencode", "789abc..."),
];
```

`build.rs` walks `plugins/<provider-*>/`, computes a canonical
sha256 over the bundled files (sorted by path, hash of each file's
bytes, hash of the concatenation), injects the table.

Loader behaviour:

1. Plugin discovered at `<some-dir>/plugin.json`. Compute bundle hash.
2. Look up `id` in `SYSTEM_PLUGIN_HASHES`.
3. If id present and hash matches → grant **system** status (cannot be
   uninstalled via UI, force-overwritten on channel update, UI shows a
   verified badge).
4. If id present and hash does NOT match → load as a normal plugin
   under that id (because the user might be hacking on a local fork);
   UI shows an "unverified" badge so the user knows.
5. If id not present → ordinary third-party plugin.

This passes the public-repo test: anyone can copy claude's source, but
their bundle won't hash-match, so they don't inherit system status.
Third-party "claude" plugins still load, just without the badge or the
core-source guarantee.

## Install / update / uninstall

Single canonical install dir: `~/.codeterm/plugins/<id>/`. Whether the
plugin came from the default channel, a community channel, or a manual
`--from-dir` drop-in, it lives in the same place. The loader treats
all of them uniformly.

```bash
codeterm plugin list                                         # installed
codeterm plugin install <plugin-id> [--channel <name>]       # default channel inferred
codeterm plugin update [--all | <plugin-id>]                 # pull channel(s), diff, apply
codeterm plugin uninstall <plugin-id>                        # blocked for system unless --force

codeterm plugin marketplace add <name> <git-url>
codeterm plugin marketplace list
codeterm plugin marketplace remove <name>
codeterm plugin marketplace refresh [<name>]                 # git pull + re-index
```

Update flow:
- `plugin update --all` runs `git pull` on every channel cache
  (`~/.codeterm/marketplace-cache/<channel>/`), reads each `channel.json`,
  compares with `~/.codeterm/plugins/<id>/plugin.json#version`, copies
  new versions in.
- System plugins are updated unconditionally on `codeterm plugin update`
  regardless of any user opt-out — the binary owns them.
- Hot-reload watcher already picks up the new files; no daemon restart.

## First-launch bootstrap

On a fresh install, `~/.codeterm/plugins/` is empty. Startup sequence:

1. Detect empty plugins dir.
2. Add default channel (`codeterm-core` → `https://github.com/...`)
   to the marketplace list automatically.
3. Run `plugin install <id>` for every entry the channel lists with a
   matching `SYSTEM_PLUGIN_HASHES` row. (For an air-gapped install
   without network, the binary also ships an embedded fallback bundle
   via `include_dir!` — used only when channel fetch fails on first
   launch.)
4. Hot-reload watcher loads them. User sees claude/codex/opencode in
   the Settings → Agents pane immediately.

This closes the "shipped binary has no providers" gap that
20-provider-plugins-ts.md left open.

## Lazy activation

Plugins do not evaluate JS until a contribution they declare is
actually requested:

| Contribution | Activated when |
|---|---|
| `providers` | First pane opens with that provider id, or first `agent spawn <id>` |
| `themes` | User selects the theme |
| `skills` | User invokes the skill |
| `hooks` | Host fires the matching event |

Plugin manifest is read at boot (cheap — JSON parse), VM is constructed
on demand, kept warm until the plugin is uninstalled or
hot-reload re-creates it.

Provider-specific exception: cwd-detection hits (`detectSessionId` for
the focused pane) happen even when no pane has that provider type yet,
so providers stay "always warm" by current architecture. Acceptable
for now; revisit when third-party providers exist.

## UI: Settings → Plugin Manager

New tab next to Agents / Notifications. Two sections:

**Installed**
- Per plugin row: name, version, source channel, verified/unverified
  badge, system lock icon, last-update timestamp.
- Update button (per plugin) + Update All button.
- Uninstall button (disabled + tooltip for system plugins).
- Expand row → permissions declared, contributions list.

**Marketplaces (channels)**
- List of channels with refresh time, plugin count, "default" label
  for `codeterm-core`.
- Add channel form (name + git URL).
- Remove button (default channel not removable).

## CLI surface (full)

```
codeterm plugin list [--json]
codeterm plugin info <id>
codeterm plugin install <id> [--channel <name>] [--from-dir <path>]
codeterm plugin update [--all | <id>] [--dry-run]
codeterm plugin uninstall <id> [--force]
codeterm plugin reload <id>                       # force hot-reload

codeterm plugin marketplace add <name> <url>
codeterm plugin marketplace remove <name>
codeterm plugin marketplace list [--json]
codeterm plugin marketplace refresh [<name>]
codeterm plugin marketplace plugins [<name>]      # what's available
```

## Phases

**Phase 1 — bundled-plugin delivery (small)**
- `build.rs` computes hashes, generates `SYSTEM_PLUGIN_HASHES`.
- `include_dir!` embeds `plugins/` into the binary for offline first-run.
- First-launch bootstrap copies embedded → `~/.codeterm/plugins/`.
- Loader hash-checks system plugins, grants/withholds system status.

Outcome: fresh `codeterm install` ships with working claude/codex/opencode.
**This alone fixes the prod-install gap.** Marketplace can wait.

**Phase 2 — channel infra (medium)**
- `channel.json` schema, marketplace cache dir,
  `codeterm plugin marketplace *` CLI surface.
- `codeterm plugin install/update/uninstall` CLI surface.
- Default channel auto-added on first launch (post-Phase-1 enhancement
  — if channel fetch succeeds, prefer it over embedded; if it fails,
  embedded keeps things working).

**Phase 3 — manifest v2 + multi-contribute (medium)**
- `schemaVersion: 2` parser, v1 compat shim, `contributes` block,
  `engines.codeterm` enforcement.
- Migrate bundled claude/codex/opencode to v2.

**Phase 4 — plugin-manager UI (medium)**
- React `PluginManagerTab.tsx` reading the new REST endpoints.
- Per-plugin install/update/uninstall flows.
- Channel add/remove UI.

**Phase 5 — lazy activation + permissions (large, can defer)**
- Move from "all plugins warm at boot" to "VM constructed on
  first-contribution-use".
- Gate `host.fs` / `host.network` by manifest `permissions`.
- Required before opening to third-party plugin authors, not before.

Phase 1 alone unblocks the install story. Phases 2–4 are independent and
can land in any order; 5 should land before any community plugins ship.

## Open questions

- **Plugin signing** — do we ever want it? Hash-based system-status is
  enough for our own plugins. Once a third-party plugin has a real user
  base, "this build is genuinely from author X" becomes worth solving
  (Ed25519 with publisher pubkeys in `marketplace.json`). Park.
- **Per-plugin update channels (stable / beta)** — Obsidian's BRAT
  pattern. Useful once someone ships a beta. Park.
- **Permission prompts** — when a plugin requests `host.network`, do we
  auto-grant from manifest or prompt the user on first use? Lean toward
  manifest-declared + audit log, no prompts (CodeTerm is a power-user
  tool). Park.
