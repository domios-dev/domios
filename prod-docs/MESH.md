# Domios Mesh Networking

## Core Principles

1. **The account is the only source of new links.** A machine joins the mesh only by linking a machine enrolled in the same account. There is no LAN broadcast, mDNS, UDP beacon, Tailnet-wide scan, manual address pairing, pairing request or introduction.
2. **Paired is necessary, not sufficient.** Traffic on a paired edge also needs that edge's admission to be open right now (see *Admission and sign-out*).
3. **AccessLevel** (Full/ReadOnly/AgentIsolated) is per-peer, per-machine and independent of account membership. Each machine decides locally what a peer can do. Default = Full.
4. **Only explicit removal deletes a pair.** `domios mesh disconnect` (local unpair) or an account revoke removes it; sign-out, expiry, outage and account switches never do.
5. **One machine = one mesh.** Cannot be in two meshes simultaneously.

## Adding a machine

Signed-in machines with Mesh enabled automatically listen on their assigned Tailscale IPv4 and IPv6 addresses and publish a health-proven endpoint to their account. No listener setting or Tailscale Serve setup is needed. Native address changes update the listener set; sign-out or disabling Mesh closes it. These listeners accept the existing authenticated peer and account-link protocols, reject local API credentials, and do not serve the browser frontend.

Windows Defender may ask to allow incoming connections for Domios on first use; macOS may show an incoming-connections prompt when its application firewall is enabled. Linux firewall rules may need to permit the Domios port on the Tailscale interface. Domios does not change firewall rules or listen on LAN addresses for this feature.

Both machines must be signed in to the same account, enrolled, online to the account registry and directly reachable (for example over the same Tailnet). Pick the machine from the account's machine list, or run:

```bash
domios mesh link <installation-id>
```

The registry supplies the target's single published endpoint; no address is ever typed. Each side verifies the other's signed membership and installation-key possession over a transcript binding both installations, both Mesh keys, the account and two fresh nonces, then EACH side obtains its own fresh online link authorization. Neither side stores the pair until both authorizations are held, and the other machine needs no human approval. Offline linking has no path: a cached machine list, a cached membership, an old request, a replayed or foreign proof, or a backend-offline participant cannot create a link. A refused free machine slot returns the typed `mesh.machines` refusal with `limit` and `active_count`.

Mesh pairing and peer calls require both daemons to run Domios 1.15.0 or later. A 1.14.9 peer receives a version error explaining that both peers must be upgraded.

Account enrollment and address publication wait for the server's retry time after a rate limit. Transient outages back off from 30 seconds to a five-minute cap. Mesh status shows the publication refusal and scheduled retry; a failed registry read keeps a confirmed publication. `domios mesh status` includes this local account state (`account_mesh` in JSON).

## Admission and sign-out

Every paired edge carries a durable admission:

- **legacy** — exactly the pairs persisted before the account gate existed. The upgrade snapshots them once, atomically with the peer records, so already-paired machines (including peers still on an older version) keep their keys, access levels and working traffic without signing in and without a deadline. Pending requests, discovered rows and anything paired later never become legacy, and a removed legacy edge can never come back through this path.
- **adopted** — linked or adopted under the account. When both sides of a legacy edge are signed in to the same account, they adopt it in place over the existing authenticated channel, with no re-pair, key rotation or drop. Adoption is one-way: an adopted edge never falls back to legacy on outage, expiry or an invalid proof.

Adopted edges carry traffic backend-free until the earlier of both machines' original signed membership expiries (at most 3600 seconds, renewed around 1800 seconds and delivered peer to peer on announces). There is no grace period. Expiry blocks protected traffic but keeps the pair record and all local work; a verified renewal reopens it.

Explicit sign-out blocks every edge, legacy and adopted, before any cleanup: inbound and outbound requests, relay, RPC, remote-authenticated non-`/api/mesh` routes and already-open streams. The pair records stay. The UI shows "Signed out — sign in to use the mesh". Signing back in to the same account restores the saved edges without re-pairing; a different account does not, because the first account a machine signs in with stays bound to its legacy edges. Upgrading without ever signing in is not a sign-out.

Other machines show **Signed out** with actions disabled after the registry confirms an explicit sign-out. Sign-in reports **present** again. Presence belongs to the retained account slot and is independent of endpoint withdrawal, Offline and membership expiry. A machine signing out offline still blocks traffic locally; other machines keep the last confirmed registry presence until a transition arrives.

A signed-in machine the account server refused a slot under the plan's machine limit is plan-locked: every edge, legacy and adopted, is blocked with the typed `plan_limit_locked` code, and new links refuse with `{"reason": "plan_limit_locked"}`. Its pairings and machine rows stay listed; an over-limit row opens the plans dialog. When the plan changes, the machine re-enrolls and traffic reopens without a restart. A release without pinned membership trust never enrolls, so it never locks and legacy edges keep working.

## Local REST availability

The local REST supervisor, not the mesh liveness detector, owns listener recovery. Listener/server exit produces a failed lifecycle event and exactly one newer generation; that replacement is advertised healthy only after socket ownership and a successful real `/api/health` response. Bind ownership failure and a still-bound listener with all four REST workers blocked have distinct diagnostics, so worker starvation does not cause listener-generation churn.

