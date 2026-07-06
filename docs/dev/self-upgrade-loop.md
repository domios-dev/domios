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
| Shutdown + relaunch | `src-tauri/src/update/shutdown.rs` — its steps 7-9 already are "relaunch + session restore + per-pane resume" |
| Restart command | `commands::restart_app` (`api/network_routes.rs` web twin) |
| Session/pane restore | the restore-matrix machinery; pane ids stabilize once the stable-id task lands |
| Continuation intent | the orchestrator refresh cycle's S4 re-orient injection (`service/orchestrator/refresh/driver.rs`) — same "inject a message into the resumed pane" primitive |

The loop is glue over these, not new subsystems.

## Design

### 1. Per-tab permission (the gate)

Self-upgrade is destructive to the running process, so it requires an
explicit opt-in on the pane that requests it — never ambient. A pane flag
`allow_self_upgrade: bool` (default false), settable only from the tab's own
menu (a visible toggle, like bypass-permissions) or a dev CLI call. The
upgrade command refuses if the calling pane lacks it. This keeps it a
deliberate, per-tab, dev-session act.

### 2. The command (hidden)

```
codeterm dev self-upgrade --intent "<what to do after restart>" [--dev]
```

- Lives under a `dev` subcommand group that is compiled/registered only for
  dev instances (`codeterm_paths::is_dev_instance()`), or hidden with
  `#[command(hide = true)]` so it never appears in `--help`.
- Refuses unless the calling pane has `allow_self_upgrade`.
- `--dev` rebuilds+restarts the dev instance (7686); default rebuilds the
  prod-style app but ONLY when explicitly on a dev machine.

### 3. Flow

1. **Record the resume marker** — before touching the binary, persist a
   durable `{pane_session_id, intent, requested_at, expected_build}` record
   (a small json under the data dir, e.g. `~/.codeterm/pending-upgrade.json`).
   Durable so it survives the process death.
2. **Build** — `./build.sh build` (or `dev`). On failure: abort, clear the
   marker, report the build error to the pane — never restart into a broken
   binary.
3. **Verify the build** — the new binary's `/api/health` build hash must
   differ from the current and match the just-built commit; a same-hash build
   means nothing changed — skip the restart, tell the pane.
4. **Apply + restart** — reuse `update::apply` / `restart_app`. The updater's
   steps 7-9 restore the session and re-open the panes.
5. **Resume-after-upgrade hook** — on startup, after session restore, the
   daemon checks `pending-upgrade.json`: if the recorded pane session id is
   back, inject the intent into that pane (the refresh-cycle S4 primitive),
   then delete the marker. One-shot; a stale marker (>N minutes, or build
   mismatch) is discarded with a log, never replayed.

### 4. Continuity oracle

Same standard as the refresh cycle: the agent's next observable action after
the injected intent must be real work (a tool call / plan read), within a
bounded window; if the pane never resumes, alert (notification) rather than
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

## Status

Design only (this doc). The seams above are real and current; implementation
is the glue command + the per-tab flag + the resume hook + the hidden `dev`
subcommand. Build it behind the dev gate; keep it out of every agent-facing
surface.

<!-- revision: 1 -->
