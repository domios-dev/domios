# CodeTerm Mesh Networking

## Core Principles

1. **Paired = trusted.** Not paired = doesn't exist.
2. **AccessLevel** (Full/ReadOnly/AgentIsolated) is per-peer, per-machine. Each machine decides locally what a peer can do. Default = Full.
3. **No crown, no king, no trust declarations, no TTL.** Flat peer-to-peer model.
4. **Unpair = ban.** Pubkey removed, auth fails naturally.
5. **One machine = one mesh.** Cannot be in two meshes simultaneously.

## Discovery

Discovery (mDNS/UDP/Tailscale) finds peers on the network. Paired peers are shown in the mesh, unpaired peers appear in the "Discovered" list.

## Auth Model

Every paired peer has an Ed25519 keypair. Requests between peers are signed with the sender's private key and verified against known public keys.

- `sign_request()` — signs a request with this peer's private key
- `verify_signed_request()` — verifies signature against known paired peer public keys
- Paired = allowed. Not paired = rejected.
- AccessLevel check can be added per-endpoint in the future.

## Flows

### Flow 1: Creating a Mesh (First Pair)

1. A and B see each other via discovery (both standalone)
2. A clicks "Connect" on B — PairingRequest sent to B
3. B sees request — clicks Approve
4. Both exchange Ed25519 public keys
5. Both machines paired — mesh created
6. Introductions broadcast to all existing peers

### Flow 2: Joining an Existing Mesh

1. C enables mesh — discovery finds peers
2. C clicks "Add to Mesh" — request sent to the peer
3. Peer approves — pubkey exchange
4. Peer sends introductions to all existing peers
5. C is now in the mesh. All peers know about C.

### Flow 3: Disconnecting a Peer

1. Machine clicks "Disconnect" on a peer — or "Exit Mesh" to leave entirely
2. Notifies all peers to remove the disconnected machine
3. Pubkey removed — auth fails naturally for any future requests

## Security

- Ed25519 signed requests with 5-minute expiry and nonce (replay protection)
- Manual approval for pairing (no auto-pair)
- Public keys exchanged during pairing, verified on every request
- Unpair = remove pubkey = auth fails

## Bridge migration

External-service bridges (Telegram today; Discord/Slack/etc. later) hold an exclusive long-poll lock against their upstream API — only one poller per bot token can be active at a time. `codeterm bridge migrate --to PEER` hot-relocates that poller from the source machine to a paired mesh peer **without restarting either daemon**. This is the recovery story for "I'm shutting down BLACKSTAR for the night, please move the Telegram bot to mac-mini until I'm back."

### CLI

```bash
codeterm bridge migrate --to PEER \
  [--service telegram] \
  [--announce] [--announce-chat-id 186809246] \
  [--no-rollback] [--force] [--claim-timeout-ms 10000]
```

Must run on the **source** machine. Running it through `--mesh PEER` is rejected (exit code `6`) because the orchestration legitimately needs to talk to both daemons in opposite directions.

### Orchestration steps

1. **Source preflight.** Read local `/api/bridge/state` and `/api/identify` to confirm the bridge is reachable and grab the source's `instance_name`. Idempotency check: if `--to` equals the source name, exit `0` with a noop log.
2. **Resolve peer.** Look up the target via `/api/mesh/peers`, pick the first reachable address + `api_port`. Missing peer → exit `5`.
3. **Target preflight (via mesh relay).** `GET /api/health` and `GET /api/bridge/state` against the target through `/api/mesh/relay`. Verify `token_present || has_secret_in_vault` (skipped with `--force`). No token and no vault entry → exit `5` (`no token and no Bitwarden secret 'telegram_bot_token' in vault`). If the target is already serving and the source is not, exit `0` with `already migrated`.
4. **Disable source config.** `PATCH /api/config integrations.telegram_enabled=false` locally — persisted before stopping the supervisor so a crash during stop doesn't leave config and runtime disagreeing.
5. **Release source.** `POST /api/bridge/stop {timeout_ms: 30000}`. The supervisor's `op_guard()` (a tokio Mutex) serializes this against any concurrent start/stop, including the one that's about to fire on the target.
6. **Enable target config (via relay).** `PATCH /api/config integrations.telegram_enabled=true` on the target so future reboots auto-start the bridge there.
7. **Claim target (via relay).** `POST /api/bridge/start {ensure_token: true, wait_for_lock_ms: <claim-timeout>}` against the target. The handler resolves the token from the **target's own** vault if the local file is missing, persists it locally, then waits for `last_poll_ok >= started_at`.
8. **Verify.** Re-query both states. Source must be `active=false`, target must be `active=true`. Any mismatch triggers rollback.
9. **Rollback (default).** If the target claim or verification fails, re-enable source config and call `POST /api/bridge/start` locally. If the source rollback succeeds, exit `1` (migration failed but cluster is consistent). If rollback also fails, exit `7` and print the manual repair command:
   ```
   codeterm config set integrations.telegram_enabled true && codeterm bridge restart
   ```
