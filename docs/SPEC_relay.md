# CodeTerm Relay — SPEC.md

## Контекст

Relay — это lightweight mesh participant, который живёт на отдельном хосте (VM/NAS/Pi с Tailscale) и выступает мостом между внешними сервисами (webhooks) и mesh panes.

**Ключевое отличие от desktop-приложения:**
- Desktop app имеет REST API (:7685), но требует запущенного приложения
- Relay — отдельный lightweight бинарник (~5-10 MB), не требует desktop UI
- Relay имеет restricted capabilities: `pane.read`, `pane.send_text`, `notify` (НЕ agent spawn, НЕ mem write, НЕ PTY)

**Проблема которую решает:**
- Telegram bridge conversational идёт через General Agent
- Многие интеграции хотят structured programmatic delivery (Stripe → pane, без agent reasoning)
- "Web UI" был в Phase 5 плана, но relay — cleaner shape

---

## Architecture Overview

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  External SaaS  │────▶│  codeterm-relay  │────▶│  Full Peer      │
│  (Stripe, GH)   │     │  (~5-10 MB)       │     │  (BLACKSTAR)    │
└─────────────────┘     └────────┬─────────┘     └────────┬────────┘
                                 │                        │
                                 │ Bearer Auth           │ Mesh Signed
                                 │ POST /api/send        │ Request
                                 ▼                        ▼
                        ┌────────────────┐        ┌─────────────────┐
                        │ Embedded SPA   │        │  Mesh Transport │
                        │ (Mobile PWA)   │        │  (Ed25519)      │
                        └────────────────┘        └─────────────────┘
```

---

## Crate Structure

### Новые crates в workspace:

```
crates/
├── codeterm-mesh-client/      # P0: Extracted from mesh/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── types.rs           # Shared types (MeshIdentity, PairedPeer, etc.)
│       ├── crown.rs           # Ed25519 crypto primitives
│       ├── transport.rs        # PeerClient, try_peer
│       └── error.rs
│
├── codeterm-pane-api/         # P0: Pane API client
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── client.rs          # REST client to full peer
│       └── types.rs           # API request/response types
│
└── codeterm-relay/             # P1+: Main binary
    ├── Cargo.toml
    └── src/
        ├── main.rs
        ├── config.rs          # TOML config + route_table
        ├── webhook.rs         # Webhook handler + template engine
        ├── api.rs             # REST API endpoints (axum)
        ├── ui.rs              # Embedded SPA
        └── mesh.rs            # Mesh client integration
```

---

## P0: Extract Crates

### codeterm-mesh-client

**Извлекается из `src-tauri/src/mesh/`:**
- `types.rs` — MeshIdentity, PairedPeer, PairedPeerDisk, DiscoveredPeer, AccessLevel, Permission, PairingRequest, MeshSyncPayload, RelayCommand, RelayResponse, PeerAnnounce, GossipPeerInfo, PeerAddress
- `crown.rs` — Ed25519 signing/verification: `generate_peer_keypair()`, `sign_request()`, `verify_signed_request()`, `public_key_from_private()`
- `identity.rs` — IdentityManager (опционально, возможно overkill для relay)
- `transport.rs` — PeerClient, try_peer
- `peers.rs` — identity/peer storage (для relay — упрощённый, только identity)

**Публичное API:**
```rust
// types
pub use codeterm_mesh_client::types::*;

// signing
pub fn generate_keypair() -> (String, String); // (private_key, public_key)
pub fn sign_request(private_key: &str, instance_id: &str) -> Result<String, String>;
pub fn verify_request(token: &str, known_keys: &HashMap<String, String>) -> Result<String, String>;

