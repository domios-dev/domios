# Open Provider Shell — Design Spec

> Status: APPROVED (decisions frozen with Andrey 2026-06-18). Mainline:
> `plugins-rethink` (tip e250bd83 at spec time). Implementation plan:
> `docs/plans/2026-06-18-open-provider-shell-plan.md`.

## Goal

Generalise the existing **`chatBackend`** capability into an **open agent
shell**: a non-PTY chat pane whose plugin talks directly to a local LLM, runs a
real **agentic ReAct loop with tool execution** inside the plugin sandbox, is
parameterised by selectable **presets** (system prompt + model + params), and
makes its active **system prompt observable** as a special first message. The
first concrete instance is the **lmstudio** plugin, talking to LM Studio's
native v1 API. The shell streams tokens.

The core stays a generic relay — it asks capabilities, never identities. Each
shell plugin owns its own agent brain, so "other, smarter shell plugins" are
possible by construction.

## Frozen decisions

| Axis | Decision |
| --- | --- |
| Chat API (lmstudio) | LM Studio **native v1** `/api/v1/chat` (+ `/api/v1/models`, load/unload) |
| Tool-calling | **Text protocol** — model emits tool calls as text; plugin parses (works on any model/endpoint, no native function-calling needed) |
| Capability shape | **Extend `chatBackend`** (not a new capability); lmstudio + future open-shell plugins benefit |
| Preset scope (v1) | **Minimal**: `{ id, name, systemPrompt, model?, params? }` + a `defaultPreset` |
| Default system prompt | **Fetch + adapt** an open agent's prompt into `references/`, trim to a tight CodeTerm-aware default |
| Tool surface | **Curated typed set**: `exec`, `read_file`, `write_file`, `codeterm`, `mem_search`, `spawn_agent` — each mapped to an existing `host.*` |
| Streaming | Token streaming via a **new sync `host.fetch` streaming primitive** (job-id + chunked poll). Chat-UI partial render already exists. |

## What already exists (verified, file:line)

- **chatBackend trait** `src-tauri/src/chatbackend/mod.rs:39-75`
  (`open_session`/`send_message`/`pump`/`poll`/`close_session`/`list_models`);
  JS-only (no native parity), contract-tested via `call_json`.
- **JS adapter** `src-tauri/src/plugin/js_runtime/chatbackend.rs`; exports
  `openSession(ctx)`, `sendMessage`, `poll`, `closeSession`, `listModels`,
  optional `pump`.
- **Spawn + poll loop** `src-tauri/src/service/spawn_chatbackend.rs`
  (500ms loop, `pump()` then `poll()`, `sink.emit(pane_id, msgs)` line ~146).
- **"Open chat"** command `open_chat_backend_pane`
  `src-tauri/src/commands/pane.rs:539-556`; REST `POST /api/panes {chat_backend, config}`
  `src-tauri/src/api/pane_routes.rs:39-94`; spec
  `PaneCommand::ChatBackend { plugin_id, config }`.
- **Streaming render is ALREADY there.** `useChatSession.ts::mergeById`
  (`src/components/Terminal/useChatSession.ts:145-151`) keys by message `id` and
  **overwrites** on re-emit → same id + growing `content` = in-place partial
  render. PTY agents (claude/codex) stream exactly this way. chatBackend reuses
  the same `pane-chat-msg-{paneId}` event → **no chat-UI rewrite**. `ChatMsg`
  needs no partial/done/delta flag (`src-tauri/src/agent/chat_types.rs:126-167`).
- **Sync job-id + poll pattern** `host_compute.rs` (host.worker/host.agent:
  `start → {jobId}`, `poll(jobId) → {done, ...}`). The streaming `host.fetch`
  mirrors this shape.
- **`host.fetch` today** `src-tauri/src/plugin/js_runtime/host_net.rs:49-98` —
  `reqwest::blocking`, **full body buffered** (line 93), no streaming. This is
  the only real gap for token streaming.
