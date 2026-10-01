# Mesh services and management

Mesh service registry, health probes, peer-management REST, and architecture.

## Mesh tab and service registry

The Mesh tab is the UI surface for the local service registry and peer services.

### Sub-tabs

- **Map** — peer terminals with connection lines that reflect recent push traffic.
- **Activity** — the activity log with `source` and `level` filters. Mesh-relayed pushes carry `source_peer` so cross-peer traffic is distinguishable from local pushes.
- **Settings** — mesh enable/disable, broadcast toggle, paired-peer access levels, instance name.

Below the canvas, two tables share the surface: **Crons** (`kind = cron`, columns: name / next-run / status) and **Services** (`kind = daemon`, columns: name / health / mode / push). Peer cards on the canvas list their services.

### Service entity

Services have `kind = cron | daemon`. Cron jobs registered via `/api/crons` are projected as virtual `cron`-kind services — no data migration, they share the same cron store. Daemons are created via `codeterm service add` (see `--help`) or `POST /api/services` / `services.create` JSON-RPC.

Service listings include the latest health; activity is bounded to 10,000 rows / 30 days.

### Discovery (peer services)

Peer services are **not gossiped** across the mesh. Expanding a peer card fetches that peer's target-authoritative `GET /api/mesh/inventory` contract. Each resource carries its machine identity, kind, provenance, and per-operation capability map. Results are cached only as last-known peer rows; cached rows remain visible but cannot mutate. The web mirror exposes the same projection through `/api/mesh/peers/{id}/services`.

The capability map uses stable operation keys (`get`, `history`,
`update_schedule`, `delete`, `enable`, `disable`, `trigger`, `cancel`) and
unavailable reasons (`not_authorized`, `stale`, `offline`,
`partial_peer_failure`, `unsupported_resource_kind`, `target_unavailable`).
Cron schedule updates use `PUT /api/mesh/peers/{id}/crons/{cron_id}` and cron
removal uses `DELETE` on the same path; both forward the target response. The
target authorizes them through `/api/crons/{id}` and the caller's access
policy. Existing cron get/history/enable/disable/trigger/cancel routes remain
unchanged, as does daemon registry deletion through `/api/services/{id}`.

The Mesh tab shows peers, services and crons; cached/offline resources remain inspectable but cannot mutate.

### Push tokens

Each daemon service has a per-service push token stored at `<data_dir>/services/<service_id>/token` (mode `0600` on Unix). The token is 64-hex-char random and returned **once** (at create, or via `services.regenerate_token` / `service token --rotate`). Daemons send `Authorization: Bearer <token>` on `POST /api/tabs/<tab_id>/send` to write to a Tab without the global access token. Token comparison is constant-time. Rotating a token also auto-flips `push_enabled = true` (you wouldn't rotate if you didn't intend to push).

### Activity log

Every service push, health transition, peer event, and cron lifecycle event is written to the `activity` table. Bounded: **10 000 rows / 30 days**. Reachable via `GET /api/activity?source=push|health|peer|cron&since=<ms>&limit=N` or `activity.list` JSON-RPC. Mesh-relayed pushes carry the originating peer's name in `source_peer`.

### Health probes

Four modes, configured per service:

| Mode | Trigger | Healthy condition |
|------|---------|-------------------|
| `http` | GET `url` + optional `health_path` (default `/healthz`) | 2xx response |
| `tcp` | TCP connect to `host:port` | connection succeeds |
| `process` | launchctl / systemd `process_label` | unit is running |
| `script` | run `script_path` | exit code 0 |

Probe interval is per-service (`health_interval`, default 30 s). Health transitions require three consecutive matching observations; single blips never surface.

### Live canvas

Health and push events update the Mesh canvas in real time:

- `service-health-changed` — `{service_id, status}`. Fired only on confirmed transitions.
- `service-pushed` — `{service_id, tab_id, body_preview}`. Fired by `POST /api/tabs/{id}/send` on token-authenticated writes **and** by the mesh-relay path when a paired peer relays a `/send` to us, so cross-peer push traffic surfaces in the canvas + activity log identically to local pushes.

The services table lists local services with health badges. The service drawer shows full config and recent activity for a selected resource. Its actions come only from the target-authoritative capability map: schedules may expose trigger, pause/resume, running-execution cancellation, schedule editing, history, and removal; protected schedules expose no removal. Daemons expose inspection and registry removal, never process stop/start, plus the existing local push-token rotation. Stale, offline, partial-failure, denied, and protected resources remain inspectable with disabled controls and their exact capability reason. Remote or destructive confirmations name the resource and target machine; the drawer waits for the returned outcome, announces success or target failure accessibly, and refreshes only after completion. The drawer dismisses on ESC, backdrop click, or its visible close button. Self-peer drawers hide pair/access-level UI (pointless for the local node).

The add-service wizard handles manual daemon registration. Smart-Add inspects running processes and offers heuristic field-mapping suggestions to pre-fill name / health-mode / process-label / port — the user reviews and edits before saving.

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

<!-- revision: 6 -->
