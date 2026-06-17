# Plugins as First-Class Chat Backends & Compute — Design

**Status:** design only. No implementation yet. Supersedes the narrower
`plugin-compute-base` draft.

---

## 1. What we want, and why

Today every chat/agent in CodeTerm is a **terminal**: a pane is a PTY running a
CLI, and provider plugins parse that PTY/JSONL stream into chat. "A chat" is
welded to "a launched process."

We want CodeTerm to host chats whose backend is **anything a plugin can drive**:

- a **direct model API** — LM Studio, Groq — talking to our chat UI with *no
  terminal* at all;
- **someone else's agent system** — a third party (e.g. a "clawdbot") brings its
  own brain and *provides its chat* through us;
- **our own agents as brains** — e.g. game NPCs, each an agent the plugin drives;
- a **preprocessor** that augments what we send, every turn, using CodeTerm's own
  capabilities (memory recall, an agent worker, a Groq call).

**Why this and not bespoke subsystems:** CodeTerm already is a multiplexer where
agents live, with a provider abstraction, persistent `mem`, background workers,
and a CLI/API to drive panes. The only missing piece is letting a plugin *be* the
chat's source/transport instead of merely parsing a terminal. Building that as
one extensible seam means the community can plug LM Studio, Groq, external agent
systems, preprocessors, and game NPC brains **without forking the daemon** — and
everything rides the plugin system we already have (capability registry +
`host.*` API), not a parallel stack.

## 2. End result (success criteria)

When this lands, all of the following are true and are **plugins**:

1. Install an **LM Studio** plugin → a new chat pane that talks to LM Studio
   directly (no terminal); the model picker is populated by the plugin.
2. `codeterm send --pane <that pane> "hi"` and `codeterm get-output --pane <that
   pane>` behave exactly as on a terminal agent — our orchestrator, mesh, and
   agents can drive a plugin-backed chat uniformly.
3. A **chat-preprocessor** plugin augments every outbound message (mem recall / a
   Groq call / one of our agent workers) before it reaches the agent;
   on/off + backend/model configurable.
4. An **agent-driven environment** plugin owns its **own workspace**, spawns a
   **team** of agent sessions by its own rules (keyed so entities are resumable —
   "remembers you"), **exposes its own verbs to those agents** (the agent calls
   `codeterm <ns> say "hello"` and the plugin bridges it to an external world),
   tracks per-entity turn stats, and injects guidance (e.g. a stay-in-character
   reminder every Nth turn). The Dread Delusion NPC plugin is one instantiation;
   the pattern is generic (sim/robot/service brains too).
5. An agent can **discover, install, and configure** any plugin on the user's
   behalf (`codeterm get plugin <id>` → docs + handles → offer → install +
   configure). The "no transcriber" voice hint becomes this agent-actionable
   discovery instead of a human-only "go install it" note.
6. The existing PTY provider path (claude/codex/opencode) is **unchanged** — all
   additions are additive.

## 3. Principle

Extend the **existing** plugin abstraction — the same one behind `statusBubble` /
`glanceView` / `secretBackend` / `transcriber` / `agentProvider`: a capability
registry plus the `host.*` API in `plugin/js_runtime/host_api.rs`. Plugins
*implement* behavior; CodeTerm provides handles. No provider-id branching in core
(capability-not-identity).

**Chosen direction: A → C.** Introduce a `chatBackend` **capability** (a
pluggable, non-PTY chat transport) — the minimal real seam that unlocks direct
chat and fits preprocessor/worker. Shape its interfaces as a **pipeline**
(stages + transports over `NormalizedChatMessage`) so we can later converge on
the full middleware-bus model (C) without rewriting the working PTY path.
(Rejected: folding into `agentProvider` (B) — that contract is too PTY/session-
file/launch-shaped; a clean new capability is less of a union-type mess.)

## 4. Current state (verified, grounds the design)

