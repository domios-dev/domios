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

## Mesh tab and service registry

The Mesh tab is the UI surface for the local service registry and peer services. It replaces the old standalone Cron Panel.

### Service entity

Services have `kind = cron | daemon`. Cron jobs registered via `/api/crons` are projected as virtual `cron`-kind services — no data migration, they share the same cron store. Daemons are created via `codeterm service add` (see `--help`) or `POST /api/services`.

### Discovery

Peer services are not gossiped. When a peer node is expanded in the Mesh canvas, the UI fetches that peer's `/api/services` in real time via mesh relay. Services are never cached in the local store — they reflect the remote peer's live state at view time.

### Push tokens

Each daemon service has a per-service push token stored in a local token file. The token is returned once (at create or rotation). Daemons send `Authorization: Bearer <token>` on `POST /api/panes/<pane_id>/send` to write to a pane without the global access token. Comparison is constant-time.

### Activity log

Every service push and health event is recorded in the activity log. Bounded to 10 000 rows / 30 days; old rows are evicted on every 100th insert. Viewable in the Mesh tab's Activity sub-tab.

### Health probes

Four modes, configured per service:

| Mode | Trigger | Healthy condition |
|------|---------|-------------------|
| `http` | GET `url` + optional `health_path` | 2xx response |
| `tcp` | TCP connect to `host:port` | connection succeeds |
| `process` | launchctl / systemd `process_label` | unit is running |
| `script` | run `script_path` | exit code 0 |

Probe interval is per-service (`health_interval`, default 30 s).

### Live canvas

The Mesh canvas renders peers as terminal-window-shaped nodes. Health and push events update the canvas in real time via Tauri events `service-health-changed` and `service-pushed` — no polling.

The ServicesTable lists local services with health badges. ServiceDrawer shows full config and recent activity for a selected service. AddServiceWizard handles manual daemon registration with a Smart-Add heuristic that inspects running processes to pre-fill fields.

Full architecture spec: `docs/superpowers/specs/2026-05-09-mesh-tab-design.md`.

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
```

<!-- revision: 4 -->
