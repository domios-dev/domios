# chatBackend Context Engines + Watcher Mode — Spec

**Status:** draft for review
**Repos:** `codeterm-canvas` (host, SDK contract), `codeterm-plugins` (LM Studio plugin, shared engine lib)
**Branch:** `feat/chatbackend-watcher-mode`

## Problem

chatBackend Tabs (LM Studio today) support exactly one shape: an interactive
chat with unbounded rolling history. Two things are missing:

1. **Context discipline.** A long-lived chat Tab accumulates context forever
   (LM Studio `previous_response_id` chaining / assembled-context fallback).
   There is no notion of a bounded context window or a summarization policy,
   and no way to define a custom context-assembly pipeline.
2. **Autonomous observation.** There is no way to attach a cheap local model to
   a running orchestration as a read-only evaluator — something that
   periodically looks at the orchestrator, its managers, and workers, decides
   whether the system is progressing, and nudges it when it stalls.

Both are instances of one missing abstraction.

## Core model: two orthogonal axes

### Axis 1 — Context engine (the state machine)

Every chatBackend session runs a **context engine**: a state machine that,
given `(state, input)`, produces the exact context for the next LLM call, and,
given the LLM response, produces the next state.

- **`chat` engine** — the special case we already have, made explicit. State =
  message window. Transition = append message, then apply a **window policy**:
  bounded size + a summarization method. v1 policy: `top` (keep system prompt +
  last N messages). Declared extension point: `llm` (fold evicted messages into
  a running summary), others later.
- **`machine` engine** — the general case. State = arbitrary JSON blob
  (size-capped) + an immutable **charter** fixed at session creation. Each
  input is a *query into the machine*: context is assembled from
  `charter + state + input` (never from raw prior transcript). The response is
  parsed into a structured **verdict** that carries the next state.

Chat is literally a special case of the machine: its charter is the system
prompt, its state is the message window, its verdict is "the assistant
message". Modeling both under one contract is what makes the rest composable.

### Axis 2 — Interaction mode

- **`interactive`** — the user (and routed Tab messages) can send inputs.
  Today's behavior.
- **`watcher`** — read-only. Nobody can type into the Tab; inputs are produced
  exclusively by the host (the tick scheduler, or a routed system source). The
  Tab is a window into the machine: the transcript shows **what was sent into
  the context** for each request and **what the model answered** — full
  transparency, zero influence. A message *is* part of the state machine, so
  injecting one from outside is forbidden by construction.

The matrix:

| | `interactive` | `watcher` |
|---|---|---|
| **`chat`** | today's LM Studio Tab | read-only chat fed by host ticks |
| **`machine`** | user queries the state machine | **orchestration health watcher** (canonical case) |

## Canonical use case: orchestration health watcher

A `machine + watcher` session spawned as a **manager** under an orchestrator
(`role_profile = "watcher"` — the frontend Managers section already reserves
this profile; the backend currently strips it).

- **Charter** (creation-time, immutable): "Observe this orchestration group.
  Decide whether the system is progressing or needs a kick. Nudge stalled
  Tabs." Plus observation config and tick interval.
- **Tick loop:** the host fires a tick every `interval` (only when the previous
  tick finished). Each tick the host assembles an **observation snapshot** —
  the orchestrator Tab + its children (role, profile, title, agent status,
  last-activity timestamp), recent parsed chat tails, recent agent reports —
  **excluding the watcher itself and any other watchers**. Snapshot + charter +
  current state go into a single LLM request.
- **Verdict** (structured JSON out): status, one-line summary, next state blob,
  and zero or more curated actions.
- **Actions** are executed by the **host**, not the model, against an
  allowlist with guardrails (below).

Design principle: **push context, don't make the model pull.** Local models
are weak at multi-round tool loops; the host hands them everything in one shot
and asks for one JSON verdict back. (A charter flag may later enable the
existing curated tool loop for stronger models — out of v1 scope.)

## Contracts

### Engine + mode config (SDK, `packages/plugin-sdk/src/capabilities.ts`)

```ts
export type SessionMode = "interactive" | "watcher";

export type ContextEngineConfig =
  | { kind: "chat"; window?: { maxMessages?: number; policy?: "top" } }
  | {
      kind: "machine";
      charter: string;                  // immutable instruction, the machine's "program"
      observations?: ObservationConfig; // what the host injects per tick (watcher)
      tickIntervalSecs?: number;        // watcher only; host-enforced bounds
    };

export interface ObservationConfig {
  orchestration?: boolean;   // tab group snapshot (default true for watchers)
  chatTail?: number;         // last N parsed messages per watched tab (0 = off)
  reports?: boolean;         // recent agent reports to the orchestrator
}
```

