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

## 2. Track A — SDK types + seam contract

**Repo/branch:** codeterm-alpha · `pr/chat-sdk-types`. **Depends on:** nothing.
**Owner:** backend-dev. **Reviewer:** code-reviewer.

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
  spawn(workspaceId: string, opts: { backend?: Backend; task: string; key?: string; commands?: string[] }):
    Promise<{ sessionId: string }>;
  get(workspaceId: string, key: string): Promise<{ sessionId: string } | null>;
  send(sessionId: string, text: string): Promise<{ reply: string }>;
  reap(sessionId: string): Promise<void>;
};
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
