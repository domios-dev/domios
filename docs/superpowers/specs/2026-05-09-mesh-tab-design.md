# Mesh tab — unified runtime panel for crons, daemons, and mesh-wide service visibility

**Status:** Design / pre-implementation
**Date:** 2026-05-09
**Owner:** Andrey
**Brainstorming session:** Mac-Air, with rollacode-bot mesh relay
**Driving voice request:** Andrey to rollacode-bot, 7m44s

## 1. Problem & vision

### 1.1 Current state

CodeTerm currently surfaces runtime state across two disconnected places:

- **Cron panel** (`src/components/CronPanel/CronPanel.tsx`) — own settings form, history, log viewer for scheduled tasks. Tied to `cron::store` backend.
- **Mesh section in Settings** (`src/components/Settings/MeshSection.tsx`, 460 lines) — peer pairing/discovery/disconnect, identity, broadcast, manual-add, exit-mesh.

There is **no surface for long-running daemons** (HTTP services like `voice_transcribe`, process-supervised tools like `mail-watcher`, faster-whisper on BLACKSTAR, briefing-daemon on rollacode-bot, etc.). They live in the OS (launchd / systemd / schtasks) and CodeTerm is unaware of their state.

There is also no **mesh-wide service visibility** — a user on Mac-Air cannot see at a glance what services are running on rollacode-bot or BLACKSTAR.

### 1.2 What we are building

One **Mesh tab** rendered next to the existing Memory tab. It contains:

1. **L1 — Local service registry**: cron jobs (existing) + long-running daemons (new), unified under one Service entity with `kind=cron|daemon`. CodeTerm is a *monitor*, not a lifecycle manager — it does not start/stop/install daemons. Daemons live independently in the OS.
2. **L2 — Mesh-wide service visibility**: pull-on-demand discovery of services on paired peers via `GET /api/services`. Per-peer ownership; no replication.
3. **L3 — Service → pane push API**: long-running daemons can write to a local pane via `POST /api/panes/<pane_id>/send` authenticated by per-service Bearer token. Local-only push (cross-machine push stays in existing `mesh send-text` / `agent report`).

Visually: peers rendered as **terminal-window mockups** (real macOS-style traffic lights, name, services as inline badges), connected by **animated lines** that pulse during active push and stay brighter for a few minutes after activity. Below the canvas, a **flat table** lists all services across all peers with filter chips. Click a peer → drawer slides in from the right with full peer-management controls (preserves all 460 lines of MeshSection.tsx functionality).

Settings is reorganized in parallel: Memory and Mesh sections move *into* their respective tabs as sub-tabs (`settings`); Settings becomes slim with toggles to enable/disable the tabs and `Go to X →` deep-links. Librarian moves into General settings, default-on.

### 1.3 End-goal context (out-of-scope for this spec, but informs design)

Andrey's longer-term framing: "плавно уходим от OpenClaw" — when the Mesh tab is mature (registry + push API + activity log + good UX), OpenClaw services (briefing portal, mail-watcher, etc.) will migrate over manually. CodeTerm becomes the unified runtime hub. **This spec does not include OpenClaw migration scripts** — those are separate feature work.

## 2. Scope

### 2.1 In scope

- Service entity (one `services::store` SQLite table, kind discriminator)
- Cron rows persisted in cron-store (existing) but mirrored into `services` view via JOIN or read-side aggregation
- Daemon rows: HTTP / TCP / process / script health-check modes (all 4)
- Per-peer pull discovery via new REST endpoints (`GET /api/services`, `GET /api/activity`)
- Mesh tab UI with map / activity sub-tabs (no separate "list" tab — flat table sits permanently below the canvas)
- ServiceDrawer (slides in on peer or service click)
- MeshSettings sub-tab (consumes all 460 lines from MeshSection.tsx)
- Per-service Bearer token in `~/.codeterm/services/<id>/token` (file 0600, no keychain)
- Activity log with bounded size (10k events / 30 days, whichever first), 4 source types
- "+ Service" wizard with Smart-Add (AI agent inspects daemon, recommends mappings)
- "+ Peer" reuses existing pairing flow
- Settings panel slim-down: Memory + Mesh sections move into respective tabs; Librarian default-on in General
- Cron panel removal (CronPanel.tsx deleted; cron details accessed via Mesh drawer)

### 2.2 Out of scope

- OpenClaw service migration (manual via agents post-ship)
- Bundled daemon shipping / `service install` command (CodeTerm is monitor only)
- Cross-mesh push routing (L3 is local-only; cross-machine work remains `codeterm send-text --pane X --mesh PEER` and `codeterm agent report`)
- Mesh-wide replicated activity log / gossip / CRDT
- WebSocket subscriptions for live updates (deferred — pull-on-tab-open is the v1 model; deltas via WS are a future evolution)
- Auto-discovery of daemons (no scanning of `~/Library/LaunchAgents/*` etc. — explicit registration only)

## 3. Architecture overview

