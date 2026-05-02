# Mesh hot-reload — refresh peer addresses without restart

**Status:** design
**Source:** Andrey, 2026-05-02

## Problem

After pairing, each peer caches the other's `addresses: Vec<PeerAddress>` in
`paired_peers[id]`. The cache is populated at pair time (or via gossip /
introduction) and is never re-consulted: when our IP changes (network swap,
new Tailscale lease, Wi-Fi change), peers keep dialing the dead address. The
only fix today is to restart CodeTerm on the peer side so it goes through
mDNS / introduction again.

Concrete pain: BLACKSTAR has this Mac's old non-Tailscale IP. Outbound
(this → BLACKSTAR) works because we have BLACKSTAR's Tailscale IP locally.
Reverse (BLACKSTAR → this) breaks because BLACKSTAR dials the wrong addr.

## Goal

Make peers update their address caches live, without restart, with three
operator surfaces:

```bash
codeterm mesh refresh                 # re-detect local addrs, push to all paired
codeterm mesh refresh --mesh PEER     # ask PEER to do the same on its side
codeterm mesh announce                # push my current addrs to all paired (no re-detect)
codeterm mesh announce --to PEER      # push to one peer
codeterm mesh peer set-addr PEER ip   # manual override of our local cache for PEER
```

## Existing surface (re-used as-is)

- `discovery::detect_tailscale` — runs `tailscale ip -4`, sets `mesh.tailscale_ip`.
- `discovery::detect_local_ip` — UDP-bind trick, sets `mesh.local_ip`.
- `MeshManager.paired_peers[id].addresses: Vec<PeerAddress>` — the cache to update.
- `MeshManager.client_for(peer_id)` — picks an address (currently Tailscale first, else first).
- `PeerClient` — builds a fresh `reqwest::Client` per call. No long-lived
  pool, so address changes take effect immediately on next outbound.
- Pairing handshake (`PairingRequest`/`PairingAccept`) and `mesh_introduce`
  already exchange `PeerAddress` lists. We will reuse the `PeerAddress` shape
  and the signed-token auth mechanism rather than invent a new one.

## Wire format

One new endpoint, one new payload, signed with the same Ed25519 token the
relay/sync paths use.

```rust
POST /api/mesh/announce
Authorization: Bearer <signed-token>
{
  "instance_id":   "<sender>",
  "instance_name": "<sender's display name>",
  "addresses":     [{ "ip": "100.x.x.x", "is_tailscale": true }, …],
  "api_port":      7685
}
→ { "ok": true } | 401/403/410
```

The receiver:
1. Verifies the signed token belongs to a known paired peer.
2. Verifies `instance_id` in body matches the sender (no spoofing peer X's
   addresses while signing as peer Y).
3. **Replaces** `paired_peers[sender_id].addresses` and `.api_port` with the
   payload values (replace, not merge — stale entries must die).
4. Persists.
5. Logs old → new for observability.
6. Emits `mesh-state-changed` so the UI re-renders the peer panel.

## CLI dispatch

| CLI form                            | Local action                                              |
|-------------------------------------|-----------------------------------------------------------|
| `mesh refresh`                      | re-detect local addrs, then send `/announce` to every paired peer |
| `mesh refresh --mesh PEER`          | relay `POST /api/mesh/refresh` to PEER (PEER does the above on its side) |
| `mesh announce`                     | send `/announce` to every paired peer (no re-detect)      |
| `mesh announce --to PEER`           | send `/announce` to one peer                              |
| `mesh peer set-addr PEER ip[:port]` | overwrite OUR local `paired_peers[PEER].addresses` only   |

`mesh refresh` (local) is also exposed as `POST /api/mesh/refresh` so it can
be relayed over mesh by the existing `/api/mesh/relay` path. No new relay
plumbing needed.

## Address detection / selection

The plan keeps the existing `PeerAddress { ip, is_tailscale }` shape. We add
a tier helper used by `client_for`:

```rust
fn address_tier(addr: &PeerAddress) -> u8 {
    if addr.is_tailscale { return 0; }      // best
    if is_lan_private(&addr.ip) { return 1; } // 10.0.0.0/8, 172.16/12, 192.168/16
    2                                        // public (worst — last resort)
}
```

`client_for` picks `min_by_key(address_tier)`.

When building our **own** address list to announce (in `refresh` and
`announce`), we collect:

1. Tailscale IP if `tailscale_ip.is_some()` → push as `is_tailscale: true`.
2. LAN IP from `local_ip` if it's a private RFC1918 → push as `is_tailscale: false`.
3. Skip public IPs entirely. Internet-facing addresses for unsigned local
   peers are out of scope (and would need NAT traversal).

If both fields are empty after re-detection we still send the empty list —
peers will pin their existing cached addresses (they only replace when the
new list is non-empty; the receiver must guard).

## Authorization

Re-uses the same gate as the update feature:

- `POST /api/mesh/announce` — open to **any paired peer** (a peer telling us
  about itself is information we always want).
- `POST /api/mesh/refresh` — gated to `access_level == Full` when invoked via
  relay (hits `path_requires_full_access` in `mesh_relay`). Local invocation
  (sender == self) bypasses the gate.
- `POST /api/mesh/peers/{id}/addr` (set-addr) — local-only, no relay path.
  Manual override on this machine.

## Backward compatibility

- Peer running pre-feature build returns 404 from `/api/mesh/announce`.
  The caller logs a warning and continues — the peer will need a manual
  update one last time.
- `mesh refresh` doesn't fail the local re-detect step if the announce step
  fails on some peers; partial success is acceptable.

## Connection pool invalidation

Already handled by current architecture — `PeerClient::new_signed` builds a
fresh `reqwest::Client` on every `client_for` call. There is no shared pool
that would cache TCP sessions to the old IP. After
`paired_peers[id].addresses` is rewritten, the next outbound call dials the
new IP.

## Logging

Both local refresh and incoming announce log the old → new addresses so a
debug session can confirm the change took:

```
[mesh/announce] sender=<peer_id> old=[100.84.x] new=[100.99.y]
[mesh/refresh] local addrs old=[100.x:7685, 192.168.1.5:7685] new=[100.99.y:7685, 192.168.1.7:7685]
```

## Test plan

- **Unit (backend):**
  - `address_tier` ordering (Tailscale < LAN < public).
  - `is_lan_private` correctly classifies 192.168/172.16/10/8.
  - `apply_announce` (helper that mutates `paired_peers[id]`): full replace
    on non-empty list; preserves entry on empty list; rejects when sender
    instance_id mismatches token.
- **Loopback (manual):**
  - Manually rewrite `paired_peers[BLACKSTAR].addresses` to a bogus IP via
    `mesh peer set-addr` → confirm `client_for` returns the bogus IP.
  - `mesh refresh` against ourselves: paired self-only mesh isn't possible,
    so the loopback test is `mesh peer set-addr` + verifying selection
    changed. Remote announce will be exercised when BLACKSTAR is upgraded.

## Risks / open questions

- **Set-addr ergonomics.** A single bad command could brick the local view of
  a peer until the user runs `mesh refresh` again. Acceptable: this command
  is for a developer who already knows what they're doing.
- **Spoofing via signed token.** The body's `instance_id` MUST match the
  signed token's `from_instance_id`; otherwise a paired peer could rewrite
  another paired peer's addresses. Validation is in the announce handler.
- **Tailscale priority.** A peer running with Tailscale up but talking to a
  peer where Tailscale is down: today we silently skip Tailscale and use
  LAN. The new tier ranking keeps that behavior. A peer that loses
  Tailscale post-pairing won't have its Tailscale entry pruned until it
  announces — that's the whole point of this feature, and the user's job
  is to run `mesh refresh` after a network change.
