# Plugins Rethink — Implementation Plan

> **For agentic workers:** each Track below is dispatched to its own worker agent
> in its own CodeTerm workspace + git worktree/branch. A worker implements its
> Track task-by-task with TDD (write failing test → run → implement → run →
> commit), runs `codeterm agent report`, and exits. Workers do **not** spawn
> workers. Steps use `- [ ]` for tracking. The fine-grained TDD sub-plan for a
> Track is produced by its worker at dispatch from the Track's contract here.

**Goal:** Make a plugin able to *be* a chat's source/transport and to drive our
agents, all through the existing capability + `host.*` abstraction — then prove
it with two runnable demos: an LM Studio chat you talk to via `codeterm send`,
and a multi-NPC "game" simulation driven by our agents.

**Architecture:** A `chatBackend` capability (non-PTY chat transport) + a
transport-agnostic send/output seam; a host-API base (message-stage pipeline with
`chatPreprocessor` + the transcriber recast as a stage; `host.worker`/`workspace`/
`agent`/`mem`; agent-callable `onAgentCommand`); a mandatory agent-facing plugin
surface. Consumers (LM Studio, preprocessor, dread NPCs) validate each layer.

**Tech Stack:** Rust (Tauri host, `src-tauri`), TypeScript/React (`src`,
`@codeterm/chat-ui`/`chat-core`), QuickJS plugin VM, the Rust CLI (`cli/`), the
type-only `@codeterm/plugin-sdk` (npm), esbuild plugin builds.

**Source spec:** `docs/plans/2026-06-17-plugins-chat-backends-and-compute-base.md`.

---

## 0. Orchestration map — repos, branches, worktrees, workspaces, agents

### 0.1 Repos that participate

| Repo | Local path | Role in this plan | Integration branch |
|---|---|---|---|
| `rollacode/codeterm` (this) | `/Users/kandrey/Developer/codeterm-alpha` | host (Rust), frontend, CLI, **`packages/plugin-sdk`** | `plugins-rethink` |
| `@codeterm/plugin-sdk` (npm) | published *from* `codeterm-alpha/packages/plugin-sdk` | shared types both plugin repos import | published `1.4.7` |
| `rollacode/codeterm-plugins` | `/Users/kandrey/Developer/codeterm-plugins` | LM Studio + chat-preprocessor plugins | `main` |
| `rollacode/dread-delusion-ai` | `/Users/kandrey/Developer/dread-delusion-ai` | dread NPC plugin + mock-game harness | `main` |

### 0.2 The SDK ordering edge (read this first)

The plugin repos consume `@codeterm/plugin-sdk` **types**. New types
(`ChatBackend`, `ChatPreprocessor`, `host.worker/workspace/agent/mem`,
`onAgentCommand`) land in `codeterm-alpha/packages/plugin-sdk` in **Track A**.
Until `1.4.7` is published, plugin-repo workers consume the SDK via a local
override so they are not blocked on a publish:

```bash
# in codeterm-plugins / dread-delusion-ai during dev:
npm install /Users/kandrey/Developer/codeterm-alpha/packages/plugin-sdk
# integration: pin the published range once 1.4.7 is out
#   "@codeterm/plugin-sdk": "^1.4.7"
```

At integration, bump `packages/plugin-sdk` to `1.4.7`, publish via the
`publish-plugin-sdk` OIDC workflow (tag `plugin-sdk-v1.4.7`), and pin it in both
plugin repos.

### 0.3 Branches (one per track; isolated worktrees prevent collisions)

| Track | Repo | Branch (off) | Worktree path (codeterm-alpha tracks) |
|---|---|---|---|
| A — SDK types + seam contract | codeterm-alpha | `pr/chat-sdk-types` (off `plugins-rethink`) | main checkout (A lands first, fast) |
| C0 — ServiceContext→host_api wiring (prereq, §1.5 F4) | codeterm-alpha | `pr/host-service-ref` (off `plugins-rethink`) | `/Users/kandrey/Developer/wt/host-service-ref` (Phase 1, parallel to A) |
| B — chatBackend core | codeterm-alpha | `pr/chatbackend-core` (off `plugins-rethink`, after A) | `/Users/kandrey/Developer/wt/chatbackend-core` |
| C — host-API base | codeterm-alpha | `pr/plugin-compute-base` (off `plugins-rethink`, after A) | `/Users/kandrey/Developer/wt/plugin-compute-base` |
| D — LM Studio plugin | codeterm-plugins | `feat/lmstudio-chatbackend` (off `main`) | repo checkout |
| E — chat-preprocessor plugin | codeterm-plugins | `feat/chat-preprocessor` (off `main`) | repo checkout (separate worktree to parallel D) |
| F — dread NPC mock + game sim | dread-delusion-ai | `feat/codeterm-npc-mock` (off `main`) | repo checkout |

Worktrees are created with the `superpowers:using-git-worktrees` flow (or
`git worktree add`); they let B and C edit `codeterm-alpha` in parallel without
stomping each other's working tree.

### 0.4 CodeTerm workspaces + agent spawns (the dogfood)

One workspace per active track, working-dir = that track's worktree/repo path; I
(Orchestrator) spawn exactly one **Worker** agent per workspace, primed with the
matching mem role-skill, then it implements its Track and `codeterm agent report`s.

```
codeterm workspace create "rethink/A-sdk-types"     --cwd /Users/kandrey/Developer/codeterm-alpha
codeterm workspace create "rethink/B-chatbackend"   --cwd /Users/kandrey/Developer/wt/chatbackend-core
codeterm workspace create "rethink/C-compute-base"  --cwd /Users/kandrey/Developer/wt/plugin-compute-base
codeterm workspace create "rethink/D-lmstudio"      --cwd /Users/kandrey/Developer/codeterm-plugins
codeterm workspace create "rethink/E-preprocessor"  --cwd /Users/kandrey/Developer/codeterm-plugins
codeterm workspace create "rethink/F-dread"         --cwd /Users/kandrey/Developer/dread-delusion-ai
```

Spawn pattern (per CLAUDE.md — generic claude agent + role skill + concrete task):

```
codeterm agent spawn claude --workspace "rethink/B-chatbackend" \
  --task "codeterm mem get skill_backend_dev_role_rust_src_tauri_for_codeterm — that is your role.
          Then implement Track B from docs/plans/2026-06-17-plugins-rethink-implementation.md
          on branch pr/chatbackend-core. TDD, commit per task, then: codeterm agent report."
```

| Track | Worker role (mem skill) | Reviewer (read-only) |
|---|---|---|
| A | backend-dev (Rust+TS, SDK is TS) | code-reviewer |
| B | backend-dev (Rust) + frontend-dev (ChatView) — split into B-rs / B-ui sub-workers if needed | code-reviewer |
| C | backend-dev (Rust + CLI) | code-reviewer |
| D | frontend-dev / plugin author (TS) | code-reviewer |
| E | frontend-dev / plugin author (TS) | code-reviewer |
| F | backend-dev (Python harness) + plugin author (TS) | code-reviewer |

