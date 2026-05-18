# Plugin marketplace + delivery

<!-- revision: 6 -->

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
codeterm plugin install <id>                          # search all channels by id; conflict → error w/ choices
codeterm plugin remove  <id> [--force]                # uninstall; --force for system
codeterm plugin update  [<id>] [--approve-perms]      # all if no id
codeterm plugin list    [--json]                      # installed
codeterm plugin info    <id>                          # version, source channel, contributions
codeterm plugin search  [<query>]                     # what's available across channels

codeterm plugin channel add     <url>                 # name derived from channel.json
codeterm plugin channel list
codeterm plugin channel remove  <name>
codeterm plugin channel refresh [<name>]              # git pull + re-index
```

Three things this surface deliberately avoids:

- **No `marketplace` noun.** A channel is the only thing being managed,
  call it that.
- **No `--channel` flag on `install` by default.** `plugin install gemini`
  searches every channel for `id == gemini`. Exactly one match → install.
  Zero → "not found, channels searched: …". Multiple → explicit error:
  `id 'gemini' exists in [community, partner-corp] — pass --channel <name>`.
  The flag exists as the disambiguator, not as the default path.
- **No `marketplace add <name> <url>`.** Channel name is read from the
  cloned repo's `channel.json` field. Two channels with the same name →
  add refuses with the conflict; user removes one or the channel author
  picks a different name.

Update flow:
- `plugin update --all` runs `git pull` on every channel cache
  (`~/.codeterm/channels/<channel>/`), reads each `channel.json`,
  compares with `~/.codeterm/plugins/<id>/plugin.json#version`, copies
  new versions in.
- System plugins are updated unconditionally on `codeterm plugin update`
  regardless of any user opt-out — the binary owns them.
- Hot-reload watcher already picks up the new files; no daemon restart.

## First-launch bootstrap

On a fresh install, `~/.codeterm/plugins/` is empty. Startup sequence:

1. Detect empty plugins dir.
2. Add default channel (`codeterm-core` → `https://github.com/...`)
   to the channel list automatically.
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
codeterm plugin install  <id> [--channel <name>] [--version <ver>] [--from-dir <path>]
codeterm plugin remove   <id> [--force]
codeterm plugin update   [<id>] [--approve-perms] [--dry-run]
codeterm plugin list     [--json]
codeterm plugin info     <id>
codeterm plugin search   [<query>]

codeterm plugin pin      <id> [<ver>]                   # pin to current ver if omitted
codeterm plugin unpin    <id>
codeterm plugin disable  <id>                           # keep installed, deactivate
codeterm plugin enable   <id>
codeterm plugin verify   [--repair]                     # hash check; --repair pulls from embedded

