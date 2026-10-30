# Domios Mesh Networking

## Account mesh

Sign in to Domios on each machine with the same account. Active account installations connect automatically when both machines have current membership and a reachable published endpoint. There is no per-connection Add, approval, or Disconnect action. To remove a machine, use **Remove from mesh** on its account card; this revokes its slot. A fresh sign-in rejoins a removed installation.

The account registry is the only source of machines in Mesh and `domios mesh peers`. Each installation appears once, identified by installation ID. Equal names do not merge installations. An expired endpoint stays visible as stale and can be removed explicitly. A signed-out or revoked installation session says **Sign in on NAME**; its last heartbeat does not authorize access.

Each side verifies signed membership and possession of both the installation and Mesh keys over one transcript, then obtains its own fresh online link permit. Both permits must verify before the connection opens. Account endpoints locate a peer; they never authorize it. One registry observation supplies a serialized connection wave. Rate-limit deadlines apply to the entire wave; concurrent lifecycle events cannot start overlapping waves.

`domios mesh status` returns identity, transport, publication, refusal and account machine summaries to authenticated local agents. Account mutations and credential-bearing account details remain local-owner only.

## Web access and endpoints

When Tailscale is connected and both the API and Mesh are enabled, the local web UI/API listens on assigned Tailscale IPv4 and IPv6 addresses, including while the account is signed out. Native address changes update the listener set. The Mesh UI card offers QR, Open and Copy only for health-proven listeners; Open carries the instance API token in a URL fragment. Browser API access requires that token. Account mesh peer requests and handshakes remain blocked while signed out or revoked, and disabling Mesh blocks peer traffic, withdraws the tailnet web listeners and stops their health proofs. Signed-in machines publish their health-proven endpoint to the account.

Verified HTTP works without Tailscale Serve HTTPS. The card prefers verified HTTPS, tailnet HTTP, then LAN HTTP, and labels the route; HTTPS-dependent browser features still need HTTPS. Loopback offers **Open on this machine**, without QR or Copy. After Tailscale or Serve setup, run `domios mesh refresh --json` to re-detect transport, reconcile Serve, verify and publish endpoints, and resync the card. Read `transport.state` and `web_ui.endpoints` there or in `domios mesh status --json` (cached local state, available while signed out). Confirm Open, Copy and QR for the proven endpoint before reporting completion. **Permission needed** distinguishes access denial from stopped or missing Tailscale.

Windows Defender may ask to allow incoming connections for Domios on first use; macOS may show an incoming-connections prompt when its application firewall is enabled. Linux firewall rules may need to permit the Domios port on the Tailscale interface. Domios does not change firewall rules or listen on LAN addresses for this feature.

## Admission and sign-out

Protected traffic requires current signed membership on both sides. Renewals travel in authenticated peer announcements. Expiry blocks requests, relays and open streams without a grace period. Signing out blocks every connection before endpoint cleanup; signing in to the same account restores account connections automatically. The backend authorizes membership and permits but never carries Mesh traffic.

A machine refused a slot under its plan limit stays blocked with `plan_limit_locked`. Account machine cards remain inspectable; the owner can remove an occupying installation or change plans. Missing membership trust cannot open a connection.

At startup Domios ignores and deletes the obsolete local pair records (`mesh/peers.json` and `mesh/account_mesh_ledger.json`) and their pair-store backups/corrupt copies. Startup reports each removed file and deletion failure. Installation identity, account keys, credentials, configuration and unrelated state are preserved.

## Local REST availability

The local REST supervisor owns listener recovery. A desktop REST generation reports `listening` after owning the socket, consuming a successful `GET /api/health` body and closing that connection. Exit emits a failed lifecycle event and creates exactly one replacement generation; bind ownership failure awaits an explicit port-release or restart event, never a timer. Status: `generation`, `phase`, `diagnostic`, `message`. Diagnostics: `listener_exited`, `bind_ownership_failed`, `workers_blocked` (bound listener whose four REST runtime workers cannot answer health); worker starvation never churns generations.

Replacement readiness is queued to the liveness owner with a monotonic recovery transition id, which restores local HTTP health, emits the existing `mesh-state-changed` resync signal, and broadcasts the refreshed announce exactly once. Distinct worker-stall recoveries within one listener generation receive distinct ids; a duplicate observation cannot claim another resync. Relay delivery keeps its existing at-most-once and structured-error behavior across the outage.