Reviewer = a separate `code-reviewer` agent (`mem get
skill_code_reviewer_role_read_only_review_for_codeterm`) that diffs the track
branch before merge and reports severity-ranked findings; the Orchestrator
gates merge on a clean review.

### 0.5 Dependency DAG + phasing (maximize parallelism)

```
        ┌─────────────────────────── Track A (SDK types + seam contract) ───────────────────────────┐
        │  packages/plugin-sdk types + a written seam contract for the outbound pipeline             │
        └───────────────┬───────────────────────────────────────────────────────┬───────────────────┘
                        │ (types ready; local-link unblocks plugin repos)         │
            ┌───────────▼───────────┐                               ┌─────────────▼─────────────┐
            │  Track B: chatBackend  │  (lands the send/output seam) │  Track C: host-API base   │
            │  core (host + UI)      │◀── seam contract ──┐          │  (stages, worker/agent,   │
            └───────┬────────┬───────┘                    └──────────│   onAgentCommand, CLI)    │
                    │        │                                       └──────┬───────────┬────────┘
        ┌───────────▼──┐  ┌──▼──────────────┐                  ┌───────────▼──┐   ┌────▼──────────────┐
        │ Track D:     │  │ Track F (needs   │                 │ Track E:     │   │ Track F (needs    │
        │ LM Studio    │  │ B for chat pane) │                 │ preprocessor │   │ C: agent/workspace│
        │ [proof of B] │  └──────────────────┘                 │ [proof of C] │   │ /onAgentCommand)  │
        └──────────────┘         (F depends on BOTH B and C)    └──────────────┘   └───────────────────┘
```

- **Phase 1 (solo, fast):** Track A. Publishes nothing yet; produces types +
  the seam contract doc both B and C build against.
- **Phase 2 (parallel):** Track B and Track C run **concurrently** in separate
  worktrees. They touch mostly disjoint files; the one shared edge — the
  outbound-send seam — is owned by **B** (B lands a stage-hook skeleton; C plugs
  stages into it). C's `host.*` work (host_api.rs, CLI) is independent of B and
  proceeds immediately. B's frontend (ChatView) is independent of C.
- **Phase 3 (parallel):** Track D (needs B) and Track E (needs C) — different
  repo, fully parallel. Track F (needs B **and** C) starts its *mock-only* parts
  early (Phase 2) against the SDK types, integrates against real host later.
- **Integration:** A → B → C merge into `plugins-rethink` in order; D/E/F live in
  their own repos and are validated against a dev daemon built from
  `plugins-rethink`.

### 0.6 Merge / integration strategy

1. `pr/chat-sdk-types` → review → merge to `plugins-rethink`. Bump+publish SDK
   `1.4.7`.
2. `pr/chatbackend-core` → rebase on updated `plugins-rethink` → review →
   `./build.sh test` + `tsc` + plugin tests green → merge.
3. `pr/plugin-compute-base` → rebase → review → green → merge.
4. In `codeterm-plugins`: `feat/lmstudio-chatbackend`, `feat/chat-preprocessor`
   pin `@codeterm/plugin-sdk@^1.4.7` → review → merge to `main`; add to
   `channel.json`.
5. In `dread-delusion-ai`: `feat/codeterm-npc-mock` → review → merge to `main`.
6. Final gate on `plugins-rethink`: full regression (`./build.sh test`,
   `./node_modules/.bin/tsc --noEmit`, `scripts/plugin-tests.sh all`,
   `./build.sh e2e`), then the two live demos (§D.acceptance, §F.acceptance) via
   `./build.sh dev`.

---

## 1. Target architecture (one diagram)

```
 USER ─types─▶ Composer ──────────────── outbound message pipeline ─────────────────▶
                          [compose-time stages]        [send-time stages]      transport
                          ┌───────────────────┐        ┌──────────────────┐   ┌─────────────────────┐
   voice attach ─────────▶│ transcriber stage  │─text──▶│ chatPreprocessor │──▶│  PTY  | chatBackend │
                          │ (match: voice)     │ (edit) │ (match: text)    │   │  (plugin transport) │
                          └───────────────────┘        └──────────────────┘   └──────────┬──────────┘
                                                                                          │
   ChatView ◀── NormalizedChatMessage ◀── inbound (PTY parse | chatBackend.poll) ◀────────┘

   host-API a plugin calls:  host.fetch · host.mem.search
                             host.worker.run                         (one-shot agent compute)
                             host.workspace.ensure / host.agent.*     (own workspace + a team)  [permissioned]
   inverse seam:  spawned agent ── codeterm <ns> <verb> ──▶ onAgentCommand(plugin) ──▶ external world

   codeterm send / get-output  ──dispatch by transport──▶  PTY write|read  OR  chatBackend.sendMessage|transcript
```

---

## 1.5 Code-investigation findings → plan deltas (VERIFIED — authoritative)

Five read-only Explore passes over the real code. Where this section conflicts
with looser prose in a Track below, **this wins**. Evidence is `file:line`.

### F1 — Non-PTY pane is the biggest cost; Track B is much bigger than "registry + enum"

A pane today is PTY-backed; `resolve_pty_id()` (`src-tauri/src/service/pty_io.rs:892`)
**errors on `None`** and gates ~10 paths. A `ChatBackend` pane has no PTY, so each
must branch on pane backing:

- **[BLOCKER] auto-PTY-spawn on send:** `ensure_pane_pty_for_send()`
  (`pty_io.rs:344-447`, esp. 382-391) *creates* a PTY when none exists; the REST
  `send_text` (`api/pane_routes.rs:204-381`, line 226) and `send_text_batch`
  (`pty_io.rs:856`) inherit it. → must early-return for ChatBackend and route to
  the plugin's `sendMessage`.
- **[BLOCKER] output reads:** `cmd_get_output`/`cmd_get_text`
  (`dispatch/mod.rs:242,290`), `get_pane_output` (`api/pane_routes.rs:445`, error
  "has no PTY"), `stream_pane_output` (`pane_routes.rs:632`) → branch to return the
  chat transcript / stream chat messages for ChatBackend.
- **[BLOCKER] pty ops:** `resize_pane`/`pty_kill` (`pty_io.rs:875`,
  `dispatch/mod.rs:288,358`) → graceful no-op/400 for ChatBackend.
- **[NEEDS-GUARD] output/screen pipeline:** `process_pty_output`
  (`service/output.rs:25`), `backend_read_loop`/`start_backend_reader`
  (`service/pty_lifecycle.rs:28-193`), `screen_state.rs` → only run for PTY panes
  (`gate on pane.pty_id.is_some()`); ChatBackend never enters them.
