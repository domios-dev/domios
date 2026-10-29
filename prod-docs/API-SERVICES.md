# Service registry API

Service registration, health, push-token, and activity endpoints.

#### Services

Daemon-kind services are registered here; cron jobs are projected as virtual `cron`-kind services but managed via `/api/crons`. Only `daemon` can be created through this API.

`POST /api/tabs/{id}/send` accepts `Authorization: Bearer <service_token>` (the per-service push token returned at create or rotation time) in addition to the normal access token. This lets a registered daemon write to a tab without holding the global access token.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/services` | `?kind=cron\|daemon&enabled=true\|false` | List all services (cron projection + registered daemons), optionally filtered |
| POST | `/api/services` | `{name, kind:"daemon", health_mode, health_interval?, push_enabled?, url?, host?, port?, process_label?, script_path?, health_path?, health_json_field?, health_ok_values?, health_degraded_values?, description?, field_mapping?}` | Register a new daemon-kind service. `kind` other than `daemon` returns 400 (cron jobs go through `/api/crons`). Returns `{service, token}` — save the token, it is shown only once. `health_interval` defaults to 30 s. `push_enabled` defaults to false; set true to allow Bearer-token writes to tabs. |
| GET | `/api/services/{id}` | — | Get service by ID. Body includes the latest health snapshot (joined from `service_health` via `ROW_NUMBER` window — most recent row only). |
| PUT | `/api/services/{id}` | `{name?, description?, url?, host?, port?, process_label?, script_path?, health_mode?, health_path?, health_json_field?, health_ok_values?, health_degraded_values?, health_interval?, field_mapping?, push_enabled?, enabled?}` | Update service fields. Each field is a sparse patch (only provided keys are written). |
| DELETE | `/api/services/{id}` | — | Delete service + its push token + activity |
| POST | `/api/services/{id}/regenerate-token` | — | Rotate push token. Returns `{token, push_enabled: true}` — old token is immediately invalidated, and `push_enabled` is auto-set to true (rotating implies you intend to push). |

**Health modes:** `http` (HTTP GET to `url` + optional `health_path`, default `/healthz`), `tcp` (TCP connect to `host:port`), `process` (launchctl/systemd unit via `process_label`), `script` (exit 0 = healthy, `script_path`). A transition requires **3 consecutive identical observations**; single blips never surface.

**HTTP health response mapper** (per-service, all optional):
- `health_json_field` — which JSON key to inspect (default: `"status"`). Ignored when body is empty or non-JSON (2xx alone → Healthy).
- `health_ok_values` — string array of values that mean Healthy, case-insensitive (default: `["ok","healthy","up"]`). Example: `["UP"]` for SonarQube, `["running"]` for custom services.
- `health_degraded_values` — values that mean Degraded (default: `["degraded","starting"]`). Anything else → Down.
- If the configured field is absent from the JSON body, the probe falls back to "2xx = Healthy".
- CLI: `--health-json-field status --health-ok-values UP,ok --health-degraded-values STARTING`

**Push-token middleware.** `POST /api/tabs/{id}/send` accepts `Authorization: Bearer <service_token>` (the per-service push token returned at create or rotation time) in addition to the normal API token. Token files live at `<data_dir>/services/<service_id>/token` (mode `0600` on Unix), are 64-hex-char random, and are matched constant-time. Every accepted push is mirrored into the activity log as a `push` event.

**Tauri events:**
- `service-health-changed` — `{service_id, status}` — fired by the health probe scheduler on confirmed status transitions only (3-tick debounced).
- `service-pushed` — `{service_id, tab_id, body_preview}` — fired when a daemon writes to a tab via its push token. Also fired by the mesh-relay path when a paired peer relays a `/send` to us, so the activity log reflects cross-peer push traffic the same way as local pushes.

#### Activity Log

Every service push, health transition, peer event, and cron lifecycle event is recorded in the bounded activity log (10 000 rows / 30 days).

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/activity` | `?source=push\|health\|peer\|cron&since=<ms>&limit=<N>` | List recent activity events newest-first. `source` filters by event source; `since` filters to `occurred_at > since` (unix ms); `limit` defaults to 500. |

Each event: `{id, occurred_at, source, source_id?, target?, source_peer?, level: info\|warn\|error, message, meta}`.

