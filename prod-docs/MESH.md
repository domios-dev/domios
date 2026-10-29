# Domios Mesh Networking

## Core Principles

1. **New links require the same account.** Link only machines enrolled in that account. No LAN broadcast, mDNS, UDP beacon, Tailnet scan, manual address pairing, pairing request or introduction.
2. **Paired edges need open admission** for traffic (see *Admission and sign-out*).
3. **AccessLevel** (Full/ReadOnly/AgentIsolated) is per-peer, locally decided per-machine, independent of account membership. Default: Full.
4. **Only `domios mesh disconnect` or account revoke deletes a pair.** Sign-out, expiry, outage and account switches retain it.
5. **One mesh per machine.**

## Adding a machine

With Tailscale connected and the API enabled, the local web UI/API listens on assigned Tailscale IPv4/IPv6 addresses even while signed out; native address changes update listeners. The Mesh card offers QR, Open and Copy only for health-proven listeners. Open carries the instance API token in a URL fragment; browser API access requires it. Sign-out or revocation blocks peer requests and handshakes. Disabling Mesh blocks peer traffic but retains the web listener. Signed-in machines publish their proven endpoint to the account.

Health-proven HTTP works without Tailscale Serve HTTPS. The card prefers verified HTTPS, tailnet HTTP, then LAN HTTP and labels the route. Loopback offers **Open on this machine**, without phone QR or Copy. HTTPS-dependent features require HTTPS. After Tailscale/Serve setup, `domios mesh refresh --json` re-detects transport, reconciles Serve, verifies/publishes web endpoints and resyncs the card. Read `transport.state` and `web_ui.endpoints` from its response or `domios mesh status --json` (cached local state, available signed out). Verify Open, Copy and QR for the proven endpoint before declaring setup complete. Access denial shows **Permission needed**, distinct from stopped/missing Tailscale.

Windows Defender may request incoming access on first use; macOS may prompt when its application firewall is enabled. Linux may need a firewall rule allowing the Domios port on Tailscale. Domios neither changes firewall rules nor listens on LAN addresses for this feature.

Both machines must be signed in to the same account, enrolled, registry-online and directly reachable (e.g. on one Tailnet). Select the account machine or run:

```bash
domios mesh link <installation-id>
```

The registry supplies the target's single published endpoint; no address is ever typed. Each side verifies the other's signed membership and installation-key possession over a transcript binding both installations, both Mesh keys, the account and two fresh nonces, then the receiving machine shows one approval request on the Mesh canvas and as an action-needed notification with Approve and Decline. The initiator shows Waiting for approval. Only Approve allows EACH side to obtain its fresh online link authorization and finish pairing. Decline, or ten minutes without an answer, cancels the request; the initiator offers Try again. Offline linking has no path: a cached machine list, a cached membership, an old request, a replayed or foreign proof, or a backend-offline participant cannot create a link. A refused free machine slot returns the typed `mesh.machines` refusal with `limit` and `active_count`.

Mesh pairing and peer calls require both daemons to run Domios 1.15.0 or later. A 1.14.9 peer receives a version error explaining that both peers must be upgraded.

Account enrollment and address publication wait for the server's retry time after a rate limit. Transient outages back off from 30 seconds to a five-minute cap. Mesh status shows the publication refusal and scheduled retry; a failed registry read keeps a confirmed publication. `domios mesh status` includes this local account state (`account_mesh` in JSON).

## Admission and sign-out

Every paired edge carries a durable admission:

- **legacy** — exactly the pairs persisted before the account gate existed. The upgrade snapshots them once, atomically with the peer records, so already-paired machines (including peers still on an older version) keep their keys, access levels and working traffic without signing in and without a deadline. Pending requests, discovered rows and anything paired later never become legacy, and a removed legacy edge can never come back through this path.
- **adopted** — linked or adopted under the account. When both sides of a legacy edge are signed in to the same account, they adopt it in place over the existing authenticated channel, with no re-pair, key rotation or drop. Adoption is one-way: an adopted edge never falls back to legacy on outage, expiry or an invalid proof.

Adopted edges carry traffic backend-free until the earlier of both machines' original signed membership expiries (at most 3600 seconds, renewed around 1800 seconds and delivered peer to peer on announces). There is no grace period. Expiry blocks protected traffic but keeps the pair record and all local work; a verified renewal reopens it.

Explicit sign-out blocks every edge, legacy and adopted, before any cleanup: inbound and outbound requests, relay, RPC, remote-authenticated non-`/api/mesh` routes and already-open streams. The pair records stay. The UI shows "Signed out — sign in to use the mesh". Signing back in to the same account restores the saved edges without re-pairing; a different account does not, because the first account a machine signs in with stays bound to its legacy edges. Upgrading without ever signing in is not a sign-out.

Other machines show **Signed out** with actions disabled after the registry confirms an explicit sign-out. Sign-in reports **present** again. Connected peers receive a signed transition notice and refresh the registry once automatically. Notices carry no row state, do not reopen traffic, and are deduplicated and rate-limited per sender. Presence belongs to the retained account slot and is independent of endpoint withdrawal, Offline and membership expiry. A machine signing out offline still blocks traffic locally; other machines keep the last confirmed registry presence until a transition arrives.

A signed-in machine the account server refused a slot under the plan's machine limit is plan-locked: every edge, legacy and adopted, is blocked with the typed `plan_limit_locked` code, and new links refuse with `{"reason": "plan_limit_locked"}`. Its pairings and machine rows stay listed; an over-limit row opens the plans dialog. When the plan changes, the machine re-enrolls and traffic reopens without a restart. A release without pinned membership trust never enrolls, so it never locks and legacy edges keep working.