Replacement readiness is queued to the liveness owner with a monotonic recovery transition id, which restores local HTTP health, emits the existing `mesh-state-changed` resync signal, and broadcasts the refreshed announce exactly once. Distinct worker-stall recoveries within one listener generation receive distinct ids; a duplicate observation cannot claim another resync. Relay delivery keeps its existing at-most-once and structured-error behavior across the outage.

## Auth Model

Every paired peer has an Ed25519 keypair. Requests between peers are signed with the sender's private key and verified against the keys of paired peers whose edge is currently open.

- Inbound: a request is accepted only from a paired peer with open admission; a blocked edge is rejected exactly like an unknown key.
- Outbound: requests, sync, announces and streams go only to peers with open admission.
- Normal peer requests and relayed requests use the same `AccessLevel` policy.
- Secret-vault routes and mesh token minting are local-only at every access level.

## Security

- Ed25519 signed requests with 5-minute expiry and nonce (replay protection)
- New links need both machines' fresh online account authorizations bound to one handshake transcript; no other path creates a pair.
- A changed endpoint is metadata; it never replaces a peer's paired key.
- Disconnect removes only the local pairing and tombstones that identity locally; it does not cascade through third parties and frees no account machine slot.
- Only an owner authenticated with the local API token can lift a local revocation with `domios mesh unrevoke <instance-id>`; an explicit account link of that machine also lifts it.

### Peer access levels

Access is enforced on direct signed requests and relayed requests. Secret routes and mesh token minting are local-only at every level.

| Level | Permitted operations |
|---|---|
| Full | All peer API operations except local-only secret routes and token minting. Sync may refresh metadata of peers this machine already paired. |
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

The default **Open** action never sends a remote daemon token to the browser. It opens the local Web UI entry origin with the local token in a URL fragment and a paired peer ID. The browser removes that fragment from history, resolves the peer name from the authenticated local peer list, and sends API traffic through the signed Mesh relay. Relay authorization uses the same policy as a direct signed peer request. REST config responses redact `api.token`, and `/api/secrets/*` cannot be relayed even with Full access.

Domios 1.15.0 uses mesh relay protocol version 2. Version 1 peers are refused with an upgrade message; both peers must run Domios 1.15.0 or later. A relayed write (send, PTY input, spawn, plan or Team change) to a tab, workspace or Team locked above the plan limit is refused with `403 plan_limit_locked` and `bytes_written=false` before anything is written; reads stay allowed. Routed messages are framed by the receiving daemon, and relayed send bodies stay unwrapped until the receiving `/send` handler adds its local route frame, preventing double wrapping.

Whole-peer Web views scope each terminal subscription to `mesh:<peer>/<tab>`, just like remote desktop tabs. The gateway requires that composite identity for output streams; an unscoped request returns 400 even when metadata reads succeed. Each subscription keeps its own peer-qualified replay cursor and uses only the serving gateway's credentials. The initial standalone boot frame establishes the cursor namespace before output arrives, so reconnects resume rather than discard the saved byte offset. A matching relay protocol does not by itself prove every feature is available on an older application version.

`POST /api/mesh/relay-to` returns `404 peer_not_paired` when no paired record exists and `503 peer_unroutable` when the paired peer has no reachable address. Both reasons are stable constants.

A relayed send succeeds only when the TARGET confirms delivery — the relay hop's `ok` attests acceptance, the peer's body is the evidence, so `data.ok = false` on `POST /api/tabs/{id}/send` is a failure under an accepting envelope. Codes are stable constants; `bytes_written` is three-valued (`false` nothing landed, `true` landed but unconfirmed, absent unknown) and absent never means zero. The relay's send deadline outlives that route's readiness wait so the peer's answer decides the outcome; reads keep the short deadline. The sender's own leg is classified the same way rather than collapsing to a bare `502` with a stable `mesh_transport_*` code on the same envelope, and only a connect-phase failure claims `bytes_written=false`.

`--mesh PEER` accepts, in order: an exact instance id, a unique instance-id prefix (the short id `mesh status` prints, 6+ hex characters), the advertised name, or the configured name (the advertised name without its ` [dev-suffix]`). Stale offline duplicates of a name never hide the live record; when two ONLINE peers answer to one name the relay refuses with `409 peer_name_ambiguous` and lists every candidate with its `instance_id`, so re-run with an id. Every relayed result names the peer that answered: `--json` object results carry `answered_by {instance_id, instance_name}`, human output prints `answered by NAME (short id)` on stderr when the peer was addressed by anything but its exact id, and a failure carries `error.peer_id`.

