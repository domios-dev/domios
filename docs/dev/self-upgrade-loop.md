# Self-upgrade loop (dev-only)

> Dev-only capability. NOT surfaced in CLI `--help`, NOT in AGENTS.md, NOT in
> agent prompts. It exists so a dev-driving agent can rebuild its own binary,
> reinstall, restart, and **continue the task it was on** — a closed
> build→install→relaunch→resume loop. Prod builds must gate it off.

## Why

During heavy self-modification (e.g. the storage-v2 rewrite) the agent hits a
wall: a fix only takes effect after a rebuild + daemon restart, and a restart
normally drops the agent's working context. Today that means a human has to
rebuild, restart, and re-brief. The self-upgrade loop closes that: the agent
requests an upgrade *with a continuation intent*, and comes back running,
re-oriented, still on task.

## The pieces already exist

| Step | Existing seam |
|---|---|
| Build | `./build.sh build` (or `dev` for the dev instance) |
| Apply/install | `src-tauri/src/update/apply.rs` (`app.restart()` at :111) |
| Shutdown + relaunch | `src-tauri/src/update/shutdown.rs` — its steps 7-9 already are "relaunch + session restore + per-tab resume" |
| Restart command | `commands::restart_app` (`api/network_routes.rs` web twin) |
| Session/tab restore | the restore-matrix machinery; Tab ids stabilize once the stable-id task lands |
| Continuation intent | the orchestrator refresh cycle's S4 re-orient injection (`service/orchestrator/refresh/driver.rs`) — same "inject a message into the resumed Tab" primitive |

The loop is glue over these, not new subsystems.

## Design

### 1. Per-tab permission (the gate)

Self-upgrade is destructive to the running process, so it requires an
explicit opt-in on the Tab that requests it — never ambient. A Tab flag
`allow_self_upgrade: bool` (default false), settable only from the tab's own
menu (a visible toggle, like bypass-permissions) or a dev CLI call. The
upgrade command refuses if the calling Tab lacks it. This keeps it a
deliberate, per-tab, dev-session act.

### 2. The command (hidden)

```
codeterm dev self-upgrade --intent "<what to do after restart>" [--dev]
```

- Lives under a `dev` subcommand group that is compiled/registered only for
  dev instances (`codeterm_paths::is_dev_instance()`), or hidden with
  `#[command(hide = true)]` so it never appears in `--help`.
- Refuses unless the calling Tab has `allow_self_upgrade`.
- `--dev` rebuilds+restarts the dev instance (7686); default rebuilds the
  prod-style app but ONLY when explicitly on a dev machine.

### 3. Flow

1. **Record the resume marker** — before touching the binary, persist a
   durable `{pane_session_id, intent, requested_at, expected_build}` record
   (a small json under the data dir, e.g. `~/.codeterm/pending-upgrade.json`).
   Durable so it survives the process death.
2. **Build** — `./build.sh build` (or `dev`). On failure: abort, clear the
   marker, report the build error to the Tab — never restart into a broken
   binary.
3. **Verify the build** — the new binary's `/api/health` build hash must
   differ from the current and match the just-built commit; a same-hash build
   means nothing changed — skip the restart, tell the Tab.
4. **Apply + restart** — reuse `update::apply` / `restart_app`. The updater's
   steps 7-9 restore the session and re-open the Tabs.
5. **Resume-after-upgrade hook** — on startup, after session restore, the
   daemon checks `pending-upgrade.json`: if the recorded Tab session id is
   back, inject the intent into that Tab (the refresh-cycle S4 primitive),
   then delete the marker. One-shot; a stale marker (>N minutes, or build
   mismatch) is discarded with a log, never replayed.

### 4. Continuity oracle

Same standard as the refresh cycle: the agent's next observable action after
the injected intent must be real work (a tool call / plan read), within a
bounded window; if the Tab never resumes, alert (notification) rather than
silently stranding it. A self-upgrade that rebuilds but never resumes is a
dead loop — worse than not upgrading.

## Safety rules

- **Dev-only.** Gate the whole command on `is_dev_instance()` (or an explicit
  dev build feature). Prod must not expose it.
- **Per-tab permission required**, always. No global default.
- **Never restart into an unbuilt/broken binary** — build success + health
  hash change are hard preconditions.
- **One-shot marker** — never replay; clear on resume, discard on staleness.
- **Not documented for agents.** No mention in AGENTS.md, role prompts, or
  CLI `--help`. This doc is the only home. An agent learns it exists only if a
  human points it here in a dev session.

## Working today: `codeterm update install --from-file`

The `dev self-upgrade` command above is still design. What already works — and
is what to use to rebuild-and-reinstall on request — is the existing installer
path with a continuation prompt:

```bash
# 1. build a signed DMG from the current tree (env -u bypasses the prod-port guard)
env -u CODETERM_API_PORT -u CODETERM_INSTANCE ./build.sh build
# -> src-tauri/target/release/bundle/dmg/CodeTerm_<ver>_aarch64.dmg

# 2. install it in place and resume THIS tab afterwards
codeterm update install \
  --from-file src-tauri/target/release/bundle/dmg/CodeTerm_<ver>_aarch64.dmg \
  --continue-tab "$CODETERM_PANE_ID" \
  --continue-prompt "you're back on the new build — continue <the task>"
```

`run_install_from_file` (`cli/src/cmd/install.rs`) writes a detached bash helper
that outlives the daemon, then returns. The helper:

1. (CLI, before detaching) forces `POST /api/session/save` so current Tab ids —
   including Tabs spawned since the last 30s autosave — are on disk.
2. quits + hard-kills the app, `rm -rf`s the old bundle, `ditto`s the new one
   into `/Applications`, relaunches, waits for `/api/health`, then delivers the
   continue-prompt to `--continue-tab` via `codeterm send`.

Restore reuses saved tab ids, so the continue-tab target resolves and the
agent resumes mid-task.

### macOS gotchas (learned the hard way — do not regress)

- **`osascript -e 'quit app "CodeTerm"'` does NOT terminate.** The CloseRequested
  handler calls `prevent_close()` + hide-to-tray, so "quit" just hides the
  window. You must actually kill the process (SIGTERM loop → SIGKILL) and wait
  for it to be gone, or every later step silently no-ops against the still-live
  old app (which then answers health and eats the continue-prompt — the install
  looks like it worked but nothing changed).
- **You cannot `ditto` over a running Mach-O** — the kernel returns ETXTBSY and
  the old binary stays. `rm -rf /Applications/CodeTerm.app` FIRST (unlink drops
  the directory entry even while a stale process holds the inode), then `ditto`
  the fresh bundle.
- **Mount at a space-free `-mountpoint`.** Volume names collide and stack
  (`/Volumes/CodeTerm`, `/Volumes/CodeTerm 1`, …); a `grep '/Volumes/[^ ]*'`
  truncates on the space to a path that does not exist. Use
  `hdiutil attach -mountpoint "$(mktemp -d)"`.
- **The `/api/health` `build` hash can be stale.** `build.rs` stamps the git
  hash but does not `rerun-if-changed=.git/HEAD`, so an incremental build keeps
  the previous hash. Do NOT trust the hash alone to confirm "the new build is
  running" — verify a behavioural signal instead (e.g. task writes land as
  `tasks/*.yaml`, not legacy `plan.json` + `*.md`).

## Status

`update install --from-file` (above) is implemented and is the current path.
The `dev self-upgrade` glue (per-tab flag + resume hook + hidden `dev`
subcommand) is still design. Build it behind the dev gate; keep it out of every
agent-facing surface.

<!-- revision: 4 -->
