# Plugin Compute Base — Design

**Status:** design only. No implementation yet.

**Goal:** Extend CodeTerm's *existing* plugin abstraction (capability registry +
`host.*` API) with the handles a plugin needs to (a) intercept/augment an
outbound chat message, and (b) dispatch real compute to CodeTerm's own worker
machinery. This is **not a new parallel subsystem** — it grows the same seam we
already grew for `statusBubble` / `glanceView` / `secretBackend` / `transcriber`
/ `agentProvider`. Two consumers validate the base: a **chat-preprocessor**
plugin and a generalized **NPC-brain / connector** plugin.

---

## 1. Framing

We already let plugins implement host-defined behaviors through two mechanisms:

- **Capabilities** — a plugin declares `capabilities.<name>: true` in
  `plugin.json`; the host calls exported methods at the right moment (e.g.
  `statusBubble(ctx)`, `transcribe(...)`). Registered in the capability
  registries, surfaced through `js_runtime`.
- **Host API** — the `host.*` surface a plugin's logic half calls
  (`src-tauri/src/plugin/js_runtime/host_api.rs`, `bind!("fetch", …)` etc.):
  read-only FS, `host.exec`, `host.fetch`, `host.shell`, secrets.

This work adds, in the same shapes:

1. a new **capability** `chatPreprocessor` (a hook on the outbound-message path), and
2. a new **host-API namespace** `host.worker.*` (dispatch compute to CodeTerm's
   worker machinery, backend-agnostic), plus `host.mem.*` (recall).

A plugin then *implements* the behavior; CodeTerm only provides the handles.
Everything flows through the plugin system — no bespoke daemons, no provider-id
branching in core.

---

## 2. The base — what CodeTerm exposes

### 2.1 `chatPreprocessor` capability (the message hook)

**Seam (real, verified):** outbound user text reaches the agent via
`commands/pane.rs::chat_send_text` → `Service::send_text_when_ready(pane_id,
text, enter)`, and the web route `POST /api/panes/:id/send`. Both funnel through
one service method — the hook lives there so desktop and web share it.

**Contract:** before the text is written to the pane, the host calls every
registered `chatPreprocessor` plugin in declared order:

```ts
// plugin logic half
chatPreprocess(ctx: {
  paneId: string;
  cwd: string | null;
  provider: string | null;     // the pane's agent provider id
  text: string;                // the user's outbound message
  turnIndex: number;           // 0-based count of user turns this session
}): { text: string } | null;   // null = pass through unchanged
```

The host threads each plugin's returned `text` into the next, then sends the
final string. Honesty rules:

- **Bounded & cancellable.** The hook runs under a per-call timeout (reuse the
  `timeoutMs` we just added to `host.exec`; same idea for the whole hook). On
  timeout/error the host logs and **sends the original text** — a slow/broken
  preprocessor never blocks or drops a message.
- **Visible.** While the hook runs, the composer shows an "augmenting…" state so
  a multi-hundred-ms backend call isn't a mystery freeze. A per-pane master
  toggle (and a global one) bypasses preprocessing entirely.
- **Opt-in.** No registered `chatPreprocessor` plugin → zero overhead, identical
  to today.

### 2.2 `host.worker.*` — dispatch to CodeTerm's worker machinery

Grounded in the **real** mechanisms (no `claude -p` anywhere):

- **Hidden-pane agent worker** (`mem/workers.rs`): `Service::spawn_pane(
  SpawnPaneSpec { visibility: Hidden, command: Agent(AgentSpawnSpec { provider,
  model, task, auto_approve, worker_id, spawned_by }) })`, throttled by
  `worker_pool.acquire(provider)`, the agent runs the task and **reports** back.
  This is the "invisible tab" the Dreamer/mem workers use.
- **Subprocess worker** (`mem/supervisor.rs`): spawns CodeTerm's own CLI
  (`codeterm mem run-worker <kind>`) → JSON on stdout. Deterministic, no LLM.

The base exposes the **hidden-pane agent worker** to plugins, backend-agnostic.
Backend (provider/model) is **never hardcoded** — it comes from plugin settings
(`config.plugins.<id>`) or a tier, exactly like the connector's per-class tiers.
For cheap/external compute the plugin can still use the existing `host.fetch`
(e.g. a Groq call) — `host.worker` is specifically for *driving our agents*.

Two modes:

```ts
// one-shot: spawn a hidden agent worker, run a task, get its report. Awaitable.
host.worker.run({
  backend?: { provider?: string; model?: string },  // default: from settings/tier
  task: string,
  timeoutMs?: number,
}): Promise<{ ok: boolean; report?: string; error?: string }>

// long-lived session (for the connector's NPC brains): a persistent hidden pane
// driven over many turns, then reaped.
host.worker.session.open({ backend?, task /* system/persona prompt */ }): Promise<{ sessionId }>
host.worker.session.send(sessionId, text): Promise<{ reply: string }>
host.worker.session.close(sessionId): Promise<void>
```