## Auth Model

Peers sign requests with Ed25519 keys. Inbound traffic requires an account-authenticated key and open admission; blocked edges are rejected as unknown keys. Outbound requests, sync, announces and streams also require open admission. Direct and relayed requests share `AccessLevel` policy; secret-vault routes and token minting remain local-only.

## Security

- Ed25519 signed requests with 5-minute expiry and nonce (replay protection)
- New links need both machines' fresh online account authorizations bound to one handshake transcript; no unsigned or stored-pair path creates a connection.
- A changed endpoint is metadata; it never replaces a peer's account-authenticated key.

### Peer access levels

Access is enforced on direct signed requests and relayed requests. Secret routes and mesh token minting are local-only at every level.

| Level | Permitted operations |
|---|---|
| Full | All peer API operations except local-only secret routes and token minting. Sync may refresh metadata of peers this machine already connected. |
| ReadOnly | `GET` routes permitted by peer policy, excluding local-only secret routes and token minting. `POST /api/mesh/sync` carries only the sender's liveness and identity. |
| AgentIsolated | Spawn agents; list only tabs spawned by this peer; read tab text and send input or attachments, exec, respond, report, or clear only on those tabs. It cannot read other tabs or workspace lists, mesh inventory, memory, configuration, secrets, or notifications. |
| Custom | Only operations granted by the configured `Permission` values; `Admin` grants ordinary peer API access and sender-only sync, except local-only routes. |

AgentIsolated ownership is stamped from the authenticated signed peer identity when an agent is spawned. Caller-supplied ownership fields cannot claim another peer's tabs. Every tab-scoped route checks the stored owner before dispatch.

Remote ownership remains until the local owner releases it. Closing the controller tab or Team on the remote machine does not currently send a reliable release signal. On the machine hosting the tab, run:

```bash
domios tab release-ownership --tab <ID>
```

The command is local-only and permits the local user/General Agent, the tab itself, or its Team Orchestrator. Once released, the remote peer loses AgentIsolated access to that tab and its `managed by` label disappears.

## Web UI discovery and relay

Every live announce carries `relay_protocol_version` and an optional typed `web_ui` capability. Endpoints are limited to verified Tailscale HTTPS, Tailscale IPv4 HTTP, and private-LAN HTTP origins. The producer probes `/api/health` and matches the running build before advertising an origin; the receiver validates and reorders the typed endpoints before storing them.

The default **Open** action never sends a remote daemon token to the browser. It opens the local Web UI entry origin with the local token in a URL fragment and a connected peer ID. The browser removes that fragment from history, resolves the peer name from the authenticated local peer list, and sends API traffic through the signed Mesh relay. Relay authorization uses the same policy as a direct signed peer request. REST config responses redact `api.token`, and `/api/secrets/*` cannot be relayed even with Full access.

Domios 1.15.0 uses mesh relay protocol version 2. Version 1 peers are refused with an upgrade message; both peers must run Domios 1.15.0 or later. A relayed write (send, PTY input, spawn, plan or Team change) to a tab, workspace or Team locked above the plan limit is refused with `403 plan_limit_locked` and `bytes_written=false` before anything is written; reads stay allowed. Routed messages are framed by the receiving daemon, and relayed send bodies stay unwrapped until the receiving `/send` handler adds its local route frame, preventing double wrapping.

Whole-peer Web views scope each terminal subscription to `mesh:<peer>/<tab>`, just like remote desktop tabs. The gateway requires that composite identity for output streams; an unscoped request returns 400 even when metadata reads succeed. Each subscription keeps its own peer-qualified replay cursor and uses only the serving gateway's credentials. The initial standalone boot frame establishes the cursor namespace before output arrives, so reconnects resume rather than discard the saved byte offset. A matching relay protocol does not by itself prove every feature is available on an older application version.

`POST /api/mesh/relay-to` returns `404 peer_not_paired` when no paired record exists and `503 peer_unroutable` when the connected peer has no reachable address. Both reasons are stable constants.