codeterm plugin channel add     <url>
codeterm plugin channel remove  <name>
codeterm plugin channel list    [--json]
codeterm plugin channel refresh [<name>]
```

That's 14 verbs total — feels like the floor for what the surface
needs to cover the case set without secret-handshake flags.

## Phases

**Phase 1 — bundled-plugin delivery (small)**
- `build.rs` computes hashes, generates `SYSTEM_PLUGIN_HASHES`.
- `include_dir!` embeds `plugins/` into the binary for offline first-run.
- First-launch bootstrap copies embedded → `~/.codeterm/plugins/`.
- Loader hash-checks system plugins, grants/withholds system status.

Outcome: fresh `codeterm install` ships with working claude/codex/opencode.
**This alone fixes the prod-install gap.** Marketplace can wait.

**Phase 2 — channel infra (medium)**
- `channel.json` schema, channels cache dir,
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

## Cases — end-to-end walkthroughs

The plan is "done" when each of these flows works exactly as
described. They double as the acceptance suite for the rollout.

### Case A — CodeTerm team publishes a system-plugin update

We bump `claude` from 0.1.0 to 0.2.0 because Anthropic changed a
flag.

1. Edit `plugins/provider-claude/plugin.js` + `plugin.json` on a
   branch. Bump `"version": "0.2.0"`.
2. PR review, merge to `master`.
3. CI runs `cargo build --release`. `build.rs` walks
   `plugins/provider-claude/`, computes canonical SHA-256, writes the
   pair into `src-tauri/src/plugin/system_plugins.rs::SYSTEM_PLUGIN_HASHES`.
   The new binary is now the only one that recognises the 0.2.0
   bundle as system.
4. CI also updates `plugins/channel.json` — bumps `"version": "0.2.0"`
   for `claude` — and tags the release on the default-channel git ref.
5. End users who already have CodeTerm 1.3.x installed (matching
   `engines.codeterm`):
   - Either auto-update CodeTerm (Tauri updater pulls 1.4.0 binary
     with the new hash table + embedded fallback) — on next launch
     the bootstrap detects channel `claude` 0.2.0 > installed 0.1.0,
     fetches it, validates hash → matches new table → installs into
     `~/.codeterm/plugins/claude/`. Hot-reload watcher reloads.
   - Or stay on 1.3.x and run `codeterm plugin update claude` — same
     install flow, but **hash check fails** because their binary still
     has the 0.1.0 hash. Plugin loads as **unverified**; UI shows the
     "unverified" badge until they update the app. Force-uninstall
     stays unlocked. They're not stranded, just notified.
6. No user data loss. `~/.claude/projects/...` untouched. Pane
   metadata (agent_session_id, model) survives via the existing
   restore path tested in P24.

Acceptance: a release where (4) is committed and (5) is reproducible
on a clean Windows / macOS / Linux install. Both update paths (with
and without app bump) verified by a `evals/plugin-update/` bench.

### Case B — Third-party developer publishes a brand-new plugin

A community dev writes `provider-gemini` and wants users to install
it from their own channel.

1. Dev creates a repo `https://github.com/community-dev/codeterm-plugins`
   with:
   ```
   /channel.json
   /provider-gemini/plugin.json     (id: "gemini", version: "0.1.0",
                                     contributes.providers, etc.)
   /provider-gemini/plugin.js
   ```
   `channel.json` lists `gemini` with `path: "provider-gemini"`,
   `version: "0.1.0"`.
2. Dev pushes. No CodeTerm-team approval, no central registry PR.
3. End user adds the channel (no name positional — `channel.json`
   provides one):
   ```bash
   codeterm plugin channel add https://github.com/community-dev/codeterm-plugins
   # Added channel 'community' (3 plugins available).
   ```
   CodeTerm clones to `~/.codeterm/channels/community/`,
   reads `channel.json`, indexes available plugins.
4. User browses:
   ```bash
   codeterm plugin search gemini
   # gemini  0.1.0  community     — Google Gemini provider
   ```
5. User installs by id — no `--channel` needed because only one
   channel offers `gemini`:
   ```bash
   codeterm plugin install gemini
   ```
   - Bundle copied to `~/.codeterm/plugins/gemini/`.
   - Loader computes hash, looks up `gemini` in `SYSTEM_PLUGIN_HASHES`
     — not present → loaded as **third-party** (no system lock, no
     verified badge, uninstallable from UI).
   - Hot-reload watcher picks it up. `gemini` appears in
     `/api/agents/providers`, "New Gemini Session" appears in the
     workspace 3-dot menu, etc.
6. Dev later ships 0.2.0. Bumps `version` in `provider-gemini/plugin.json`
   AND in `channel.json`. Pushes.
7. User runs `codeterm plugin update` (no id → updates everything)
   or `update gemini`:
   - `git pull` on `~/.codeterm/channels/community/`.
   - Diff: installed 0.1.0 vs channel 0.2.0 → install.
   - Same hot-reload reload.

Acceptance: a "third-party plugin walkthrough" doc in the repo
walks a dev through these exact six steps, and an integration test
under `evals/plugin-third-party/` runs them against a local file://
channel.

### Case C — End user removes a plugin

User decides they don't need OpenCode anymore.

```bash
codeterm plugin remove opencode
# Refusing: 'opencode' is a system plugin (shipped with CodeTerm).
#   Use --force to remove anyway; it will be re-installed on next update.
codeterm plugin remove opencode --force
# Removed ~/.codeterm/plugins/opencode/
# Provider 'opencode' unregistered from the live daemon.
```

Hot-reload watcher detects the directory gone, unregisters the
provider, panes that had `agent_type=opencode` keep their metadata
but the provider trait calls become no-ops (status badges go grey,
existing sessions become read-only). UI hides the "New OpenCode
Session" menu entry.

