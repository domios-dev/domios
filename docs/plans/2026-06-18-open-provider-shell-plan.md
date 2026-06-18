# Open Provider Shell — Implementation Plan

> **For workers (Codex unless noted):** one track = ONE worker on its own branch
> off `plugins-rethink`, STRICT TDD (red→green→commit; write the test if
> missing), **commit BEFORE reporting**. Role hint inline (role skills 404 in
> mem). Report `DONE <track>: <branch>, <gate>, <sha>` or `BLOCKED: reason`.
> No backticks in shell strings. Never spawn agents. Merge target:
> `plugins-rethink` (one mainline). Orchestrator (Opus) gates + merges each.
>
> Spec: `docs/plans/2026-06-18-open-provider-shell-spec.md`.

**Goal:** Turn `chatBackend` into an open agent shell (streaming + ReAct tools +
presets + observable system prompt); lmstudio is the first instance, talking to
LM Studio native v1; verify it installs and works live.

**Gates — codeterm-alpha:** `export CARGO_TARGET_DIR=/Users/kandrey/Developer/.psetup-target`
then `cd src-tauri && cargo fmt && cargo build && cargo test <module> && cargo clippy --lib -- -D warnings`
(clean for YOUR files; ~141 baseline rustc-1.95 errors are pre-existing noise) ·
`./node_modules/.bin/tsc --noEmit` for TS · CLI `cd cli && cargo test <module>`.
**Gates — codeterm-plugins (lmstudio):** in the plugin dir
`node ../scripts/build-plugin.mjs lmstudio` (or `npm run build:all`) ·
`npx tsx tests` / `node plugin.test.cjs` · `npx tsc --noEmit`.

**Dependency order:** `S0 → S5`; `S1 → {S2, S3, S4}`; `S6 → S5`; `S5 dep S0+S1`;
`S7, S8` last. Roots dispatchable now: **S0, S1, S6** (S6 = research/authoring,
no code dep).

---

## Track S0 — streaming `host.fetch` primitive — branch `ops-s0-fetch-stream`

**Role:** Rust plugin-runtime engineer (`src-tauri/src/plugin/js_runtime`).

**Files:**
- Modify: `src-tauri/src/plugin/js_runtime/host_net.rs` (add streaming fns)
- Modify: `src-tauri/src/plugin/js_runtime/host_api.rs` (bind `host.fetch.stream`/`streamPoll`/`streamClose`)
- Modify: `packages/plugin-sdk/src/*` (SDK types for the streaming API)
- Test: a unit/integration test against a local hyper/mock server (or a chunked stub)

**Contract (mirror the host.worker sync job-id+poll shape):**
```
host.fetch.stream(optsJson) -> { jobId }           // opts = {url, method, headers, body, timeoutMs}
host.fetch.streamPoll(jobId) -> { chunks: string[], done: bool, status?: number, error?: string }
host.fetch.streamClose(jobId) -> void
```
- A background thread runs `reqwest` with streaming (`bytes_stream`), decoding
  chunks (UTF-8 lossy) into a per-`jobId` buffer (a shared `Mutex<HashMap<jobId, JobState>>`
  in the sandbox/registry, analogous to the worker registry). `streamPoll` drains
  buffered chunks and reports `done`/`status`/`error` without blocking.
- **Confinement:** reuse the EXACT same network-permission allowlist check that
  `host.fetch` applies (`host_net.rs`) before starting the stream — no new
  permission class. Document in a one-line comment that this is egress-equivalent
  to `host.fetch`.
- Sync per step (QuickJS). No Promises.

- [ ] Step 1: failing test — `stream` returns a jobId; `streamPoll` yields chunks then `{done:true}`; a blocked (non-allowlisted) host returns an error like `host.fetch`. RED.
- [ ] Step 2: implement bg-thread streaming + per-job buffer + 3 binds + SDK types. GREEN.
- [ ] Step 3: gate + commit `feat(plugin): streaming host.fetch (job-id + chunked poll) (S0)`.

---

## Track S1 — chatBackend seam enrichment — branch `ops-s1-seam`

**Role:** Rust plugin-runtime + SDK engineer.