- **Markers** `src-tauri/src/message_markers.rs` + `packages/chat-core/src/markers.ts`
  (parity), rendered as cards in `packages/chat-ui/src/MessageBubble.tsx`,
  detected in `src/lib/chatPrefixes.ts`. Kinds today: voice, plugin_config,
  photo, file, telegram, mesh, pane, report, hook.
- **Spawn param flow** CLI `cli/src/args/agent.rs` + `cli/src/cmd/agent.rs` →
  API `src-tauri/src/api/agent_routes.rs` → `SpawnAgentParams`
  `src-tauri/src/service/orchestrator.rs` → provider. No `system_prompt` field
  exists anywhere yet.
- **Current lmstudio plugin** `~/Developer/codeterm-plugins/lmstudio/src/plugin.ts`
  (176 lines): OpenAI-compat `/v1/chat/completions` + `/v1/models`, blocking,
  `stream:false`, no system prompt / presets / tools / loop. `hostApi:2`,
  `version 0.1.0`, network allow `localhost:1234`. Cursor-based `poll(sid, cursor)`
  with stable `lmstudio-N` ids.

## LM Studio native v1 API (per Andrey's docs)

```
POST http://localhost:1234/api/v1/chat
  { "model": "...", "system_prompt": "...", "input": "..." }   # stateful
GET  /api/v1/models                         # list
POST /api/v1/models/load | /unload          # model mgmt (JSON)
POST /api/v1/models/download                # BINARY → out of v1 scope (host.fetch text-only)
```

Native `/api/v1/chat` is **stateful** and supports **streaming** and
**system_prompt** as a first-class field, but NOT custom tools — which is fine,
we use the text tool-protocol. Multi-turn (tool results + follow-ups) uses LM
Studio stateful-chat continuation; the exact continuation field is confirmed
against the LM Studio docs in S5, with a fallback of re-sending assembled context
as `input`.

## Architecture

### The shell loop (lives in the plugin)

```
sendMessage(text):  enqueue user turn; kick the ReAct loop (async, advanced by pump)
pump(sid):          drive one step of the in-flight loop:
                      - stream the LLM call via host.fetch streaming (S0); append
                        growing assistant message (stable id) each chunk
                      - when the LLM message completes, scan it for tool blocks
                      - if tool blocks: execute each via host.* (curated tool),
                        append a tool_result message, continue the loop
                      - else: turn done
poll(sid, cursor):  return transcript delta (stable ids → in-place streaming)
```

Iteration cap (e.g. 8 tool rounds/turn) prevents runaway loops. The loop is
entirely plugin-side; the core only relays.

### Text tool protocol (default prompt teaches it)

The model emits zero or more fenced blocks:

````
```codeterm-tool
{"tool": "exec", "args": {"cmd": "codeterm pane list"}}
```
````

The plugin scans the completed assistant message for ```codeterm-tool fences,
parses JSON, runs the curated tool, and appends a `tool_result`. No fence =
final answer.

### Curated tool set (each → existing host.*; no new host hole except S0)

| Tool | Args | host.* mapping |
| --- | --- | --- |
| `exec` | `{cmd, cwd?}` | `host.exec` |
| `read_file` | `{path}` | `host.readFile` |
| `write_file` | `{path, content}` | `host.writeFile` |
| `codeterm` | `{args}` | `host.exec("codeterm " + args)` (explicit CLI intent) |
| `mem_search` | `{query}` | `host.mem` search |
| `spawn_agent` | `{provider, task, workspace?}` | `host.agent.spawn` / `host.worker.start` |

Each tool execution is gated by the plugin's granted permissions (confinement
rule #4). The set is closed in v1.

### Presets (plugin-owned)

In the plugin config (`config.plugins.lmstudio` / its `config.yaml`):

```yaml
defaultPreset: codeterm
presets:
  - id: codeterm
    name: CodeTerm Assistant
    systemPrompt: "<the fetched+adapted CodeTerm-aware default>"
    model: ""        # blank = whatever LM Studio has loaded
    params: { temperature: 0.7 }
  - id: rhymes
    name: Rhyme Bot
    systemPrompt: "You answer only in rhymes."