- **[OK, leverage] precedent already exists:** restore writes `pty_id: None`
  (`multiplexer/state/snapshot.rs:180`) and the frontend respawns on mount —
  **the non-PTY pane model already exists**; `get_pane_status` returns "Idle" for
  None (`hierarchy.rs:159-163`); `close_pane` handles None
  (`multiplexer/state/pane.rs:176-208`). Mesh routes through dispatch → fails
  gracefully.
- **[RESTORE GAP]** a restored ChatBackend pane must NOT auto-spawn a shell on
  next send — branch `ensure_pane_pty_for_send` on backing type and trigger a
  plugin `openSession` instead.

**Delta:** add a `Pane.backing` discriminator (`Shell | Agent | ChatBackend`),
and make Track B an explicit checklist over the ~10 sites above. NOT a small task.

### F2 — Inbound rendering is nearly free; reuse `StructuredSink` + `model_catalog`

ChatView already consumes a normalized stream: it listens to Tauri event
`pane-chat-msg-{paneId}` (`src/components/Terminal/useChatSession.ts:109`) with
`{ pane_id, messages, cursor? }`, emitted by `StructuredSink`
(`src-tauri/src/service/pane_chat/structured_sink.rs`), rendered by
`@codeterm/chat-ui` `MessageBubble`. There is a REST twin
(`api/pane_chat_routes.rs`: `/chat/start`, `/chat?since=N`).

- `NormalizedChatMessage` minimal = `{ id, type, content }` (chat-core
  `types.ts:40`); Rust `ChatMsg` (`agent/chat_types.rs:126`). Stable `id` is used
  for client-side merge across polls.
- Model picker = `ModelInfo { id, display_name, description? }`
  (`agent/provider.rs:102`) registered via `model_catalog::register(...)`
  (`agent/model_catalog.rs:78`); frontend reads `provider.available_models`.

**Delta:** Track B's chatBackend `poll` output is pushed into the existing
`StructuredSink`/`pane-chat-msg` path (host adapter does the poll→emit), so
**frontend work shrinks to: render ChatBackend panes as chat (TerminalPane
already has a `chat_view`/PaneSessionChat branch) + populate the picker from
`listModels` via `model_catalog::register`.** Correct SDK message type to
`{ id, type, content, ... }`.

### F3 — Preprocessor hook point is `pane_routes.rs:204`, NOT `pty_io` (+ a 2nd web hook)

All user sends converge at the REST `send_text` handler
(`api/pane_routes.rs:204`): Tauri `chat_send_text`, web bridge, and delayed sends.
Distinguish **user-turn vs system** via `source_pane` / `svc_token_match`
(service-push = skip). System sends MUST be excluded: orchestrator task delivery
(`service/orchestrator.rs:721`), agent restart (`agent_restart.rs:240,300`),
hierarchy reports (`hierarchy.rs:99`), GA init (`general_agent/lifecycle.rs:408`),
post-update (`update/post_update.rs:79`), install suggest (`system_info.rs:218`).
**Do NOT hook `pty_io.rs send_text*`** — too low, can't tell origin. **Second
hook required:** the browser chat path goes `ChatView.tsx:176 sendMessage` →
`write_pty_string` → `POST /api/pty/{id}/write` (`web_routes.rs:316`), bypassing
`pane_routes`. The stage pipeline must cover both, or the web composer routes
through the preprocessed path.

**Delta:** Track C hooks the stage pipeline at `pane_routes.rs:204` (gated on
user-turn detection) **and** the `/api/pty/{id}/write` web path; explicitly
exclude the listed system senders.

### F4 — host.worker/agent/mem need a Service ref the VM doesn't have today (prereq) + non-blocking contract

`host_api::install()` closures capture only `Arc<PluginSandbox>` — **no
`ServiceContext`/app_handle** (`plugin/js_runtime/host_api.rs:213-417`). The VM is
**not `Send`**; `call_json` holds a `parking_lot::Mutex` for the whole call
(`vm.rs:121-137,337-387`). `host.fetch` bridges async-from-sync by
`std::thread::spawn(reqwest::blocking).join()` (`host_net.rs:44-98`) — fine for a
short HTTP call, **fatal if held for an agent turn** (it would serialize every
other call to that plugin, e.g. the chatBackend `poll`).

- Hidden-pane worker = `svc.spawn_pane(SpawnPaneSpec{Hidden, Agent{…}})` +
  `worker_pool.acquire` + report via `GraphWorkerRegistry.complete/get`
  (`mem/workers.rs:359-519,287-300`; `spawn.rs:179`). `spawn_pane` is sync;
  lifecycle is an async task + poll loop.
- `host.mem.search` is **feasible & sync** — direct `svc.mem.search()`
  (`mem/manager_search.rs:12-124`).

**Delta — two changes:**
1. **NEW prerequisite task (Track C0, [BLOCKER]):** thread
   `Option<Arc<ServiceContext>>` into `host_api::install` + `PluginVm`. Caveat:
   `PluginVm` is built *before* `ServiceContext` exists → use late-binding
   (`OnceCell`/setter on the sandbox/VM, populated at service init), not a
   constructor arg. Blocks all of `host.worker/agent/workspace/mem`.
2. **Contract correction (Track A):** `host.worker`/`host.agent` are
   **non-blocking job-id + poll**, not blocking await:
   `host.worker.start(opts) -> { jobId }` + `host.worker.poll(jobId) ->
   { done, report? }` (reuse `GraphWorkerRegistry`). `host.agent.send` likewise
   returns a ticket the plugin polls. `host.mem.search` stays sync.

### F5 — Permission + routing: model EXISTS; new perms + a grant-persistence gap + namespace risk

`network`/`subprocess`/`secrets` are declared (`manifest.rs:440-455`) and enforced
in the sandbox (`host_api.rs:371-410`: `can_reach`/`can_exec`/`secrets_allowed`).
Settings seam exists: `host.settingsJson()` → `config.plugins.<id>`
(`host_api.rs:366`, `loader.rs:170-177`; **read at load → edits need reload**).
The grant dialog exists (`InstallGrantDialog.tsx`) and `CapabilityChip.tsx:57-74`
already lists `manage_workspaces`/`spawn_agents` **but they are NOT enforced**.
Agent→daemon routing is JSON-RPC over the unix socket, dispatched by prefix
(`socket/server/mod.rs:311-383`); closest pattern is `agent.report`
(`socket/server/agent.rs:55`).

**Deltas (Track C):**
- LM Studio (Track D) needs only `network` (add the LM Studio host to the
  manifest allowlist) — no new kind.
- `host.workspace`/`host.agent`/`onAgentCommand` need **new enforced permissions**
  `manage_workspaces` / `spawn_agents` / `agent_commands`: extend
  `PermissionsConfig` (`manifest.rs:440`), the sandbox (`host_api.rs:54-202`),
  loader stamping (`loader.rs:180-186`), the snapshot derive (`plugins.rs:228-258`),
  and `CapabilityChip`/`InstallGrantDialog`.
