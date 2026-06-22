# OpenCode Structured Prompt Handle Spike

> Worker spike on `task/ocsse`, 2026-06-20. Goal: prove opencode prompt handling through
> its local HTTP/SSE server, with the PTY used for display only.

## Fixed-port spawn

OpenCode `1.17.8` accepts fixed server flags on the interactive TUI command:

```bash
opencode --port 44234 --hostname 127.0.0.1
```

CodeTerm can pass these through the existing plugin path because
`plugins/provider-opencode/src/plugin.ts::buildLaunchCommand` appends `LaunchParams.args`, and
`src-tauri/src/service/orchestrator.rs::build_agent_command` forwards `SpawnAgentParams.args` into
`AgentLaunchParams`.

Proven dev spawn through the safe bench wrapper:

```bash
bash -lc 'source evals/_lib/lib.sh && dc agent spawn opencode \
  --title oc-fixed-port-spike \
  --cwd /Users/kandrey/Developer/wk-ocsse \
  --args="--port" --args="44234" --args="--hostname" --args="127.0.0.1" \
  --task "Say READY and wait."'
```

The spawned process listened on `127.0.0.1:44234`, and `GET /global/health` returned
`{"healthy":true,"version":"1.17.8"}`.

Spawn gap: there is no host-side `structuredPromptPort` field yet. The CLI works with repeated
`--args="..."`; plain `--args --port 44234` is rejected by clap as a top-level option. For product
adoption, add a capability-gated host allocation field, not a provider-id branch:

- provider capability: `supports_structured_prompts`
- launch params: `structuredPromptServer: { hostname: "127.0.0.1", port: number }`
- opencode plugin maps it to `--hostname <host> --port <port>`
- host stores `{ pane_id, provider_id, session_id, base_url }` after the session binding hook

## Event Stream

Subscribe:

```bash
GET http://127.0.0.1:44234/event?directory=/Users/kandrey/Developer/wk-ocsse
Accept: text/event-stream
```

The first event is `server.connected`. Prompt events are single-line SSE `data:` JSON objects.

### `question.asked`

Real event captured from the fixed-port dev-spawned opencode server:

```json
{
  "id": "evt_ee67be408001zcNMv6949CfSTS",
  "type": "question.asked",
  "properties": {
    "id": "que_ee67be408001WL6jVtNtkCVvgx",
    "sessionID": "ses_11984d20effeYwtPyq3SmqN61A",
    "questions": [
      {
        "question": "Which option should CodeTerm choose?",
        "header": "Spike",
        "options": [
          { "label": "Alpha", "description": "" },
          { "label": "Beta", "description": "" },
          { "label": "Gamma", "description": "" }
        ]
      }
    ],
    "tool": {
      "messageID": "msg_ee67bd93a001I6fwyCtZVa6JxA",
      "callID": "call_00_B2wCyaEQAcWsktpSEnEz1817"
    }
  }
}
```

Legacy reply endpoint proven:

```bash
POST /question/{requestID}/reply
Content-Type: application/json

{ "answers": [["Alpha"]] }
```

The API returned `200 true`, emitted:

```json
{
  "type": "question.replied",
  "properties": {
    "sessionID": "ses_11984d20effeYwtPyq3SmqN61A",
    "requestID": "que_ee67be408001WL6jVtNtkCVvgx",
    "answers": [["Alpha"]]
  }
}
```

OpenCode then proceeded and produced assistant text `Alpha`.

Session-scoped v2 route from OpenAPI, preferred for CodeTerm adoption:

```bash
POST /api/session/{sessionID}/question/{requestID}/reply
Content-Type: application/json

{ "answers": [["Alpha"]] }
```

Expected status: `204`.

### `permission.asked`

Real event captured after creating a session with `permission: [{ permission: "bash", pattern:
"*", action: "ask" }]` and prompting a harmless `bash` tool call:

```json
{
  "id": "evt_ee67caaaf002RZo3QdRsn8Fhno",
  "type": "permission.asked",
  "properties": {
    "id": "per_ee67caaaf001BSCXvdfm7TD8TR",
    "sessionID": "ses_1198373ddffe8oUv9RcM42yjL4",
    "permission": "bash",
    "patterns": ["printf OPENCODE_HANDLE_PROOF"],
    "metadata": {
      "command": "printf OPENCODE_HANDLE_PROOF",
      "description": "Print OPENCODE_HANDLE_PROOF string"
    },
    "always": ["printf *"],
    "tool": {
      "messageID": "msg_ee67ca056001Wxepdb3j2dour2",
      "callID": "call_00_n1CJjdrXWkuuGCu7baAS4753"
    }
  }
}
```

Deprecated session reply endpoint requested in the task and proven:

```bash
POST /session/{sessionID}/permissions/{permissionID}
Content-Type: application/json

{ "response": "once" }
```

The API returned `200 true`, emitted:

```json
{
  "type": "permission.replied",
  "properties": {
    "sessionID": "ses_1198373ddffe8oUv9RcM42yjL4",
    "requestID": "per_ee67caaaf001BSCXvdfm7TD8TR",
    "reply": "once"
  }
}
```

OpenCode then proceeded and produced assistant text `DONE`.

Session-scoped v2 route from OpenAPI, preferred for CodeTerm adoption:

```bash
POST /api/session/{sessionID}/permission/{requestID}/reply
Content-Type: application/json

{ "reply": "once" }
```

Expected status: `204`. `reply` is one of `once`, `always`, `reject`.

## Pending Prompt Mapping

Map SSE to CodeTerm `pending_prompt` without screen parsing:

- `question.asked` / `question.v2.asked`
  - `id`: `properties.id`
  - `session_id`: `properties.sessionID`
  - `kind`: `question`
  - `question`: if one question, `questions[0].question`; for multiple, keep sections in order
  - `header`: `questions[n].header`
  - `options`: `questions[n].options[].label` plus descriptions
  - `multi_select`: `questions[n].multiple === true`
  - response: ordered `answers: string[][]`

- `permission.asked`
  - `id`: `properties.id`
  - `session_id`: `properties.sessionID`
  - `kind`: `permission`
  - `question`: `metadata.description || metadata.command || permission`
  - `options`: `once`, `always`, `reject`
  - details: `permission`, `patterns`, `always`, `metadata`, `tool`
  - response: `response` on legacy endpoint or `reply` on v2 endpoint

The integration should be capability-gated:

```text
if provider.supports_structured_prompts() {
  subscribe to provider structured prompt handle
} else {
  fall back to generic/provider screen parsing
}
```

Core must never branch on `"opencode"`. The provider supplies the handle type and endpoints; PTY
remains display-only for these prompt classes.

## Verdict

Feasible. OpenCode’s local server is a gold-standard non-scraping handle for permission and question
prompts in interactive mode. The round-trip is proven end-to-end: SSE prompt event -> HTTP reply ->
`*.replied` event -> opencode continues.

<!-- revision: 1 -->
