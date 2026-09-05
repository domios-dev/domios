# CodeTerm Mesh Networking

## Core Principles

1. **Paired = trusted.** Not paired = doesn't exist.
2. **AccessLevel** (Full/ReadOnly/AgentIsolated) is per-peer, per-machine. Each machine decides locally what a peer can do. Default = Full.
3. **No crown, no king, no trust declarations, no TTL.** Flat peer-to-peer model.
4. **Unpair = ban.** Pubkey removed, auth fails naturally.
5. **One machine = one mesh.** Cannot be in two meshes simultaneously.

## Discovery

Discovery (mDNS/UDP/Tailscale) finds peers on the network. Paired peers are shown in the mesh, unpaired peers appear in the "Discovered" list.

## Local REST availability

The local REST supervisor, not the mesh liveness detector, owns listener
recovery. Listener/server exit produces a failed lifecycle event and exactly
one newer generation; that replacement is advertised healthy only after socket
ownership and a successful real `/api/health` response. Bind ownership failure
and a still-bound listener with all four REST workers blocked have distinct
diagnostics, so worker starvation does not cause listener-generation churn.

Replacement readiness is queued to the liveness owner with a monotonic recovery
transition id, which restores local HTTP health, emits the existing
`mesh-state-changed` resync signal, and broadcasts the refreshed announce
exactly once. Distinct worker-stall recoveries within one listener generation
receive distinct ids; a duplicate observation cannot claim another resync.
Relay delivery keeps its existing at-most-once and structured-error behavior
across the outage.

## Auth Model

Every paired peer has an Ed25519 keypair. Requests between peers are signed with the sender's private key and verified against known public keys.

- `sign_request()` — signs a request with this peer's private key
- `verify_signed_request()` — verifies signature against known paired peer public keys
- Paired = allowed. Not paired = rejected.
- Normal peer requests and relayed requests use the same `AccessLevel` policy.
- Secret-vault routes and mesh token minting are local-only at every access level.

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

## Web UI discovery and relay

Every live announce carries `relay_protocol_version` and an optional typed
`web_ui` capability. Endpoints are limited to verified Tailscale HTTPS,
Tailscale IPv4 HTTP, and private-LAN HTTP origins. The producer probes
`/api/health` and matches the running build before advertising an origin; the
receiver validates and reorders the typed endpoints before storing them.

The default **Open** action never sends a remote daemon token to the browser.
It opens the local Web UI entry origin with the local token in a URL fragment
and a paired peer ID. The browser removes that fragment from history, resolves
the peer name from the authenticated local peer list, and sends API traffic
through the signed Mesh relay. Relay authorization uses the same policy as a
direct signed peer request. REST config responses redact `api.token`, and
`/api/secrets/*` cannot be relayed even with Full access.

Relay protocol version `1` is intentionally incompatible with older daemons. Both relay paths reject
older peers with `426 Upgrade Required`; the Mesh card disables **Open** and requests an update.

`POST /api/mesh/relay-to` returns `404 peer_not_paired` when no paired record exists and
`503 peer_unroutable` when the paired peer has no reachable address. Both reasons are stable constants.

A relayed send succeeds only when the TARGET confirms delivery — the relay hop's `ok` attests acceptance, the peer's body is the evidence, so `data.ok = false` on `POST /api/tabs/{id}/send` is a failure under an accepting envelope. Codes are the constants in `codeterm-types` (`mesh::delivery`); `bytes_written` is three-valued (`false` nothing landed, `true` landed but unconfirmed, absent unknown) and absent never means zero. The relay's send deadline outlives that route's readiness wait so the peer's answer decides the outcome; reads keep the short deadline. The sender's own leg is classified the same way rather than collapsing to a bare `502` — `send_relay` returns a `TransportError` carrying a `mesh_transport_*` constant onto that same envelope, and only a connect-phase failure claims `bytes_written=false`.

The serving browser origin remains its gateway. During an outage an already-loaded page can render,
but cannot fetch or switch peers. Failover needs a short-lived, single-use handoff grant, never a permanent token.

## Remote plan commands

`plan`, `spec`, `task`, `effort`, and `team` accept `--mesh PEER`. The CLI resolves
files and stdin locally, then sends the resulting content through the signed mesh
RPC endpoint to the peer's in-process plan dispatcher. Implicit local Tab identity
and working-directory defaults are not forwarded; explicit project paths refer to
the target machine. Mutations require `Full` peer access and carry an idempotency ID.

Mesh RPC requests carry the control-plane `protocol_version` integer. A peer on a
newer protocol refuses an older (or versionless) caller with the typed
version-skew error (`-32002`, both versions + recovery command), and the relay
forwards that error untranslated — see `codeterm docs API-TRANSPORT`.

## Cross-mesh worker reports (spawn + explicit bind)

`plan task dispatch` is **local-only**: it has no `--mesh`/`--peer` flag. It spawns
the Worker on the same daemon, binds the execution attempt, and exports
`CODETERM_PLAN_ID`/`CODETERM_TASK_ID` into the Worker's tab. A Worker spawned on a
remote peer with `agent spawn --mesh PEER --role Worker` therefore has **no attempt
on the origin plan store and no plan/task env**, so a bare `agent ack` / `agent
report --status` refuses with "tab has no plan/task assignment" and the message
degrades to a plain tab message.

