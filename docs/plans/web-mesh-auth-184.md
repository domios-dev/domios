# Web-UI cross-mesh peer switch — route through the mesh relay (GH #184)

> Owner: Dante (orchestrator), from Andrey via BLACKSTAR 2026-06-20. In the web UI,
> switching the active machine to a mesh peer (e.g. ROG) fails with an auth/token error.
> Mandate: clean architecture, no band-aids. I own the design + delegate. Tracking #184.
> Branch `plugins-rethink`.

## Root cause — it's transport/reachability, NOT token plumbing
The shipped design is **direct browser→peer re-point**: on switch, `WebPeerPicker`
(`src/components/WebPeerPicker/WebPeerPicker.tsx:94`) calls the LOCAL daemon
`GET /api/mesh/peers/{id}/web-token`; the local daemon contacts peer B directly
(`src-tauri/src/api/mesh_routes/mod.rs:142-222`, server-to-server over `http://{peer.ip}:{port}`),
B mints a short-lived in-memory token (`mod.rs:224-249`, TTL 1h), and the client stores it
per-host + `setApiHost(B)` + reloads (`webApi.ts:88-92`) — re-pointing the whole SPA at B.

The token flow is internally consistent. It fails because the **browser cannot reach B**:
- **Reachability (primary):** the mint succeeds from daemon A's vantage, but the returned
  `browser_host` (`mod.rs:201`, derived from B's own addresses) is handed to the BROWSER —
  a phone/remote machine that can reach A's HTTPS front but has NO route to B's raw
  LAN/tailscale IP:port. The browser's first call to B fails at the network layer →
  surfaces as "token rejected" / "could not switch" (`WebGate.tsx:36`,
  `WebPeerPicker.tsx:107`) — reads as an auth error.
- **Mixed content:** HTTPS SPA (tailscale-serve on A) + B with no `https_url` → the
  HTTPS-origin browser blocks the cross-origin `http://B` fetch. Same symptom.
- **Fragility:** B's token is peer-local, in-memory, 1h TTL → silent rejection on B
  restart / expiry.
Exact wrong assumption: `mesh_routes/mod.rs:201` assumes the browser shares daemon A's
reachability to B. It doesn't.

## Fix — proxy remote requests through the local daemon's mesh relay
The mesh ALREADY has a complete authenticated peer proxy the web switch should ride but
bypasses: **`POST /api/mesh/relay`** (`src-tauri/src/api/mesh_routes/relay.rs:205`) —
self-authenticating (signed mesh token), SSRF-hardened (`validate_relay_path`),
access-gated, forwards any `/api/*` to the target peer's loopback. It is exactly what the
CLI `--mesh PEER` commands use.

Shape:
- Browser stays pointed at the LOCAL daemon (`apiHost()` = self); keep using its existing
  LOCAL token (already works). Do NOT re-point at B; do NOT mint/hand a B-scoped token to
  the browser.
- When a peer is the active target, route `webFetch`/`webJson` (`webApi.ts:100-151`) as
  `POST /api/mesh/relay` to the LOCAL daemon with `{target_peer_id, method, path, body}`.
  The local daemon relays to B over the already-paired signed channel and returns B's
  response.
- SSE stream (`webEvents.ts:48`): the relay doesn't stream today. Either add a streaming
  relay variant, OR fall back to the existing polling tick (`webEvents.ts:96`) for remote
  peers until streaming relay exists. (Start with poll-fallback; streaming relay is a
  follow-up.)

Why this is the right architecture:
- **Single reachability assumption:** the browser only ever reaches the daemon that served
  it (A) — which it provably can. B only needs to be reachable from A — which it is (that
  IS "paired"). Kills both failure modes outright.
- **No new auth surface:** reuses the relay's existing signature auth, access levels,
  nonce replay protection, SSRF guards.
- **Deletes fragility:** the direct-contact `mesh_peer_web_token` endpoint
  (`mod.rs:142-222`) + the per-host browser-token machinery (`tokenFor`/`setTokenFor` as a
  cross-mesh mechanism) + the `web_tokens` TTL cache can go — fewer tokens, no 1h re-auth,
  no B-scoped secret in the browser. (token-exchange = option b = what ships + fails;
  do NOT double down on it.)

## Tasks
- **W1 (TS + light Rust) — relay-route the web peer switch** [dispatch now]: add a
  relay-target mode to `webApi.ts` (`webFetch`/`webJson` → `POST /api/mesh/relay` when a
  peer is active); `WebPeerPicker` sets the relay target instead of `apiHost`+minted token;
  `webEvents.ts` poll-fallback for remote peers; DELETE `mesh_peer_web_token` + the
  per-host cross-mesh token machinery. Reuse `relay.rs` as-is. Verify the full peer-switch
  works in the web UI (rollacode-bot can drive a web→ROG switch repro).
- **W2 (Rust) — streaming relay variant** [follow-up]: SSE/stream over the relay so remote
  peers get live events, not just polling. Optional once W1 lands.

## Guardrails
- Reuse `/api/mesh/relay` exactly — do NOT add a parallel auth path; its signature auth +
  SSRF guards are the security boundary.
- Browser never holds a peer-scoped secret; only the local token. The relay is the trust
  boundary (paired peers).
- Clean rewrite (no external users) — delete the direct-repoint path rather than leaving
  both; no compat shim.
- rollacode-bot can repro the web→peer switch against ROG for verification.

<!-- revision: 1 -->