Sessions reuse the same `spawn_pane(Hidden, Agent{…})` + `worker_pool` path;
`send`/`reply` reuse `send_text_when_ready` + the existing chat-tail read. Idle
sessions are reaped on a TTL (matches `mem/workers.rs` reaping). Concurrency is
bounded by `worker_pool` so a plugin can't exhaust agent slots.

> **QuickJS note:** the VM is synchronous (`call_json`); these handles are
> async from the host side (they spawn panes / await reports). They follow the
> same async-host / sync-VM bridge that `host.fetch` already uses in the
> transcriber, returning a JSON envelope the VM unwraps.

### 2.3 `host.mem.*` — recall

```ts
host.mem.search({ query: string, k?: number }): Promise<{ hits: Array<{ id, text, score }> }>
```

Backed by the existing mem search (`POST /api/mem/search` equivalent, in-process).
Read-only. Lets the preprocessor "quickly run over memory, grab something
relevant, attach it" without the plugin holding daemon creds.

---

## 3. Consumers (validators — separate specs later)

### 3.1 Chat-preprocessor plugin → `codeterm-plugins` channel

Implements `chatPreprocessor`. Per user turn: optionally `host.mem.search` the
message for relevant memories, optionally run a `host.worker.run` (our agent) or
`host.fetch` (Groq) to rewrite/expand, and return augmented `text`. Settings
(`config.plugins.<id>`): on/off, which backend (Groq | worker | mem-only), model,
how to compose (prepend context / rewrite). Proves the base's hook + worker +
mem + per-plugin-settings all compose.

### 3.2 NPC-brain / connector plugin → its own channel (e.g. dread-delusion-ai)

Generalizes the `CODETERM_CONNECTOR.md` pattern as a *plugin* over the base:
opens a `host.worker.session` per NPC (persona = the task prompt), feeds player
input via `session.send`, returns the reply, tracks per-session turn stats, and
**injects a stay-in-character reminder every Nth turn** (the "каждое 10-е
сообщение" rule) by augmenting the sent text. Proves the base's **session mode** +
stat-tracking + reminder-injection. (The fully *external*, scoped-grant connector
from `feat/external-connectors` is a heavier separate track; the plugin form is
what rides this base.)

---

## 4. Extensibility & discipline (CLAUDE.md plugin rules)

- **Capability-not-identity.** Core branches on `chatPreprocessor`/`host.worker`
  presence, never on plugin ids. New backend = a new variant behind
  `host.worker`, consumers untouched.
- **Native + JS parity.** `chatPreprocessor` and `host.worker` get cases in
  `parity_test.rs` so native and JS drop-ins agree on shared fixtures.
- **Host-API audit.** `host.worker` and `host.mem` are new confinement holes:
  `host.worker` can only spawn **hidden** panes under `worker_pool` limits with
  `auto_approve` scoped to the worker (no access to user panes); `host.mem` is
  read-only search. Both are documented in `docs/PLUGINS.md` host-API table.
- **Additive surface.** All additions are new methods/capabilities; no existing
  contract changes → no drop-in breaks.

---

## 5. Error handling

- Preprocessor timeout/throw → send original text, log, surface a one-line
  non-blocking notice in the composer.
- `host.worker.run` failure → `{ ok: false, error }`; the consumer decides
  (preprocessor falls back to un-augmented; NPC plugin falls back to static text).
- Session backend dies → `session.send` returns an error envelope; the plugin
  can reopen. Reaping never strands a worker_pool permit.

---

## 6. Testing

- `parity_test.rs`: native vs JS `chatPreprocessor` over fixed messages; both
  produce identical augmented text.
- Service test: `chat_send_text` with a registered preprocessor sends the
  augmented string; with a throwing/slow one sends the original (timeout path).
- `host.worker.run` integration: a hidden-pane worker runs a trivial task and its
  report round-trips to the calling plugin; `worker_pool` cap respected.
- Plugin-side (in `codeterm-plugins`): preprocessor compose logic unit-tested via
  `tsx` like the existing git/bitwarden tests.

---

## 7. Open decisions (resolve before the plan)

1. **Preprocessor ordering/merge** when multiple are registered — declared order
   chaining (assumed) vs single-preprocessor-only.
2. **Session-mode scope now or later** — ship `host.worker.run` (one-shot, covers
   the preprocessor) first and defer `host.worker.session` to the NPC-brain track,
   or design both up front. (Leaning: design both, implement one-shot first.)
3. **`host.mem` write access** — recall-only now (assumed) vs also letting a
   plugin persist (the NPC "remembers you" need — but that can ride `host.worker`
   sessions + existing mem instead).