- **[GAP] grant persistence:** today grants are *derived from the manifest*, not
  stored/enforceable per user edit. Powerful new perms need a persisted
  `plugin_id → granted[]` store checked at the host call. Add this as a Track C
  task (it's a real prerequisite for honest `host.workspace/agent` gating).
- **`onAgentCommand` routing = NEW:** extend the socket dispatch so a method whose
  prefix matches an installed plugin id with an `agent_commands` grant routes to
  that plugin's handler. **[RISK] namespace collision** with built-in domains
  (`agent`, `workspace`, …) — use a reserved prefix, e.g. `plugin:<id>.<verb>`
  (agent calls `codeterm plugin <id> <verb>`), not a bare `<id>.<verb>`.
  **[RISK] sync-from-VM:** the handler returns synchronously; if it must await an
  external call, it uses the same job-id/poll bridge as F4.
- **[OPEN] trust inheritance:** does a plugin-spawned agent inherit the plugin's
  grants? Decide in Track C's sub-plan (default: spawned agents get a *narrower*
  set, never `admin`).

### Phasing delta

Insert **Track C0 (ServiceContext→host_api wiring)** as a [BLOCKER] prerequisite
that lands before the `host.worker/agent/workspace/mem` parts of Track C (it's a
small, self-contained host refactor — can run in Phase 1 alongside Track A). Track
B grows to the F1 branch-checklist (still Phase 2). Track A adopts the F4
job-id/poll contract and the F2 message/model shapes.

## 1.6 Resolved review concerns (decisions)

**R1 — B↔C both touch the send path → resolved by hook ownership, not by
serializing the tracks.** B owns every send **entry** edit; at each entry
(`pane_routes.rs:204`, the dispatch send cmd, `/api/pty/:id/write`) B routes
through `PaneTransport` **and** calls `run_outbound_stages(pane_id, text)` shipped
as identity (`|_, t| t`). **C never edits those entry files** — C implements the
`chatpipeline` module that the hook delegates to and fills the hook body. So the
only shared line (the hook call) is authored once by B; C's stage work lands in a
*new* module, and C's `host.*` work is parallel from day one. Order: B lands
entry+transport+empty-hook first; C connects when the hook exists. Conflict-free.

**R2 — long-lived agent sessions: the PLUGIN owns a small LRU pool (decided).**
CodeTerm builds **no** session pool. The plugin keeps ~5 live sessions (you won't
talk to more than 3–4 NPCs at once), measures activity, and **reaps the oldest
inactive** itself via `host.agent.reap`. It keeps its own `key (npc_id) → sessionId`
map and re-opens on demand. CodeTerm only provides the handles: `host.agent.spawn`
(start with a prompt), `host.agent.resume(sessionId)` (start a **specific** stored
session), `host.agent.get(key)`, `reap`. (Light note: keep plugin-session
concurrency from starving mem workers — a tiny separate budget; the ~5 cap makes
this a non-issue in practice.)

**R3 — restore is plugin-driven (decided).** A non-PTY pane's session **is one of
our agent sessions** — persisted as today (provider JSONL under
`~/.codeterm/...`, or sqlite for opencode-style). On restart the plugin re-opens
by stored id (`host.agent.resume`); if the plugin kept no reference, we simply
don't restore that one — acceptable. **No new CodeTerm restore machinery for
plugin panes.** (Future, unlikely: a pure non-PTY *chat* CLI agent of our own would
store chats as JSONL in the global `~/.codeterm` like Claude/Codex, or sqlite like
opencode — same persistence model, reused.)

**Stats for the reminder cadence:** the plugin counts its own turns (it owns the
send/recv loop — free); for token/cost it may call optional `host.agent.usage`
(read-only over the existing `parseUsage`/`UsageSnapshot`). No new stats subsystem.

