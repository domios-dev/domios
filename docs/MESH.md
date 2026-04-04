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

## Architecture

```
mesh/
  mod.rs            — MeshManager (facade) + broadcast_sync + introductions
  identity.rs       — signing, authentication
  crown.rs          — Ed25519 crypto primitives (sign/verify requests, keypair generation)
  types.rs          — all types
  peers.rs          — disk storage
  discovery.rs      — mDNS + UDP + Tailscale
  pairing.rs        — pairing protocol (X25519 key exchange)
  transport.rs      — HTTP client for peer communication
```

<!-- revision: 2 -->