`ChatBackendOpenSessionCtx` gains `mode?: SessionMode` and
`engine?: ContextEngineConfig`. Both ride the existing spawn-config channel
(`chatbackend_spawn_config` → openSession ctx → `_meta` line in the durable
JSONL), which already survives daemon restarts.

### Tick contract (host → plugin, fire-and-forget)

A new optional plugin export, mirrored as an optional `ChatBackendProvider`
trait method (default: unsupported), same pattern as `setModel`:

```ts
// Enqueue one machine query. MUST return immediately (like sendMessage);
// the existing pump/poll loop drives the streaming completion.
watcherTick?(sessionId: string, input: WatcherTickInput): void;

export interface WatcherTickInput {
  tick: number;                    // monotonic tick counter
  nowMs: number;                   // host clock (plugins avoid Date.now)
  state: unknown;                  // authoritative state blob (host-held)
  observations?: unknown;          // host-assembled snapshot, shape per ObservationConfig
}
```

**Never a synchronous LLM call.** The QuickJS VM is shared across all of a
plugin's sessions; holding it for a full completion would starve every other
Tab. `watcherTick` enqueues; `pump()` streams; `poll()` delivers.

### Verdict contract (plugin → host, via the transcript stream)

The plugin emits the finished evaluation as a typed transcript message —
`NormalizedChatMessage { type: "watcher_verdict", content: "<json>" }` — which
flows through the existing poll loop and structured sink (and is therefore
persisted to the durable JSONL for free: a built-in audit trail).

```json
{
  "status": "ok" | "attention" | "stalled",
  "summary": "one-line assessment",
  "state": { "...": "next state blob, echoed and updated each tick" },
  "actions": [
    { "kind": "nudge", "pane": "<pane_id>", "message": "..." }
  ]
}
```

The host parses tolerantly (fenced or naked JSON; the `host.toolcall.parse`
tolerance philosophy). A malformed verdict degrades to
`status: "attention"` with the raw text as summary and no actions — a broken
model can never act, only be seen.

### Action allowlist + guardrails (host-side)

v1 kinds: `nudge` (Messenger send to a Tab **within the watched group**),
`notify` (OS notification), `report` (agent report to the parent
orchestrator).

**Who sets what.** Tunable limits form a `WatcherPolicy` set by the
**spawner** (typically the orchestrator creating its watcher; the model can
never touch its own policy — verdicts write state, never policy). The **host
owns the bounds** and clamps whatever the spawner asks for:

```
WatcherPolicy {
  tick_interval_secs:   default 120,  clamp 30..3600
  nudge_cooldown_secs:  default 600,  clamp >= 60
  max_actions_per_tick: default 3,    clamp <= 5
}
```

Non-negotiable guardrails, all host-enforced regardless of policy:

- target must be inside the watcher's observed group (no arbitrary Tabs);
- the action-kind allowlist is not extensible by config;
- state blob size cap (16 KB) — oversized state is rejected keeping the
  previous state, with a logged warning;
- nudge messages are delivered with a watcher-origin marker so recipients see
  who kicked them.

### Transparency (the watcher UI contract)

Each tick the Tab transcript shows, in order:

1. a collapsible **context card** — exactly what was assembled and sent
   (`type: "context_request"`, rendered like the existing system-prompt card);
2. the streaming assistant reply (thinking + content, as today);
3. a **verdict card** — parsed status/summary/actions, with a status badge on
   the Tab.

"You can watch everything, you can change nothing."

## Who owns what (the abstraction boundary)

| Concern | Owner |
|---|---|
| Engine/mode **types**, tick + verdict contracts | `packages/plugin-sdk` (type-only, published to npm — stays type-only) |
| Engine **executor** (window policy, machine context assembly, verdict shaping) | **new npm package `@codeterm/chat-engine`** (`packages/chat-engine` in codeterm-canvas, version-synced via `sync:versions`, published like plugin-sdk); plugins add it as a dev dependency and esbuild bundles it into `plugin.js`. Zero runtime deps, pure functions — so any third-party plugin, not just ours, becomes watcher-capable |
| LLM **transport** (LM Studio native v1 today; OpenAI-compatible, websocket, whatever — tomorrow) | each plugin, ~one `complete(request)` function |
| Tick **scheduling**, observation snapshot, verdict parsing, **action execution**, state persistence, read-only enforcement, role/spawn/UI | host (`codeterm-canvas`) |

