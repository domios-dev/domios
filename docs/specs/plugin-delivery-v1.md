# Plugin Delivery v1

Status: implemented by the CLI local filesystem path.

## Scope

This is the first working slice of the larger channel/update design in
`docs/plans/relay/25-plugin-channels.md`.

The runtime already loads JS provider plugins from:

- repository `plugins/` in dev/source builds;
- user install dir `<data-dir>/plugins/<id>/` (`~/.codeterm-dev/` in dev,
  `~/.codeterm/` in production).

The runtime hot-reload watcher polls those roots and reloads changed
`plugin.json` / `plugin.js` bundles. Plugin delivery v1 adds the missing
CLI layer that places and updates folders in the user install dir.

## Commands

```bash
codeterm plugin list [--json]
codeterm plugin install --from-dir <path> [--id-override <id>]
codeterm plugin install <id> [--channel <name>]
codeterm plugin update [<id>]
codeterm plugin update-system [<id>]
codeterm plugin remove <id> [--force]
codeterm plugin channel add <url-or-path> [--name <name>]
codeterm plugin channel list
codeterm plugin channel refresh [<name>]
codeterm plugin channel diff [<name>]
```

## Storage

- Installed plugins: `<data-dir>/plugins/<id>/`
- Install metadata: `<data-dir>/plugins/<id>/.codeterm-install.json`
- Channel cache: `<data-dir>/channels/<name>/`
- Channel registry: `<data-dir>/plugin-channels.json`
- Audit log: `<data-dir>/logs/plugin-events.log`

The CLI and daemon use the same data directory routing. Dev builds and dev
consoles use `~/.codeterm-dev/`; production uses `~/.codeterm/`. Test
harnesses may set `CODETERM_DATA_DIR` to an absolute sandbox path for CLI
plugin commands.

`plugin list --json` returns:

- `id`, `version`, `display_name`
- `install_path`, `install_time`
- `source.channel` / `source.path` for channel installs
- `manifest_hash` (SHA-256 of installed `plugin.json`)
- `verified` (visible trust flag; unverified plugins are usable but shown)
- `last_change` (`installed`, `updated`, or `downgraded`)

## Supported Sources

v1 supports:

- local plugin folder via `install --from-dir`;
- local channel directory via `channel add <path>`;
- git channel via `channel add <url>` when `git` is available.
- git channel subdirectories via `<repo-url>#<ref>:<subdir>`.

Channel manifests are `channel.json` files with:

```json
{
  "schemaVersion": 1,
  "name": "example",
  "plugins": [
    { "id": "gemini", "path": "provider-gemini", "version": "0.1.0" }
  ]
}
```

`source` is accepted as an alias for `path`.

## System Plugin Updates

Bundled provider plugins (`claude`, `codex`, `opencode`) can update
without a full CodeTerm app release:

```bash
codeterm plugin update-system
codeterm plugin update-system claude
```

The command ensures the official `codeterm-core` channel exists, then
refreshes it and copies only system plugin ids into
`~/.codeterm/plugins/<id>/`.

Default source:

```text
https://github.com/rollacode/codeterm.git#master:plugins
```

For tests or private/internal releases, override it with:

```bash
CODETERM_SYSTEM_PLUGIN_CHANNEL_URL=file:///.../plugins codeterm plugin update-system
CODETERM_SYSTEM_PLUGIN_CHANNEL_URL=C:\repo\plugins codeterm plugin update-system
```

or another git ref/subdir:

```bash
CODETERM_SYSTEM_PLUGIN_CHANNEL_URL=https://github.com/rollacode/codeterm.git#my-branch:plugins
```

Old app binaries may load a freshly updated system plugin as
`unverified` until the main app binary catches up with the new embedded
hash table. That is intentional: the plugin can still be used, but the
UI should not claim it matches the currently installed app's embedded
bundle.

## Update Semantics

- `update <id>` reinstalls from recorded metadata.
- `update` without an id updates all installed plugins with metadata.
- For channel installs, `channel refresh` runs first so the latest
  channel checkout is used.
- Hot reload is delegated to the running daemon's existing watcher; the
  CLI does not need a daemon call.
- `channel diff [name]` observes channel drift without installing. Git
  channels are checked out to a temporary directory so the registered channel
  cache is not mutated.

## Plugin Manager UI

Settings → Plugins mirrors the CLI contract. It shows installed plugins,
registered channels, channel diff badges, one-click refresh/install/update/
remove actions, and the recent JSONL audit events.

## Non-Goals

- Permission prompts and permission delta approvals.
- Signed third-party releases.
- Background update discovery.
- Lazy activation.
- Zip/tarball/npm/github polymorphic sources beyond plain git channel
  checkout.