On next `codeterm plugin update` (or next app restart with default
channel fetched), opencode reinstalls because it's a system plugin
the binary owns. To keep it out permanently, user adds an
override:

```bash
codeterm plugin disable opencode
# (writes ~/.codeterm/plugin-overrides.json — read by the bootstrap)
```

Same flow for third-party plugins, minus the `--force` gate and the
reinstall — uninstall sticks until the user explicitly reinstalls.

Acceptance: uninstall flow tested for both system + third-party cases
in `evals/plugin-uninstall/`. The pin/disable mechanism is part of
Phase 2's CLI surface.

### Case D — Fresh install on a new machine

User downloads `codeterm.exe` (or `.dmg`, or `.AppImage`) and launches
for the first time. Network is up.

1. Bootstrap: `~/.codeterm/plugins/` empty.
2. Bootstrap auto-adds default channel `codeterm-core`
   (`https://github.com/codeterm/plugins.git` — or whatever URL the
   binary was built with).
3. Tries `git clone` → success → reads `channel.json` → finds
   `claude`, `codex`, `opencode`.
4. For each: copies into `~/.codeterm/plugins/<id>/`, validates hash
   against `SYSTEM_PLUGIN_HASHES`, marks system.
5. App finishes booting. Settings → Agents shows three system
   providers, all verified.

Same machine, **offline** first launch:

1–2. Same.
3. `git clone` fails (no network).
4. Bootstrap falls back to `include_dir!`-embedded bundle baked into
   the binary at build time — extracts the same three plugin folders
   into `~/.codeterm/plugins/<id>/`. Hash still matches because the
   embedded bundle is the same one CI hashed.
5. App boots with the three providers. UI shows "offline" status on
   the channel, no community channel available yet.
6. When network returns, `codeterm plugin marketplace refresh` (or
   the next launch) pulls the channel and unblocks updates.

Acceptance: smoke test on Windows + macOS + Linux installer with
network on; same with network off. Both paths land the user in the
same place.

### Case E — User adds a community channel after running for months

User has been on stock CodeTerm. Friend points them at a community
plugin.

1. `codeterm plugin channel add git@github.com:my-org/internal-plugins.git`
   Private repo — uses user's existing SSH/git creds, no codeterm-
   specific auth. Channel name comes from the repo's `channel.json`
   (e.g. `internal-team`).
2. `codeterm plugin channel refresh internal-team` (also runs
   automatically on add).
3. `codeterm plugin search` lists everything available across all
   channels; the channel column shows where each entry comes from.
4. `codeterm plugin install corp-llm` — id is unique, installs from
   the only channel offering it.
5. `codeterm plugin update` from then on includes every channel in
   the pull cycle.
6. If two channels offer the same `id`, install errors with `id
   'claude' exists in [codeterm-core, sketchy-channel] — pass
   --channel <name>`. System ids (claude/codex/opencode) always
   resolve to the system source regardless of `--channel` — no
   shadow override.

Acceptance: dual-channel scenario covered in
`evals/plugin-channels/`, including the conflict error path and the
system-id protection.

### Case F — A plugin crashes / panics / loops

A community plugin throws on every `detect_session_id` call, or its
VM hangs in an infinite loop on first launch.

1. Per-plugin call sites already use `try_call::<T>` and return
   `None` on `MissingFunction`. Extend this to count consecutive
   errors per `(plugin_id, fn_name)`. Threshold (e.g. 5 consecutive
   errors within 30s) flips the plugin into **quarantined**.
2. Quarantined plugin: VM dropped, no further calls dispatched, UI
   shows "disabled — crashed N times, last error: ..." with a Re-enable
   button. Other plugins keep working — one bad apple doesn't take
   the daemon down.
3. Hangs: every `try_call` already runs on a thread with the plugin's
   mutex — add a configurable wall-clock budget per call (default
   2s; provider hot-path overrides explicit). Exceeded → kill the
   VM thread, increment error counter.
4. System plugins follow the same rule but Re-enable on quarantine
   is automatic on next launch (we don't want a transient SQLite
   lock taking claude offline forever).