This split is the variability the feature exists for: a future `groq` /
`openai-compat` plugin (own auth, own endpoint) implements a transport,
imports the shared engine, declares `chatBackend: { spawnable: true, watcher:
true }` — and immediately works as a chat Tab *or* a watcher inside an
orchestration. LM Studio stays its own plugin because its native API is its
own; nothing becomes a combine.

## Orchestration integration

**Manager becomes a first-class role.** Today the backend role model is only
`AgentRole::{Orchestrator, Worker}` and a "manager" is a UI illusion — a
Worker whose `role_profile` string happens to be in the frontend's
`MANAGER_ROLE_PROFILES` set; unknown profiles (including `watcher`) are
silently stripped to generic Worker. That changes:

- `AgentRole` gains `Manager`: `{ Orchestrator, Manager, Worker }`. A manager
  is the orchestrator's delegated leadership; `role_profile` is the manager's
  **specialization** (`planner`, `watcher`), required and validated.
- Hierarchy invariants (`multiplexer/state/role.rs`): a Manager requires an
  Orchestrator parent; a Manager cannot spawn children (same leaf rule as
  Worker, v1); Workers still cannot spawn.
- Normalization (`role_profile.rs`): `--role Planner` → `Manager` + profile
  `planner`; `--role Watcher` → `Manager` + profile `watcher`; a Manager with
  a missing or unknown profile is a **spawn error**, never a silent
  downgrade. Existing planner spawn paths (`orchestration new`, planner spawn
  sugar, pairing nudges) migrate to `Manager`. No compat shims for old
  persisted Tabs (repo rule: no external users, delete legacy aggressively).
- Spawn validation: `--role Watcher` additionally requires a watcher-capable
  chatBackend provider (manifest `chatBackend: { watcher: true }`).
- The sidebar **Managers** section keys off the structural role
  (`role == Manager`); the profile only picks the badge (`PLANNER`,
  `WATCHER`). The frontend `MANAGER_ROLE_PROFILES` set is deleted in favor of
  the role.
- Observation snapshot derives the watched group from `parent_pane_id`
  (the orchestrator) — consistent with how the hierarchy already works.

## Persistence & restart

- `WatcherSpec` (charter ref, engine config, interval, state blob, last tick,
  last status) lives on the Tab and round-trips with Tab state, like
  `role_profile` does.
- The openSession config already persists as the `_meta` JSONL line; the
  existing lazy-reopen path (`chatbackend_reopen_if_restored`) re-opens the
  session with the same mode/engine after a daemon restart. The host re-arms
  the tick scheduler from `WatcherSpec`.

## Non-goals (v1)

- No PTY-agent watchers — chatBackend only.
- No tool loop inside watcher ticks (evaluation + curated actions only).
- No `llm` summarization policy implementation (declared, not built).
- No watcher-to-watcher communication; watchers exclude each other from
  observation.
- No CodeTerm-internal-planner integration (user-directed: it is not ready).
- No combined multi-provider plugin; per-provider plugins + shared library.
- No UI builder for charters — CLI flags / files first.

## Acceptance criteria

1. `codeterm agent spawn lmstudio --role Watcher --parent <orch> --charter-file c.md --interval 90s`
   creates a read-only chatBackend Tab under the orchestrator's Managers
   section; `codeterm send` into it is rejected with a clear error; the
   composer is hidden.
2. Every `interval`, the Tab shows a context card → model reply → verdict
   card; the verdict updates the Tab's status badge; state persists across
   ticks and across a daemon restart.
3. With a deliberately stalled fixture orchestration, the watcher emits a
   `stalled` verdict and the orchestrator Tab receives exactly one
   watcher-marked nudge (cooldown proven by a second tick not re-nudging).
4. A chat Tab opened with `engine: { kind: "chat", window: { maxMessages: N } }`
   never sends more than N messages of history to the LLM (assert via plugin
   test on the assembled request).
5. All existing LM Studio chat behavior is unchanged when no mode/engine is
   specified (default: `interactive` + `chat`, unbounded — bit-for-bit the
   current behavior).
6. The `watcherTick` trait/adapter surface is covered by a `call_json`
   contract test (chatBackend is JS-only; per architecture rules).