**Files:**
- Modify: `src-tauri/src/chatbackend/mod.rs` (extend open_session ctx contract; add list_presets)
- Modify: `src-tauri/src/plugin/js_runtime/chatbackend.rs` (JS adapter: pass ctx fields; `listPresets()` export)
- Modify: `src-tauri/src/service/pane_transport_chat.rs` (thread systemPrompt/preset/model into the open ctx)
- Modify: `packages/plugin-sdk/src/*` (ChatBackend SDK types: ctx fields + optional `listPresets`)
- Test: `call_json` contract tests in `chatbackend.rs` / `chatbackend/tests.rs`

**Contract:**
- `openSession(ctx)` envelope gains OPTIONAL `systemPrompt?: string`, `preset?: string`,
  `model?: string` (in addition to `paneId`, `config`). Backward-safe: absent → today's behavior.
- New OPTIONAL export `listPresets() -> [{ id, name, description? }]`; adapter returns `[]` if absent.
- Keep `listModels()` as-is.

- [ ] Step 1: failing contract test — a stub JS backend echoes `ctx.systemPrompt`/`ctx.preset` back through `openSession`; `listPresets` surfaces a fixture; a backend without it returns `[]`. RED.
- [ ] Step 2: implement ctx threading + `listPresets` adapter + SDK types. GREEN.
- [ ] Step 3: gate + commit `feat(plugin): chatBackend openSession ctx (systemPrompt/preset/model) + listPresets (S1)`.

---

## Track S2 — unified spawn (agent spawn routes chatBackend) — branch `ops-s2-spawn` (dep S1)

**Role:** Rust core + CLI engineer.

**Files:**
- Modify: `cli/src/args/agent.rs` + `cli/src/cmd/agent.rs` (add `--system`, `--preset` flags; pass through; show shells in `agent providers` output, tagged by kind)
- Modify: `src-tauri/src/api/agent_routes.rs` (`SpawnAgentBody`: `system_prompt`, `preset`; **`list_agent_providers` also emits chatBackend shells**)
- Modify: `src-tauri/src/service/orchestrator.rs` + `src-tauri/src/service/spawn.rs` (`SpawnAgentParams` fields; **route by capability** — consult `chatbackend_registry.get(id)` vs `agent_registry`)
- Modify: `src-tauri/src/commands/pane.rs` (`open_chat_backend_pane`: accept system_prompt/preset/model) + `src-tauri/src/api/pane_routes.rs` (`CreateShellPaneBody`)
- Test: spawn-routing unit test + a discovery test (a registered chatBackend id appears in the providers listing tagged `shell`)

**Contract:**
- **Dynamic, never hardcoded.** `lmstudio` (and any third-party shell) is known
  ONLY because the plugin manifest declares `{id, capabilities.chatBackend:true}`
  and the host registers it by id (`hot_reload.rs:89-92` → `chatbackend_registry`,
  `.ids()`/`.get(id)`). S2 reads that registry; there is no `"lmstudio"` literal
  anywhere in core (architecture rule #1: core asks capability, never identity).
- **Discovery (the gap this track closes).** `codeterm agent providers` /
  `GET /api/agents/providers` (`list_agent_providers`) today lists ONLY
  `agent_registry.provider_ids()` (PTY agents). Extend it to ALSO enumerate
  `chatbackend_registry.ids()`, each entry tagged with a `kind` (`agent` | `shell`)
  so the CLI/UI can show "which agents/shells are available" — making third-party
  shells first-class, not UI-only. Each shell's display metadata comes from its
  manifest/describe (id, name, icon), NOT a hardcoded table.
- **Spawn routing.** `codeterm agent spawn <id> [--system TEXT] [--preset ID] [--model M]`:
  if `<id>` is in the chatBackend registry → spawn `PaneCommand::ChatBackend` whose
  `config` carries `systemPrompt`/`preset`/`model`; else the PTY agent path. No
  flag → plugin's `defaultPreset`. Same fields on `open_chat_backend_pane` + `POST /api/panes`.
- Spawn validation (the "available providers: …" error in spawn.rs/orchestrator.rs)
  must include shell ids too, so an unknown id message lists both kinds.

- [ ] Step 1: failing tests — (a) spawn with a chatBackend id routes to ChatBackend carrying system/preset; agent id unaffected. (b) a registered chatBackend id surfaces in the providers listing tagged `shell`. RED.
- [ ] Step 2: implement flags → body → params → capability routing + unified discovery (agent + shell) from the registries. GREEN; tsc; cli cargo test.
- [ ] Step 3: gate + commit `feat(agent): spawn + discover chatBackend shells via agent spawn/providers (S2)`.

---

## Track S3 — observability: system_prompt marker — branch `ops-s3-marker` (dep S1)

**Role:** Rust + React/TS engineer.

**Files:**
- Modify: `src-tauri/src/message_markers.rs` (+ `message_markers_tests.rs`) — add `system_prompt` kind
- Modify: `packages/chat-core/src/markers.ts` — mirror `markSystemPrompt`
- Modify: `src/lib/chatPrefixes.ts` — detect `system_prompt`
- Modify: `packages/chat-ui/src/MessageBubble.tsx` — render a collapsible "System prompt" markdown card
- Test: Rust parity (`message_markers_tests.rs`) + the TS marker round-trip test

**Contract:** `system_prompt(body)` marker (no attrs, payload = the prompt).
Rendered as a distinct collapsible card titled "System prompt", body as markdown,
with a one-line explanation that this is the shell's system prompt. (Tool-call /
tool-result / partial-stream rendering already exist — do NOT rebuild them.)