Acceptance: `evals/plugin-crash/` runs a synthetic plugin that
panics every call and verifies the daemon stays healthy + quarantine
log entries land + Re-enable restores normal operation.

### Case G — App upgrade leaves a plugin's `engines` range unsatisfied

User on CodeTerm 1.3.x has `corp-llm` installed declaring
`"engines": { "codeterm": "^1.3.0" }`. They upgrade to CodeTerm 2.0.

1. On launch, loader reads each manifest's `engines.codeterm` range.
   Mismatch → plugin **deactivated**, NOT uninstalled. Files stay.
2. UI shows "incompatible with this CodeTerm version — needs
   ^1.3.0, you're on 2.0.0". Provider doesn't register, panes that
   used it become read-only (same flow as Case C uninstall).
3. `codeterm plugin update <id>` checks if any channel has a newer
   version with a compatible range; if yes, installs + reactivates.
4. Install-time variant: `plugin install` against an incompatible
   plugin refuses cleanly with the same message instead of silently
   installing a broken bundle.
5. System plugins must never trip this check on a paired release
   (we own both the binary and the bundle) — but if they do, embedded
   `include_dir!` fallback that ships with the binary IS by definition
   compatible, so loader falls through to it.

Acceptance: matrix in `evals/plugin-engines/` covers install-time
refusal, app-upgrade deactivation, and downgrade-to-compatible recovery.

### Case H — Plugin removed from its channel

Third-party dev unpublishes `gemini` from `community` channel.

1. Next `plugin marketplace refresh community` notices `gemini` no
   longer in `channel.json`.
2. Installed `gemini` plugin **stays installed** — files in
   `~/.codeterm/plugins/gemini/` are untouched. Plugin keeps working.
3. UI Plugin Manager shows `gemini` with an "orphan" badge — the
   plugin is installed but not reachable from any channel any more
   (no future updates).
4. `plugin update gemini` errors with "no channel offers gemini —
   either reinstall from a different channel or uninstall locally".
5. User decides: keep it (frozen), reinstall from another channel
   that picked it up, or uninstall.

Same flow for case where the **channel itself** is removed
(`marketplace remove community`): all plugins from that channel
flip to "orphan", local files untouched.

Acceptance: covered in `evals/plugin-orphan/` — plugin survives
channel removal, gets flagged, manual uninstall works.

### Case I — Downgrade and version pinning

User updated to claude 0.2.0, hit a regression, wants to roll back.

```bash
codeterm plugin install claude --version 0.1.0
# Downgrading 'claude' 0.2.0 → 0.1.0. Continue? [y/N]
codeterm plugin pin claude 0.1.0
# Pinned 'claude' at 0.1.0. `plugin update` will skip it.
```

Pin lives in `~/.codeterm/plugin-overrides.json`:

```json
{ "claude": { "pin": "0.1.0", "disabled": false } }
```

`disabled: true` is the keep-out-permanently from Case C.

For system plugins, pin works but UI nudges: "you're pinned below
the version this CodeTerm release expects; system status downgraded
to unverified". The user can run a hash-mismatched system plugin —
just without the lock + verified badge.

`plugin update` flow respects pins: skipped channel entries shown
in the dry-run output so the user remembers they're pinned.

Acceptance: `evals/plugin-pin/` covers downgrade install, pin
respect during update, system-status downgrade on pinned-old.

### Case J — Hash mismatch / disk corruption on a system plugin

User's `~/.codeterm/plugins/claude/plugin.js` got corrupted (disk
hiccup, manual edit experiment, sync mishap).

1. Loader computes hash on each startup. For ids in
   `SYSTEM_PLUGIN_HASHES`, mismatch means one of two things:
   - User has hacked it for local dev: plugin still loads, UI shows
     "unverified — locally modified".
   - File got corrupted: plugin probably won't even parse.
2. Self-heal path: `codeterm plugin verify --repair` (or implicit
   on launch when a system plugin fails to parse) re-extracts that
   id from `include_dir!` embedded fallback, overwriting the bad
   copy. Hash matches again → system status restored.
3. UI exposes the same button: "Re-install from embedded source".

Acceptance: `evals/plugin-repair/` covers both the
parse-fail-auto-repair path and the manual `--repair` invocation.

