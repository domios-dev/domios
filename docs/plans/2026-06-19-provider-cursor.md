# provider-cursor — 4th core provider plugin (wraps `cursor-agent`) — FULL integration

Branch: `feat/provider-cursor` (off plugins-rethink). Mirror of `plugins/provider-codex` /
`provider-opencode`. Full parity (chat-view, session parse, detect, resume, usage), not a stub.

## Grounded facts (measured, not assumed)
- `cursor-agent` = PTY interactive agent CLI (like codex) → `transport: pty`. Installed at
  `~/.local/bin/cursor-agent`, logged in (g.andry90@gmail.com).
- **Auto-approve / trust**: `cursor-agent -f/--force` (also satisfies the per-dir Workspace
  Trust gate). This is the `--dangerously-skip-permissions` equivalent → `autoApproveFlags: ["--force"]`.
- **Session store is LOCAL sqlite** (opencode-like, NOT codex-JSONL):
  `~/.cursor/chats/<projectHash>/<sessionUuid>/store.db` + `meta.json`.
  - `store.db` schema: `blobs(id TEXT PRIMARY KEY, data BLOB)` + `meta(key TEXT, value TEXT)`.
    The transcript lives in OPAQUE BLOBs (~10 per 1-turn chat) — encoding must be decoded
    (Phase 2 research; likely msgpack/protobuf/gzip-json).
  - `meta.json`: `{schemaVersion, createdAtMs, hasConversation, updatedAtMs}` — use for
    enumerate + timestamps.
  - `<projectHash>` = hash of cwd (e.g. `/private/tmp/ct_cursor_probe` → `99e0288b…`); algorithm TBD (md5-ish).
  - `<sessionUuid>` = the session id (e.g. `1a7e8655-f97a-…`).
- **Clean wire format** via `cursor-agent -p --output-format stream-json` (one JSON object/line):
  - `{"type":"system","subtype":"init","session_id":"<uuid>","model":"Auto",…}`
  - `{"type":"user","message":{"role":"user","content":[{"type":"text","text":…}]},"session_id":…}`
  - `{"type":"assistant","message":{"role":"assistant","content":[…]},…}`
  - `{"type":"result","subtype":"success","duration_ms":…,"result":"…","usage":{inputTokens,outputTokens,cacheReadTokens,cacheWriteTokens},…}`
- **`cursor-agent ls` needs a raw TTY** (Ink) → fails non-interactively. Enumerate sessions
  via the FILESYSTEM (`~/.cursor/chats/`), never via `ls`.
- **Resume**: `cursor-agent --resume <sessionUuid>` (or `resume` = latest).
- **Models**: no list command; curated manifest set, default `auto`. In-TUI switch `/model {model}`;
  launch `--model <id>`. Full id list: cursor.com/docs/models-and-pricing.md.
- **Usage**: per-turn tokens in the stream-json `result.usage` (NOT a `/status` gauge like codex).

## Chat-view strategy decision
store.db blobs are opaque → the clean transcript source is **stream-json**. Two candidate paths
(Phase 2 picks the one that works):
- **(preferred) structured-chat via CLI replay**: `usesStructuredChat() = true` +
  `readStructuredChat(cwd, sessionId, cursor)` shells `cursor-agent --resume <id> -p
  --output-format json` (or reads+decodes store.db) to return `{messages, cursor}`. Mirrors
  opencode's structured path. MUST run off the async runtime (spawn_blocking) — see the
  hot-path discipline rule in AGENTS.md.
- **(fallback) blob decode**: reverse-engineer store.db blob encoding and read directly.
Decide empirically in Phase 2 (decode 1 blob; test `--resume <id> -p json` replay).

