# Integration configuration

Configuration keys for messaging, hooks, transcription, and secret-backed integrations.

## Integrations

External service integrations are configured under `[integrations]`.

### Telegram Bridge

Connect a Telegram bot to the General Agent. Messages are routed bidirectionally:
- **Inbound** (Telegram to Agent): Messages arrive as `[Telegram: user_id] text` in the PTY. Rapid messages within 300ms are debounced into a single delivery. A typing indicator is sent on every inbound message.
- **Outbound** (Agent to Telegram): The agent sends replies via CLI: `codeterm bridge send --to USER_ID --service telegram "text"`. Without `--append`, always sends a new message. Use `--append` to concatenate text to the previous message (auto-inserts `\n\n` paragraph break, edits in-place within 60s window). Works from any tab, not just the General Agent.

```toml
[integrations]
telegram_enabled = false
telegram_allowed_users = []     # Telegram user IDs as strings, empty = allow all
```

**Setup:**
1. Create a bot via @BotFather on Telegram, get the token
2. Save token to `~/.codeterm/secrets/telegram.token` (Windows: `%APPDATA%/codeterm/secrets/telegram.token`)
3. Enable: `codeterm config set integrations.telegram_enabled true`
4. Optionally restrict users: `codeterm config set integrations.telegram_allowed_users '["123456789"]'`
5. Restart CodeTerm (hot-reload not yet supported)

Connected services are shown as icons next to the tab title in the sidebar (`connected_services` field on the tab object).

Settings UI: Settings > Integrations > Telegram (toggle + allowed users list).

### Discord (coming soon)

```toml
discord_enabled = false
discord_allowed_users = []
```

### Webhooks (integration bridge)

```toml
webhook_enabled = false
webhook_port = 7687
```

### Pre-forward text hooks (since v1.2.0)

Generic intercept point for inbound bridge messages BEFORE they reach the General Agent tab. An external daemon receives the message via HTTP and decides one of three outcomes: forward unchanged, replace (daemon already responded — bridge skips the tab), or annotate with metadata that the agent can act on.

Same shape as `voice_transcribe` (health-cache + timeout + fallback) but routed for text/command messages.

```toml
[integrations.text_hooks]
enabled = false
daemon_url = "http://127.0.0.1:7892"
timeout_ms = 2000
health_check_ttl_secs = 60
fallback_on_error = "forward"      # "forward" | "drop"
```

**Daemon protocol:**

```
GET  /healthz                      → 200 "ok"

POST /pre_forward
Body: {
  "text":         "turn on the lights downstairs",
  "channel":      "telegram",
  "user_id":      "123456789",
  "msg_id":       "12345",
  "tab_id":      "<target tab>",
  "attachments":  [{"kind": "voice", "path": "/tmp/...", "filename": "..."}]
}

Response (one of):
  {"action": "forward"}                            // standard pass-through
  {"action": "replace",
   "respond": [
     {"channel": "telegram", "to": "123456789",
      "text": "done — lights are on downstairs"}
   ]}                                              // bridge sends reply, skips GA
  {"action": "forward_with_metadata",
   "metadata": {"router_label": "light_cmd",
                "extracted_action": "lights.sh downstairs on"}}
                                                   // GA gets text + [Hook metadata: …] line
```

**Use cases:**

- **Trivial command shortcut** — regex first, then Groq llama-3.1-8b (~65ms): 80%+ of TG commands resolved in <100ms instead of 2-5s Claude latency, no token burn.
- **FAQ/auto-reply** — embedding match against curated answers + cached weather/time API.
- **Rate-limit / debounce** — drop duplicates within an N-second window without touching agent prompts.
- **Channel routing** — annotate mesh-relay pings as routine in metadata so GA logs them and moves on.

**Safety / guarantees:**

- Bridge does NOT execute user code. Daemon is a separate process the user runs (parallel to `voice-transcribe`); bridge just talks HTTP.
- Health-cache TTL prevents pummeling a known-down daemon during inbound bursts.
- `fallback_on_error = "forward"` (default) means the message reaches GA when the hook is unreachable, broken, or times out — fail-open. `"drop"` is fail-closed (silence on hook error).
- For `replace`, only `channel: "telegram"` is wired today. Mesh / Discord routes will be added when those bridges land replies.
- The hook only sees `service`-prefixed messages — relayed mesh /send and webhook events bypass it for now (they take a different inject path).

**Setup:**

1. Run a daemon listening on `daemon_url` that handles `GET /healthz` and `POST /pre_forward`.
2. `codeterm config set integrations.text_hooks.enabled true`
3. `codeterm config set integrations.text_hooks.daemon_url http://127.0.0.1:7892` (or wherever your daemon listens)
4. Restart bridge (`codeterm bridge restart`) — picks up the new config.

### Secrets

Pluggable secret store for the `codeterm mem secret` command family and the Settings → Secrets UI. Configure the active backend, (for Bitwarden) the default org/collection that new items land in, and (for Bitwarden) the server URL plus optional auto-unlock.

```toml
[integrations.secrets]
backend = "local"                # "local" (default), "bitwarden", or "mock"
default_organization = ""        # Default org id for new items (Bitwarden); empty = personal vault
default_collection = ""          # Default collection id for new items (Bitwarden)

[integrations.secrets.bitwarden]
server_url = "https://vault.bitwarden.com"   # Public vault, or self-hosted Vaultwarden URL
persist_master_password = false              # Auto-unlock after daemon restart by saving the master password in the keychain alongside the session token. Off by default.
```

Backends:
- `local` (default) — one entry per secret in the OS keychain (macOS Keychain / libsecret / Windows Credential Manager) under service `codeterm-secrets`. A non-secret index of names + ids + folders + timestamps lives at `~/.codeterm/secrets/local-index.json`; values are never written there. Offline, no external service.
- `bitwarden` — shells out to the official `bw` CLI. Install with `brew install bitwarden-cli` (macOS), `npm i -g @bitwarden/cli` (Linux/cross-platform), or `scoop install bitwarden-cli` (Windows). When `bw` is missing the Settings → Secrets page shows the install hint, an "Install Bitwarden CLI" button, and the CLI returns exit code 6 (backend unavailable). Supports `vault.bitwarden.com` and self-hosted Vaultwarden via `server_url`.
- `mock` — in-memory only. Anything you save is lost on daemon restart. Use for dev / screenshots / tests.

The unlocked Bitwarden session token is stored in the keychain under service `codeterm.secrets.bitwarden` (account `session`). With `persist_master_password = true`, the master password lives in the same keychain (account `master_password`) so the daemon can auto-unlock after a restart — opt-in trade-off because that's one more secret on the same disk surface as the session. In dev builds the "keychain" is a `0600` JSON file at `~/.codeterm-dev/keychain/<service>.json` (signed-binary-changes-on-rebuild make the OS keychain unusable); release builds use the real OS keychain. On Linux without libsecret the keychain probe fails and the backend reports `Unavailable` — we never silently fall back to plaintext on disk in production builds.

**Secrets do not cross the mesh.** `codeterm --mesh PEER mem secret …` exits with code 7 and a clear error. The vault is local to the machine that holds the keychain.

Full secret-store reference, including CLI, storage internals, error mapping, and plugin authoring: `codeterm docs SECRETS`.

Settings UI: Settings > Secrets — backend picker, server URL + Apply (Bitwarden), Install Bitwarden CLI button (when `bw` missing), login (password or API key), Remember-master-password toggle, default scope, recently saved items, lock / sign-out.

<!-- revision: 2 -->