### Case K — Permissions change between versions

`corp-llm` v1 declared `permissions.network: false`. v2 declares
`permissions.network: ["api.corp.local"]`.

1. `plugin update corp-llm` sees the manifest delta in permissions.
2. UI surfaces a one-time "approve new permissions" dialog before
   activating the new version. Old version keeps running until
   approved.
3. CLI variant: `plugin update --all` skips plugins with pending
   permission deltas; lists them at the end. `plugin approve corp-llm`
   acknowledges and applies.
4. System plugins (claude/codex/opencode) skip this — the binary
   owns them, their permissions are implicit.
5. The plugin overrides file tracks each plugin's last-approved
   permission set so subsequent identical updates pass through.

Acceptance: `evals/plugin-perms/` flips a fixture plugin's
permissions between two installs, checks the prompt fires +
approval persists.

### Case L — Plugin author iterates locally before publishing

A developer is building `provider-grok` and wants to test it against
their live CodeTerm without pushing to a channel.

```bash
# in the plugin's source dir on disk
codeterm plugin install --from-dir .
# Installed 'grok' 0.0.1-dev from local dir → ~/.codeterm/plugins/grok/
# Watcher will hot-reload on every save.
```

`--from-dir` symlinks (or copies, on Windows where symlinks need
admin) the source directory into `~/.codeterm/plugins/<id>/`. Hot-reload
watcher already handles file-change → VM reload, so dev's edit/save
cycle is: edit `plugin.js`, save, alt-tab to a pane, behavior changed.
No restart, no `git push`.

The plugin is marked **local-dev** in `plugin info`: no channel
source, version suffix `-dev` if the manifest doesn't already use a
prerelease tag. `plugin update` skips it (nothing to pull from). To
"publish", the dev pushes to their git repo and lists it in a
`channel.json`; users add the channel via Case B.

Acceptance: a `docs/contributing/plugin-author-guide.md` walks
through `--from-dir` workflow + the "promote to channel" step.
Smoke test in `evals/plugin-local-dev/`.

### Case M — System plugin shipped with a critical bug, has to be yanked

We ship claude 0.2.0; within hours someone reports it deletes session
files. We need to pull it without waiting for users to upgrade the
whole binary.

1. `channel.json` in `codeterm-core` gains a `yanked` field:
   ```json
   { "id": "claude", "path": "provider-claude", "version": "0.2.0",
     "yanked": "session-file deletion bug, use 0.2.1" }
   ```
2. We immediately push a 0.2.1 that fixes the bug. Both versions
   coexist in the channel; 0.2.0 is just flagged.
3. `plugin update` (auto or manual) on any user's machine sees
   `yanked` on the installed version → rolls forward to 0.2.1
   automatically, even if the user pinned 0.2.0. Yanking is
   stronger than pinning by design (the only way to override is
   `plugin pin <id> 0.2.0 --acknowledge-yanked`).
4. Channel UI shows yanked versions with a strike-through and the
   reason string.
5. For users who haven't run `plugin update` in a while, next launch
   does a `channel refresh` if last refresh > 24h and applies the
   yank rollover transparently.

Same primitive works for third-party channels: a community author can
yank their own broken release the same way.

Acceptance: `evals/plugin-yank/` flips a fixture channel to mark a
version yanked, verifies forward-rollover + the `--acknowledge-yanked`
escape hatch.

### Case N — User learns about available updates

User has been on CodeTerm for a month; meanwhile claude shipped
0.3.0, codex shipped a config tweak, a community channel they added
got two new plugins.

1. Background `channel refresh` runs on a schedule (default: every
   6h, opt-out in settings). Cheap — just `git pull` on each cached
   channel dir.
2. Diff detection produces three states per id:
   - **Updated** (version in channel > installed)
   - **Yanked** (Case M)
   - **New available** (in channel, not installed)
3. Settings → Plugins gains a badge showing the count of
   updates/new available. Click → Plugin Manager opens with the
   updated rows highlighted + an "Update All" button.
4. No interrupt-style desktop notifications by default — Plugin
   Manager is the surface, not the toast. Power users can opt-in
   to a tray notification "N plugin updates available" via a
   settings toggle.