```
┌─────────────────────────────────────────────────────────────┐
│  FRONTEND: src/components/MeshPanel/                         │
│   ├ MeshPanel.tsx (root, sub-tab router: map / activity)     │
│   ├ MeshCanvas.tsx (terminals-as-terminals + lines)          │
│   ├ ServicesTable.tsx (flat list, all services)              │
│   ├ ServiceDrawer.tsx (peer + service details)               │
│   ├ MeshSettingsTab.tsx (identity / network / manual-add)    │
│   ├ AddServiceWizard.tsx (manual + Smart-Add via agent)      │
│   └ useMeshData.ts (hook: fetch + merge from peers)          │
└─────────────────────────────────────────────────────────────┘
              │ Tauri invoke + REST (same origin)
              ▼
┌─────────────────────────────────────────────────────────────┐
│  BACKEND: src-tauri/src/services/                            │
│   ├ store.rs        — Service registry CRUD + persistence   │
│   ├ health.rs       — health probe (4 modes) + cache (TTL)  │
│   ├ push.rs         — Bearer-token validator for /panes/send │
│   ├ activity.rs     — bounded log writer + reader            │
│   ├ smart_add.rs    — AI-agent-assisted field mapping        │
│   └ mod.rs          — facade                                 │
│                                                               │
│  Cron stays in src-tauri/src/cron/ — Service::list aggregates│
│  cron-store rows + service-store rows on read.               │
│                                                               │
│  REST: src-tauri/src/api/services_routes.rs                  │
│   GET    /api/services          — list local + (optional)    │
│                                    paired-peer aggregated    │
│   POST   /api/services          — create new daemon          │
│   GET    /api/services/:id      — single service detail      │
│   PUT    /api/services/:id      — update                     │
│   DELETE /api/services/:id      — unregister (no stop)       │
│   POST   /api/services/:id/probe — manual health probe       │
│   GET    /api/activity          — bounded activity log       │
│                                                               │
│  Existing /api/panes/<id>/send gets:                         │
│    - Bearer token middleware (validates against services tbl)│
│    - source_service_id stamped on activity log on success    │
└─────────────────────────────────────────────────────────────┘
              │ pull-on-tab-open
              ▼
┌─────────────────────────────────────────────────────────────┐
│  PEERS via mesh routing (uses existing mesh::peers state):   │
│   GET /api/services    — peer's local registry view          │
│   GET /api/activity    — peer's bounded activity log         │
│   No replication, no gossip. Caller merges by timestamp.    │
└─────────────────────────────────────────────────────────────┘
```

### 3.1 Layer decomposition

- **L1 (registry)**: new `services::store` (SQLite at `~/.codeterm/services.db`). Cron-store stays untouched — `Service::list_local` aggregates rows from both stores into a unified view at read time. No data migration; cron remains canonically in `cron-store`, daemons go to `services-store`. The unified Service struct discriminates via `kind` field.
- **L2 (mesh visibility)**: zero new mesh primitives. Reuses `mesh::peers` paired-peer list to iterate `GET /api/services` calls in parallel on tab open, with per-peer timeout + offline marker.
- **L3 (push API)**: `POST /api/panes/<pane_id>/send` is the canonical pane-write endpoint that already exists for cron/agent use. Adds a Bearer-token middleware; if `Authorization: Bearer <tk>` is present, validate against `services` table. If valid → stamp `source_service_id` on the resulting `activity` row. If absent or invalid → fall through to existing API token auth (preserves agent-report path).

### 3.2 Reuse, don't rebuild

- ✅ Memory's Sigma + physics worker for canvas rendering (already proven)
- ✅ MeshSection.tsx 460 lines of peer logic — move wholesale into `MeshSettingsTab.tsx`
- ✅ `cron::store`, `cron::scheduler` — no internal changes
- ✅ `POST /api/panes/<id>/send` — extend with token middleware
- ✅ `bridge::sanitize` — apply to push-message text before pane write

### 3.4 Background probe scheduler

A single `tokio::spawn` long-running task in `services::health` walks the `services` table every `min(service.health_interval)` seconds (default 30s). For each `enabled` service whose last probe is older than its `health_interval`, it:

1. Runs the mode-appropriate probe (HTTP GET / TCP connect / launchctl/systemctl query / shell script)
2. Inserts a row into `service_health` with status + latency + raw response
3. If status transition (healthy ↔ degraded ↔ down), inserts an `activity` row of source=`health`
4. Emits a Tauri event `service-health-changed` with `{service_id, status}` so the frontend updates without polling

The voice_transcribe-style 60-second TTL cache (`bridge::voice_transcribe::cached_health`) wraps the probe so a frontend `services_probe` invocation within the TTL returns the cached value instead of hammering the daemon. Manual probe via `POST /api/services/:id/probe` bypasses the cache.

### 3.3 Settings reorganization (parallel concern)

Settings panel slims down:

- **General** keeps theme, keybindings; gains Librarian toggle (default-on)
- **Memory section** → moves into Memory tab as a `settings` sub-tab
- **Mesh section** → moves into Mesh tab as the `settings` sub-tab
- **Bridge / Agents sections** → stay in Settings
- **New in Settings**: two toggles "Enable Memory tab" / "Enable Mesh tab" with `Go to Memory →` / `Go to Mesh →` deep-link buttons that switch the active tab

This is a separate sub-feature implementable independently — it is captured here because it changes the discoverability story for Memory and Mesh together.

## 4. Data model

### 4.1 `services` table (SQLite)

```sql
CREATE TABLE services (
  id              TEXT PRIMARY KEY,            -- "svc_<slug>" or uuid
  name            TEXT NOT NULL,
  kind            TEXT NOT NULL,               -- 'daemon' (cron rows are virtual; see 4.3)
  description     TEXT,
  -- Connectivity
  url             TEXT,                        -- for kind=daemon HTTP: "http://127.0.0.1:7891"
  host            TEXT,                        -- for tcp mode: "127.0.0.1"
  port            INTEGER,                     -- for tcp mode
  process_label   TEXT,                        -- for process mode: "com.codeterm.voice"
  script_path     TEXT,                        -- for script mode: "~/.codeterm/services/foo/check.sh"
  -- Health
  health_mode     TEXT NOT NULL,               -- 'http' | 'tcp' | 'process' | 'script'
  health_path     TEXT,                        -- for http: "/healthz" (default)
  health_interval INTEGER NOT NULL DEFAULT 30, -- seconds between probes
  health_ttl      INTEGER NOT NULL DEFAULT 60, -- cache TTL seconds
  -- UI mapping (smart-add result)
  field_mapping   TEXT,                        -- JSON: {ui_field: jsonpath_in_response}
  -- Auth (L3 push)
  push_enabled    BOOLEAN NOT NULL DEFAULT 0,
  -- Timestamps
  registered_at   INTEGER NOT NULL,            -- epoch ms
  updated_at      INTEGER NOT NULL,            -- epoch ms (bumped on edit OR field_mapping change)
  -- Lifecycle
  enabled         BOOLEAN NOT NULL DEFAULT 1
);

CREATE INDEX idx_services_kind ON services(kind);
CREATE INDEX idx_services_enabled ON services(enabled);
```

Token is stored separately as `~/.codeterm/services/<id>/token` (32 random bytes hex, file mode 0600). NEVER in the DB to keep DB exportable / shareable without leaking secrets.

### 4.2 `service_health` table (separate, ephemeral / append-only)

```sql
CREATE TABLE service_health (
  service_id      TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
  probed_at       INTEGER NOT NULL,            -- epoch ms
  status          TEXT NOT NULL,               -- 'healthy' | 'degraded' | 'down'
  response        TEXT,                        -- raw JSON if http, empty otherwise
  latency_ms      INTEGER,
  PRIMARY KEY (service_id, probed_at)
);

CREATE INDEX idx_health_service_time ON service_health(service_id, probed_at DESC);
```

Eviction: keep last 1000 probes per service OR last 7 days, whichever first. Trim on insert if either threshold exceeded.

### 4.3 Cron as virtual Service

Cron rows live canonically in `cron::store` (no migration). `Service::list_local()` returns:

```rust
fn list_local() -> Vec<Service> {
    let mut out = services_store.list().into_iter().map(Service::from).collect();
    out.extend(cron_store.list().into_iter().map(Service::from_cron));
    out
}
```