## Local REST availability

The local REST supervisor owns listener recovery. A desktop REST generation reports `listening` after owning the socket, consuming a successful `GET /api/health` body and closing that connection. Exit emits a failed lifecycle event and creates exactly one replacement generation; bind ownership failure awaits an explicit port-release or restart event, never a timer. Status: `generation`, `phase`, `diagnostic`, `message`. Diagnostics: `listener_exited`, `bind_ownership_failed`, `workers_blocked` (bound listener whose four REST runtime workers cannot answer health); worker starvation never churns generations.

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
- Disconnect removes the pairing locally and sends the peer a signed removal bound to that connection. An offline peer learns it on its next authenticated contact. Both machines then offer Connect, which requires a new approval. No account machine slot is freed and third-party pairs are unchanged.
- Only an owner authenticated with the local API token can lift a local revocation with `domios mesh unrevoke <instance-id>`; an explicit Connect or Approve on that machine also lifts it.

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

Use plan-bound dispatch for accountable tasks; general `agent spawn --mesh PEER --role Worker` is task-less, with no origin attempt.

1. Spawn the remote Worker so its report target is the mesh peer that owns the plan:

   ```bash
   domios agent spawn claude --mesh PEER --role Worker --cwd <peer-workspace-path> --task-file <local-brief-path>
   ```

   A brief is authored prose, so it travels as a file. `--task "<brief>"` is refused once the text is multiline or over 240 characters: your shell would have expanded it before the CLI ran, and a remote Worker cannot be handed a silently truncated assignment. `--task-file -` with a single-quoted heredoc works the same way.

   Mesh spawns require `--cwd` to name a workspace directory on the peer; the peer refuses one that is not an absolute, existing directory there. Git Bash rewrites POSIX arguments such as `/srv/project` into Windows paths before the CLI runs, so pass a POSIX `--cwd` with `MSYS_NO_PATHCONV=1` set. A `--task-file` path is read on the caller machine and its contents are sent; the local path is never sent to the peer.

   **Task-less Workers may edit peer source**, but own no task and omit `--execution-intent`: the peer refuses `mutating` without a task binding. Keep `--role Worker` so the peer applies Worker doctrine, including exactly one `domios agent report` per turn. Roleless spawn is accepted but loses that doctrine and automatic reporting home.

Task-less spawn does not participate in the origin task/report/merge lifecycle.

## Peer API compatibility

Remote CLI refusals (`ok:false`) and RPC errors exit nonzero and print the peer's reason, including with `--json`.

Utility shell deadlines require an agent running locally on the target peer to own expiry reporting and cleanup. Remote `tab new --timeout`, `tab timeout`, and `tab restart --timeout` are refused. Ask an agent running on that peer to create or manage the shell with `--timeout`; do not omit a required QA/build deadline to bypass the refusal.

Before a relayed CLI operation, the caller reads the peer's authenticated `/api/identify` response. Peers older than 1.7.8 use the pane-era wire contract, so the caller translates tab routes and identifier fields at the relay boundary and normalizes the response back to the current tab contract. The preflight and operation use distinct signed nonces. This adapter does not invent endpoints or features absent from the older peer.

Recurring delayed-message create and edit operations require `delayed_recurrence` in the target's advertised identify `features`. The origin refuses when the feature is missing or no feature list is advertised, naming the peer and its reported version and directing the user to update the peer or send a one-shot. One-shot delays remain available to older peers.

Bridge migration between peers: `MESH-BRIDGE`.

## Mesh-disabled gate (503)

When `config.mesh.enabled = false`, the auth middleware short-circuits every `/api/mesh/*` path (other than `/api/mesh/ping`) with `503 Mesh networking is disabled`. The same 503 also fires for any caller authenticated via the mesh signed-token path — so other peers attempting to relay into us also get rejected, not just the local UI. The local instance Bearer credential keeps working for non-mesh routes regardless.

**Disabling mid-session** (PATCH `/api/config mesh.enabled=false`) additionally marks every paired peer `online = false` and emits `mesh-state-changed` so the UI updates immediately. Re-enabling restores the routes; peer state picks back up on the next ping tick. Mesh off rejects remote access, though outbound health pings to paired peers may continue.

## Related documents

Service registry, health, inventory fields, and peer endpoints: `domios docs MESH-SERVICES`.

Mesh inventory is target-authoritative: machine-qualified rows carry provenance and capabilities derived from authenticated access, target policy, and the target route. Cached rows stay inspectable, but stale, offline, partial, denied, or protected mutations stay disabled with their reason. Daemons are registry entries, never controllable processes. Schedule actions appear only when granted. Remote or destructive actions name the target, await and announce its outcome, then refresh; failures never become optimistic local success.

Inventory opens as a full-screen searchable List beside the existing Mesh network graph. One shared mode bubble switches between the List and the graph of connected computers. Keyboard navigation preserves resource identity and focus; peer detail separates Services and Schedules and labels cached, offline, denied, and partial states.

### Signed peer consent and notices (no local credentials)

| Method | Path | Body | Effect |
|---|---|---|---|
| POST | `/api/mesh/link/approval` | `{link_id, installation_signature, mesh_signature}` | Waits for the owner's Approve or Decline; no permit meanwhile. |
| POST | `/api/mesh/disconnect-notice` | `{from, to, link_nonce, signature}` | Removes only this exact connection. |
| POST | `/api/mesh/account-presence` | `{metadata_revision}` | Triggers one rate-limited registry refresh. |

<!-- revision: 72 -->