5. CLI mirrors: `plugin update --dry-run` lists what would change.

Acceptance: `evals/plugin-update-discovery/` simulates a stale
channel cache + verifies refresh + badge counts.

### Case O — Power user backs up + restores their plugin setup

User sets up a second machine and wants the same channels + plugins
as their main one. Mesh-sync is parked; we still need an answer.

```bash
# on machine A
codeterm plugin export > my-setup.json
# on machine B, after a fresh codeterm install
codeterm plugin import my-setup.json
# Adding channel 'community' from https://...
# Installing 4 plugins (3 from community, 1 system pin)
```

`my-setup.json` is a small declarative file:

```json
{
  "schemaVersion": 1,
  "channels": [
    { "url": "https://github.com/community/codeterm-plugins" }
  ],
  "installed": [
    { "id": "claude",     "version": "0.2.1" },
    { "id": "gemini",     "version": "0.3.0", "channel": "community" },
    { "id": "corp-llm",   "version": "1.4.2", "channel": "internal-team",
      "pinned": true }
  ],
  "disabled":  ["opencode"],
  "overrides": { /* permission approvals, etc */ }
}
```

System plugins reference their version but install always resolves
to whatever the running binary considers system (no drift). Third-
party plugins install their exact version, pinned. Private channels
in the export carry the URL only — user re-auths via their own git
credentials, no secrets shipped in the export.

`plugin import` is idempotent: re-running with the same file is a
no-op.

Acceptance: `evals/plugin-export-import/` round-trips a fixture
setup across two `~/.codeterm/` dirs.

### Case P — Plugin discovery (finding new channels)

Adding a channel still requires knowing its URL. Where does a user
learn about channels in the first place?

1. **Curated index in the docs site** (`docs/plugins-channels.md`)
   — listing of channels we know about, with description + author +
   "last updated" timestamp. Editable by community via PR.
2. **No in-app channel discovery in this rev.** A "Discover" tab
   that searches a global index is post-MVP; deferred until at least
   five channels exist in the wild.
3. Default channel is always pre-added (Case D bootstrap), so the
   user gets value with zero channel discovery.

Acceptance: `docs/plugins-channels.md` exists with the default
channel entry and a template stanza for community submissions.

## Documentation + prompt updates that ship with this work

Easy to forget, gates on shipping:

- **`src-tauri/prompts/codeterm_cli.md`** — full command reference.
  Add the 14 `plugin` verbs from the CLI surface section.
- **`src-tauri/prompts/codeterm_rule.md`** — GA trigger table.
  Add rows for "install plugin X" / "list available plugins" /
  "uninstall X" / "channel add <url>" → matching commands.
- **`src-tauri/prompts/seed/skill_codeterm.md`** — bridge_mem
  skill catalog. Add one bullet line per verb family
  (`codeterm plugin {install,remove,update,channel,...}`).
- **`docs/contributing/plugin-author-guide.md`** — NEW. Walk
  authors through the local-dev → channel-publish flow (Case L
  → Case B).
- **`docs/plugins-channels.md`** — NEW. Curated channel index
  (Case P).
- **`docs/API.md`** — REST/Tauri endpoints for the plugin/channel
  surface (whatever the CLI calls into).
- **`CLAUDE.md`** — short pointer at the bottom: "to extend agent
  providers, see plugins/README.md + docs/plans/relay/20 +
  docs/contributing/plugin-author-guide.md".

Phase 1 ships the prompt updates with the build that contains the
`plugin` CLI verbs — agents inside CodeTerm shouldn't see verbs
they don't yet have.

## Parked (out of scope this rev, listed so we don't pretend we forgot)

- **Bundle integrity beyond top-level hash.** Right now we hash the
  bundle directory canonically — covers tampering but not "channel
  shipped a half-bundle". A `files` field in `plugin.json` listing
  every shipped file + per-file sha would close the last 1%. Defer
  until we see a bad-bundle bug in the wild.
- **Plugin rename / `replaces: [oldId]` migration.** Useful when a
  community plugin rebrands. Park behind seeing the first rename in
  practice.