`Service::from_cron` projects a `CronJob` into a `Service` with:
- `kind = "cron"`
- `schedule`, `next_run`, `last_run`, `consecutive_failures` populated from cron-specific fields
- All write ops on a cron-kind Service are routed to `cron::store` (the drawer's "Edit cron" UI calls existing `cron.update` RPC)

This avoids data migration risk while presenting a unified Service to the UI.

### 4.4 `activity` table (SQLite, bounded)

```sql
CREATE TABLE activity (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  occurred_at     INTEGER NOT NULL,            -- epoch ms
  source          TEXT NOT NULL,               -- 'push' | 'health' | 'peer' | 'cron'
  source_id       TEXT,                        -- service_id, peer_id, or cron run_id
  target          TEXT,                        -- pane_id for push, peer_id for peer-events
  level           TEXT NOT NULL,               -- 'info' | 'warn' | 'error'
  message         TEXT NOT NULL,               -- one-line human-readable
  meta            TEXT                          -- JSON details: {body_preview, transition_from/to, etc}
);

CREATE INDEX idx_activity_time ON activity(occurred_at DESC);
CREATE INDEX idx_activity_source ON activity(source, occurred_at DESC);
```

Eviction policy: trim to most recent 10000 rows OR drop rows older than 30 days, whichever evicts more. Run on every 100th insert (cheap heuristic, no separate timer).

### 4.5 `Service` Rust struct (single source of truth, mirrors REST shape)

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,            // Cron | Daemon
    pub description: Option<String>,
    pub host: Option<String>,         // for daemon
    pub url: Option<String>,          // for daemon http
    pub port: Option<u16>,            // for daemon tcp
    pub process_label: Option<String>,// for daemon process
    pub script_path: Option<String>,  // for daemon script
    pub health_mode: HealthMode,      // Http | Tcp | Process | Script
    pub health_path: Option<String>,  // default "/healthz"
    pub health: HealthSnapshot,       // live: status + last probed + recent fields
    pub field_mapping: Option<FieldMapping>,
    pub push_enabled: bool,
    pub schedule: Option<String>,     // cron-only
    pub next_run: Option<i64>,        // cron-only
    pub last_run: Option<RunInfo>,    // cron-only
    pub registered_at: i64,
    pub updated_at: i64,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HealthSnapshot {
    pub status: HealthStatus,         // Healthy | Degraded | Down | Unknown
    pub probed_at: Option<i64>,
    pub latency_ms: Option<u64>,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
    pub extra: serde_json::Value,     // arbitrary additional fields the daemon returned
}
```

### 4.6 `ActivityEvent` shape

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityEvent {
    pub id: i64,
    pub occurred_at: i64,
    pub source: ActivitySource,       // Push | Health | Peer | Cron
    pub source_id: Option<String>,
    pub target: Option<String>,
    pub source_peer: Option<String>,  // populated by client when merging from remote peers
    pub level: ActivityLevel,         // Info | Warn | Error
    pub message: String,
    pub meta: serde_json::Value,
}
```

`source_peer` is `None` for events from the local peer; for events fetched from a remote peer's `/api/activity`, the merging client stamps it with the peer name for UI attribution (e.g. "rollacode-bot · voice-transcribe down").

## 5. API surface

### 5.1 REST endpoints (new, in `src-tauri/src/api/services_routes.rs`)

| Method | Path | Body / Params | Description |
|--------|------|---------------|-------------|
| GET    | `/api/services`                  | `?kind=daemon&enabled=true` | List local services (cron + daemon), filterable |
| POST   | `/api/services`                  | `{name, kind, ...}` | Create new daemon-kind service. Returns Service with newly-generated token. |
| GET    | `/api/services/:id`              | — | Single service detail with health |
| PUT    | `/api/services/:id`              | `{name?, url?, health_mode?, ...}` | Update daemon-kind. Cron-kind rejected (use `/api/crons/:id`). |
| DELETE | `/api/services/:id`              | — | Unregister + delete token + drop activity entries |
| POST   | `/api/services/:id/probe`        | — | Force health probe now, return latest snapshot |
| POST   | `/api/services/:id/regenerate-token` | — | Rotate Bearer token; returns new value once |
| GET    | `/api/activity`                  | `?since=<ts>&source=push&limit=500` | Bounded activity log query |

### 5.2 Push endpoint (existing, augmented)

`POST /api/panes/:pane_id/send` already exists. We add a token-validation middleware:

1. If `Authorization: Bearer <tk>` is present:
   - Look up `<tk>` in services tokens (file `~/.codeterm/services/<id>/token`)
   - If found and service is `enabled` and `push_enabled` → allow, stamp `source_service_id = <id>` on the activity row
   - If not found → 401 Unauthorized
2. If header absent → fall through to existing API token / mesh auth (preserves cron worker / agent report flow)

Service tokens are NOT valid for any other endpoint (no scope-creep). They authorize ONE thing: pane-write.

### 5.3 What a daemon implements (the contract)

For `kind=daemon` HTTP services, the daemon SHOULD expose:

```http
GET /healthz HTTP/1.1
Host: 127.0.0.1:7891

200 OK
Content-Type: application/json
{
  "status": "ok",                  // required: "ok" | "degraded" | "down"
  "version": "0.3.1",              // optional, recommended
  "uptime_secs": 14523,            // optional, recommended
  "request_count": 421,            // optional, custom field — UI shows as key/value
  "last_error": null               // optional, custom
}
```

Required: `status`. Everything else is optional. CodeTerm renders the recommended fields (`status`, `version`, `uptime_secs`, `request_count`, `last_error`) with nice formatting; arbitrary other fields render as a plain key/value list. Smart-Add lets the user override the field-mapping at register time.

For non-HTTP daemons (`tcp` / `process` / `script` modes), the daemon does not need to implement anything — CodeTerm probes externally. UI shows only up/down with no fields.

For `kind=daemon` services that opt into push (L3): on `service add`, CodeTerm prints the token once. The daemon reads it from a config / env var of its choosing and includes `Authorization: Bearer <tk>` on `POST /api/panes/:id/send`.

### 5.4 Tauri commands (frontend → backend bridge)

```rust
#[tauri::command] fn services_list(kind: Option<String>) -> Result<Vec<Service>, String>;
#[tauri::command] fn services_create(input: ServiceCreate) -> Result<Service, String>;
#[tauri::command] fn services_update(id: String, patch: ServiceUpdate) -> Result<Service, String>;
#[tauri::command] fn services_delete(id: String) -> Result<(), String>;
#[tauri::command] fn services_probe(id: String) -> Result<HealthSnapshot, String>;
#[tauri::command] fn services_regenerate_token(id: String) -> Result<String, String>;
#[tauri::command] fn services_smart_add_inspect(url: String) -> Result<SmartAddResult, String>;
#[tauri::command] fn activity_list(since: Option<i64>, source: Option<String>, limit: Option<usize>) -> Result<Vec<ActivityEvent>, String>;
```

### 5.5 CLI

```
codeterm service list                           # local services
codeterm service add <name> --kind daemon \
    --health-mode http --url http://127.0.0.1:7891 \
    [--health-path /healthz] [--push]          # register a daemon
codeterm service add <name> --kind daemon --health-mode tcp --host 127.0.0.1 --port 9201
codeterm service add <name> --kind daemon --health-mode process --process-label com.codeterm.foo
codeterm service add <name> --kind daemon --health-mode script --script-path /path/to/check.sh
codeterm service show <id>                     # detail with health + recent activity
codeterm service edit <id> [flags]             # mirror cron edit
codeterm service delete <id>
codeterm service probe <id>                    # force a probe now
codeterm service token <id> [--rotate]         # print token, optionally rotate
codeterm activity [--since <ts>] [--source push] [--limit 500]
```

Cron CLI stays as `codeterm cron *` (unchanged). Mesh tab UI surfaces both, but the CLIs remain partitioned for clarity.

## 6. UI components

### 6.1 File layout

```
src/components/MeshPanel/
├── MeshPanel.tsx              # root, sub-tab router (map / activity), stats header
├── MeshCanvas.tsx             # terminals-as-terminals graph + connection lines
├── ServicesTable.tsx          # flat list below canvas (always shown, never filtered by peer click)
├── ServiceRow.tsx             # one row in the flat table
├── ServiceDrawer.tsx          # slides from right on peer or service click
├── PeerTerminal.tsx           # one terminal-window-shaped peer node in canvas
├── ConnectionLine.tsx         # SVG line + animated push pulse
├── MeshSettingsTab.tsx        # absorbs MeshSection.tsx (identity / network / manual-add / exit)
├── AddServiceWizard.tsx       # modal: manual entry + Smart-Add option
├── AddPeerDialog.tsx          # reuses existing peer-add flow from MeshSection
├── useMeshData.ts             # hook: fetch local + paired-peer services, merge
├── useActivityLog.ts          # hook: tail activity events from local + peers
├── meshApi.ts                 # thin invoke wrapper for services_*, activity_*
└── types.ts                   # shared TS types mirroring Rust structs
```

Soft target: each file under 400 lines. `MeshCanvas.tsx` may approach the existing `MemoryCanvas.tsx` complexity (~423 lines) which is the precedent.

### 6.2 Top-level layout

Header (mirrors `MemoryPanel.tsx` aesthetic exactly):
```
Mesh   3 paired · 19 services · 17 ✓ · 1 ✗   ● mesh online
       │ map │ activity │                             [⟳ Refresh] [+ Service] [+ Peer]
```

Two sub-tabs: `map` (default) and `activity`. The flat ServicesTable always sits below the canvas in `map` view; in `activity` view the canvas is replaced by the activity timeline.

### 6.3 `MeshCanvas.tsx`

Visual: each peer rendered as a fake macOS terminal window:
- Title bar with traffic lights (red/yellow/green dots), peer name, IP
- Body: `$ codeterm services list` prompt plus inline service badges (color-coded by health, name + `· kind` suffix)
- This-machine peer is centered with accent border + soft glow
- Other peers positioned via simple force layout (reuses Memory's physics worker; small node count means initial positions can be hardcoded radial then settled)

Connection lines:
- Static dashed line between paired peers (idle baseline)
- Solid colored line + traveling dot when push happens (live event from `useActivityLog`)
- Recently-active line stays brighter for ~5 minutes after last push, then fades

Discovered (not-paired) peers render dimmed with dashed border and a "Pair too?" button on the side.

Click a peer terminal → `ServiceDrawer` slides in with peer details (services list, access dropdown, disconnect). The flat table below the canvas does NOT filter — it always shows all 19 services. (Decision: drawer-only behavior; manual host filter is via the chip above the table.)

### 6.4 `ServicesTable.tsx`

Columns: `●` (health dot) · Name · Host · Type · Status · Uptime · `›` (drawer).

Filter chips above the table: `all (19)` · `host: any ▾` · `kind: any ▾` · `status: any ▾`. Sort default by Host then Status.

Click a row → opens `ServiceDrawer` with that service's details (push test message, recent activity, edit, delete-or-disable, regenerate-token).

### 6.5 `ServiceDrawer.tsx`

Right-side slide-in (350-400px). Two contexts:

**Peer context** (clicked a peer terminal): peer name, IP, paired status badge, paired-time, access dropdown (full / read_only / agent_isolated), services list summary, Refresh + Disconnect buttons.

**Service context** (clicked a service row): service name, kind, health snapshot (status + version + uptime + extra fields), URL/host/port/process-label/script-path as relevant, recent activity (last 20 events for this service), Edit, Probe-now, Push-test-message, Regenerate-token, Delete.

For cron-kind services the drawer additionally shows: schedule editor (with the `--tz`, `--timeout-secs`, `--jitter-max-ms` flags landed in v1.1.24), next/last run, history, log viewer. This absorbs all existing CronPanel detail-view functionality. Edits route through the existing `/api/crons/:id` PUT (cron-kind writes never go through the new services REST surface — that surface is for daemon-kind only, see §5.1).

### 6.6 `MeshSettingsTab.tsx`

Direct port of the existing 460-line `MeshSection.tsx`. No functional changes — same invoke calls (`mesh_get_status`, `mesh_list_peers`, `mesh_refresh_discovery`, `mesh_connect`, `mesh_disconnect`, `mesh_approve_pairing`, `mesh_reject_pairing`, `mesh_add_manual`, `mesh_set_access`, `mesh_exit`, `mesh_start_broadcast`, `mesh_stop_broadcast`), same component shape, same auto-refresh-15s + `mesh-state-changed` listener. Move the file, rename, adjust import paths, delete the original `Settings/MeshSection.tsx`.

### 6.7 `AddServiceWizard.tsx`

Two paths in one modal:

**Manual** (default):
- Name (text)
- Health mode (radio: HTTP / TCP / Process / Script)
- Mode-specific fields (URL+path / Host+port / Process label / Script path)
- "Enable push API" toggle (on → token displayed once in success view)
- Submit → POST `/api/services` → success page with token shown once + copy button

**Smart-Add** (button on first step):
- User enters URL, clicks "Inspect"
- Backend `services_smart_add_inspect` does a single `GET <url>/healthz` (or root), captures response
- AI agent (Haiku) is given the response shape + asked to:
  - Suggest service name based on response/headers
  - Recommend health mode
  - Build initial `field_mapping` JSON suggesting which response fields map to recommended UI slots (status / version / uptime)
- Wizard pre-fills the form with the suggestions, user reviews/edits, submits

Smart-Add uses the existing agent infrastructure (worker headless spawn) with a tightly-scoped prompt and short timeout (30s).

### 6.8 Activity sub-tab

Plain virtualized timeline. One row per event:
```
14:23:11  Mac-Air      ⤳ voice-transcribe → pane a3f1b2c7   "transcript: '...'"
14:22:45  rollacode-bot ● voice-transcribe became degraded     latency 4200ms
14:21:03  Mac-Air      ✓ briefing-daemon healthy after 12m down
14:20:50  Mac-Air      ✓ cron mem-dreaming-auto fired (run b12) → success in 43s
```

Color-coded by source (push / health / peer / cron). Filter chips above for source + level. Auto-refresh every 5s while sub-tab is visible.

### 6.9 Settings-panel slim-down

`src/components/Settings/SettingsPanel.tsx` removes the Memory and Mesh sections. Adds a new "Tabs" section near the top:

```
Tabs
  ☑ Memory  →  Go to Memory →
  ☑ Mesh    →  Go to Mesh   →
```

Toggling off hides the tab from the main toolbar. Toggling on enables it (also auto-enables underlying subsystem if needed: enabling Mesh → flips `mesh.enabled = true`, kicks discovery, same as MeshSection's current toggle behavior).

`General` section gains a Librarian toggle (default-on). Other sections (Bridge, Agents, Theme, Keybindings) are unchanged.

## 7. Mesh sync data flow

### 7.1 Pull-and-merge model (chosen, A from brainstorm)

When the user opens the Mesh tab:

1. `useMeshData()` fires:
   - `services_list()` (local) → gets local Services array
   - For each `paired_peer` in `mesh_list_peers()`:
     - `fetch(peer.url + "/api/services", { headers: mesh-auth })` with 3s timeout
     - On success → array of remote Services, stamp `source_peer = peer.name` on each
     - On timeout / error → mark peer as "unreachable" in UI, do not include its services
   - Merge all arrays into a flat `services[]` for canvas + table rendering
2. `useActivityLog()` fires similarly for `/api/activity?since=<last_seen>`:
   - Pull from local + each paired peer
   - Merge by `occurred_at`, dedupe by `(source_peer, source, source_id, occurred_at)` if duplicates
   - Re-fetch every 5s while activity sub-tab is open OR while a push is in flight (to pick up live events)

### 7.2 Persistence and bounds

- **Local services**: persisted forever in `services` table until user deletes
- **Local activity**: bounded eviction at 10000 rows OR 30 days, run every 100 inserts
- **Local health probes**: bounded at 1000 per service OR 7 days, run on insert
- **Remote peer data**: NOT persisted on the consumer side. Mesh tab caches in memory only for the session; on tab close, GC. Next open re-fetches from peers fresh.

### 7.3 Failure modes

- Peer offline → "rollacode-bot offline · last seen Xm ago" badge in canvas, services attributed to that peer omitted from flat table. No data loss; data lives on the offline peer and reappears when it's back.
- Peer responds slow → 3s timeout per `/api/services` call (parallel, not sequential). Slow peer shown as "loading…" with spinner; flat table populates as each peer responds.
- Local DB write fails → log error, surface as a toast in UI, do not block other operations. Service registry is durable; activity log can tolerate occasional write failures (it's observability, not source of truth).

### 7.4 Cross-mesh push (out of scope, FYI)

L3 push is local-only by design. If a daemon on rollacode-bot wants to write into a Mac-Air pane, it uses the existing `codeterm send-text --pane <id> --mesh Mac-Air` CLI wrapper, which goes through the established mesh-message routing. This keeps the L3 contract simple (one HTTP endpoint, one Bearer token, one local pane) and avoids designing a cross-mesh push routing protocol that does not need to exist for v1.

## 8. Migration

### 8.1 What gets deleted

- `src/components/CronPanel/CronPanel.tsx` (the current cron management UI)
- `src/components/CronPanel/` directory if empty after removal
- `src/components/Settings/MeshSection.tsx` (moved into MeshSettingsTab.tsx)
- Cron tab toolbar entry; cron is accessed only via Mesh tab

### 8.2 What gets refactored

- `src/components/Settings/SettingsPanel.tsx` — drops Memory and Mesh sections, adds Tabs section, adds Librarian toggle
- `src/components/MemoryPanel/MemoryPanel.tsx` — adds a `settings` sub-tab (mirrors what MeshSettingsTab will be) absorbing memory-related settings
- Toolbar component (wherever Memory/Mesh buttons live) — gates buttons on Settings → Tabs toggles

### 8.3 What stays untouched

- `src-tauri/src/cron/` — all of it; no changes to scheduler, store, or RPC handlers
- `src-tauri/src/mesh/` — all of it; no changes to peer logic, identity, broadcast, pairing
- `src-tauri/src/api/cron_routes.rs` — cron HTTP API unchanged
- `bridge/sanitize.rs` — used by services::push but not modified

### 8.4 Migration ordering for the implementation plan

1. Backend services-store (pure addition, ships unused) → ✅ tests pass
2. Backend services REST + Tauri commands (pure addition) → ✅ CLI workable
3. Backend push-token middleware (pure addition, opt-in) → ✅ daemons can push if they know
4. Frontend MeshPanel skeleton + sub-tab router (renders empty) → ✅ button shows tab
5. Frontend MeshCanvas + PeerTerminal (renders peers from existing `mesh_list_peers`) → ✅ visual works without services
6. Frontend ServicesTable + useMeshData (wire real services data) → ✅ list populated
7. Frontend ServiceDrawer (peer + service contexts) → ✅ click opens drawer
8. Frontend AddServiceWizard (manual path, no Smart-Add yet) → ✅ register works end-to-end
9. Frontend MeshSettingsTab (port MeshSection.tsx) → ✅ peer management lives in tab
10. Settings reorg: drop Mesh + Memory sections from Settings, add Tabs toggles → ✅ slim Settings
11. Memory tab settings sub-tab (parallel to MeshSettingsTab effort, can be a different agent) → ✅ Memory absorbs its settings
12. Activity sub-tab + useActivityLog → ✅ activity timeline visible
13. Smart-Add (Haiku-based inspect) → ✅ wizard pre-fills smart suggestions
14. Connection-line animation + push activity wiring → ✅ "talking" lines work
15. Cron panel deletion → ✅ cron lives only in Mesh tab
16. Documentation: docs/MESH.md update, AGENT.md update, codeterm skill update → ✅ discovery wired

Each step is independently shippable / revertable. Steps 1-3 land the backend with no UI change. Steps 4-9 land the new tab as additive. Step 10 is the breaking change for Settings.

## 9. Testing strategy

### 9.1 Backend (Rust)

- **services::store**: CRUD + token generation + token rotation + cascade delete + bounded activity insert/evict (mirrors cron::store::tests pattern, ~30 tests)
- **services::health**: each of the 4 modes — http (200/non-200/timeout/JSON parse), tcp (open/closed), process (launchctl mock on macOS, systemctl mock on Linux), script (exit-0/non-zero/timeout). One test per mode + cache-TTL test.
- **services::push (token middleware)**: valid token allows, invalid 401, missing header falls through to existing auth, `push_enabled=false` rejects, disabled-service rejects.
- **services::activity**: insert, query by since/source/limit, eviction at 10k+30days threshold.
- **REST routes**: list / create / update / delete / probe via `axum::Router` test harness.

### 9.2 Frontend (TypeScript / Vitest)

- `useMeshData.ts`: fetch + merge correctness, peer offline handling, source_peer attribution.
- `useActivityLog.ts`: incremental polling (since-cursor), dedup on merge.
- `MeshCanvas.tsx`: peer positioning correctness (this-machine center, others radial), service-badge rendering by health.
- `ServiceDrawer.tsx`: peer-context vs service-context branch, cron-kind extra fields rendering.

### 9.3 E2E (WebDriver)

One scenario per shippable step from §8.4 ordering, but the critical path:

- Open Mesh tab → see local cron rows in table → click peer terminal → drawer shows correct peer
- Add new HTTP daemon via wizard → service appears in table + canvas → token displayed
- Daemon pushes to pane via `POST /api/panes/X/send` with token → pane receives message + activity row created + connection line lights up
- Cron drawer: edit schedule via the existing tz/timeout/jitter flags from v1.1.24

### 9.4 Bench (`evals/`)

- New `evals/services-tab/` set: smart-add-correctness — given N synthetic daemon `/healthz` responses, does the AI-mapping suggest sensible field mappings? (offline benchmark, no network calls during eval; fixtures live in evals/services-tab/fixtures/).

## 10. Open questions / future work

These are deliberately deferred from v1:

1. **WebSocket subscription deltas** — pull-on-tab-open is enough for v1 with 5-30 services. When peer count or service count grows, add a `mesh-events` WS channel and have Mesh tab subscribe to it for live updates. Schema-compatible evolution; no v1 design choice blocks this.

2. **Service grouping / labels** — when service count grows past ~30, surface tags / categories in the wizard and group rows in the table (e.g. by `tag=ml`, `tag=infra`). Not needed yet.

3. **Bundled daemon shipping** — codeterm could optionally ship reference daemon implementations (voice_transcribe.py, mail-watcher) and a `service install <preset>` command that drops them into `~/Library/LaunchAgents` and registers them. Andrey explicitly removed this from v1 scope ("codeterm — монитор, не lifecycle manager"). Re-evaluate at v1.2+ if frequent re-setup pain.

4. **OpenClaw migration recipes** — when this spec ships, OpenClaw services migrate manually via agents. If the friction is real, write per-service migration recipes as skills.

5. **Cross-mesh push** — if a use case emerges where a daemon on a peer needs to push directly into another peer's pane (not via the existing `mesh send-text`), revisit L3 with a routing layer. None known for v1.

6. **Mesh updated_at sync issue** (rollacode-bot reported 2026-05-09) — `mem_items.updated_at` not bumped on librarian re-vectorize of source-file content changes. Tracked separately as `learning_v1_1_25_backlog_mem_skill_row_updated_at_not_bumped_on_file` in mem. Unrelated to this spec but in the same release window.

## 11. Appendix: brainstorm decisions log

For traceability, the chain of choices made during the 2026-05-09 brainstorm session with Andrey:

| # | Decision | Rationale (Andrey's words) |
|---|----------|---------------------------|
| 1 | Full L1+L2+L3 in one iteration | Coherent design over three migrations |
| 2 | CLI `service add` + UI "+ Service" button only; no auto-discovery | "остальное не надо" |
| 3 | Single Service entity with kind=cron \| daemon | "Одна Service-сущность с kind: cron \| daemon" |
| 4 | All 4 health modes (http / tcp / process / script) | "все делай че я просил" |
| 5 | Pull-on-demand mesh discovery (not gossip, not WS) | Simplest; matches existing peer pattern |
| 6 | Per-service Bearer token in `~/.codeterm/services/<id>/token` (no keychain) | "keychain мне не нравится... на винде такого нет" |
| 7 | AI-agent-assisted Smart-Add for field mapping + health-mode recommendation | "у агента размапить там как надо нам в нужные поля" |
| 8 | Tab renamed Services → Mesh; Settings reorganized; Memory absorbs its settings; Librarian default-on in General | "давай назовем табу не Services а меш" + entire reorg quote |
| 9 | Cron panel deleted; cron details in Mesh drawer | "Убирается полностью, cron-детали в drawer внутри Mesh tab" |
| 10 | Activity sub-tab merges all 4 sources (push / health / peer / cron) | "Push messages, Health transitions, Peer events, Cron fires" |
| 11 | OpenClaw migration NOT in this spec | "openclaw не надо это все руками агент добавит" |
| 12 | A: pull-and-merge sync model | "как правильней?" → A explained + accepted |
| 13 | L3 push is local-only | "Только локальные services пишут в мои pane-ы" |
| 14 | Cron is first-class on canvas + in flat list | "кроны прям в меше показывать как сервисы на пирах прям на карте" |
| 15 | Click peer → drawer only; flat table stays full | "Только drawer открывается, таблица ниже остаётся flat" |
| 16 | Visual: peers as terminal-window mockups with traffic lights, services as inline badges, animated connection lines | "сами консольки как консольки да там они типа подключены" |

---

**End of design spec.**