10. **Optional `--announce`.** When `--announce` is set, post `[CodeTerm] Telegram bridge migrated to <to> from <from>` to `--announce-chat-id` (default `186809246`) via `POST /api/bridge/announce` on the target. Failures here are non-fatal — the migration is already done.

### Secret locality guarantee

Each daemon resolves its own vault. The CLI never reads `telegram_bot_token` from the source machine and never sends it across the wire — `--ensure-token` only ever fires on the daemon that already owns the secret store. This matches the existing local-only contract for `--mesh PEER mem secret …` (which exits `7`) and the daemon's refusal to relay `/api/secrets/*` over mesh transport. Each peer holds its own vault scoped to its own keychain, full stop.

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--to` | *(required)* | Target peer `instance_name` (or `instance_id`) as shown in `codeterm mesh peers`. |
| `--service` | `telegram` | Which bridge to migrate. Only `telegram` today. |
| `--announce` | `false` | Post a one-shot sentinel message after success. |
| `--announce-chat-id` | `186809246` | Telegram chat id for `--announce`. Defaults to the maintainer's chat. |
| `--no-rollback` | `false` | Skip the source-recovery step on failure (leaves source disabled). |
| `--force` | `false` | Skip the target preflight (`token_present || has_secret_in_vault`) check. |
| `--claim-timeout-ms` | `10000` | How long the target may take to claim the upstream lock before the migration is considered failed. |

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success (or no-op: already on target) |
| `1` | Generic failure — rollback completed (cluster consistent) |
| `5` | Preflight (target unreachable, no token + no vault entry, peer not in `/api/mesh/peers`) |
| `6` | Usage error (e.g. ran via `--mesh` — must run on the source machine) |
| `7` | Split-brain (rollback also failed — manual repair needed) |

## Mesh-disabled gate (503)

When `config.mesh.enabled = false`, the auth middleware short-circuits every `/api/mesh/*` path (other than `/api/mesh/ping`) with `503 Mesh networking is disabled`. The same 503 also fires for any caller authenticated via the mesh signed-token path — so other peers attempting to relay into us also get rejected, not just the local UI. The local Bearer token (`api.token`) keeps working for non-mesh routes regardless.

**Disabling mid-session** (PATCH `/api/config mesh.enabled=false`) additionally: stops the discovery broadcast, marks every paired peer `online = false`, clears the discovered-peers list, and emits `mesh-state-changed` so the UI updates immediately. Re-enabling restores the routes; broadcast and peer state pick back up on the next discovery / ping tick. (Code-side gap: `mesh::ping_scheduler` does not consult `mesh.enabled` itself — it keeps trying to ping paired peers regardless. Inbound to us is rejected by the middleware, so peers we ping still see us as offline. Functionally OK because the auth middleware is the kill-switch, but the outbound ping traffic continues. Tracked for follow-up.)

## Peer-ping scheduler

`mesh::ping_scheduler` runs as a background tokio task. Every 20 s it pings every paired peer's `/api/mesh/ping` (4 s timeout) and flips the peer's `online` flag on transition. Coming back online refreshes `last_seen`. Any change emits `mesh-state-changed` so the UI re-renders without polling. The scheduler picks the lowest-tier reachable address per peer (Tailscale > LAN > public) so it doesn't burn bandwidth on the wrong path.

## Mesh tab and service registry

The Mesh tab is the UI surface for the local service registry and peer services. It replaces the old standalone Cron Panel and absorbs the older `[Crons]` sidebar route.

### Sub-tabs

- **Map** — radial peer terminals + connection lines. Line width / opacity tracks recent push volume between peers; bidirectional pulse animates on each new event. Pan/zoom (mouse drag + wheel + reset/+/− buttons), with a "fit to view" button that runs automatically on first paint so every peer is visible cold-start.
- **Activity** — `useActivityLog` polls `/api/activity` every 5 s with `source` and `level` chip filters. Mesh-relayed pushes carry `source_peer` so cross-peer traffic is distinguishable from local pushes.
- **Settings** — formerly `MeshSection.tsx`, ported into the Mesh tab as `MeshSettingsTab.tsx`. Mesh enable/disable, broadcast toggle, paired-peer access levels, instance name.

Below the canvas, two tables share the surface: **Crons** (`kind = cron`, columns: name / next-run / status) and **Services** (`kind = daemon`, columns: name / health / mode / push). A bold accent divider separates them — same surface, different lifecycles. Each peer card on the canvas shows up to 5 of its services with a `+N` overflow badge; cards are sized to a fixed 240-px width so the layout doesn't jitter as services come and go.

### Service entity

Services have `kind = cron | daemon`. Cron jobs registered via `/api/crons` are projected as virtual `cron`-kind services — no data migration, they share the same cron store. Daemons are created via `codeterm service add` (see `--help`) or `POST /api/services` / `services.create` JSON-RPC.

SQLite tables (introduced in v1.2.0):

- `services` — id, name, kind, description, url/host/port/process_label/script_path, health_mode/health_path/health_interval/health_ttl, field_mapping, push_enabled, registered_at, updated_at, enabled.
- `service_health` — `(service_id, probed_at)` PK, status, response (jsonb), latency_ms. Latest snapshot is joined into `services.list/get` via a `ROW_NUMBER()` window — only the most recent row reaches the API.
- `activity` — id, occurred_at, source (push/health/peer/cron), source_id, target, level, message, meta (jsonb). Bounded — see Activity log.
- `schema_version` — single-row guard for future migrations.

### Discovery (peer services)

Peer services are **not gossiped** across the mesh. When a peer card is expanded on the canvas, the UI calls a Tauri command (`mesh_peer_services_list`) which relays `GET /api/services` to that peer in real time via `/api/mesh/relay`. Results are never persisted locally. If the cached peer pubkey is stale, the relay path auto-refreshes the peer cache before erroring.

### Cold-start cache (local services only)

`useMeshData` writes the local `/api/services` + `/api/mesh/peers` snapshots to `localStorage` with a 24-hour TTL. On reload the cached payload renders immediately while the live fetch runs in the background — no "loading…" flash on cold start. The refresh button uses an `animate-spin` chevron instead of replacing the table with text.

### Push tokens

Each daemon service has a per-service push token stored at `<data_dir>/services/<service_id>/token` (mode `0600` on Unix). The token is 64-hex-char random and returned **once** (at create, or via `services.regenerate_token` / `service token --rotate`). Daemons send `Authorization: Bearer <token>` on `POST /api/panes/<pane_id>/send` to write to a pane without the global access token. The auth middleware validates push tokens (`services::push::validate_push`) **before** falling through to the mesh signed-token check, so a daemon Bearer never accidentally lands in the peer code path. Token comparison is constant-time. Rotating a token also auto-flips `push_enabled = true` (you wouldn't rotate if you didn't intend to push).

### Activity log

Every service push, health transition, peer event, and cron lifecycle event is written to the `activity` table. Bounded: **10 000 rows / 30 days**, evicted on every 100th insert. Reachable via `GET /api/activity?source=push|health|peer|cron&since=<ms>&limit=N` or `activity.list` JSON-RPC. Mesh-relayed pushes carry the originating peer's name in `source_peer`. The Activity sub-tab polls every 5 s.

### Health probes

Four modes, configured per service:

| Mode | Trigger | Healthy condition |
|------|---------|-------------------|
| `http` | GET `url` + optional `health_path` (default `/healthz`) | 2xx response |
| `tcp` | TCP connect to `host:port` | connection succeeds |
| `process` | launchctl / systemd `process_label` | unit is running |
| `script` | run `script_path` | exit code 0 |

Probe interval is per-service (`health_interval`, default 30 s). The `services::health::scheduler` ticks every 30 s globally, persists each snapshot into `service_health`, and applies a **3-tick debounce** before firing a transition: a status only "promotes" to confirmed (and emits `service-health-changed` + an `activity` row) after 3 consecutive identical observations. Single-tick blips never page the UI.

### Live canvas

The Mesh canvas renders peers as terminal-window-shaped nodes. Health and push events update the canvas in real time via Tauri events:

- `service-health-changed` — `{service_id, status}`. Fired only on confirmed transitions (3-tick debounced).
- `service-pushed` — `{service_id, pane_id, body_preview}`. Fired by `POST /api/panes/{id}/send` on token-authenticated writes **and** by the mesh-relay path when a paired peer relays a `/send` to us, so cross-peer push traffic surfaces in the canvas + activity log identically to local pushes.

The ServicesTable lists local services with health badges. ServiceDrawer shows full config + recent activity for a selected service, with cron-aware actions (trigger / enable-disable / edit-schedule / history) and daemon-aware actions (probe / rotate token / enable-disable). The drawer dismisses on ESC, backdrop click, or its visible close button. Self-peer drawers hide pair/access-level UI (pointless for the local node).

`AddServiceWizard` handles manual daemon registration. Smart-Add inspects running processes (`launchctl list` / `systemctl --user list-units` / `ps`) and offers heuristic field-mapping suggestions to pre-fill name / health-mode / process-label / port — the user reviews and edits before saving.

Full architecture spec: `docs/superpowers/specs/2026-05-09-mesh-tab-design.md`.

## REST surface for peer management

`/api/mesh/peers/*` ships both body-style and path-style endpoints. The path-style ones are aliases — they wrap the same handler with `request_id` / `instance_id` lifted from the URL — and exist so mesh tab UI components and curl scripts don't have to hand-build the body shape.

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/mesh/peers/add-manual` | Inject a `Manual` discovered peer by Tailscale IP + port (skips mDNS / UDP scan). Body `{address, port}`. Returns `{ok, instance_id}`. Used by the +Peer popup auto-discovery. |
| `POST` | `/api/mesh/peers/approve/{id}` | Path-style alias for `POST /api/mesh/approve {request_id}`. |
| `POST` | `/api/mesh/peers/reject/{id}` | Path-style alias for `POST /api/mesh/reject {request_id}`. |
| `POST` | `/api/mesh/peers/disconnect/{id}` | Path-style alias for `POST /api/mesh/disconnect {instance_id}`. |
| `DELETE` | `/api/mesh/peers/{id}` | Remove peer (used internally by disconnect broadcast). |

All four respond `503` when `mesh.enabled=false` (see top of doc).

## Architecture

```
mesh/
  mod.rs            — MeshManager (facade) + broadcast_sync + introductions
  identity.rs       — signing, authentication
  crown.rs          — Ed25519 crypto primitives (sign/verify requests, keypair generation; legacy name, no crown concept)
  types.rs          — all types
  peers.rs          — disk storage
  discovery.rs      — mDNS + UDP + Tailscale
  pairing.rs        — pairing protocol (X25519 key exchange)
  transport.rs      — HTTP client for peer communication
  ping_scheduler.rs — peer-ping (20 s interval, online/offline transitions)

services/
  store/            — sqlite-backed services + service_health + schema_version
  health/           — http / tcp / process / script probes + scheduler (30 s tick, 3-tick debounce)
  activity.rs       — bounded activity log writer/reader (10k rows / 30 days)
  tokens.rs         — per-service push tokens (file-backed, 0600, constant-time)
  push.rs           — Bearer token middleware for /api/panes/*/send
  smart_add.rs      — heuristic field-mapping suggestions for AddServiceWizard
```

<!-- revision: 5 -->
