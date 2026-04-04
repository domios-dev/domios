# CodeTerm Mesh Networking

## Core Principles

1. **Mesh = network with one King.** King is the sole authority. Crown JWT signed with Ed25519.
2. **One machine = one mesh.** Cannot be in two meshes simultaneously.
3. **New device in mesh trusts ALL peers** except those King marked Untrusted.
4. **King decides who is Untrusted** — propagates to all peers. Default is Trusted.
5. **Untrusted = one-way ban.** Cannot send commands to the network, cannot broadcast in discovery. BUT trusted peers CAN send commands TO the untrusted machine (one-way: network can write to it, it cannot write to network).
6. **No election.** Only manual crown transfer for now.

## Discovery — What an Unpaired Machine Sees

Discovery (mDNS/UDP/Tailscale) finds **meshes, not individual machines**. If 3 machines share the same `mesh_id` in their beacon — show one entry:

```
┌─────────────────────────────────┐
│ Available Meshes                │
│                                 │
│ 🔷 Home Lab (3 devices)        │
│    King: MacBook-Air            │
│    [Join Mesh]                  │
│                                 │
│ 🖥️ BLACKSTAR (standalone)      │
│    Not in a mesh                │
│    [Create Mesh] [Pair]         │
└─────────────────────────────────┘
```

Standalone machine (not in a mesh) — shown separately. Can pair 1-on-1 to create a new mesh.

## Flows

### Flow 1: Creating a Mesh (First Pair)

1. A and B see each other via discovery (both standalone)
2. A clicks "Pair" on B → PairingRequest sent to B
3. B sees request → clicks Accept
4. **B becomes King** (acceptor claims crown)
5. B generates crown keypair, mesh_id, signs JWT
6. Both machines paired + trusted (default)
7. Mesh created — now shown as a group in discovery

### Flow 2: Joining an Existing Mesh

1. C enables mesh → discovery finds "Home Lab (3 devices), King: MacBook-Air"
2. C clicks **"Join Mesh"** → request goes **ONLY to King** (not all peers)
3. UI on C: "Request sent. Waiting for approval from King (MacBook-Air). Time remaining: 2:00"
4. On King: notification + pending request. Options:
   - Click Approve in Settings UI
   - Tell General Agent: "approve the pending request from BLACKSTAR"
5. King approves → pubkey exchange → King sends introductions to all existing peers
6. **If C had its own crown (was in its own mesh) — crown is stripped.** C accepts crown_public from the new King.
7. C is now in the mesh, trusted by default. All peers know about C.

### Flow 3: Mesh Invites a Machine

1. King (or trusted peer) sees standalone machine D in discovery
2. Clicks "Invite to Mesh" → PairingRequest sent to D
3. D sees: "Mesh 'Home Lab' (King: MacBook-Air) wants to add you"
4. D clicks Accept → if D had a crown, it is **stripped**
5. D receives crown_public from King, becomes part of the mesh
6. King sends introductions + sync to all peers

### Flow 4: King Marks a Machine Untrusted

1. King via UI or General Agent: `mesh set-trust PEER untrusted`
2. TrustDeclaration(peer=X, level=Untrusted) signed with TTL
3. Broadcast sync → all peers receive and apply
4. X is now:
   - **Cannot** send relay commands to the network
   - **Cannot** broadcast in discovery (beacon disabled)
   - **CAN** receive commands from trusted peers (one-way: network writes to X, X cannot write to network)
5. Effectively a one-way ban from active participation

### Flow 5: Leaving the Mesh

1. Machine clicks "Exit Mesh" → notifies all peers
2. Clears local state (crown, peers, trust)
3. Remaining peers remove it from paired_peers

### Flow 6: Crown Transfer

1. King selects new King → transfer_crown
2. New King generates keypair, signs JWT with version+1
3. Broadcast → all peers update crown_public
4. Old King loses keypair, becomes regular trusted peer

## Trust Model

- **Default: Trusted.** Every peer in the mesh is trusted unless King says otherwise.
- King signs `TrustDeclaration` with **5-minute TTL** (Ed25519)
- Peers verify signature + expiry locally
- Expired → falls back to Trusted (safe default for the inverted model — King only marks Untrusted)
- Background refresh: King re-signs expiring declarations, peers request via `/api/mesh/trust/refresh`
- King offline + declarations expired = all peers return to Trusted (default)
- Only `Untrusted` declarations matter — absence of a declaration = Trusted

## Security: Fake King Protection

Problem: attacker broadcasts beacon with same `mesh_id` and different crown.

Protection:
- On join, the new peer receives `crown_public` from King (Ed25519 pubkey)
- All subsequent sync/trust verified against this pubkey
- If a beacon arrives with same `mesh_id` but **different crown pubkey** → ignored, warning logged
- Crown pubkey is pinned on first join (trust-on-first-use, like SSH known_hosts)

## Crown Conflict Resolution

- When joining a mesh, if the joining machine has its own crown → crown is **stripped**
- `update_crown_public()` already handles this: if incoming crown has higher version and different holder, local crown is relinquished
- On pairing: ONLY the acceptor can claim crown. Joiner/requester never claims.
- If both somehow end up as King (bug) → higher crown version wins, lower relinquishes

## Known Bugs (To Fix)

1. **Dual kings** — during pairing, both machines could claim crown. Fix: only acceptor claims, joiner always accepts foreign crown.
2. **Crown conflict on join** — joining machine's own crown must be stripped on receiving foreign crown_public.
3. **UI not reactive** — all mesh state changes must emit `mesh-state-changed`, frontend subscribes.
4. **Trust model inverted** — remove auto_trust_peer. Default = trusted. King only untrusts.
5. **Discovery shows individual machines** — group by mesh_id into mesh entries.
6. **Join goes to all peers** — should go to King only.
7. **No pending request UX** — need timer, status feedback on joining machine.

## Architecture

```
mesh/
  mod.rs            — MeshManager (facade) + broadcast_sync + refresh_trust + introductions
  identity.rs       — signing, authentication
  crown_manager.rs  — crown lifecycle (claim, transfer, accept, conflict resolution)
  trust.rs          — TTL trust declarations (King untrusts, default trusted)
  crown.rs          — Ed25519 crypto primitives
  types.rs          — all types
  peers.rs          — disk storage
  discovery.rs      — mDNS + UDP + Tailscale (group by mesh_id)
  pairing.rs        — pairing protocol
  transport.rs      — HTTP client for peer communication
```

<!-- revision: 1 -->