Full cross-mesh dispatch (remote spawn + attempt bound on the origin plan + env
exported into the remote tab + report routed back across the mesh) is **not wired**
and is out of scope. Use the supported spawn-then-bind pattern instead:

1. Spawn the remote Worker so its report target is the mesh peer that owns the plan:

   ```bash
   codeterm agent spawn claude --mesh PEER --role Worker --task "<brief>"
   ```

   This is a **supported task-less Worker shape**, including for work that
   changes source on the peer: it owns no task because none can exist for it,
   and it declares no `--execution-intent`. Keep `--role Worker` — that is the
   field the peer resolves the Worker doctrine from, including the rule that
   every turn ends in exactly one `codeterm agent report`. A roleless spawn is
   accepted too and loses that doctrine silently, which on a remote machine
   means the work never reports home. `--execution-intent mutating` is still
   refused without a task binding, on the peer's own daemon; the mesh has no
   binding to give it, so leave the flag off.

2. Have the remote Worker report with **explicit origin ids** — it never has env
   ids, so pass them by hand. The daemon homes the report to the plan's owning
   machine over the mesh (text/status only; attachments are not relayed):

   ```bash
   codeterm agent report --status in_review --plan <origin-plan> --task <task> \
     --sha <lane-HEAD> "STATUS: in_review ..."
   ```

   Because explicit `--plan`/`--task` are supplied, the CLI does not fall back to a
   plain tab message; the report carries the origin ids and homes to the owning
   peer when the spawning tab has a mesh report target.

3. If the remote Worker was **not** spawned with a mesh report target (no homing
   peer), the report cannot reach the origin store. In that case the Orchestrator
   attaches the arriving tab message to the task by hand, or re-runs the report
   from a tab that does have the origin peer as its report target.

Binding a remote spawn's attempt directly onto the origin plan store still needs
cross-daemon plan resolution and remains a documented follow-up, not a shipped
capability.

## Peer API compatibility

Before a relayed CLI operation, the caller reads the peer's authenticated
`/api/identify` response. Peers older than 1.7.8 use the pane-era wire contract,
so the caller translates tab routes and identifier fields at the relay boundary
and normalizes the response back to the current tab contract. The preflight and
operation use distinct signed nonces. This adapter does not invent endpoints or
features absent from the older peer.

## Bridge migration

External-service bridges (Telegram today; Discord/Slack/etc. later) hold an exclusive long-poll lock against their upstream API — only one poller per bot token can be active at a time. `codeterm bridge migrate --to PEER` hot-relocates that poller from the source machine to a paired mesh peer **without restarting either daemon**. This is the recovery story for "I'm shutting this machine down for the night, please move the Telegram bot to another peer until I'm back."

### CLI

```bash
codeterm bridge migrate --to PEER \
  [--service telegram] \
  [--announce] [--announce-chat-id <chat-id>] \
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
10. **Optional `--announce`.** When `--announce` is set, post `[CodeTerm] Telegram bridge migrated to <to> from <from>` to `--announce-chat-id` (defaults to the target's first allow-listed bridge user) via `POST /api/bridge/announce` on the target. Failures here are non-fatal — the migration is already done.

### Secret locality guarantee

Each daemon resolves its own vault. The CLI never reads `telegram_bot_token` from the source machine and never sends it across the wire — `--ensure-token` only ever fires on the daemon that already owns the secret store. This matches the existing local-only contract for `--mesh PEER mem secret …` (which exits `7`) and the daemon's refusal to relay `/api/secrets/*` over mesh transport. Each peer holds its own vault scoped to its own keychain, full stop.

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--to` | *(required)* | Target peer `instance_name` (or `instance_id`) as shown in `codeterm mesh peers`. |
| `--service` | `telegram` | Which bridge to migrate. Only `telegram` today. |
| `--announce` | `false` | Post a one-shot sentinel message after success. |
| `--announce-chat-id` | *(first allow-listed user)* | Telegram chat id for `--announce`. Defaults to the target's first allow-listed bridge user. |
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

## Related documents

Service registry, health, inventory fields, and peer endpoints: `codeterm docs MESH-SERVICES`.

Mesh inventory is target-authoritative: machine-qualified rows carry provenance and capabilities derived from authenticated access, target policy, and the target route. Cached rows stay inspectable, but stale, offline, partial, denied, or protected mutations stay disabled with their reason. Daemons are registry entries, never controllable processes. Schedule actions appear only when granted. Remote or destructive actions name the target, await and announce its outcome, then refresh; failures never become optimistic local success.

Inventory opens as a full-screen searchable List beside the existing Mesh network graph. One shared mode bubble switches between the List and the graph of connected computers. Keyboard navigation preserves resource identity and focus; peer detail separates Services and Schedules and labels cached, offline, denied, and partial states.

<!-- revision: 27 -->