**R5 — chatBackend security = trust-by-source (decided).** A chatBackend plugin
sees all chat and can `host.fetch` to its allowlisted host — that risk is governed
by **where you install it from**: a trusted channel (`codeterm-core` /
`codeterm-plugins`) is vetted; a third-party/untrusted repo is "at your own risk."
This reuses the existing system-plugin trust + channel model; no new mechanism.
The grant dialog should make the chatBackend case explicit ("sees your chat;
network → <allowlist>"). The network allowlist still bounds *where* it can send.

**R6 — `get-output` contract is UNCHANGED (decided).** Today it returns
`{ output: <plain text>, offset }` (terminal text, optional ansi-strip,
incremental `offset`; `dispatch/mod.rs:234`). We do **not** change it — a
chatBackend pane simply **flattens its transcript to the same plain text + offset**
(e.g. `"[user] …\n[assistant] …\n"`), so orchestrators/agents parse it exactly as
today. Zero contract change; the chatBackend transport just conforms.

**R7 — LM Studio testing (decided).** LM Studio's own local server **is** the
backend; the plugin only *connects* to it (`host.fetch`). So: unit-test the
plugin's pure logic against a **stubbed `host.fetch`** (cursoring, request shape),
and run the live demo against the **real local LM Studio server**. No separate mock
server to build — "just give the ability to connect to LM Studio."

---

## 2. Track A — SDK types + seam contract

**Repo/branch:** codeterm-alpha · `pr/chat-sdk-types`. **Depends on:** nothing.
**Owner:** backend-dev. **Reviewer:** code-reviewer.
> ⚠ Apply §1.5 corrections: `host.worker`/`host.agent` are **job-id + poll** (F4),
> not blocking await; `NormalizedChatMessage` minimal `{ id, type, content }` and
> `ModelInfo { id, display_name, description? }` (F2).

**Files:**
- Modify: `packages/plugin-sdk/src/capabilities.ts` — add `ChatBackend`,
  `ChatPreprocessor`, `NormalizedChatMessage` re-export note, `Model`,
  `AgentCommandCtx`/`onAgentCommand`.
- Modify: `packages/plugin-sdk/src/host.ts` — add `host.worker`, `host.workspace`,
  `host.agent`, `host.mem` interface types.
- Modify: `packages/plugin-sdk/src/index.ts` — re-export the new types.
- Modify: `packages/plugin-sdk/package.json` — version `1.4.7`.
- Create: `docs/plans/contracts/outbound-pipeline-seam.md` — the written seam
  contract B and C share (stage signature, transport-dispatch signature, ordering
  rules, conflict-filter shape). **No code, but exact signatures.**

**Interface contract (the types every other track imports):**

```ts
// capabilities.ts
export interface ChatBackend {
  openSession(ctx: { paneId: string; config: unknown }): { sessionId: string };
  sendMessage(sessionId: string, text: string): void;
  poll(sessionId: string, cursor?: string | null):
    { messages: NormalizedChatMessage[]; cursor: string | null; done?: boolean };
  closeSession(sessionId: string): void;
  listModels?(): Model[];
}
export interface ChatPreprocessor {
  match: "text";                                   // the slot it grabs (conflict guard)
  chatPreprocess(ctx: {
    paneId: string; cwd: string | null; provider: string | null;
    text: string; turnIndex: number;
  }): { text: string } | null;                     // null = passthrough
}
export interface AgentCommandHandler {              // the inverse seam
  onAgentCommand(ctx: { sessionId: string; verb: string; args: string[] }):
    { result: string } | { error: string };
}
export interface Model { id: string; displayName: string }

// host.ts (additions to the Host interface)
worker: { run(opts: { backend?: Backend; task: string; timeoutMs?: number }):
  Promise<{ ok: boolean; report?: string; error?: string }> };
workspace: {
  ensure(opts: { name: string; externalRoot?: string }): Promise<{ workspaceId: string }>;
  panes(workspaceId: string): Promise<PaneRef[]>;
};
agent: {
  // start a FRESH session with a prompt; key = the plugin's own id (e.g. npc_id)
  spawn(workspaceId: string, opts: { backend?: Backend; task: string; key?: string; commands?: string[] }):
    Promise<{ sessionId: string }>;
  // start/re-attach a SPECIFIC existing session by id (LRU re-open / restore)
  resume(workspaceId: string, sessionId: string): Promise<{ sessionId: string } | null>;
  get(workspaceId: string, key: string): Promise<{ sessionId: string } | null>; // plugin key→session lookup
  // job-id/poll (F4): send returns a ticket, plugin polls for the reply (never blocks the VM)
  send(sessionId: string, text: string): Promise<{ ticket: string }>;
  poll(ticket: string): Promise<{ done: boolean; reply?: string }>;
  reap(sessionId: string): Promise<void>;
  usage?(sessionId: string): Promise<UsageSnapshot | null>; // optional stats for reminder cadence
};
// UsageSnapshot mirrors the host's existing parseUsage output (tokens/cost), read-only.
mem: { search(opts: { query: string; k?: number }):
  Promise<{ hits: { id: string; text: string; score: number }[] }> };
// Backend = { provider?: string; model?: string }
```

**TDD slice (representative first task):**
- [ ] Write `packages/plugin-sdk/src/__tests__/types.test.ts` asserting a value
  typed `ChatBackend` with the four required methods compiles and a missing
  method is a type error (use `tsc --noEmit` as the oracle, like the existing
  type-only checks).
- [ ] Run `cd packages/plugin-sdk && npx tsc --noEmit` → expect FAIL (types
  absent).
- [ ] Add the interfaces above to `capabilities.ts`/`host.ts`/`index.ts`.
- [ ] Run `npx tsc --noEmit` → PASS.
- [ ] Commit `feat(sdk): chatBackend/chatPreprocessor/host compute types`.

**Acceptance:** `tsc --noEmit` green in `packages/plugin-sdk`; the root
`./node_modules/.bin/tsc --noEmit` still green; seam contract doc written.

---

## 3. Track B — `chatBackend` core (Spec 1)

**Repo/branch:** codeterm-alpha · `pr/chatbackend-core` (worktree
`/Users/kandrey/Developer/wt/chatbackend-core`). **Depends on:** A.
**Owner:** backend-dev (Rust) + frontend-dev (ChatView). **Reviewer:** code-reviewer.
> ⚠ Per §1.5 F1 this Track is LARGE: add a `Pane.backing` discriminator and branch
> the ~10 PTY-coupled sites (resolve_pty_id callers, `ensure_pane_pty_for_send`,
> get_output/get_text/stream, resize/kill, output/screen pipeline) + the restore
> hook. Per F2, reuse `StructuredSink`/`pane-chat-msg` for inbound and
> `model_catalog::register` for `listModels` — frontend shrinks accordingly.

**Files:**
- Create: `src-tauri/src/chatbackend/mod.rs` — `ChatBackendRegistryHandle` mirroring
  `crate::bubble`/`crate::view_host` registry shape.
- Modify: `src-tauri/src/service/mod.rs:114-123` — add
  `pub chatbackend_registry: crate::chatbackend::ChatBackendRegistryHandle,`.
- Modify: `src-tauri/src/service/spawn.rs:39` — add
  `PaneCommand::ChatBackend { plugin_id: String, config: serde_json::Value }`.
- Modify: `src-tauri/src/plugin/js_runtime/` — a `chatbackend.rs` adapter calling
  the plugin's `openSession`/`sendMessage`/`poll`/`closeSession`/`listModels` via
  `call_json` (mirror `js_runtime/transcriber.rs`).
- Modify: **send seam** `src-tauri/src/service/pty_io.rs:86 send_text` /
  `send_text_when_ready` — dispatch: `ChatBackend` pane → registry `sendMessage`;
  PTY → existing path. (B **owns** this seam; it also lands the empty
  `run_outbound_stages(text)` hook that Track C fills.)
- Modify: **output seam** `src-tauri/src/api/pane_routes.rs:445 get_pane_output` —
  `ChatBackend` pane → return transcript from `poll`; PTY → existing.
- Modify: registration in `src-tauri/src/lib.rs` startup loop (where bubble/glance/
  view registries are populated from scanned plugins) — register `chatBackend`
  capability plugins.
- Modify: `src/components/Terminal/useChatSession.ts:77` + `ChatView.tsx:14`
  (the "direct API chat mode" placeholder) — source messages from the chatBackend
  poll for `ChatBackend` panes; render via existing `MessageBubble`; populate the
  model picker from `listModels`.
- Test: `src-tauri/src/chatbackend/tests.rs`, plus a `parity_test.rs` case.

**Interface contract B exposes to C:** `run_outbound_stages(paneId, text) ->
text` is the single hook C extends with stages; B calls it in the send seam
before dispatching to transport. B ships it as identity (`|_, t| t`).

**TDD slice (first task — registry + PaneCommand):**
- [ ] `src-tauri/src/chatbackend/tests.rs`: `registers_and_dispatches` — register
  a fake backend, assert `sendMessage`/`poll` route to it.
- [ ] `./build.sh test chatbackend` → FAIL (module absent).
- [ ] Implement `chatbackend/mod.rs` registry + `PaneCommand::ChatBackend`.
- [ ] `./build.sh test chatbackend` → PASS. Commit.
- [ ] Next task: send-seam dispatch test (`send` on a ChatBackend pane calls
  `sendMessage`, PTY pane unchanged) → implement → pass → commit.
- [ ] Next: `get_pane_output` returns transcript for ChatBackend pane → … .
- [ ] Frontend: ChatView renders chatBackend poll messages (component test or
  Playwright against a stub backend) → … .

**Acceptance:** `./build.sh test` green; `parity_test.rs` covers chatBackend;
a stub `chatBackend` plugin shows messages in ChatView; `codeterm send`/`get-output`
on a `ChatBackend` pane route to the plugin. PTY providers unchanged.

---

## 4. Track C — host-API base (Spec 2)

**Repo/branch:** codeterm-alpha · `pr/plugin-compute-base` (worktree
`/Users/kandrey/Developer/wt/plugin-compute-base`). **Depends on:** A (types),
B (the `run_outbound_stages` hook — C rebases once B lands it; C's `host.*` work
proceeds immediately, independent of B).
**Owner:** backend-dev (Rust + CLI). **Reviewer:** code-reviewer.
> ⚠ Per §1.5: do **Track C0** first ([BLOCKER] wire `Arc<ServiceContext>` into
> `host_api::install`+`PluginVm` via late-binding — F4). Hook the stage pipeline at
> `api/pane_routes.rs:204` (user-turn gated) **and** `/api/pty/{id}/write`, NOT
> `pty_io` (F3). `host.worker/agent` = job-id+poll (F4). Add enforced perms
> `manage_workspaces`/`spawn_agents`/`agent_commands` + **grant persistence** (F5).
> Route `onAgentCommand` as `plugin:<id>.<verb>` to dodge domain collision (F5).

**Files:**
- Create: `src-tauri/src/chatpipeline/mod.rs` — the ordered stage pipeline; a
  `Stage` with a `match` filter; fills B's `run_outbound_stages`. Holds the
  `chatPreprocessor` stage registry + the conflict-filter check used at install.
- Modify: `src-tauri/src/plugin/js_runtime/host_api.rs:225` (the `bind!` block) —
  add `host.worker.run`, `host.workspace.ensure/panes`, `host.agent.spawn/get/
  send/reap`, `host.mem.search`, backed by `spawn_pane(Hidden, Agent{…})` +
  `worker_pool` (mirror `mem/workers.rs`) and the mem search API.
- Create: `src-tauri/src/plugin/js_runtime/agent_commands.rs` — route
  `codeterm <ns> <verb>` from a spawned agent to the owning plugin's
  `onAgentCommand`.
- Modify: `src-tauri/src/transcribe/` + `chatpipeline` — recast the transcriber as
  a **compose-time stage** (`match: "voice"`), preserving editability +
  `bridge/voice_transcribe.rs` Telegram reuse.
- Modify: `cli/src/args/plugin.rs:4` (+ `cli/src/cmd/plugin/`) — add
  `PluginAction::Get { id }` → returns agent-doc + handles + settings schema
  (the mandatory agent-facing surface / discovery).
- Modify: `src-tauri/src/bridge/voice_transcribe.rs:32` — the "no transcriber"
  hint becomes the agent-actionable `codeterm get plugin transcribe`.
- Modify: `plugin.json` permission model + the Extensions install-grant dialog —
  declare/approve `host.workspace`/`host.agent` (permissioned).
- Test: stage-pipeline tests, host-API integration tests, `parity_test.rs` for
  `chatPreprocessor` + `onAgentCommand`, CLI contract test for `get`.

**TDD slice (first task — stage pipeline + conflict guard):**
- [ ] `src-tauri/src/chatpipeline/tests.rs`: `stages_run_in_order` and
  `overlapping_filter_is_flagged` (two `match:"text"` stages → install check
  returns a conflict).
- [ ] `./build.sh test chatpipeline` → FAIL.
- [ ] Implement the ordered pipeline + filter-overlap check; wire into B's
  `run_outbound_stages`.
- [ ] PASS. Commit. Then: `host.worker.run` integration test (hidden-pane worker
  round-trips a report) → implement → pass → commit. Then `host.agent` resume by
  `key`; then `onAgentCommand` round-trip; then transcriber-as-stage; then CLI
  `get`.

**Acceptance:** `./build.sh test` green; preprocessor stage + transcriber stage
chain (`voice→text→augment`); `host.worker.run`/`host.agent` spawn hidden panes
under `worker_pool`; `onAgentCommand` routes a spawned agent's verb to the plugin;
`codeterm get plugin <id>` returns the agent surface; permissions enforced.

---

## 5. Track D — LM Studio chatBackend plugin (proof of Track B)

**Repo/branch:** codeterm-plugins · `feat/lmstudio-chatbackend`. **Depends on:**
B (chatBackend) + A (types, local-linked). **Owner:** plugin author (TS).
**Reviewer:** code-reviewer.

**What it is:** a `chatBackend` plugin (`lmstudio/`) you can install, that opens a
session against a local LM Studio HTTP server, and that you **talk to via
`codeterm send`** — no terminal.

**Files (in codeterm-plugins):**
- Create: `lmstudio/plugin.json` (capability `chatBackend`, `permissions` declaring
  outbound `host.fetch` to the configured base URL), `lmstudio/src/plugin.ts`,
  `lmstudio/settings.schema.json` (base URL, model), `lmstudio/plugin.test.cjs`.
- Modify: `channel.json` (add `lmstudio`), root build (`build:all`).

**`lmstudio/src/plugin.ts` shape:**
```ts
import type { ChatBackend, Model } from "@codeterm/plugin-sdk";
const sessions = new Map<string, { messages: any[]; cursor: number }>();
const cfg = () => JSON.parse(host.settingsJson());     // { baseUrl, model }
const plugin: ChatBackend = {
  openSession(ctx) { const id = ctx.paneId; sessions.set(id, { messages: [], cursor: 0 }); return { sessionId: id }; },
  sendMessage(sid, text) {
    const r = JSON.parse(host.fetch(JSON.stringify({
      method: "POST", url: `${cfg().baseUrl}/v1/chat/completions`,
      body: JSON.stringify({ model: cfg().model, messages: [{ role: "user", content: text }] }),
    })));
    const reply = JSON.parse(r.body).choices[0].message.content;
    sessions.get(sid)!.messages.push({ role: "assistant", text: reply });
  },
  poll(sid, cursor) {
    const s = sessions.get(sid)!; const from = Number(cursor ?? 0);
    return { messages: s.messages.slice(from), cursor: String(s.messages.length) };
  },
  closeSession(sid) { sessions.delete(sid); },
  listModels(): Model[] { /* GET /v1/models → map */ return []; },
};
export default plugin;
```

**TDD slice:** `plugin.test.cjs` (via `tsx`) unit-tests `poll` cursoring +
`sendMessage` request shape against a fake `host.fetch`. Build via
`scripts/build-plugin.mjs lmstudio`.

**Acceptance (live demo, proves Track B):** install `lmstudio` into a dev daemon
built from `plugins-rethink`; open a chatBackend pane; **`codeterm send --pane
<id> "hello"`** → reply appears in ChatView; `codeterm get-output --pane <id>`
returns the transcript. Back-and-forth works with no terminal.

---

## 6. Track E — chat-preprocessor plugin (proof of Track C)

**Repo/branch:** codeterm-plugins · `feat/chat-preprocessor`. **Depends on:** C.
**Owner:** plugin author (TS). **Reviewer:** code-reviewer.

**Files:** `chat-preprocessor/plugin.json` (capability `chatPreprocessor`,
`match:"text"`), `chat-preprocessor/src/plugin.ts`, `settings.schema.json`
(on/off, backend `mem|worker|groq`, model, compose mode), `plugin.test.cjs`;
`channel.json`.

**`src/plugin.ts` shape:**
```ts
import type { ChatPreprocessor } from "@codeterm/plugin-sdk";
const plugin: ChatPreprocessor = {
  match: "text",
  chatPreprocess(ctx) {
    const s = JSON.parse(host.settingsJson());
    if (!s.enabled) return null;
    const hits = JSON.parse(host.mem.search(JSON.stringify({ query: ctx.text, k: 3 }))).hits;
    if (!hits.length) return null;
    const ctxBlock = hits.map((h: any) => `- ${h.text}`).join("\n");
    return { text: `${ctx.text}\n\n<context>\n${ctxBlock}\n</context>` };
  },
};
export default plugin;
```

**TDD slice:** unit-test compose logic (mem hits → augmented text; disabled →
null; no hits → null) via `tsx` against a fake `host.mem`.

**Acceptance (proves Track C):** with the plugin installed and enabled, an
outbound message in any pane is augmented with mem context before the agent sees
it; toggling off → passthrough; the timeout path sends the original text.

---

## 7. Track F — dread NPC mock + mock-game simulation (the headline proof)

**Repo/branch:** dread-delusion-ai · `feat/codeterm-npc-mock`. **Depends on:**
B (chatBackend) + C (workspace/agent/onAgentCommand). **Owner:** backend-dev
(Python harness) + plugin author (TS). **Reviewer:** code-reviewer.

**Goal:** prove the *agent-driven-environment* pattern end-to-end **without the
real game** — a chat with **several NPC characters acting**, where a mocked
"game" interface sits on the other side and we demonstrate it really works.

**Two pieces:**

**(F1) The NPC `chatBackend` plugin** (`codeterm-npc/`, TS) — installed into
CodeTerm:
- `openSession` → `host.workspace.ensure({ name: "dread-npcs", externalRoot:
  <mock game folder> })`.
- For each `npc_id` in the scene → `host.agent.get(ws, npc_id)` else
  `host.agent.spawn(ws, { task: <persona prompt from npc_scripts/<id>.md>,
  key: npc_id, commands: ["say","look","move","emote"] })`.
- Player line in → route to the addressed NPC's `host.agent.send`; reply out as
  `NormalizedChatMessage` tagged with the speaker.
- `onAgentCommand({verb:"say",args})` → forward the NPC's spoken line to the mock
  game; `look/move/emote` → mutate + return mock world state.
- Per-NPC turn counter → every Nth turn, prepend a stay-in-character reminder.

**(F2) The mock-game harness** (`mock_game/`, Python — reuses `npc_server/`
scaffolding) — stands in for the real game:
- A tiny chat UI / REST endpoint: the "other side" is **just a chat** where you
  type as the player and see NPCs act/speak.
- Sends `{npc_id, player_input}` to the NPC plugin's pane (via `codeterm send`)
  and renders replies + the `onAgentCommand` actions (say/move) it receives back.
- Seeds a scene with **multiple characters** (e.g. `npc_morozov`, `npc_xenia`,
  `npc_culwich` from `npc_scripts/`).

**Mock-first parallelism:** F1's logic is developed against the SDK types from day
one; where the real host isn't merged yet, F uses a thin local `host` stub
(`mock_game/host_stub`) so the **multi-character acting chat** is demonstrable
before B/C land, then re-pointed at the real daemon at integration.

**TDD slice:**
- [ ] `codeterm-npc/plugin.test.cjs`: persona→spawn task mapping; reminder fires
  on turn N; `onAgentCommand("say")` formats a game-bound line. Run via `tsx`.
- [ ] `mock_game` test: a scripted 2-NPC exchange round-trips (player line →
  two NPCs respond in character → a `move` action updates mock state).

**Acceptance (the demo):** run the mock game; type a player line; **two+ NPC
agents reply in character**, one performs an action (`codeterm dread move …` →
mock world updates), and after N turns a stay-in-character reminder is injected.
This proves: own workspace + a team of keyed agent sessions + agent-callable
plugin verbs + stat-driven reminders — i.e. the full environment pattern, with
the game mocked.

---

## 8. Self-review (spec coverage)

- chatBackend transport + `PaneCommand::ChatBackend` + registry → Track B. ✓
- transport-agnostic `send`/`get-output` → Track B. ✓
- `chatPreprocessor` stage + message-stage pipeline + conflict guard → Track C. ✓
- transcriber recast as compose-time stage → Track C. ✓
- `host.worker`/`workspace`/`agent`/`mem` + permissions → Track C. ✓
- `onAgentCommand` inverse seam + `codeterm <ns> <verb>` routing → Track C. ✓
- mandatory agent-facing surface (`codeterm get plugin`) + voice hint → Track C. ✓
- SDK types for all of the above → Track A. ✓
- consumers: LM Studio (B-proof) → D; preprocessor (C-proof) → E; dread NPC
  environment (B+C proof) → F. ✓
- "CodeTerm bakes nothing into prompts" honored: F1 teaches NPCs their verbs via
  workspace files / spawn task / reminders, not host prompt injection. ✓

**Open decisions (from the spec) that workers must resolve in their sub-plan
before coding:** preprocessor ordering; chatBackend inbound poll-vs-push; session
depth; `onAgentCommand` routing impl; stage-conflict hard-block-vs-warn +
match-filter granularity.

---

## Appendix B1 — pane↔PTY decoupling: design + COMPLETE site inventory

> Purpose: remove the F1 fear by knowing *every* place, not "about 10". This is
> the authoritative checklist for Track B. The fear is uncertainty; this is the
> certainty. Sweep done by exhaustive grep of `resolve_pty_id`,
> `ensure_pane_pty_for_send`, the `send_text*` family, dispatch commands,
> output/screen state, and the frontend.

### B1.1 The design — one seam, not scattered `if`s

1. **Single source of truth:** add `Pane.backing: PaneBacking` —
   `enum PaneBacking { Pty, ChatBackend { plugin_id: String, session_id: String } }`.
   A ChatBackend pane has `pty_id: None` **permanently**. One helper
   `Pane::is_chat_backend()`.
2. **A `PaneTransport` seam at the pane-I/O boundary.** The few *pane-keyed* entry
   operations dispatch on backing instead of calling `resolve_pty_id` directly:
   `send(text, enter)`, `read_transcript(since)`, `resize(cols,rows)` (no-op for
   chat), `kill`/`close`. Two impls: `PtyTransport` (today's path) and
   `ChatBackendTransport` (calls `chatbackend_registry`). Entry points call
   `svc.pane_transport(pane_id)?.send(...)`. **This collapses ~20 scattered
   branches into one dispatch + two impls — and it *is* the "transport-agnostic"
   the spec promised.**
3. **No backend reader at spawn for ChatBackend** → `output_buf`/`screen_state`
   are never populated for it, so the whole PTY read/parse pipeline
   (`process_pty_output`, `backend_read_loop`, `ScreenState`) is simply *never
   entered* — nothing to guard inside it.
4. **One auto-spawn guard:** make `ensure_pane_pty_for_send` backing-aware — for a
   ChatBackend pane early-return (and lazily call the plugin's `openSession`),
   never spawn a shell. Fixing this **one** function neutralizes **all 9** callers.
5. **Skip `await_pty_link` for ChatBackend** (it polls for a PTY that never links →
   would hang). Its callers gate on backing.
6. **Frontend:** `TerminalPane`/`TabContent` branch on backing → render the chat UI
   (the `PaneSessionChat` path already exists), skip `useXterm`/`usePtyConnection`.
7. **Completeness guard (the thing that kills the fear):** an executable test that,
   for a ChatBackend pane, exercises *every* entry op
   (send/get_output/get_text/stream/resize/kill/close/await_pty_link) and asserts
   **none spawns a PTY and none errors "no PTY"**; plus a CI grep asserting no NEW
   `resolve_pty_id` call is added *outside* the transport module. After this, "did
   we miss a spot?" is answered by a green test, not by worry.

### B1.2 Complete site inventory (every coupling point)

Category key: **[T]** route via `PaneTransport` (entry point) · **[G]** covered by
the single `ensure_pane_pty_for_send` guard · **[R]** never populated/started for
ChatBackend (no reader) · **[FE]** frontend branch · **[OK]** already None-safe or
Agent-only path · **[V]** verify intent in sub-plan.

**`resolve_pty_id` callers (20):**

| Site | Cat | Action |
|---|---|---|
| `service/pty_io.rs:86 send_text` (entry) | [T] | dispatch to transport |
| `service/pty_io.rs:208 send_text_when_ready` (entry) | [T] | dispatch |
| `service/pty_io.rs:856 send_text_batch` (entry) | [T] | dispatch |
| `service/pty_io.rs:876 resize` | [T] | no-op for chat |
| `service/pty_io.rs:886` (read/get) | [T] | transcript |
| `service/pty_io.rs:350` (inside ensure_pane_pty_for_send) | [G] | the guard |
| `service/pty_io.rs:489` | [R] | internal, PTY-only after branch |
| `service/pty_io.rs:815` | [R] | internal, PTY-only |
| `dispatch/mod.rs:242 cmd_get_output` | [T] | transcript |
| `dispatch/mod.rs:290 cmd_get_text` | [T] | transcript |
| `api/pane_routes.rs:316` (send) | [T] | dispatch |
| `api/pane_routes.rs:489 get_pane_output` | [T] | transcript |
| `api/pane_routes.rs:644 / :684 stream_pane_output` | [T] | stream chat msgs |
| `api/pane_routes.rs:182 await_pty_link` | [G] | skip for chat (else hangs) |
| `api/pane_routes.rs:749` (cleanup) | [OK] | None-safe guard |
| `bridge/manager/mod.rs:737` (bridge→pane send) | [T] | dispatch (Telegram→chat) |
| `api/mem_routes/items.rs:730` (attach pane output to mem) | [V] | transcript or skip |
| `service/spawn.rs:896` (send launch shell cmd) | [OK] | Agent-only; chat never reaches |
| `service/pane.rs:382` | [R] | internal |
| `service/pane.rs:512` (close) | [OK] | None-safe |

**`ensure_pane_pty_for_send` callers (9) — ALL covered by guard B1.1#4:**
`multiplexer/commands.rs:92` · `api/pane_routes.rs:226` (send_text route) ·
`api/workspace_routes.rs:162` · `service/spawn.rs:546` · `service/pty_io.rs:87`
(send_text) · `service/pty_io.rs:214` (send_text_when_ready) ·
`socket/server/workspace.rs:75`. (Plus the def at `pty_io.rs:344`.)

**Dispatch commands (socket/CLI handlers) — each branches on backing:**
`dispatch/mod.rs:224 cmd_send_text` [T] · `:234 cmd_get_output` [T] ·
`:285 cmd_get_text` [T] · `:360 cmd_pty_resize` [T no-op] · `:370 cmd_pty_kill` [T no-op].

**`send_text*` family:** pane-keyed entries `send_text:86`, `send_text_when_ready:208`,
`send_text_batch:856` → [T]. PTY-keyed `send_text_to_pty:506`, `_smart:99`,
`_verified:550` → [OK] (reached only after resolution, i.e. PTY panes only).

**Output/screen state:** `pty/screen_state.rs get_screen_text:82`, `OutputBuffer`
(lib.rs:50-57, fed by the backend reader) → [R] never populated for ChatBackend
(no reader started at spawn).

**Frontend:** `Terminal/TabContent.tsx:51` (passes `pane.pty_id`) ·
`Terminal/TerminalPane.tsx:11 usePtyConnection / :94 useXterm` → [FE] branch on
backing, render chat instead. `Terminal/PaneSessionChat.tsx` already drives a
chat-style pane (reuse/extend). `ChatComposer.tsx:429 write_pty_string` is the web
send path → routes through the transport once the pane is ChatBackend.

**Spawn:** `PaneCommand::ChatBackend` must NOT call `start_backend_reader`
(`service/pty_lifecycle.rs:25`) — that's what keeps [R] true.

### B1.3 Track B task order (revised from this inventory)

1. `Pane.backing` enum + `is_chat_backend()` + `PaneCommand::ChatBackend`. Test.
2. `PaneTransport` trait + `PtyTransport` (wrap today's path, no behavior change —
   prove parity) + `svc.pane_transport()`. Test PTY unchanged.
3. `ChatBackendTransport` + `chatbackend_registry` + the js_runtime adapter. Test.
4. Re-point the **[T]** entry sites at the transport (send/get_output/get_text/
   stream/resize/kill + dispatch commands + bridge send). Test each.
5. The **[G]** guard in `ensure_pane_pty_for_send` + skip `await_pty_link`. Test
   no-auto-spawn.
6. Spawn path: ChatBackend starts no reader; inbound `poll`→`StructuredSink`. Test.
7. Frontend **[FE]** branch (render chat, no xterm). Playwright against a stub.
8. **Completeness test** (B1.1#7) + CI grep guard. Green = fear gone.