- **Mesh peer plugin sync.** Each machine manages its own plugin set
  today. Cross-peer "install the same plugins as my laptop" would be
  a nice ergonomic feature but is a different problem from
  marketplace and should not pollute the core flow.
- **Audit log of `host.*` calls per plugin.** Useful security-review
  surface once third-party plugins are common. Wire when the third
  community plugin lands — earlier than that, premature.
- **Bandwidth-bound channel refresh + telemetry.** Today every
  `marketplace refresh` does a full `git pull`. Once we have many
  channels with many plugins, a manifest-only fetch (just
  `channel.json` HEAD) is the obvious optimisation. Park.

## Schema review against VSCode / Claude Code / Zed / Obsidian

Compared our v2 draft against the four reference manifests. Adopting
the lessons that aren't VSCode-style bloat. Edits to the manifest
shown below; rationale in inline comments.

### Plugin manifest v2 (revised)

```jsonc
{
  "schemaVersion": 2,                          // manifest shape; bump
                                               //   on breaking change
  "id": "claude",                              // immutable; doubles as
                                               //   the folder name
                                               //   (Obsidian lesson:
                                               //   id != folder breaks
                                               //   lifecycle hooks)
  "version": "0.1.0",                          // strict semver. If
                                               //   omitted → git SHA
                                               //   becomes the version
                                               //   (Claude Code rule;
                                               //   convenient for
                                               //   --from-dir)
  "displayName": "Claude Code",
  "description": "Anthropic Claude provider with hooks + skills.",
  "author": {                                  // object, not bare
    "name": "codeterm-core",                   //   string — matches
    "email": "team@codeterm.ai",               //   Claude + npm
    "url": "https://codeterm.ai"
  },
  "license": "MIT",                            // SPDX id
  "repository": "https://github.com/codeterm/plugins",
  "homepage": "https://codeterm.ai/plugins/claude",
  "keywords": ["claude", "anthropic", "provider"],
  "categories": ["providers"],                 // curated set;
                                               //   marketplace
                                               //   filtering
  "icon": "icon.svg",                          // relative to plugin dir
  "preview": false,                            // beta/unstable flag

  "engines": { "codeterm": "^1.3.0" },         // runtime compat

  "activationEvents": [                        // VSCode's biggest perf
    "onProvider:claude",                       //   lesson — declare
    "onWorkspace"                              //   when to wake the
  ],                                           //   VM; default "lazy"

  "extensionKind": ["local", "mesh"],          // local / remote /
                                               //   mesh — relevant
                                               //   when codeterm peers
                                               //   sync plugin state

  "contributes": {                             // explicit ONLY for
    "providers": [                             //   contributions that
      { "id": "claude", "entry": "plugin.js" } //   need a JS entry.
    ]                                          //   skills/, hooks/
                                               //   discovered from
                                               //   their dirs.
  },

  "dependencies": {                            // other plugin ids
    "host-utilities": "^0.2.0"                 //   this one needs
  },
  "conflictsWith": [],                         // plugin ids that
                                               //   can't coexist
                                               //   (e.g. two
                                               //   claude-resume
                                               //   patchers)

  "permissions": {                             // granular caps; tag
    "fs":        { "~/.claude": "rw" },        //   each fs path with
    "sqlite":    { "~/.claude/store.db": "r" }, //  mode
    "network":   ["api.anthropic.com"],        // allow-list (false →
                                               //   no network)
    "processes": ["claude"],                   // spawn allow-list
    "env":       { "read": ["CLAUDE_*"],
                   "write": [] }
  }
}
```

### Edits adopted from the audit

1. **Keep `id` (not `name`).** Obsidian's id≡folder rule is what we
   already do via the watcher; renaming buys nothing and breaks Rust
   `register_provider(id)` ergonomics. `displayName` already covers
   the human label.
2. **`repository`, `homepage`, `license`, `keywords`, `categories`,
   `icon`, `preview`** — all free metadata, used by every system.
3. **`author` becomes an object** with `{name, email?, url?}`.
   Matches Claude / npm.
4. **`activationEvents` baked in from day one.** Default = lazy
   (`onProvider:<id>`, `onSkill:<id>`, `onHook:<event>`). Plugin can
   opt into `onStartup` if it really has to. VSCode's biggest
   retrofit pain — don't repeat.