- `PaneCommand` = `Shell | Agent` (`src-tauri/src/service/spawn.rs:39`) — both
  PTY-backed. No non-PTY chat exists.
- `ChatView.tsx:14` is an explicit placeholder for a "direct API chat mode (pick
  model, call `/chat` directly)" — the intent is already reserved, unbuilt.
- `@codeterm/chat-core` `NormalizedChatMessage` + `@codeterm/chat-ui`
  `MessageBubble` already abstract chat **rendering/normalization** (used by
  ChatView, PaneSessionChat, RunChatPopup). This is the lingua franca.
- Workers are real and **not** `claude -p`: a **subprocess** worker
  (`mem/supervisor.rs` → `codeterm mem run-worker <kind>` → JSON on stdout) and a
  **hidden-pane agent** worker (`mem/workers.rs` →
  `spawn_pane(SpawnPaneSpec{ visibility: Hidden, command: Agent(AgentSpawnSpec{
  provider, model, task, auto_approve, worker_id }) })`, throttled by
  `worker_pool`, reports back). The hidden-pane worker is the "invisible tab" the
  Dreamer uses.
- Send seam: `commands/pane.rs:495 chat_send_text` → `send_text_when_ready` (with
  PTY/TUI readiness: `await_agent_tui_on_screen`, `verify_submit`,
  `poll_shell_prompt`), also `POST /api/panes/:id/send`.
- Output seam: `codeterm get-output` → `api/pane_routes.rs:445 get_pane_output`
  reads the PTY `output_buf` / screen text.

## 5. Architecture — three layers

### Layer 1 (core) — `chatBackend` capability

A chat session is fed by a **transport**. Today the transport is implicit (PTY +
provider parser). We add a plugin-implemented transport.

**New pane command:** `PaneCommand::ChatBackend { plugin_id, config }` — a pane
with no PTY; its lifecycle and messages are driven by the named plugin.

**Capability `chatBackend`** (plugin logic half), entirely in
`NormalizedChatMessage` so rendering is free via `MessageBubble`:

```ts
openSession(ctx: { paneId: string; config: unknown }): { sessionId: string };
sendMessage(sessionId: string, text: string): void;
poll(sessionId: string, cursor?: string | null):
  { messages: NormalizedChatMessage[]; cursor: string | null; done?: boolean };
closeSession(sessionId: string): void;
listModels?(): { id: string; displayName: string }[];   // feeds the model picker
```

**Host side:** a `ChatBackendRegistry` (sibling to the other capability
registries). For a `ChatBackend` pane, **send** dispatches to the plugin's
`sendMessage` (not `send_text_when_ready`/PTY) and the chat **source** is the
plugin's `poll` (not a session-file tail). The `ChatView` "direct API chat mode"
placeholder becomes the renderer for these panes.

**Transport-agnostic pane I/O (core tenet).** So our whole toolkit drives
plugin-backed chats like any pane, the send/output seams become transport-aware:

- **`send`** (`chat_send_text` / `POST /api/panes/:id/send` / `codeterm send`)
  dispatches: `ChatBackend` pane → plugin `sendMessage`; PTY pane → today's path.
  "Readiness" for a chatBackend = session open (plugin-defined), not TUI-on-screen.
- **`get-output`** (`get_pane_output` / `/panes/:id/output`) for a chatBackend
  pane returns the **chat transcript** (`NormalizedChatMessage[]`), not a byte
  screen — structured, and cleaner.

Result: `codeterm send`, `get-output`, spawn, report-routing, and mesh work
uniformly on chatBackend panes. PTY-specific notions (`screenHasTui`,
verify-submit, JSONL parsing) simply don't apply — the plugin reports its own
status/readiness.

### Layer 2 — host-API base (permissioned handles)