// peer client
pub struct PeerClient { ... }
impl PeerClient {
    pub fn new_signed(addr: &str, port: u16, token: &str) -> Self;
    pub async fn ping(&self) -> bool;
    pub async fn auth_post_json(&self, path: &str, body: &impl Serialize) -> Result<Value, String>;
    pub async fn relay(&self, cmd: &RelayCommand) -> Result<RelayResponse, String>;
}
```

**Dependencies:**
```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
base64 = "0.22"
ed25519-dalek = { version = "2", features = ["rand_core"] }
uuid = { version = "1", features = ["v4", "serde"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
tokio = { version = "1", features = ["full"] }
```

### codeterm-pane-api

**Клиент к pane API full peer (relayed через mesh или напрямую):**
```rust
pub struct PaneApiClient {
    peer_client: PeerClient,
}

// Methods
pub async fn list_panes(&self) -> Result<Vec<PaneInfo>, String>;
pub async fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String>;
pub async fn get_output(&self, pane_id: &str, offset: Option<usize>) -> Result<String, String>;
pub async fn stream_output(&self, pane_id: &str) -> Result<SseStream, String>;
```

**API endpoints которые использует:**
- `GET /api/panes` — list panes
- `POST /api/panes/{id}/send` — send text
- `GET /api/panes/{id}/output` — get output
- `GET /api/panes/{id}/output/stream` — SSE stream

**Relay capabilities (для capability checking):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RelayCapability {
    PaneRead,       // GET /api/panes, GET /api/panes/{id}/output
    PaneSendText,   // POST /api/panes/{id}/send
    Notify,         // POST /api/notifications
}
```

---

## P1: Minimal Relay Binary

### Command Line Interface

```bash
codeterm-relay run [OPTIONS]

OPTIONS:
    --port <PORT>              HTTP port (default: 7690)
    --mesh-port <PORT>         Mesh API port (default: 7691)
    --config <PATH>            Config file (default: ~/.codeterm-relay/config.toml)
    --routes <PATH>            Routes file (default: ~/.codeterm-relay/routes.toml)
    --identity <PATH>          Identity file (default: ~/.codeterm-relay/identity.json)
    --bearer-token <TOKEN>     Bearer token for API auth
    --tailscale-funnel         Expose via Tailscale Funnel (HTTPS)
    --verbose                  Enable verbose logging
```

### Directory Structure

```
~/.codeterm-relay/
├── config.toml           # Main config
├── routes.toml          # Webhook routes
├── identity.json        # Ed25519 identity (generated on first run)
└── mesh/
    └── peers.json        # Paired peers (full mesh participants)
```

### REST API (P1)

```
GET  /api/health                         # Health check
GET  /api/identify                        # Relay identity info
GET  /api/peers                          # List paired full peers
GET  /api/peers/{id}/panes               # List panes on peer
POST /api/send                           # Send text to pane
     Body: { "peer": "BLACKSTAR", "pane": "billing", "text": "..." }
GET  /api/panes/{id}/output              # Get pane output
GET  /api/panes/{id}/output/stream       # SSE stream
GET  /api/routes                         # List routes
POST /api/routes/reload                  # Reload routes from disk

# Webhooks (public, with webhook-specific auth)
POST /webhook/<route_id>                 # Webhook endpoint

# SPA
GET  /                                   # Embedded SPA (mobile-friendly UI)
```

### Auth Model

**Bearer token (simple, for now):**
- `--bearer-token` flag sets the token
- Header: `Authorization: Bearer <token>`
- Used for `/api/*` endpoints

**Webhook auth (per-route config):**
- HMAC-SHA256 for generic webhooks
- `X-Hub-Signature-256` for GitHub
- `X-Stripe-Signature` for Stripe
- `?secret=<webhook_secret>` for simple cases

### Config Format (config.toml)

```toml
[relay]
# Instance name (shown in mesh)
name = "home-relay"
# Bearer token for API access
bearer_token = "change-me"
# API bind address
bind = "0.0.0.0:7690"

[mesh]
# Mesh API port (for full peers to connect)
api_port = 7691
# Ed25519 private key (base64) - generated if not present
# private_key = "..."

[logging]
level = "info"
```

### Routes Format (routes.toml)

```toml
[routes.stripe-failed]
# Match conditions (AND logic)
match.path = "/webhook/stripe"
match.header.X-Stripe-Event = "payment_intent.payment_failed"
# Target
target.peer = "BLACKSTAR"
target.pane = "billing"
# Template (handlebars)
template = "❌ Stripe failure: ${{json.data.object.amount}} for {{json.data.object.metadata.user_email}}"

[routes.github-pr]
match.path = "/webhook/github"
match.header.X-GitHub-Event = "pull_request"
target.peer = "BLACKSTAR"
target.pane = "general"
template = "📝 New PR: {{json.pull_request.html_url}} by @{{json.pull_request.user.login}}"

[routes.github-push]
match.path = "/webhook/github"
match.header.X-GitHub-Event = "push"
target.peer = "BLACKSTAR"
target.pane = "ci"
template = "🔔 Push to {{json.repository.full_name}}: {{json.commits.0.message}}"
```

**Match conditions:**
- `match.path` — URL path prefix match
- `match.header.<Header-Name>` — header value match
- `match.json_path` — JSONPath query on body (e.g., `$.type`)
- `match.json_value` — expected value for json_path match

**Template engine:**
- Handlebars-style: `{{json.field}}`, `{{header.X-Custom}}`
- JSONPath for nested access: `{{json.data.object.amount}}`
- Emoji prefixes for visual differentiation

---

## P2: Webhook Routing

### Match Evaluation

```rust
pub struct RouteMatch {
    path: Option<String>,           // Path prefix
    header: HashMap<String, String>, // Header exact match
    json_path: Option<(String, String)>, // (jsonpath, expected_value)
}

impl Route {
    pub fn matches(&self, request: &WebhookRequest) -> bool {
        // Path check
        if let Some(ref path) = self.match.path {
            if !request.path.starts_with(path) {
                return false;
            }
        }

        // Header checks
        for (header, value) in &self.match.header {
            if request.headers.get(header) != Some(value) {
                return false;
            }
        }

        // JSON checks
        if let Some((path, expected)) = &self.match.json_path {
            let actual = jsonpath_get(&request.body, path);
            if actual != expected {
                return false;
            }
        }

        true
    }
}
```

### Template Rendering

```rust
pub fn render_template(template: &str, ctx: &TemplateContext) -> String {
    // ctx contains:
    // - json: parsed JSON body
    // - headers: HashMap of headers
    // - path: request path
    // - query: query params

    // Handlebars-like substitution
    let mut result = template.to_string();

    // {{json.field}} - JSONPath
    for cap in TEMPLATE_JSON_REGEX.captures_iter(template) {
        let jsonpath = &cap[1];
        let value = jsonpath_get(&ctx.json, jsonpath);
        result = result.replace(&cap[0], &value);
    }

    // {{header.NAME}} - Header lookup
    for cap in TEMPLATE_HEADER_REGEX.captures_iter(&result) {
        let header_name = &cap[1];
        let value = ctx.headers.get(header_name).unwrap_or(&"");
        result = result.replace(&cap[0], value);
    }

    result
}
```

### Webhook Auth

```toml
[routes.stripe-webhook]
match.path = "/webhook/stripe"
target.peer = "BLACKSTAR"
target.pane = "billing"

# HMAC verification
[routes.stripe-webhook.auth]
type = "hmac_sha256"
secret = "${STRIPE_WEBHOOK_SECRET}"  # env var expansion

[routes.github-webhook]
match.path = "/webhook/github"
target.peer = "BLACKSTAR"
target.pane = "general"

[routes.github-webhook.auth]
type = "github"
secret = "${GITHUB_WEBHOOK_SECRET}"
```

---

## P3: Embedded SPA

### UI Features

1. **Peer/Pane selector**
   - List paired full peers
   - Show panes per peer
   - Filter by pane type/name

2. **Send text interface**
   - Text input
   - Send button
   - Recent sends history

3. **Output viewer**
   - Live output via SSE
   - Scrollback buffer
   - ANSI color support

4. **Dashboard mode**
   - Grid of pane outputs
   - Auto-refresh

### Technical Stack

- **React** with minimal dependencies
- Bundled via Vite + `@vercel/ncc` or `cargo-leptos` (pure Rust alternative)
- Embedded via `include_dir!()` macro
- Mobile-first responsive design

### API for SPA

```rust
// Frontend calls relay's own API
GET  /api/relay/peers        // List paired full peers
GET  /api/relay/peers/{id}/panes  // Panes on peer
POST /api/relay/send        // Send to pane via relay

// Response shapes
#[derive(Serialize)]
pub struct RelayPeersResponse {
    pub instance_id: String,
    pub instance_name: String,
    pub peers: Vec<RelayPeerInfo>,
}

#[derive(Serialize)]
pub struct RelayPeerInfo {
    pub instance_id: String,
    pub instance_name: String,
    pub online: bool,
    pub pane_count: usize,
}

#[derive(Deserialize)]
pub struct SendRequest {
    pub peer: String,     // instance_id or instance_name
    pub pane: String,     // pane id or name
    pub text: String,
}
```

---

## P4: Tailscale Funnel

```bash
# One command to expose publicly
codeterm-relay run --tailscale-funnel

# What happens:
# 1. Get Tailscale hostname
# 2. Enable Funnel on the port
# 3. Certificate auto-provisioning
# 4. Public HTTPS URL ready
```

```rust
// Tailscale Funnel integration
async fn setup_tailscale_funnel(port: u16) -> Result<String, String> {
    // `tailscale funnel 7690` enables HTTPS
    // Returns the public HTTPS URL
}
```

---

## P5: Route Gallery (Templates)

Pre-baked route templates for popular services:

```bash
# Interactive setup
codeterm-relay setup --template stripe
codeterm-relay setup --template github
codeterm-relay setup --template slack
```

### Template Catalog

| Service | Route | Auth | Template |
|---------|-------|------|----------|
| Stripe | `/webhook/stripe` | `X-Stripe-Signature` | payment_intent events |
| GitHub | `/webhook/github` | `X-Hub-Signature-256` | PR, push, CI |
| Slack | `/webhook/slack` | `X-Slack-Signature` | slash commands |
| Vercel | `/webhook/vercel` | Bearer | deployments |
| Railway | `/webhook/railway` | Bearer | deployments |
| Netlify | `/webhook/netlify` | Bearer | deploys |

---

## P6: Durable Webhook Queue

### Problem

When a full peer is offline, webhooks arrive but have nowhere to go.

### Solution

```rust
pub struct WebhookQueue {
    dir: PathBuf,
}

impl WebhookQueue {
    // Enqueue when peer offline
    pub fn enqueue(&self, route_id: &str, event: WebhookEvent) -> Result<u64, String>;

    // Drain when peer comes online
    pub fn drain(&self, peer_id: &str) -> Vec<WebhookEvent>;

    // Retry logic
    pub fn retry_failed(&self) -> Vec<(u64, WebhookEvent)>;
}

#[derive(Serialize, Deserialize)]
pub struct WebhookEvent {
    pub id: u64,
    pub route_id: String,
    pub target_peer: String,
    pub target_pane: String,
    pub template: String,
    pub body: Value,
    pub headers: HashMap<String, String>,
    pub received_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
}
```

---

## Security Considerations

### Capability Model

Full peers MUST check relay capabilities before serving requests:

```rust
// On full peer side (in mesh_routes.rs):
async fn mesh_relay(
    Path(target_peer): Path<String>,
    Json(cmd): Json<RelayCommand>,
) -> Result<Json<RelayResponse>, StatusCode> {
    // Get relay's public key from peer info
    let relay_info = get_relay_info(&target_peer).ok_or(StatusCode::NOT_FOUND)?;

    // Check if relay has required capability
    if !relay_info.capabilities.contains(&RequiredCapability::PaneSendText) {
        return Err(StatusCode::FORBIDDEN);
    }

    // Proceed with request
}
```

### Access Control

| Endpoint | Required Capability |
|----------|-------------------|
| GET /api/peers | None (public info) |
| GET /api/peers/{id}/panes | PaneRead |
| POST /api/send | PaneSendText |
| GET /api/panes/{id}/output | PaneRead |

### Relay Identity

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayIdentity {
    pub instance_id: String,       // UUID v4
    pub instance_name: String,      // "home-relay"
    pub public_key: String,        // Ed25519
    pub capabilities: Vec<RelayCapability>,
    pub paired_peers: Vec<String>, // instance_ids of full peers
}
```

---

## Implementation Order

### P0 — Extract Crates (S, refactor)
- [ ] `codeterm-mesh-client` — types, crown, transport
- [ ] `codeterm-pane-api` — pane API client
- [ ] Update `src-tauri` to use extracted crates
- [ ] Tests for extracted crates

### P1 — Minimal Relay Binary (S, ~1 week)
- [ ] Basic binary with `--help`
- [ ] Config file loading
- [ ] Identity generation/storage
- [ ] Bearer auth middleware
- [ ] `POST /api/send` — send text to pane via mesh
- [ ] `GET /api/peers` — list paired peers
- [ ] Health endpoint

### P2 — Webhook Routing (S, ~1 week)
- [ ] TOML route config parsing
- [ ] Path matching
- [ ] Header matching
- [ ] JSONPath matching
- [ ] Handlebars template rendering
- [ ] HMAC webhook verification
- [ ] GitHub webhook verification

### P3 — Embedded SPA (M, ~2 weeks)
- [ ] React app scaffold
- [ ] Peer/pane selector UI
- [ ] Send text UI
- [ ] Output viewer with SSE
- [ ] Mobile-responsive design
- [ ] Bundle into binary

### P4 — Tailscale Funnel (S, ~2 days)
- [ ] `tailscale funnel` integration
- [ ] Auto-certificate provisioning
- [ ] Public URL display

### P5 — Route Gallery (S, ~1 week)
- [ ] Interactive setup wizard
- [ ] Pre-built route templates
- [ ] Documentation

### P6 — Durable Queue (M, ~1 week)
- [ ] On-disk queue implementation
- [ ] Peer online detection
- [ ] Retry with backoff
- [ ] Dead letter handling

---

## Open Questions

### 1. Multi-tenant support
v1 = one relay = one Ed25519 keypair = one mesh. Multi-tenant overkill for v1 but architecture must not block it.

**Decision:** Architecture supports multiple identities but UI doesn't expose it yet.

### 2. PWA vs Tauri Mobile
PWA free + immediate, native better for push notifications. PWA first.

**Decision:** PWA in P3, Tauri Mobile as future work.

### 3. Same-binary-mode vs dedicated
`codeterm --relay-only` simpler dist, but bloats binary 10x with PTY/multiplexer code never used.

**Decision:** Separate binary (recommended in spec).

### 4. Embedded SPA bundling
Options:
- Vite + ncc (JS toolchain required for build)
- Sycamore/Yew (pure Rust, compiles with cargo)
- Leptos (pure Rust, compiles with cargo)

**Decision:** Yew or Leptos for pure Rust simplicity, or vanilla JS+CSS if JS toolchain is acceptable.

---

## Dependencies

### Core
```toml
tokio = { version = "1", features = ["full"] }
axum = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }
tower-http = { version = "0.6", features = ["cors"] }
```

### Crypto
```toml
ed25519-dalek = { version = "2", features = ["rand_core"] }
sha2 = "0.10"
hmac = "0.12"
base64 = "0.22"
```

### Utilities
```toml
handlebars = "5"          # Templates
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
dirs = "6"
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
```

### Optional
```toml
tailscale = { version = "0.5", optional = true }  # Tailscale Funnel
```

---

## File Structure (Target)

```
codeterm-relay/
├── Cargo.toml
├── crates/
│   ├── codeterm-mesh-client/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs
│   │       ├── crown.rs
│   │       ├── transport.rs
│   │       └── error.rs
│   │
│   ├── codeterm-pane-api/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── client.rs
│   │       └── types.rs
│   │
│   └── codeterm-relay/
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs
│       │   ├── cli.rs
│       │   ├── config.rs
│       │   ├── routes.rs
│       │   ├── webhook/
│       │   │   ├── mod.rs
│       │   │   ├── matcher.rs
│       │   │   └── template.rs
│       │   ├── api/
│       │   │   ├── mod.rs
│       │   │   ├── send.rs
│       │   │   ├── peers.rs
│       │   │   └── webhooks.rs
│       │   ├── mesh.rs
│       │   ├── ui/
│       │   │   └── mod.rs
│       │   └── error.rs
│       └── ui/
│           ├── index.html
│           ├── src/
│           │   └── App.tsx
│           └── dist/   # Built assets
│
└── tests/
    └── integration_tests.rs
```

---

## Success Criteria

### P0 (Extraction)
- [ ] `cargo build -p codeterm-mesh-client` works
- [ ] `cargo build -p codeterm-pane-api` works
- [ ] Existing tests pass after extraction
- [ ] No breaking changes to `codeterm` desktop app

### P1 (Minimal Relay)
- [ ] `codeterm-relay run --help` shows all options
- [ ] Config file created on first run
- [ ] Can pair with a full peer
- [ ] `POST /api/send` delivers text to pane
- [ ] Bearer auth works

### P2 (Webhook Routing)
- [ ] Routes load from TOML
- [ ] Path matching works
- [ ] JSONPath matching works
- [ ] Templates render correctly
- [ ] HMAC verification works

### P3 (Embedded SPA)
- [ ] UI loads at `/`
- [ ] Can select peer and pane
- [ ] Can send text
- [ ] Can view output stream
- [ ] Mobile-friendly

---

*Generated: 2024-05-08*
*Based on: codeterm Issue #62 (codeterm-relay proposal)*