- [ ] Step 1: failing test — Rust build/parse round-trip for `system_prompt`; TS `buildMarker`/`parseMarker` round-trip. RED.
- [ ] Step 2: implement marker (Rust+TS) + detect + card render. GREEN; tsc.
- [ ] Step 3: gate + commit `feat(chat): system_prompt marker + observable card (S3)`.

---

## Track S4 — presets storage + settings UI — branch `ops-s4-presets` (dep S1)

**Role:** React/TS + plugin-config engineer.

**Files:**
- Modify: lmstudio `settings.schema.json` + `config.yaml` (presets list + `defaultPreset`) in `~/Developer/codeterm-plugins/lmstudio`
- Modify: the settings renderer if needed for an array-of-objects field — `src/components/.../SchemaRenderer*` (check what field types it supports; extend minimally if presets need an object-array editor)
- Test: a schema/render unit test for the presets field

**Contract:** the plugin's settings expose an editable `presets` array
(`{id,name,systemPrompt,model?,params?}`) + a `defaultPreset` select; writes go to
`config.plugins.lmstudio` via the existing settings-delivery seam. If SchemaRenderer
lacks object-array support, add the minimal field type (no broad refactor).

- [ ] Step 1: failing test — presets schema renders + writes the expected config shape. RED.
- [ ] Step 2: implement schema + any minimal renderer support. GREEN; tsc.
- [ ] Step 3: gate + commit `feat(plugin): lmstudio presets + defaultPreset settings (S4)`.

---

## Track S6 — default CodeTerm-aware system prompt — branch `ops-s6-prompt` (no code dep; feeds S5)

**Role:** Prompt engineer / research (Opus-class reasoning; can be done by orchestrator or a careful worker).

**Files:**
- Add (gitignored): `references/<open-agent>/` — fetched open-source agent prompt to study (check LICENSE; opencode/aider are permissive)
- Create: `~/Developer/codeterm-plugins/lmstudio/prompts/codeterm-default.md` — the adapted default system prompt text (shipped inside the plugin / referenced by the default preset)