## Related documents

Bridge, setup-hook, and cron endpoints: `domios docs API-INTEGRATIONS`. Usage and log observability: `domios docs API-STATE`. Memory endpoints: `domios docs API-MEMORY`.


## Additional REST routes

Peer actions retain Mesh access checks; plugin operations retain their granted capabilities.

| Method | Path | Operation |
|--------|------|-----------|
| DELETE | `/api/mesh/peers/{id}/crons/{cron_id}` | Delete a peer cron job. |
| DELETE | `/api/mesh/peers/{id}/services/{service_id}` | Delete a peer service. |
| DELETE | `/api/plugins/{id}/receiver-target` | Clear a plugin receiver destination. |
| GET | `/api/mesh/inventory` | Read Mesh peer inventory. |
| GET | `/api/mesh/peers/{id}/crons/{cron_id}` | Read a peer cron job. |
| GET | `/api/mesh/peers/{id}/crons/{cron_id}/history` | Read a peer cron run history. |
| GET | `/api/plugins/snapshot` | Read installed plugin snapshot. |
| GET | `/api/plugins/tab-menu` | Read plugin-provided tab menu entries. |
| GET | `/api/plugins/{id}/receiver-target` | Read a plugin receiver destination. |
| GET | `/api/plugins/{id}/ui/{*path}` | Read a sandboxed plugin UI asset. |
| PATCH | `/api/services/{id}` | Patch service fields. |
| POST | `/api/mesh/enabled` | Enable or disable Mesh. |
| POST | `/api/mesh/exit` | Leave the Mesh. |
| POST | `/api/mesh/peers/{id}/access` | Set peer access permissions. |
| POST | `/api/mesh/peers/{id}/crons/{cron_id}/{action}` | Apply a peer cron action. |
| POST | `/api/mesh/unrevoke` | Lift a peer identity revocation. |
| POST | `/api/mesh/worker-dispatch` | Admit a signed remote Worker dispatch. |
| POST | `/api/mesh/worker-dispatch-status` | Read a signed remote dispatch status. |
| POST | `/api/mesh/worker-lane-bundle` | Transfer a signed remote lane Git bundle. |
| POST | `/api/mesh/worker-observation` | Submit an authenticated remote Worker observation. |
| POST | `/api/plugins/action` | Invoke a plugin action. |
| POST | `/api/plugins/bubble` | Read plugin bubble content. |
| POST | `/api/plugins/chatbackend/open` | Open a plugin chat-backend tab. |
| POST | `/api/plugins/chatbackend/session-info` | Read a plugin chat-backend session. |
| POST | `/api/plugins/chatbackend/switch-model` | Switch a plugin chat-backend model. |
| POST | `/api/plugins/glance` | Read a plugin glance. |
| POST | `/api/plugins/tab-menu/action` | Invoke a plugin tab-menu action. |
| POST | `/api/plugins/remote-task-call` | Local owner only: `{tabId,pluginId,sessionId,method,args?}`, only live remote `read/refresh/submit`; claims must match live backing. |
| POST | `/api/plugins/recording-suggestion` | Local owner only: `{pluginId,args:{inputs,microphones}}`; declared/granted recording plugin, fixed selection callback. |
| POST | `/api/plugins/view-call` | Local owner only: `{pluginId,viewFrame,method,args?}`. Host-private document lease required; callback gets resolved owner context. See SURFACES. |
| POST | `/api/plugins/view-frame/revoke` | Local host loader only: `{pluginId,registration}` retires context and all media handles; strict 1 KiB body. |
| POST | `/api/plugins/view-frame/register` | Local host loader after document proof: `{pluginId,registration,scope}` remints a fresh lease/context. Original owner/workspace/Team/provider registration must still match; strict 16 KiB body. |
| POST | `/api/plugins/view-host-action` | Check or perform a plugin view's declared user-gesture action; refusals are typed results. |
| POST | `/api/plugins/view-host-action/download` | Stream a plugin view's own media file as a download. |
| POST | `/api/services/smart-add-inspect` | Inspect a candidate service before registration. |
| PUT | `/api/mesh/peers/{id}/crons/{cron_id}` | Update a peer cron job. |
| PUT | `/api/plugins/{id}/receiver-target` | Set a plugin receiver destination. |

<!-- revision: 9 -->