```

Edited via the existing settings UI (SchemaRenderer). `listPresets()` surfaces
them. Spawn-time `--system` overrides the preset's systemPrompt; `--preset`
picks one; no flag → `defaultPreset`.

### Spawn integration (unified)

`codeterm agent spawn <id>` learns that `<id>` may be a **chatBackend** provider
(by capability, not a hardcoded list): if so it routes to the chatBackend spawn
path, threading `system_prompt` / `preset` / `model` into the `openSession` ctx
envelope `{ paneId, config, systemPrompt?, preset?, model? }`. New CLI flags
`--system` and `--preset`; mirrored on `open_chat_backend_pane` / `POST /api/panes`.

### Observability — system prompt as a special message

New marker kind `system_prompt` (Rust `message_markers.rs` + TS
`chat-core/markers.ts`, parity-tested). On session start with a resolved system
prompt, the plugin emits a first `system_prompt` message; chat-UI renders it as a
distinct collapsible **markdown card** labelled "System prompt" (detected in
`chatPrefixes.ts`, rendered in `MessageBubble.tsx`). Tool-call and tool-result
messages already render (ChatMsg tool fields + CodetermCard) — no new work.

### Streaming primitive (S0)

```
host.fetch.stream(optsJson) -> { jobId }          # start a streaming GET/POST
host.fetch.streamPoll(jobId) -> { chunks: string[], done: bool, status?, error? }
host.fetch.streamClose(jobId)                       # cleanup
```

Implemented in `host_net.rs`: a background thread runs the request with
`reqwest` streaming (`bytes_stream`), pushes decoded chunks into a per-job buffer
keyed by `jobId`; `streamPoll` drains the buffer synchronously (mirrors
host.worker poll). **Confinement:** reuses the SAME network-permission allowlist
check as `host.fetch` — same egress, just chunked, so no new permission class
(audit per rule #4: confinement-equivalent to existing `host.fetch`).

## Must-not-break

- **chatPreprocessor** (`src-tauri/src/chatpreprocessor/` + `chatpipeline/`) — a
  separate send-time text stage, gated to PTY user turns; chatBackend panes are
  staged at their own transport. The system-prompt seed must NOT double-run text
  stages. Verify in S8.
- **Dreamer** (mem REM worker) — unrelated; do not touch.
- **Existing chatBackend contract** — new `openSession` ctx fields are OPTIONAL;
  a shell without presets/system prompt still works.
- **Dread NPC** (`codeterm-npc`) — a DIFFERENT plugin type (provider selection,
  not an open LLM shell). Out of scope. The optional ctx fields keep any
  chatBackend usage it may have backward-safe; verify its real type from
  `codeterm-plugins` during S5 and confirm no regression.

## Out of scope (explicitly future)

- State-machine / multi-step workflow orchestration inside the shell.
- Native function-calling (we use the text protocol).
- Binary model **download** UX (host.fetch is text-only; download stays in the LM
  Studio app). Model **load/unload** (JSON) are in scope.
- Report-routing harness for shell panes (a shell may call `codeterm agent report`
  via the `codeterm` tool, but no dedicated harness in v1).

## Acceptance (S8, live on dev, macOS)

1. Install lmstudio on dev; open a shell pane; the active **system-prompt card**
   shows the default preset.
2. With LM Studio running locally, the reply **streams** token-by-token.
3. Ask "list my panes" → the model emits a `codeterm-tool` exec → the plugin runs
   `codeterm pane list` → tool-call + tool-result render → final answer uses it.
4. `codeterm agent spawn lmstudio --system "You answer only in rhymes."` opens a
   shell whose system-prompt card shows the rhyme prompt and whose replies rhyme.
5. Switch preset → new system prompt takes effect on the next session.
6. chatPreprocessor, Dreamer, and `codeterm-npc` are unaffected.
7. Plugin installs cleanly from the channel and the docs explain how to use it.

<!-- revision: 1 -->
