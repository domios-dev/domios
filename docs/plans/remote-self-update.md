# Remote self-update over the mesh

**Status:** design
**Source:** Andrey, 2026-05-02

## Goal

Trigger a peer machine to upgrade CodeTerm in place — manifest fetch → ed25519
verify → install → graceful restart → pane auto-resume — from another machine
on the mesh. No interactive login required on the peer.

```bash
codeterm update --check --mesh BLACKSTAR
codeterm update --apply --mesh BLACKSTAR
```

The local `codeterm update --apply` already drives the full flow via
`tauri-plugin-updater`. The only missing piece is exposing it over the mesh.

## Non-goals (v1)

- Streaming progress updates (download %). v1 returns a final status; the peer
  log is the source of truth for in-flight detail.
- Forcing peers to update. The caller asks; the peer either complies or rejects
  based on access level.
- Rollback / pre-flight health check. Same constraints as local update.
- Cross-version negotiation beyond a clear "peer too old" error. We do not
  attempt to upgrade peers running a release that predates this feature; those
  get bootstrapped by hand once.

## Existing surfaces (reused, unchanged)

- `update::apply::check_and_apply` — canonical entrypoint. Drives the Tauri
  updater (download → verify → install → graceful shutdown → restart).
- `/api/update/check` (GET) — returns `UpdateInfo` (current/latest/url/etc.).
  Caches result for 1h (`force=true` to skip cache).
- `/api/update/install?method=auto` (POST) — spawns the updater on a fresh
  thread; returns immediately with `status: updating`. Requires `last_check` to
  be populated first (otherwise `400 Run check first`).
- `/api/mesh/relay` (POST, paired-only) — generic relay envelope:
  `{ method, path, body }`. Verifies the caller's Ed25519 signature against the
  set of paired-peer public keys, then re-issues the call against the local API
  with a freshly signed token.
- CLI `--mesh PEER` global flag → `mesh_cli::run_via_mesh` →
  `cli_command_to_api(cmd)` → `mesh_relay_generic`. Currently
  `cli_command_to_api` returns `("GET", "/api/update/check", None)` for **every**
  update variant, which is why `--apply --mesh` silently no-ops (it relays a
  check call instead of an install call).

## Design

### Wire format

No new endpoints, no new envelope. Reuse `/api/mesh/relay` and the existing
`/api/update/{check,install,channel}` endpoints. The only change is teaching
the CLI to translate update subcommands into the right (method, path, body)
tuple.

This means peers running today's `develop` (with `/api/update/install`
already wired) are immediately reachable. There is no protocol negotiation:
either the peer's relay forwards to a working `/api/update/install` or it
doesn't. The "peer too old" case (e.g. v1.0.1 missing the install endpoint)
surfaces as an HTTP 404 in the relay response, which the CLI maps to a clear
"peer too old, manual update needed" error.

### CLI dispatch

`cli/src/mesh_cli.rs::cli_command_to_api` switches on the resolved
`UpdateAction` (after applying the `--check` / `--apply` / `--channel`
shortcuts):

| CLI form                                          | Method | Path                              | Body                          |
|---------------------------------------------------|--------|-----------------------------------|-------------------------------|
| `update --check` / `update check`                 | GET    | `/api/update/check?force=…`       | none                          |
| `update --apply` / `update install --auto`        | POST   | `/api/update/install?method=auto` | none (manifest_url passes through as a query param when set) |
| `update --channel` (no value) / `update channel`  | GET    | `/api/update/channel`             | none                          |
| `update channel <name>`                           | POST   | `/api/update/channel`             | `{"channel":"<name>"}`        |

`--apply` over the mesh runs **two** relay calls in sequence:

1. POST `/api/update/check?force=true` to populate the peer's `last_check`
   cache. This is required by the existing local handler — a relayed apply
   without a prior check would 400.
2. POST `/api/update/install?method=auto` to start the install.

The CLI prints a one-line status for each step:

```
[BLACKSTAR] checking…  current v1.1.1, latest v1.2.0 — update available
[BLACKSTAR] applying…  status=updating, message="Downloading and installing v1.2.0. App will restart."
```

If step 1 reports `update_available=false` we short-circuit and skip step 2.