**Contract:** a tight default system prompt that gives the shell: (a) what
CodeTerm is + the observability ethos, (b) the available CLI handles it can call
(`codeterm pane/agent/mem/...` — concise), (c) the **exact text tool protocol**
(```codeterm-tool fenced JSON) and the curated tool list + arg schemas from the
spec, (d) how to report / when to stop (iteration discipline). Editable by the
user; it is just the `defaultPreset.systemPrompt`.

- [ ] Step 1: fetch + read an open agent's system prompt into `references/`; note structure + license.
- [ ] Step 2: author `codeterm-default.md`; keep it tight; align the tool-protocol text EXACTLY with S5's parser.
- [ ] Step 3: commit `docs(plugin): CodeTerm-aware default system prompt for the open shell (S6)`.

---

## Track S5 — lmstudio plugin: native v1 + ReAct loop + tools + streaming — branch `ops-s5-lmstudio` (dep S0+S1+S6)

**Role:** QuickJS/TS plugin engineer (`~/Developer/codeterm-plugins/lmstudio`). The big one.

**Files:**
- Rewrite: `src/plugin.ts` (native v1 chat; ReAct loop in `pump`; text tool parser; curated tools; streaming via S0; presets via S1 ctx)
- Modify: `plugin.json` (permissions: keep network allow; bump nothing yet), `config.yaml`, `settings.schema.json` (consume S4 shape), `README.md`, `configHelp`
- Rebuild: `plugin.js` via the build script
- Test: `plugin.test.cjs` — fake host (fetch.stream stub, exec stub, settings) drives: openSession with systemPrompt seeds a system_prompt message; sendMessage streams an assistant message (growing id); a `codeterm-tool` exec block triggers host.exec and appends a tool_result then a final answer; iteration cap respected.

**Contract:**
- `openSession(ctx)`: resolve preset (ctx.preset || defaultPreset), apply
  ctx.systemPrompt override; emit the `system_prompt` first message; store session
  state (transcript, system prompt, model, LM Studio chat handle if stateful).
- Chat via native v1 `POST /api/v1/chat` `{model, system_prompt, input}`,
  **streaming** through `host.fetch.stream`/`streamPoll` (S0); accumulate chunks
  into the assistant message (stable id) each `pump` tick.
- On assistant completion, parse ```codeterm-tool fences; execute the curated tool
  (`exec`/`read_file`/`write_file`/`codeterm`/`mem_search`/`spawn_agent`) via the
  matching `host.*`; append `tool_result`; continue loop (cap 8 rounds/turn).
- `listModels()` via `/api/v1/models`; `listPresets()` from config; model
  load/unload via `/api/v1/models/load|unload` (optional glance action, nice-to-have).
- Multi-turn: use LM Studio stateful-chat continuation; confirm the exact field
  against LM Studio docs; fallback = resend assembled context as `input`.
- FIRST: confirm `codeterm-npc`'s real type from `~/Developer/codeterm-plugins` and
  ensure none of this regresses it.

- [ ] Step 1: failing tests against the fake host (see Test above). RED.
- [ ] Step 2: implement native-v1 streaming ReAct loop + tools + presets + system-prompt seed. GREEN. Rebuild plugin.js.
- [ ] Step 3: gate + commit `feat(lmstudio): open agent shell — native v1, streaming, ReAct tools, presets (S5)`.

---

## Track S7 — docs + UX — branch `ops-s7-docs` (dep S2,S3,S4,S5)

**Role:** Docs engineer.

**Files:**
- Modify: `docs/PLUGINS.md` (open-shell section: presets, system prompt, tools, how to spawn) — bump `<!-- revision -->`
- Modify: `docs/API.md` / CLI docs (new `agent spawn --system/--preset`; chatBackend routing) — bump revision
- Modify: lmstudio `README.md` + `configHelp` (how to point at LM Studio, pick/edit presets, what the system-prompt card is)

- [ ] Step 1: write the docs reflecting the merged behavior.
- [ ] Step 2: commit `docs: open provider shell — presets, system prompt, tools, spawn flags (S7)`.

---

## Track S8 — verify (orchestrator, live on dev macOS)

- [ ] `./build.sh dev --wait`; install lmstudio on dev (`--from-dir ~/Developer/codeterm-plugins/lmstudio`).
- [ ] Open a shell pane → system-prompt card shows the default preset.
- [ ] With LM Studio running, reply **streams** token-by-token.
- [ ] "list my panes" → `codeterm-tool` exec → `codeterm pane list` runs → tool-call+result render → final answer uses it.
- [ ] `codeterm agent spawn lmstudio --system "You answer only in rhymes."` → rhyming shell; card shows the prompt.
- [ ] Switch preset → new system prompt on next session.
- [ ] chatPreprocessor + Dreamer + `codeterm-npc` unaffected.
- [ ] Plugin installs cleanly; docs accurate. Then `git branch --no-merged plugins-rethink` audit.

## Self-review checklist
- Core asks capability, not identity (S2 routing). ✓
- New host surface (S0) reuses the existing network allowlist — no new confinement hole. ✓
- chatBackend ctx fields OPTIONAL — backward-safe for codeterm-npc + existing shells. ✓
- Streaming reuses the existing id-merge render — no chat-UI rewrite. ✓
- Tool protocol text in S6 matches the S5 parser EXACTLY (single source of truth). ✓
- Presets editable; system prompt observable; lmstudio installs + works (S8). ✓

<!-- revision: 1 -->