A relayed send succeeds only when the TARGET confirms delivery — the relay hop's `ok` attests acceptance, the peer's body is the evidence, so `data.ok = false` on `POST /api/tabs/{id}/send` is a failure under an accepting envelope. Codes are stable constants; `bytes_written` is three-valued (`false` nothing landed, `true` landed but unconfirmed, absent unknown) and absent never means zero. The relay's send deadline outlives that route's readiness wait so the peer's answer decides the outcome; reads keep the short deadline. The sender's own leg is classified the same way rather than collapsing to a bare `502` with a stable `mesh_transport_*` code on the same envelope, and only a connect-phase failure claims `bytes_written=false`.

`--mesh PEER` accepts, in order: an exact instance id, a unique instance-id prefix (the short id `mesh status` prints, 6+ hex characters), the advertised name, or the configured name (the advertised name without its ` [dev-suffix]`). Stale offline duplicates of a name never hide the live record; when two ONLINE peers answer to one name the relay refuses with `409 peer_name_ambiguous` and lists every candidate with its `instance_id`, so re-run with an id. Every relayed result names the peer that answered: `--json` object results carry `answered_by {instance_id, instance_name}`, human output prints `answered by NAME (short id)` on stderr when the peer was addressed by anything but its exact id, and a failure carries `error.peer_id`.

Relayed sends follow the target's verdict: `bytes_written=true` is accepted with `delivery: pending` even if submit confirmation outlives the relay; `false` or unknown is failure. `error.class` is `deterministic` (authorization, protocol, unknown tab, 4xx: the same request fails again), `transient` (busy tab, saturated window, 429/502–504: retry may help), or `unknown` (unclassified peer refusal). Only transient failures recommend retry. `send --mesh` never echoes payloads. A `--to-shell` line over 4095 bytes to a non-Windows peer is refused before sending with `line_exceeds_tty_limit`, because canonical-mode TTYs can truncate it despite a successful write.

A rename or address change announced while a connected peer is unreachable is remembered and re-announced when that peer next comes online.

The serving browser origin remains its gateway. During an outage an already-loaded page can render, but cannot fetch or switch peers. Failover needs a short-lived, single-use handoff grant, never a permanent token.

## Remote plan commands

`plan`, `spec`, `task`, `effort`, and `team` accept `--mesh PEER`. The CLI resolves files and stdin locally, then sends the resulting content through the signed mesh RPC endpoint to the peer's in-process plan dispatcher. Implicit local Tab identity and working-directory defaults are not forwarded; explicit project paths refer to the target machine. Mutations require `Full` peer access and carry an idempotency ID.

Mesh RPC requests carry the control-plane `protocol_version` integer. A peer on a newer protocol refuses an older (or versionless) caller with the typed version-skew error (`-32002`, both versions + recovery command), and the relay forwards that error untranslated — see `domios docs API-TRANSPORT`.

## Cross-mesh worker reports (spawn + explicit bind)

### Plan-bound mesh dispatch

`plan task dispatch <plan> <task> --mesh PEER --execution-intent mutating` creates an origin attempt before the signed peer request; generic mesh plan RPC runs against the peer's plan store.

`PEER` accepts a paired instance ID or name. Ambiguous live names are refused; use the instance ID to select one.

ACK and reports accept prior nonces issued to the same tab under its current session, verified by the hosting peer. A missing remote tab or an expired Worker ACK retires the origin attempt. Pending updates, dispatch, and respawn reconcile that state; `domios plan task respawn <plan> <task> --attempt <attempt>` dispatches a replacement on the attempt's recorded mesh peer. Lane history stays available for review and cleanup.

Models are validated by the peer's catalogue during Worker spawn. Use a qualified route such as `--model provider:model` when that provider is installed only on the peer; the origin preserves that route without consulting its local model catalogue.

The peer requires the same repository identity and base commit, provisions its lane, and binds the Worker to origin plan/task/attempt IDs. Missing repo/base, offline peers, and lost relays are explicit attempt states; failed attempts close so dispatch can retry. The origin checks that the peer advertises `remote_task_dispatch` (`/api/identify` features) before creating an attempt, and an HTTP refusal from the peer is `peer_refused`, not `relay_lost`. ACK/report relays authenticate the Worker peer, attempt, worker tab, and `CODETERM_SESSION_BINDING_NONCE`; they do not require local Plan owner authority or `caller_tab_id`. Both peers must run the same control-plane protocol version. A lost dispatch response triggers one signed lookup for the existing Worker; recovered lane evidence records the spawn, while response loss alone is `relay_lost`, never proof of `peer_offline`.