### Peer-side authorization

Mesh relay already gates on "must be paired". For `update` we tighten this:
the peer rejects relay calls whose path matches `/api/update/install` (or
`POST /api/update/channel`) when the calling peer's `access_level` is not
`Full`. This lives in `mesh_routes::mesh_relay` so it covers the install +
channel-write paths consistently. Read-only endpoints (`GET
/api/update/check`, `GET /api/update/channel`) remain available to all paired
peers regardless of access level — same as today.

Error surface: `403 Forbidden` with `{ ok: false, error: "Peer not authorized
to install updates (access_level=…)" }`. The CLI prints this verbatim and
exits non-zero.

Local `/api/update/install` is unchanged — only relay-originated calls go
through the gate.

### Restart flow

The peer continues to call `update::apply::check_and_apply` exactly as the
local path does. After install, `shutdown::graceful_shutdown` runs (saves
panes, flushes mem, etc.) and `app.restart()` brings the daemon back. Panes
auto-resume on next start. No mesh-specific restart code.

The mesh caller's `apply` relay call returns immediately with `status:
updating` (the install runs on a background thread). The caller does **not**
wait for the peer to come back online; verification is the caller's
responsibility (poll `update --check --mesh PEER` after a minute, or watch the
mesh-state-changed events in the UI).

### Version negotiation / "peer too old"

If the peer is running a release that predates `/api/update/install` (none in
the current line — the endpoint shipped in 1.1.0), the relay forwards to
`localhost` and gets a 404. `mesh_relay` propagates that as
`{ ok: false, status: 404 }` in the relay response. The CLI detects 404 on
the install path and prints:

```
Peer 'BLACKSTAR' is running an older CodeTerm without remote-update support.
Bootstrap manually one last time, then this command will work going forward.
```

Same treatment for relay-not-found (peer pre-mesh-relay), though that'd
require a v0.x peer which doesn't exist in the wild.

### Backward compatibility

The local update path (`codeterm update --apply` without `--mesh`) is
untouched. No new fields on `UpdateInfo`, no schema changes, no migration.

## Implementation

### Files

- `cli/src/mesh_cli.rs`
  - Replace the catch-all `Commands::Update { .. } => …` branch in
    `cli_command_to_api` with a real `match` on the resolved subcommand.
  - Add `run_update_via_mesh` helper that orchestrates the two-step
    check-then-apply for `--apply`, and special-case 404 responses to print
    the "peer too old" hint.
  - `mesh_cli::run_via_mesh` calls `run_update_via_mesh` instead of the
    generic relay when the command is `Commands::Update` with `--apply`.
- `src-tauri/src/api/mesh_routes.rs::mesh_relay`
  - Before forwarding, look up the sender's `PairedPeer` and check
    `access_level == Full` for path-prefix matches `/api/update/install` and
    `POST /api/update/channel`. Return 403 otherwise.
  - Local-relay path (`sender_id == our_id`) is exempt — relay-from-self is
    only used by tests/loopback.

### Tests

- Unit: extend the existing `cli_command_to_api` coverage (if any — otherwise
  add a small smoke test) to assert the new mappings.
- Loopback: `codeterm update --check --mesh <self>` round-trips through the
  local `/api/mesh/relay` and prints the same `UpdateInfo` as the direct
  call. (Self-mesh works because we have our own public key in
  `known_public_keys`.)
- Manual: skip the actual `--apply --mesh` against a real peer (would
  reinstall a peer we depend on). Andrey will exercise it by hand once
  BLACKSTAR is on a build that includes this change.

## Risks / open questions

- **Two-step apply is not atomic.** A peer could check, then a different
  caller could run another check (force=true) that races and ends up
  installing a different version. Mitigation: low likelihood (manual
  multi-machine sequencing), and worst case the peer just installs whatever
  the manifest says is latest. Accept for v1.
- **No streaming.** Caller sees `status: updating` and silence until a poll.
  If users request progress, we'd add a tail-the-peer-log shortcut (pane
  output for the General Agent on the peer is one option).
- **`access_level` is currently always `Full` in the codebase** (default,
  never changed). The gate is forward-looking: it makes the contract
  explicit so future read-only / agent-isolated peers don't get install
  privileges by accident.
