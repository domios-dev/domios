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
5. Restart Domios (hot-reload not yet supported)

Connected services are shown as icons next to the tab title in the sidebar (`connected_services` field on the tab object).

Settings UI: Settings > General > Messengers (Telegram bot bridge toggle + allowed users list).

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

### Graph nodes (plugin declarations)

A plugin contributes graph nodes through `capabilities.graphNodes` in its manifest: media `converters` and typed `eventSources`. Both lists are optional, and a declared block must name at least one node.

```json
"capabilities": {
  "graphNodes": {
    "converters": [{ "from": "audio", "to": "text" }],
    "eventSources": [{
      "kind": "telegram.message",
      "title": "Telegram: new message",
      "params":  { "type": "object", "properties": { "chat_id": { "type": "string", "maxLength": 128 } }, "required": ["chat_id"] },
      "payload": { "type": "object", "properties": { "text": { "type": "string", "maxLength": 16384 } }, "required": ["text"] }
    }]
  }
}
```

- **converters** declare which media kinds (`text`, `audio`, `video`, `image`) the plugin turns into which (`from` differs from `to`, at most 16). The plugin exports `convert(request)` returning `{ text }`, `{ path }` (a file inside the request's `outputDir`), or `{ error }`. The host chains conversion steps across plugins from the declarations alone, so a plan never depends on which plugin serves a step. A `transcriber` capability is registered as `audio -> text` automatically.
- **eventSources** declare up to 16 event kinds per plugin (`^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`, at most 64 bytes, unique per plugin) with a 1-128 byte title and typed `params`/`payload` schemas. The schema subset is an object of scalar or one-level scalar-array properties (at most 32; `maxLength` <= 65536, `maxItems` <= 256) with optional `description` and `enum`; `$ref`, combinators (`oneOf`/`anyOf`), `patternProperties` and nested objects are refused at parse.
### Model endpoints and presets

`codeterm model-provider` routes to the installed provider declaring model configuration support. In the app, each such harness's plugin page has a **Custom model endpoints** section with the same operations plus edit and model discovery: a typed key is saved to the secret store (masked, never shown again), or a saved secret is copied into the endpoint's owned slot. The HTTP endpoint `POST /api/model-providers` accepts `?provider=<id>` to address one harness.

```bash
codeterm model-provider add --name local --kind openai-compatible --base-url http://localhost:1234/v1 --secret local-key --model model-id
codeterm model-provider list
codeterm model-provider test --name local
codeterm model-provider remove --name local
codeterm model-provider preset add --name demo --model local/model-id --prompt "Review the task" --temperature 0.2 --top-p 0.9 --steps 12
codeterm model-provider preset list
codeterm agent spawn opencode --preset demo
codeterm model-provider preset remove --name demo
```

For `add`, pipe the key from a secure source on stdin; `--secret` is a name, never the value. Repeat `--model` for several models, or omit it to discover models through a connection test. Kinds are `openai-compatible` and `anthropic`. The key lives in the Domios secret store and is injected into the launch environment; OpenCode config contains only `{env:NAME}` references. Endpoint removal deletes only the Domios-owned entry and its owned secret; foreign entries and secrets are preserved.

Connection tests have a ten-second failure deadline and return models or typed `timeout`, `unauthorized`, `connection`, or `invalid_response` errors. Configuration and secret-store refusals are also typed; returned errors omit keys and remote response bodies.

Presets map a name to a model, prompt and optional parameters (temperature 0–2, top-p 0–1, steps 1–10000). They are Domios-owned OpenCode agents; foreign agents cannot be overwritten or removed. Select a preset in New Session or with `agent spawn --preset`; conflicting model or agent launch arguments are refused. Recorded launch parameters preserve the preset on restart/resume. Provider and preset changes emit versioned catalogue events, so the picker and model chip refresh without a reload. OpenCode model variants appear as reasoning levels and travel with launch and live model switching; the chip identifies the live or restart/resume path.

### Secret store

Pluggable secret store for the `codeterm mem secret` command family and the Settings → Secrets UI. Configure the active backend, (for Bitwarden) the default org/collection that new items land in, and (for Bitwarden) the server URL. Auto-unlock is not a config key: a secret-backend plugin opts in through its manifest and the unlock request (see the secret-store reference below).

```toml
[integrations.secrets]
backend = "local"                # "local" (default), "bitwarden", or "mock"
default_organization = ""        # Default org id for new items (Bitwarden); empty = personal vault
default_collection = ""          # Default collection id for new items (Bitwarden)

[integrations.secrets.bitwarden]
server_url = "https://vault.bitwarden.com"   # Public vault, or self-hosted Vaultwarden URL
```

Backends:
- `local` (default) — a plaintext JSON file at `<data_dir>/.secrets` (`~/.codeterm/.secrets`; dev `~/.codeterm-dev/.secrets`), mode `0600` on Unix. Values are **not encrypted** and do **not** use the OS keychain; treat the file like an `.env`. A metadata-only index (names, ids, folders, timestamps) lives at `<data_dir>/secrets/local-index.json`. Offline, no external service.
- `bitwarden` — shells out to the official `bw` CLI. Install with `brew install bitwarden-cli` (macOS), `npm i -g @bitwarden/cli` (Linux/cross-platform), or `scoop install bitwarden-cli` (Windows). When `bw` is missing the Settings → Secrets page shows the install hint, an "Install Bitwarden CLI" button, and the CLI returns exit code 6 (backend unavailable). Supports `vault.bitwarden.com` and self-hosted Vaultwarden via `server_url`.
- `mock` — in-memory only. Anything you save is lost on daemon restart. Use for dev / screenshots / tests.

A secret-backend plugin keeps its persisted session and any remembered credential in its own host bucket (service `codeterm.plugin.<plugin-id>`), which uses the same plaintext local file as the `local` backend. Nothing here uses the OS keychain.

**Secrets do not cross the mesh.** `codeterm --mesh PEER mem secret …` exits with code 7 and a clear error. The vault is local to the machine that holds it.

Full secret-store reference, including CLI, storage internals, error mapping, and plugin authoring: `codeterm docs SECRETS`.

Settings UI: Settings > Secrets — backend picker, server URL + Apply (Bitwarden), Install Bitwarden CLI button (when `bw` missing), login (password or API key), default scope, recently saved items, lock / sign-out.

<!-- revision: 9 -->
