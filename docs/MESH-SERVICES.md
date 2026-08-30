# Mesh services and management

Mesh service registry, health probes, peer-management REST, and architecture.

## Mesh tab and service registry

The Mesh tab is the UI surface for the local service registry and peer services. It replaces the old standalone Cron Tabl and absorbs the older `[Crons]` sidebar route.

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

Peer services are **not gossiped** across the mesh. When a peer card is expanded on the canvas, the UI calls a Tauri command (`mesh_peer_services_list`) which fetches that peer's target-authoritative `GET /api/mesh/inventory` contract. Each resource carries its machine identity, kind, provenance, and per-operation capability map. Results are cached only as last-known peer rows; cached rows remain visible but cannot mutate. The web mirror exposes the same projection through `/api/mesh/peers/{id}/services`.

The capability map uses stable operation keys (`get`, `history`,
`update_schedule`, `delete`, `enable`, `disable`, `trigger`, `cancel`) and
unavailable reasons (`not_authorized`, `stale`, `offline`,
`partial_peer_failure`, `unsupported_resource_kind`, `target_unavailable`).
Cron schedule updates use `PUT /api/mesh/peers/{id}/crons/{cron_id}` and cron
removal uses `DELETE` on the same path; both forward the target response. The
target authorizes them through `/api/crons/{id}` and the caller's access
policy. Existing cron get/history/enable/disable/trigger/cancel routes remain
unchanged, as does daemon registry deletion through `/api/services/{id}`.

### Cold-start cache (local services only)

`useMeshData` writes the local `/api/mesh/inventory` projection + `/api/mesh/peers` snapshots to `localStorage` with a 24-hour TTL. On reload the cached payload renders immediately while the live fetch runs in the background — no "loading…" flash on cold start. The refresh button uses an `animate-spin` chevron instead of replacing the table with text.

### Push tokens

Each daemon service has a per-service push token stored at `<data_dir>/services/<service_id>/token` (mode `0600` on Unix). The token is 64-hex-char random and returned **once** (at create, or via `services.regenerate_token` / `service token --rotate`). Daemons send `Authorization: Bearer <token>` on `POST /api/tabs/<tab_id>/send` to write to a Tab without the global access token. The auth middleware validates push tokens (`services::push::validate_push`) **before** falling through to the mesh signed-token check, so a daemon Bearer never accidentally lands in the peer code path. Token comparison is constant-time. Rotating a token also auto-flips `push_enabled = true` (you wouldn't rotate if you didn't intend to push).

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
- `service-pushed` — `{service_id, tab_id, body_preview}`. Fired by `POST /api/tabs/{id}/send` on token-authenticated writes **and** by the mesh-relay path when a paired peer relays a `/send` to us, so cross-peer push traffic surfaces in the canvas + activity log identically to local pushes.

The ServicesTable lists local services with health badges. ServiceDrawer shows full config and recent activity for a selected resource. Its actions come only from the target-authoritative capability map: schedules may expose trigger, pause/resume, running-execution cancellation, schedule editing, history, and removal; protected schedules expose no removal. Daemons expose inspection and registry removal, never process stop/start, plus the existing local push-token rotation. Stale, offline, partial-failure, denied, and protected resources remain inspectable with disabled controls and their exact capability reason. Remote or destructive confirmations name the resource and target machine; the drawer waits for the returned outcome, announces success or target failure accessibly, and refreshes only after completion. The drawer dismisses on ESC, backdrop click, or its visible close button. Self-peer drawers hide pair/access-level UI (pointless for the local node).

`AddServiceWizard` handles manual daemon registration. Smart-Add inspects running processes (`launchctl list` / `systemctl --user list-units` / `ps`) and offers heuristic field-mapping suggestions to pre-fill name / health-mode / process-label / port — the user reviews and edits before saving.

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
  push.rs           — Bearer token middleware for /api/tabs/*/send
  smart_add.rs      — heuristic field-mapping suggestions for AddServiceWizard
```

<!-- revision: 4 -->