The CLI reports a relayed send from the peer's own verdict, the same way a local send does: bytes that reached the target PTY (`bytes_written=true`) print as accepted with `delivery: pending` even when the peer's submit confirmation outlived the relay, and only `bytes_written=false` or unknown is a failure. Failures carry `error.class` — `deterministic` (the same request will be refused again: authorization, protocol mismatch, unknown tab, 4xx), `transient` (retry can help: busy tab, saturated window, 429/502-504) or `unknown` (a peer refusal that did not say). The retry hint follows the class, and only `transient` says retry. `send --mesh` prints only the result — the payload is never echoed. A `--to-shell` payload with a line over 4095 bytes to a non-Windows peer is refused before sending (`line_exceeds_tty_limit`, nothing written), because a canonical-mode tty truncates such a line while the write reports success.

A rename or address change announced while a paired peer is unreachable is remembered and re-announced when that peer next comes online.

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

The general `agent spawn --mesh PEER --role Worker` path remains task-less and does not create an origin attempt. Use plan-bound dispatch for accountable work; use task-less mesh spawn only for exploratory work without a plan assignment.

1. Spawn the remote Worker so its report target is the mesh peer that owns the plan:

   ```bash
   domios agent spawn claude --mesh PEER --role Worker --cwd <peer-workspace-path> --task-file <local-brief-path>
   ```

   A brief is authored prose, so it travels as a file. `--task "<brief>"` is refused once the text is multiline or over 240 characters: your shell would have expanded it before the CLI ran, and a remote Worker cannot be handed a silently truncated assignment. `--task-file -` with a single-quoted heredoc works the same way.

   Mesh spawns require `--cwd` to name a workspace directory on the peer; the peer refuses one that is not an absolute, existing directory there. Git Bash rewrites POSIX arguments such as `/srv/project` into Windows paths before the CLI runs, so pass a POSIX `--cwd` with `MSYS_NO_PATHCONV=1` set. A `--task-file` path is read on the caller machine and its contents are sent; the local path is never sent to the peer.

   This is a **supported task-less Worker shape**, including for work that changes source on the peer: it owns no task because none can exist for it, and it declares no `--execution-intent`. Keep `--role Worker` — that is the field the peer resolves the Worker doctrine from, including the rule that every turn ends in exactly one `domios agent report`. A roleless spawn is accepted too and loses that doctrine silently, which on a remote machine means the work never reports home. `--execution-intent mutating` is still refused without a task binding, on the peer's own daemon; the mesh has no binding to give it, so leave the flag off.

The general mesh spawn path remains available for task-less work; it does not participate in the origin task/report/merge lifecycle above.

## Peer API compatibility

Remote CLI refusals (`ok:false`) and RPC errors exit nonzero and print the peer's reason, including with `--json`.

Utility shell deadlines require an agent running locally on the target peer to own expiry reporting and cleanup. Remote `tab new --timeout`, `tab timeout`, and `tab restart --timeout` are refused. Ask an agent running on that peer to create or manage the shell with `--timeout`; do not omit a required QA/build deadline to bypass the refusal.

Before a relayed CLI operation, the caller reads the peer's authenticated `/api/identify` response. Peers older than 1.7.8 use the pane-era wire contract, so the caller translates tab routes and identifier fields at the relay boundary and normalizes the response back to the current tab contract. The preflight and operation use distinct signed nonces. This adapter does not invent endpoints or features absent from the older peer.

Recurring delayed-message create and edit operations require `delayed_recurrence` in the target's advertised identify `features`. The origin refuses when the feature is missing or no feature list is advertised, naming the peer and its reported version and directing the user to update the peer or send a one-shot. One-shot delays remain available to older peers.

## Bridge migration

Moving a Telegram bridge between peers (CLI, orchestration steps, secret locality, flags, exit codes): see `domios docs MESH-BRIDGE`.

## Mesh-disabled gate (503)

When `config.mesh.enabled = false`, the auth middleware short-circuits every `/api/mesh/*` path (other than `/api/mesh/ping`) with `503 Mesh networking is disabled`. The same 503 also fires for any caller authenticated via the mesh signed-token path — so other peers attempting to relay into us also get rejected, not just the local UI. The local instance Bearer credential keeps working for non-mesh routes regardless.

**Disabling mid-session** (PATCH `/api/config mesh.enabled=false`) additionally marks every paired peer `online = false` and emits `mesh-state-changed` so the UI updates immediately. Re-enabling restores the routes; peer state picks back up on the next ping tick. Mesh off rejects remote access, though outbound health pings to paired peers may continue.

Peer reachability updates automatically and emits `mesh-state-changed`.

## Related documents

Service registry, health, inventory fields, and peer endpoints: `domios docs MESH-SERVICES`.

Mesh inventory is target-authoritative: machine-qualified rows carry provenance and capabilities derived from authenticated access, target policy, and the target route. Cached rows stay inspectable, but stale, offline, partial, denied, or protected mutations stay disabled with their reason. Daemons are registry entries, never controllable processes. Schedule actions appear only when granted. Remote or destructive actions name the target, await and announce its outcome, then refresh; failures never become optimistic local success.

Inventory opens as a full-screen searchable List beside the existing Mesh network graph. One shared mode bubble switches between the List and the graph of connected computers. Keyboard navigation preserves resource identity and focus; peer detail separates Services and Schedules and labels cached, offline, denied, and partial states.

<!-- revision: 63 -->
