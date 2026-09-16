# Web Access

Reach CodeTerm's full UI from your phone or any browser — over your own
Tailscale network, with no cloud middleman in the path.

## 1. What it is, and why Tailscale

CodeTerm serves its complete web UI at the daemon root. "Web access" is the act of
opening that UI from another device: your phone on the couch, a laptop in
another room, a paired machine across the world.

The transport is **Tailscale**, and that choice is deliberate:

- **No cloud middleman.** Traffic goes device → device across your private
  Tailnet (WireGuard). Anthropic, CodeTerm, and any SaaS relay see *nothing* —
  there is no server in the middle to log, inspect, or break.
- **Tailnet-only by default.** The browser UI URL is reachable only by machines
  signed into *your* Tailnet. It is not exposed to the public internet.
- **Free HTTPS with a real FQDN.** Tailscale Serve terminates TLS and gives you
  a clean `https://<machine>.<tailnet>.ts.net/` URL. No mixed-content
  warnings, no self-signed cert nags — phones treat it as a first-class HTTPS
  site (required for installable web-app / PWA behaviour).
- **Mesh-symmetric.** The same mechanism that lets *you* reach a machine lets a
  *paired peer* reach it. `codeterm web link --peer <name>` mints a link against
  that peer's own HTTPS FQDN and its own token — the privacy model is identical
  in both directions.

The token you paste at the connect screen is this machine's API token. Treat it
like a password; anyone with the token *and* Tailnet access reaches your UI.

## 2. Setup

### The serve command

Point Tailscale Serve at the port CodeTerm's API listens on:

```bash
tailscale serve --bg --https=443 http://127.0.0.1:<port>
```

- **`<port>` is CodeTerm's own API port:** `7685` for prod, `7686` for dev.
  Check it in **Settings → Network** (it's shown in every browser UI URL there).
- `--bg` keeps the proxy running in the background across reboots.
- `--https=443` gives you the clean URL with no port suffix
  (`https://host.ts.net/`). If 443 is already taken, use another port
  (e.g. `--https=8443`); the URL then carries that suffix
  (`https://host.ts.net:8443/`).

CodeTerm only advertises the HTTPS URL once Serve actually proxies to *this*
daemon's port — so the link it shows is never a dead 502.

### The `codeterm web link` helper

To get a ready-to-open URL with the token already embedded:

```bash
codeterm web link            # prints https://host.ts.net/#token=…  (or IP fallback)
codeterm web link --query    # ?token=… form — easier to type, but lands in logs
codeterm web link --peer mbp # link for a paired peer (its FQDN, its token)
```

The token is carried in the URL **fragment** (`#token=`) by default — fragments
never leave the browser, so the token stays out of server access logs, proxies,
and `Referer` headers. The embedded token is the stable `api.token` from
`config.toml`, not a short-lived mesh token, so the link keeps working.

## 3. macOS first-time gotcha: `tailscale cert`

On macOS the Tailscale client runs in a sandboxed container that does **not**
auto-provision a TLS certificate. Run this **once** before (or right after)
enabling serve:

```bash
tailscale cert <your-machine>.<tailnet>.ts.net
```

Without it, `https://…/` returns **000** (connection made, no TLS handshake)
and looks like serve is misconfigured even when it isn't. The cert is cached
after the first run. Linux and Windows provision certs automatically — this step
is macOS-only.

## 4. Port-collision warning

**Do not point `tailscale serve` at a port CodeTerm itself listens on for
anything other than its HTTPS front.** In particular, never set the serve
*HTTPS* port to CodeTerm's API port, and never aim serve at the *other*
CodeTerm instance's port:

- `7685` = prod API, `7686` = dev API.
- Pointing serve's HTTPS listener at one of these clobbers CodeTerm's own
  listener — which also carries the **mesh-relay** traffic. Mesh peers go dark.

`rollacode-bot` tripped exactly this: serve was bound to the daemon's own port
and silently broke mesh relay. The correct shape is **HTTPS on 443 (or 8443) →
proxy to `http://127.0.0.1:<api-port>`** — two distinct ports.

## 5. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `/` returns **404** | Your build shipped without the `dist/` bundle (the route is registered only when the bundle is present). Hit stable v1.4.0 installs. | Update to **1.4.1+**. |
| HTTPS URL returns **000** | macOS: no TLS cert in the sandboxed container. | Run `tailscale cert <fqdn>` once (see §3). |
| HTTPS URL is a **502** | Serve points at a different port than this daemon. | Re-run serve targeting the correct `127.0.0.1:<api-port>`. |
| Mesh peers went offline after setup | Serve bound to CodeTerm's own / sibling API port. | See §4 — use 443/8443, proxy to the API port. |
| No HTTPS URL in Settings, only IP links | Serve not configured for this port yet. | Run the §2 command; the tabl switches to HTTPS on refresh. |
| Token rejected | Wrong machine's token, or no `api.token` in config. | Each peer's link needs *that peer's* token; set one in Settings → Network. |

## 6. Quickstart (3 steps)

1. **(macOS only, once)** provision the cert for your Tailnet FQDN:
   `tailscale cert <your-machine>.<tailnet>.ts.net`
2. **Serve** (use your real API port, 7685 prod / 7686 dev):
   `tailscale serve --bg --https=443 http://127.0.0.1:7685`
3. **Open** the link from your phone:
   `codeterm web link` → tap the `https://…ts.net/#token=…` URL.