New `host.*` namespaces any plugin can call. Powerful ones are **permissioned**:
declared in `plugin.json` `permissions` and shown for approval at install (like
git's `subprocess: { allow: ["git"] }`), mirroring the connector's
manage_workspaces/spawn_agents grant — but at plugin-install level.

```ts
// message preprocessing — a STAGE on the send path of ANY session (PTY or chatBackend)
// (capability, not host.*) chatPreprocess(ctx) -> { text } | null   // null = passthrough

// compute dispatch — backed by the real worker machinery (spawn_pane Hidden + worker_pool)
host.worker.run({ backend?, task, timeoutMs? }): Promise<{ ok, report?, error? }>;   // one-shot

// own workspace + a TEAM of agent sessions inside it  [permissioned]
host.workspace.ensure({ name, externalRoot? }): Promise<{ workspaceId }>;
host.workspace.panes(workspaceId): Promise<PaneRef[]>;
host.agent.spawn(workspaceId, { backend?, task, key?, commands? }): Promise<{ sessionId }>; // key = e.g. npc_id
host.agent.get(workspaceId, key): Promise<{ sessionId } | null>;                  // "remembers you"
host.agent.send(sessionId, text): Promise<{ reply: string }>;
host.agent.reap(sessionId): Promise<void>;

// the INVERSE direction: the plugin exposes its own verbs to the agents it
// spawned; the agent invokes `codeterm <ns> <verb> …` (or a tool), CodeTerm
// routes the call back to the plugin, the plugin acts (bridge to an external
// world) and returns a result the agent sees. (capability, not host.*)
onAgentCommand?(ctx: { sessionId, verb, args }): { result: string } | { error: string };

// memory recall (read-only)
host.mem.search({ query, k? }): Promise<{ hits: { id, text, score }[] }>;
```

`backend` is `{ provider?, model? }` resolved from plugin settings
(`config.plugins.<id>`) or a tier — **never hardcoded**. For cheap external
compute a plugin keeps using the existing `host.fetch` (e.g. a Groq POST);
`host.worker`/`host.agent` are for driving *our* agents. `host.worker.run` is the
one-shot case of `host.agent.spawn`+`send`+`reap`. Under the hood: the same
`spawn_pane(Hidden, Agent{…})` + `worker_pool` + workspace machinery, exposed.

**Agent-callable plugin handles (the inverse seam).** Driving an agent is only
half the loop; the other half is letting the agent *act through the plugin*. When
a plugin spawns an agent it declares a command namespace + verbs (`commands` on
`spawn`). The spawned agent invokes them the way agents already call us today —
`codeterm <ns> <verb> <args>` (the agent's CLI is already in its PATH; this is a
new routed namespace, not raw shell) — and CodeTerm dispatches the call to the
plugin's `onAgentCommand` handler. The plugin acts (e.g. bridges the call to an
external process/service over `host.fetch` or its own state) and returns a result
string the agent sees as the command's output. This makes a plugin a small
**environment** its agents live in: it provisions them, hands them verbs, and is
the bridge between what they "do" and the outside world.