## Full provider surface (from the Explore checklist — grounded in codex+opencode)
Runtime (`src/plugin.ts` → `plugin.js`): detectFromTitle, detectFromOutput, screenHasTui,
isOutputNoise, hasEventMarkers/detectEvents, buildLaunchCommand, buildResumeCommand,
sessionExists, sessionCreatedMs, enumerateSessions, parseSessionMessages (search),
detectSessionModel, discoverModels, + EITHER (sessionJsonlPath+parseSessionDelta) OR
(usesStructuredChat+readStructuredChat), + hooks (checkIntegration / installIntegration /
uninstallIntegration / ensureIntegrationOnStartup) if cursor exposes a session-start hook,
+ parseUsage (from stream-json result.usage). Frontend (`src/index.ts`): ProviderPlugin
{meta, detect, statusOf→null, Renderer→raw, actions:[], noisePatterns, titlePattern}.

## Decomposition into worker tasks (sequential — one coherent plugin)
- **CW1 (Phase 1, foundation):** `plugins/provider-cursor/` scaffold — plugin.json manifest
  (spawn cfg, curated models default auto, modelSwitchFormat `/model {model}`,
  modelLaunchArgsFormat `["--model","{model}"]`, autoApproveFlags `["--force"]`, installer),
  src/plugin.ts (detect fingerprints from a captured cursor-agent TUI sample + buildLaunch/
  buildResume), src/index.ts, package.json, add `cursor` to plugins/channel.json. Build via
  `node scripts/build-plugin.mjs cursor`. Gate: tsc + the plugin test. Install into DEV +
  restart + VERIFY: `/api/agents/providers` lists cursor (transport pty, no kind), +new
  session shows Cursor, spawns `cursor-agent` (logged in → real run), `--force` + `--model`
  applied. ACCEPTANCE = the abstraction accepts a 4th provider (Andrey's core worry).
- **CW2 (Phase 2, chat-view + sessions):** decide chat path (CLI-replay structured vs blob
  decode), implement readStructuredChat OR sessionJsonl path, enumerateSessions +
  sessionExists + sessionCreatedMs (from `~/.cursor/chats/` + meta.json), detectSessionModel,
  detectSessionId (scan newest session dir for cwd-hash, or parse stream-json init),
  parseUsage (result.usage). Wire resume-across-restart.
- **CW3 (tests — first-class, Andrey's priority):** unit tests (detection fingerprints;
  transcript parse from real stream-json fixtures; resume command). Extend `evals/restore-matrix`
  to include `cursor` (RM_PROVIDERS) so session-restore is covered. Manual/Playwright oracle:
  chat renders, a sent message round-trips, a restored cursor pane shows history.

## Testing plan (the moments that MUST be closed)
1. **Registration**: cursor in `/api/agents/providers`, transport pty, surfaces in +new session & Settings.
2. **Spawn + run**: open cursor pane → cursor-agent launches with --force, accepts a task, replies.
3. **Model select**: `--model <id>` at launch + `/model` switch mid-session.
4. **Chat works**: chat-view renders the cursor transcript (user+assistant turns) from the session.
5. **Messages work**: a message sent from the chat composer reaches cursor-agent and the reply renders.
6. **Session restore**: kill+restart daemon → restored cursor pane keeps session id + chat history;
   `--resume <id>` re-attaches. Covered by extending evals/restore-matrix.
7. **Usage** (if surfaced): token counts from result.usage.
8. **Detection**: a manually-launched `cursor-agent` pane is detected as cursor (fingerprints).
Gate every worker: tsc + plugin test + `./build.sh test` + pre-push clippy/fmt. Verify on DEV
before prod. NO provider-id hardcoding in core (pure JS drop-in; parity_test.rs N/A — no native).

## Then prod
Once CW1–CW3 verified on dev (7686): install provider-cursor into prod (7685) plugins.

## Orchestration
Workers = native codeterm Worker panes parented to my pane (154cec62), each in its own
worktree off feat/provider-cursor; I gate+merge each into feat/provider-cursor. Watchdog =
`codeterm send --delay` self-reminders (native, not harness). Sequential: CW1 → gate →
CW2 → gate → CW3 → gate → dev verify → prod.

<!-- revision: 2 -->
