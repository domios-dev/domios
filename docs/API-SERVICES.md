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

**Health modes:** `http` (HTTP GET to `url` + optional `health_path`, default `/healthz`), `tcp` (TCP connect to `host:port`), `process` (launchctl/systemd unit via `process_label`), `script` (exit 0 = healthy, `script_path`). The probe scheduler ticks every 30 s and requires **3 consecutive identical observations** before firing a transition event — single-tick blips never page the UI.

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

Every service push, health transition, peer event, and cron lifecycle event is recorded in the bounded activity log (sqlite, capped at 10 000 rows / 30 days, evicted every 100 inserts).

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/activity` | `?source=push\|health\|peer\|cron&since=<ms>&limit=<N>` | List recent activity events newest-first. `source` filters by event source; `since` filters to `occurred_at > since` (unix ms); `limit` defaults to 500. |

Each event: `{id, occurred_at, source, source_id?, target?, source_peer?, level: info\|warn\|error, message, meta}`.

## Related documents

Bridge, setup-hook, and cron endpoints: `codeterm docs API-INTEGRATIONS`. Usage and log observability: `codeterm docs API-STATE`. Memory endpoints: `codeterm docs API-MEMORY`.

<!-- revision: 2 -->