**Teaching the agent its verbs is the plugin's job — CodeTerm bakes nothing into
prompts.** We do **not** touch our prompt-save system or inject verb docs on the
host side. CodeTerm only guarantees the *handles*; how the agent learns what it
can call is the plugin's responsibility, through channels it already has: write
an `AGENTS.md`/`CLAUDE.md` into its own workspace (the spawned agent's cwd is in
that workspace, so it's picked up automatically), seed the initial `task` prompt
on `host.agent.spawn`, inject guidance every Nth turn (the reminder cadence), or
return instructions in the `onAgentCommand` result. Which of these (or which
mix) is pure plugin technique — out of scope for the host.

> **QuickJS bridge:** the VM is synchronous (`call_json`); these handles are
> async host-side (spawn panes, await reports). They use the same async-host /
> sync-VM bridge `host.fetch` already uses (JSON envelope the VM unwraps).

### Layer 3 — consumers (validate the base; each its own spec)

- **LM Studio / Groq direct chat** (`codeterm-plugins`) — a `chatBackend` plugin:
  `openSession` connects, `sendMessage` POSTs to the API, `poll` streams the reply
  as `NormalizedChatMessage`; `listModels` feeds the picker. No terminal.
- **clawdbot "bring your own brain"** — a `chatBackend` proxy to a third-party
  agent system, surfacing its chat in our UI.
- **chat-preprocessor** (`codeterm-plugins`) — a `chatPreprocessor` stage,
  implementable entirely on this base: on each user turn it optionally
  `host.mem.search`es the message for relevant memories and/or runs a
  `host.worker.run` (one of our agents) or `host.fetch` (e.g. Groq), then returns
  the augmented `text`. Per-plugin settings (`config.plugins.<id>`): on/off, which
  backend (mem-only | worker | external API), model, compose mode (prepend
  context vs rewrite). Validates: the preprocessor stage + `host.mem` + `host.worker`
  + per-plugin settings all compose. Nothing game- or vendor-specific.

- **Agent-driven environment / connector pattern** (generalized; the dread game
  is one instantiation). A plugin that: (1) `host.workspace.ensure`s its own
  workspace (optionally `externalRoot`, e.g. inside a game folder); (2) spawns
  agents **by its own rules** via `host.agent.spawn(key=…)` — one per logical
  entity (an NPC, a role, a task), `host.agent.get(key)` to resume so the entity
  "remembers"; (3) hands those agents a set of **its own verbs** (`commands` +
  `onAgentCommand`) so an agent can *act* — `codeterm <ns> say "hello"`, `… look`,
  `… move` — and the plugin **bridges those calls to an external world** (a game,
  a service, a simulation) and feeds results back; (4) keeps per-entity state
  (turn stats) and **injects guidance** by augmenting what it sends (e.g. a
  stay-in-character reminder every Nth turn). The plugin is thus a small
  *environment* its agents inhabit and communicate with the outside through.
  - **Dread Delusion NPCs** (`dread-delusion-ai`, separate channel) is exactly
    this pattern: workspace rooted in the game folder, an agent per `npc_id`,
    NPC-facing verbs the agent calls to speak/act in the game, the stay-in-
    character reminder cadence — the `CODETERM_CONNECTOR.md` scenario realized as
    a **plugin via host-API**. The fully external scoped-grant connector
    (`feat/external-connectors`) stays a heavier separate track; the plugin form
    rides this base. Other instantiations of the same pattern: a sim/robot driver,
    a service-orchestration brain — anything where agents act in an external world
    through plugin-defined handles.

### Cross-cutting — agent-facing plugin surface (MANDATORY for all plugins)

Plugins are managed **by agents**, not only by humans clicking the Extensions UI.
So **every** plugin (provider, chatBackend, preprocessor, environment, secret,
transcriber, …) MUST expose a uniform agent-facing surface — this is a hard
requirement, not a per-plugin nicety:

1. **Docs the agent can read** — a concise, agent-oriented description: what it
   is, when to use it, how to install and configure it. Discoverable via a stable
   command, e.g. `codeterm get plugin <id>` returns the plugin's agent-doc +
   declared handles + settings schema (whether or not it's installed yet).
2. **Handles the agent can call** — install (`codeterm plugin install <id>`),
   configure (write `config.plugins.<id>` against the settings schema), and
   operate. So an agent can set a plugin up end-to-end on the user's behalf
   (offer → user confirms → agent installs + configures → done), and **plugins
   must be agent-configurable**, not UI-only.

**Discovery hooks replace dead-end hints.** Where the host today tells a human to
go install something, it should instead surface an **agent-actionable** pointer.
Concretely: the voice path's `bridge/voice_transcribe.rs:32` hint ("no
transcriber installed; install the Transcriber plugin from your channel") becomes
something like `codeterm get plugin transcribe` — so an agent transcribing for
the first time learns the plugin exists, offers to install it ("want me to set up
voice transcription?"), and on yes installs **and configures** it. The transcriber
is the first example; the requirement is universal.

This is consistent with "CodeTerm bakes nothing into prompts" (§5): the host
doesn't inject plugin docs into any agent's prompt — it exposes a *discovery
command + handles*; the agent pulls what it needs, when it needs it.

## 6. Data flow — the outbound message pipeline

**A message is a message.** Sending a voice note is *not* a separate code path —
it is an outbound message that happens to carry a property (a voice attachment)
some stage can filter on. The outbound path is therefore **one ordered pipeline
of stages**; each stage declares a **match filter** (what it grabs) and only acts
on messages it matches, passing the rest through untouched.

- **Transcriber = a specialized stage** that matches *messages with a voice
  attachment* and transforms voice→text. It is effectively a "truncated
  preprocessor": same family, one fixed job. (Today it is the frontend-initiated
  `transcribe_voice_attachment` → `transcribe_registry`; the bare-message hint
  lives at `bridge/voice_transcribe.rs:32`.)
- **Preprocessor = a general stage** that matches *text* and augments it — the
  more sophisticated, configurable one (mem recall / worker / Groq).

**Ordering follows the chain, and it's why they compose cleanly:** a voice
message is grabbed by the transcriber (the preprocessor did *not* match it —
no text yet); the transcriber emits text; *then* the preprocessor matches that
text and does its thing. So:

```
message (text | voice | …)
  → [transcriber stage]  (matches voice → text; result is EDITABLE in composer)
  → [preprocessor stage] (matches text → augmented text; send-time, transparent)
  → transport.send  (PTY | chatBackend)
```

The compose-time/send-time distinction is just *where in the chain* a stage sits:
the transcriber's output must be editable before send (and the Telegram bridge
reuses `format_attachment_line`), so it runs at compose-time; the preprocessor
runs at send-time after the text is final.

**Conflict / race guard (at install).** Two stages whose match filters **overlap
the same slot** (e.g. two general text-preprocessors) would both grab the same
message → a race / double-processing. So a stage's match filter is part of its
manifest, and **on installing a new stage that overlaps an already-installed one,
CodeTerm warns the user**: "you already have <X> that grabs this; it would run
twice." Non-overlapping stages are fine — the transcriber (voice) and a text
preprocessor never collide, they chain. (Open: hard-block vs warn-and-allow.)

**Revisit the transcriber (decided): yes.** It is recast as the reference
*compose-time stage* of this pipeline — not a bespoke command — proving stages
handle attachment→text, not just text→text, while **preserving** its editable
result and Telegram-bridge reuse. Scheduled in Spec 2 (the stage/pipeline spec).

- **Inbound:** `transport (PTY parse | plugin poll) → NormalizedChatMessage →
  ChatView/MessageBubble`.

## 7. Security & sandboxing

- `chatBackend` plugins run in the existing QuickJS sandbox; their only outside
  reach is the declared `host.*` surface (e.g. `host.fetch` to LM Studio/Groq).
- `host.workspace.*` and `host.agent.spawn` are real confinement holes — they can
  create workspaces and spawn agents. Gated by install-time **permissions** in
  `plugin.json`, surfaced in the grant dialog; `host.agent` may only spawn
  **hidden** panes under `worker_pool` limits and may only touch sessions it
  spawned (no access to user panes). `host.mem.search` is read-only.
- A chatBackend's UI half (if any) keeps the existing sandboxed-iframe rules.

## 8. Error handling

- Preprocessor timeout/throw → send original text, log, one-line non-blocking
  composer notice. A slow/broken preprocessor never blocks or drops a message
  (reuse the `timeoutMs` we added to `host.exec`).
- `chatBackend` errors: `openSession` fail → pane shows a clear error state, not a
  blank panel; `sendMessage`/`poll` error → surfaced in the transcript; the user
  can retry/close.
- `host.worker.run`/`host.agent` failure → error envelope; the consumer decides
  fallback (preprocessor → un-augmented; NPC → static dialogue). Reaping never
  strands a `worker_pool` permit.

## 9. Testing

- `parity_test.rs`: native vs JS for `chatPreprocessor` (identical augmented
  text on fixtures) and for the `chatBackend` contract surface.
- Service tests: `send` on a `ChatBackend` pane routes to `sendMessage`;
  `get_pane_output` on it returns the transcript; PTY panes unaffected.
  Preprocessor timeout path sends the original text.
- `host.worker.run` integration: a hidden-pane worker runs a trivial task and its
  report round-trips; `worker_pool` cap respected; `host.agent` resume by `key`.
- `onAgentCommand` round-trip: a spawned agent invokes a plugin verb, CodeTerm
  routes it to the handler, and the returned result reaches the agent.
- Plugin-side (`codeterm-plugins`): LM Studio/preprocessor logic unit-tested via
  `tsx`, like the existing git/bitwarden suites.

## 10. Discipline (CLAUDE.md plugin rules)

Capability-not-identity (core branches on capability presence, never plugin id) ·
native↔JS parity tests for `chatBackend` and `chatPreprocessor` · audit every new
`host.*` handle against the sandbox · all additions additive (PTY providers
untouched) · powerful handles (`workspace`/`agent`) permissioned.

## 11. Decomposition into specs/plans (build order)

1. **Spec 1 — core `chatBackend`**: capability + `PaneCommand::ChatBackend` +
   `ChatBackendRegistry` + transport-agnostic `send`/`get-output` + ChatView
   direct-mode wiring + `listModels` picker. The load-bearing abstraction; first.
2. **Spec 2 — host-API base**: `chatPreprocessor` stage + `host.worker.*` +
   `host.workspace.*`/`host.agent.*` + `host.mem.*`, with the permission model.
3. **Spec 3 — consumers**: LM Studio reference `chatBackend` plugin (validates
   Spec 1) + chat-preprocessor (validates Spec 2), both in `codeterm-plugins`;
   NPC brain in `dread-delusion-ai` (validates workspace/agent/team).

Each gets its own brainstorm → plan → implementation cycle. This document is the
umbrella design they share.

## 12. Open decisions (resolve per-spec before its plan)

1. **Preprocessor ordering** when several are registered — declared-order
   chaining (assumed) vs single-preprocessor.
2. **Inbound delivery** for `chatBackend` — host **polls** `poll(cursor)`
   (assumed, simplest) vs a push callback the plugin calls.
3. **Session mode depth** — ship one-shot `host.worker.run` first (covers the
   preprocessor) and `host.agent`/workspace with Spec 2/3, or design all up front
   (leaning: design all here, implement incrementally).
4. **`host.mem` writes** — recall-only now (assumed); NPC "memory" rides
   `host.agent` session persistence + existing mem rather than a plugin write API.
5. **Agent→plugin verb routing** for `onAgentCommand` — a routed `codeterm <ns>
   <verb>` CLI namespace the spawned agent calls (decided; reuses the agent's
   existing CLI access) vs an injected tool/MCP surface. Open part is only the
   routing implementation, not the surface.
6. **Stage conflict handling** — when a newly installed stage's match filter
   overlaps an existing one, **hard-block** the install vs **warn-and-allow**
   (assumed: warn-and-allow, let the user decide). Also: how granular the match
   filter is (message kind: text/voice/image vs a richer predicate).

**Resolved (recorded above, no longer open):**
- *Verb discovery* — **the plugin's job, not CodeTerm's.** The host bakes nothing
  into prompts; the plugin teaches its agents via its own channels (workspace
  `AGENTS.md`/`CLAUDE.md`, initial `task`, periodic injection, `onAgentCommand`
  result). See §5.
- *Transcriber vs preprocessor* — different pipeline **phases** (transcriber =
  compose-time/editable; preprocessor = send-time/transparent); transcriber is
  retrofitted as the reference compose-time stage in Spec 2. See §6.