5. **`extensionKind` array** — we have mesh, so VSCode's local/remote
   split is immediately relevant. Plugin states which contexts it's
   safe in.
6. **`dependencies` + `conflictsWith`** — VSCode's
   `extensionDependencies` distilled to one field each. Skipping
   `extensionPack` (curation primitive we don't need at MVP).
7. **`permissions` expanded** — added `processes` (spawn allow-list),
   `env` (read/write env-var allow-lists), tagged fs paths with
   `r`/`rw`. Closer to a real capability model, still parseable.
   Granularity matters once third-party plugins land — VSCode
   regrets having none.
8. **Drop empty `contributes` arrays for `themes/skills/hooks`** in
   examples. Claude's pattern: presence of `skills/`, `hooks/hooks.json`
   directories auto-discovers. Only `providers` needs an explicit
   entry script.
9. **`${CODETERM_PLUGIN_ROOT}` env var** — bound when invoking any
   provider/hook command, equals the plugin's install directory.
   Plugins reference resources relocatably (`${CODETERM_PLUGIN_ROOT}/svg/claude.svg`).
10. **Version-resolution rule** spelled out: explicit `version` wins;
    if absent (only allowed for `--from-dir` installs and channel
    entries with `ref` instead of `version`), the git SHA the plugin
    was built from becomes the version. Channel manifests MUST
    specify a version (no implicit-SHA in user-facing channels).

### Edits deferred (not worth complexity this rev)

- `bin/` Claude convention — providers already handle their CLI
  themselves; `cliBinary` + `installer` in current manifest cover it.
- `extensionPack` (VSCode) — curation primitive, premature.
- `qna`, `sponsor`, `badges`, `pricing`, `galleryBanner` —
  marketplace decoration, can grow into them organically.

### Channel manifest v2

Channel manifest gets a small but important upgrade — polymorphic
plugin sources, owner block, reserved-name validation.

```jsonc
{
  "schemaVersion": 1,
  "name": "codeterm-core",
  "displayName": "CodeTerm Core",
  "description": "Official provider plugins shipped by the CodeTerm team.",
  "owner": {                                   // required; channel
    "name": "CodeTerm Team",                   //   maintainers visible
    "email": "team@codeterm.ai",               //   to users so they
    "url": "https://codeterm.ai"               //   know who to trust
  },
  "metadata": {
    "pluginRoot": ".",                         // relative base for
                                               //   `source` paths
                                               //   below — lets a
                                               //   channel keep
                                               //   plugins in a
                                               //   subdir
    "version": "2026.05.18"                    // catalog version,
                                               //   bumped by author
                                               //   on every plugin
                                               //   add/update
  },
  "plugins": [
    {
      "id": "claude",
      "source": "provider-claude",             // relative dir in this
                                               //   channel repo
      "version": "0.2.0"                       // MUST match the
                                               //   referenced plugin's
                                               //   own plugin.json
    },
    {
      "id": "gemini",
      "source": { "type": "github",            // polymorphic source:
                  "repo": "vendor/codeterm-gemini",
                  "ref":  "v0.3.0" },
      "version": "0.3.0"
    },
    {
      "id": "experimental",
      "source": { "type": "npm",               // future expansion —
                  "package": "@vendor/codeterm-plugin",
                  "version": "0.1.0" },
      "version": "0.1.0"
    }
  ]
}
```

### Reserved names and path hygiene

Bake validation into channel-add + plugin-publish:

- Reserved channel names: `codeterm`, `codeterm-core`, `codeterm-official`,
  `core`, `system`, `default`. Channel registering with a reserved
  name + a non-system source is rejected at `plugin channel add`.
- Reserved plugin ids: same list. A community plugin claiming
  `id: "claude"` loads (Case J) but with the unverified flag and
  never claims system status — hash check blocks shadowing.
- No `../` in any source path. Paths normalised to plugin's own
  install dir on copy. (Claude's cache-copy lesson.)
- `id` must match the install folder name. Mismatch = manifest
  validation error. (Obsidian's lifecycle-hook lesson.)
- Channel name with mixed case + spaces lower-snake on save
  (`Cool Stuff` → `cool-stuff`), warned to author at publish.

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