After origin review accepts the report, run `domios plan task merge-remote-lane <plan> <task> <attempt>`. The origin asks that authenticated peer for the bound lane bundle and refuses unless its tip equals the accepted report SHA. Integration requires a clean checkout on the dispatch-time destination branch, verifies the bundle, and creates a real merge commit with ancestry. Cherry-pick and squash are invalid.

Both peers enforce Full access and owner rules. No token, provider credential, secret, or secret-bearing environment enters the task payload.

### Task-less Workers

Exploratory work without a plan uses task-less spawn; it creates no origin attempt. Spawn from the machine owning the plan so reports return there:

```bash
domios agent spawn claude --mesh PEER --role Worker --cwd <peer-workspace-path> --task-file <local-brief-path>
```

Use `--task-file` for authored briefs. Multiline or over-240-character `--task` values are refused to prevent shell expansion or truncation. `--task-file -` accepts a single-quoted heredoc. The origin reads the file and sends its contents, never its local path.

`--cwd` must name an absolute, existing peer workspace directory. From Git Bash, set `MSYS_NO_PATHCONV=1` for POSIX paths such as `/srv/project`.

Keep `--role Worker`: it selects Worker doctrine and exactly one `domios agent report` per turn. Roleless spawn loses that reporting doctrine. Task-less Workers may edit peer source, but own no task or execution intent; `--execution-intent mutating` is refused without a task binding. They do not participate in plan-bound attempt, report or merge accounting.

## Peer API compatibility

Remote CLI refusals (`ok:false`) and RPC errors exit nonzero and print the peer's reason, including with `--json`.

Utility shell deadlines require an agent running locally on the target peer to own expiry reporting and cleanup. Remote `tab new --timeout`, `tab timeout`, and `tab restart --timeout` are refused. Ask an agent running on that peer to create or manage the shell with `--timeout`; do not omit a required QA/build deadline to bypass the refusal.


Recurring delayed-message create and edit operations require `delayed_recurrence` in the target's advertised identify `features`. The origin refuses when the feature is missing or no feature list is advertised, naming the peer and its reported version and directing the user to update the peer or send a one-shot. One-shot delays remain available to older peers.

Bridge migration between peers: `MESH-BRIDGE`.

## Mesh-disabled gate (503)

When `config.mesh.enabled = false`, the auth middleware short-circuits every `/api/mesh/*` path (other than `/api/mesh/ping`) with `503 Mesh networking is disabled`. The same 503 also fires for any caller authenticated via the mesh signed-token path — so other peers attempting to relay into us also get rejected, not just the local UI. The local instance Bearer credential keeps working for non-mesh routes regardless.

**Disabling mid-session** (PATCH `/api/config mesh.enabled=false`) additionally marks every connected peer `online = false` and emits `mesh-state-changed` so the UI updates immediately. Re-enabling restores the routes; peer state picks back up on the next ping tick. Mesh off rejects remote access, though outbound health pings to connected peers may continue.

## Related documents

Service registry, health, inventory fields, and peer endpoints: `domios docs MESH-SERVICES`.

Mesh inventory is target-authoritative: machine-qualified rows carry provenance and capabilities derived from authenticated access, target policy, and the target route. Cached rows stay inspectable, but stale, offline, partial, denied, or protected mutations stay disabled with their reason. Daemons are registry entries, never controllable processes. Schedule actions appear only when granted. Remote or destructive actions name the target, await and announce its outcome, then refresh; failures never become optimistic local success.

Inventory opens as a full-screen searchable List beside the existing Mesh network graph. One shared mode bubble switches between the List and the graph of connected computers. Keyboard navigation preserves resource identity and focus; peer detail separates Services and Schedules and labels cached, offline, denied, and partial states.

### Signed peer notices (no local credentials)

| Method | Path | Body | Effect |
|---|---|---|---|
| POST | `/api/mesh/account-presence` | `{metadata_revision}` | Triggers one rate-limited registry refresh. |

<!-- revision: 74 -->
