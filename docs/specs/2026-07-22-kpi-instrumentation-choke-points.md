# KPI instrumentation — choke-point and usage-source inventory

Phase 1 deliverable for `pl_dwojf1` / `sp_tqeuiy` (task `task_8ac8dh`). Scope: **discovery
only** — event schema draft, exact instrument points, per-provider usage-field notes,
storage decision. No collection code, CLI, API, or backfill implementation here (that is
phases 2–5, tracked as `task_ckaxja`, `task_xr8mx3`, `task_2e9qf0`, `task_cmu92w`).

## 1. Event row fields

One row per coordination event, matching the spec's attribution contract:

| field | type | source |
|---|---|---|
| `plan_id` | `Option<String>` | resolved plan/effort context at the choke point |
| `task_id` | `Option<String>` | `CODETERM_TASK_ID`, already exported per-pane at spawn/bind time (`src-tauri/src/service/pty_io/lifecycle.rs:195`) |
| `tab` / `pane_id` | `String` | the pane the event is attributed to |
| `role` | `Option<AgentRole>` | orchestrator / planner / worker / general, from `pane.role` |
| `provider` | `String` | `claude` / `codex` / `opencode` / other |
| `model` | `Option<String>` | resolved model at event time |
| `timestamp` | `u64` (unix secs) | event time |
| `kind` | enum | `send` \| `report` \| `spawn` \| `nudge` \| `heartbeat` \| `poll` \| `bounce` \| `accept` \| `token_usage` \| `compaction` |
| `bytes` | `u64` | payload size for the event (message body, heartbeat text, brief, etc.) |

Unattributable events (no plan bound) roll up to workspace scope, per spec §Attribution.
Schema carries a version field from day one (phase 2 concern; see §5).

## 2. Instrumentation map — exact seams

### Messenger::deliver (sends)

`src-tauri/src/service/messenger.rs:218` — `Messenger::deliver`. Single writer for every
PTY-routed and chatBackend-routed message. Two sub-paths inside:
- ChatBackend transport: `messenger.rs:239` (`dispatch_delivery(DeliveryTransport::ChatBackend, ...)`)
- PTY transport: `messenger.rs:281` (`dispatch_delivery(DeliveryTransport::Pty, ...)`), actual
  text handoff at `messenger.rs:286` (`svc.send_text_detached`)

`envelope.kind` (`MessageKind::Message | Report | ...`, already on the envelope at
`messenger.rs:226`) gives the marker-kind for free; `body.len()` gives bytes. This is the
correct single choke point — every reminder/report/plain-send path funnels through it
(`deliver_automated_plan_reminder` below calls it too).

### hierarchy::submit_report (reports, bounce/accept)

`src-tauri/src/service/hierarchy.rs:406` — `Hierarchy::submit_report`. Full attribution
(plan_id, task_id, pane, role, model) is resolved by `hierarchy.rs:447` (role) and
`hierarchy.rs:448` (model). The delivery itself happens at `hierarchy.rs:606`
(`self.gateway_deliver(...)`), but the durable single-writer persistence point is one layer
down: `ReportStore::add_detailed_with_model_identity` at `src-tauri/src/agent/reports.rs:213`,
which already UPSERTs a row into the existing sqlite `OrchestrationLedger`
(`src-tauri/src/agent/ledger.rs`) at `reports.rs:269-280`. **Extend this existing write,
don't duplicate it** — it already carries plan_id/task_id/model/status in scope.

Bounce/accept verdicts are a separate seam, not part of `submit_report`: the JSON-RPC
handlers `src-tauri/src/socket/server/plan_review.rs:100` (`"plan.review.accept"` →
`store.accept_review(...)`) and `plan_review.rs:124` (`"plan.review.bounce"` →
`store.bounce_review(...)`) are the single writers for the "bounce count / first-attempt
accept rate" KPI.

### Agent spawn

Two layers exist; pick the outer one for attribution completeness:

- **Low-level single writer** (spec-visible bytes but no resolved role/model yet):
  `src-tauri/src/service/spawn.rs:192` — `spawn_pane_with_ready_and_state_change`, which
  every spawn path (sync and async, agent/shell/chatBackend) funnels through before
  dispatching to `spawn_visible_agent_impl` / `spawn_hidden_agent_impl` / etc.
- **Attribution-complete success returns** (role + model + agent + task binding all
  resolved): three `Ok(SpawnResult { .. })` sites in
  `src-tauri/src/service/orchestrator/spawn.rs`:
  - `:138` — `spawn_agent_sync_with_ready` (sync entry, used by internal callers)
  - `:231` — `spawn_agent_with_state_change`, shell/chatBackend early-return branch
  - `:598` — `spawn_agent_with_state_change`, full agent-launch success path (the one the
    CLI/API `spawn_agent` at `src-tauri/src/api/agent_routes.rs:218` actually hits)

Recommendation: instrument at the three `Ok(SpawnResult ..)` sites (role/model/brief-bytes
all in scope there), not the shared low-level fn — resolving that would require threading
role+model back down through `SpawnPaneSpec`, which the existing code doesn't do.

### Nudge / heartbeat scheduler

`src-tauri/src/service/hierarchy/plan_reminder_nudge.rs:440` —
`deliver_automated_plan_reminder`. Every nudge variant (idle poke `:229`, timed plan
reminder `:419`, complete-wrapup `:349`) funnels through this one function before calling
`Messenger::deliver` at `:442`. `text.len()` gives bytes; the caller already knows which
`NudgeKind` fired. Heartbeat re-injection specifically is
`crate::service::orchestrator_digests::prepend_orchestrator_heartbeat`, called from
`plan_reminder_nudge.rs:223` (idle poke path) and `:408` (owner-targeted reminder path) —
count/size these calls separately from plain nudges for the "heartbeat re-injection count
and bytes" KPI line.

### Provider usage ingestion — see §3 (separate seam per provider, not a single choke point)

## 3. Per-provider usage-field extraction

Verified against real local session files on this machine during this design pass
(field shapes only reproduced below; no third-party file paths or message content
included).

### Claude — native, already implemented

`src-tauri/crates/codeterm-plugin/src/agent_claude/provider/usage.rs:17` (`read`) /
`:25` (`read_from_path`) already parses `~/.claude/projects/<encoded-cwd>/<session>.jsonl`
line-by-line, re-parsing the whole file each call (its own doc comment flags this as a
phase-1 shortcut — phase 3 needs incremental tail-parse by mtime + byte offset, not a
rewrite of this reader).

Real per-line shape (`message.usage`):
```json
{
  "type": "assistant",
  "timestamp": "2026-07-17T13:01:25.834Z",
  "message": {
    "role": "assistant",
    "model": "claude-opus-4-8",
    "usage": {
      "input_tokens": 2,
      "cache_creation_input_tokens": 24382,
      "cache_read_input_tokens": 25382,
      "output_tokens": 204
    }
  }
}
```
Fields map directly to `UsageSnapshot` (`usage.rs:54-57`).

**Compaction** — confirmed real marker, not yet detected anywhere in the codebase:
```json
{"type":"system","subtype":"compact_boundary","content":"Conversation compacted",
 "compactMetadata":{"trigger":"auto","preTokens":944410,"postTokens":413282,
                     "cumulativeDroppedTokens":531128,"durationMs":101922}}
```
`trigger` is `"auto"` or `"manual"`. One line per compaction event — trivial to count on
the same tail-parse pass.

### Codex — no native reader exists; the research-bundle script is the reference implementation

There is **no** Rust or JS code today that parses Codex rollout jsonls for token usage.
`plugins/provider-codex/src/usage.ts` (`parseCodexUsage`) is unrelated — it screen-scrapes
the TUI's percentage-remaining status line for the account-level rate-limit gauge (the
existing `codeterm usage` feature), not per-turn token counts. Do not reuse it or its file
for KPI ingestion.

The real source is the rollout jsonl (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`,
one continuous file per Codex session). Real per-line shape:
```json
{
  "timestamp": "2026-05-21T06:48:35.288Z",
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "last_token_usage": {
        "input_tokens": 12642,
        "cached_input_tokens": 11136,
        "output_tokens": 164,
        "reasoning_output_tokens": 45,
        "total_tokens": 12806
      }
    }
  }
}
```
One `event_msg:token_count` line per API turn — this line IS the "turns" counter too
(spec KPI #1 and #2 share a source for Codex). The research-bundle's
`codex_rollout_extract.py` (see §6 prerequisite) implements this parse and is a validated
starting point, not the spec — its own README documents counting-rule deltas vs the
manual autopsy that phase 5 must resolve by matching the autopsy's rules, not the
script's.

No compaction marker was found in local Codex corpora during this pass — Codex context
compaction detection is an **open decision** (see §7).

### OpenCode — no reader exists; storage is sqlite, not jsonl

OpenCode has no per-session jsonl at all. Session/message/token data lives in one sqlite
db at `~/.local/share/opencode/opencode.db` (path resolution in
`plugins/provider-opencode/src/plugin.ts:784`, `opencodeDbPath()` — `$XDG_DATA_HOME` or
`~/.local/share`, `opencode` subdir, forced even on Windows). Confirmed present and
queryable on this machine (993 message rows). Relevant tables: `session` (id, directory,
title, time_created/updated) and `message` (id, session_id, time_created/updated, `data`
JSON blob) — the existing `enumerateSessions`/`detectSessionModel` JS methods already
query `session`/`message` but only for session discovery and model detection, never token
totals.

Real `message.data` shape for an assistant row with usage:
```json
{
  "role": "assistant",
  "modelID": "big-pickle",
  "providerID": "opencode",
  "cost": 0,
  "tokens": {
    "total": 23895,
    "input": 279,
    "output": 73,
    "reasoning": 247,
    "cache": { "read": 23296, "write": 0 }
  },
  "time": { "created": 1784633081000, "completed": 1784633087961 }
}
```
Ingestion here means a sqlite query (`SELECT data FROM message WHERE session_id = ?`),
not a tail-parse — a materially different code path from Claude/Codex's line-oriented
readers. No compaction concept found for OpenCode in this pass — flag as open decision
(§7); OpenCode's context handling may not expose one the same way.

## 4. Storage decision — SQLite table, not JSONL sidecar

**Recommendation: a new sqlite table**, following the existing `OrchestrationLedger`
pattern (`src-tauri/src/agent/ledger.rs`) rather than a jsonl sidecar under the plan dir.

Rationale:
- `rusqlite` is already a direct dependency and already used for exactly this shape of
  problem — an append/upsert-heavy, schema-versioned, per-effort ledger
  (`CURRENT_LEDGER_SCHEMA_VERSION` at `ledger.rs:12`). No new dependency, no new pattern.
- The report choke point (§2) already writes a durable sqlite row on every report; adding
  KPI columns/rows there is strictly less work than standing up a parallel jsonl writer.
- Spec requires "aggregation at query time" (§Collection) — SQL `GROUP BY` over an
  indexed table is a better fit than re-scanning a growing jsonl file per query, especially
  once `kpi show --window` needs arbitrary time-bucketing.
- Concurrent access: sqlite's single-writer/multi-reader model matches "daemon is the only
  writer, CLI/API/canvas are readers" exactly; a jsonl sidecar would need the same
  single-writer discipline manually (file locking or a channel to one writer task) to avoid
  torn writes, buying nothing over sqlite for the cost of reimplementing what sqlite already
  does.

Where jsonl is still correct and stays: **provider session logs themselves** (Claude/Codex
jsonl, read-only inputs) and **backfill mode**, which by definition parses historical jsonl
into the ledger — that's an ingestion format, not the ledger's own storage.

Tradeoff acknowledged: a jsonl sidecar is trivially `tail -f`-able and needs no schema
migration story. Given schema versioning is already a hard requirement (spec Postulates)
and the codebase already has a versioned-sqlite-ledger pattern to copy, this tradeoff
favors sqlite.

## 5. No-LLM collection invariant

Every choke point in §2 is a plain Rust function on the daemon's existing execution path —
no counter here calls an agent, a provider API, or an LLM. Token/usage ingestion (§3) reads
provider-written session files/db that already exist on disk; it does not call the
provider's API either. This matches the plan constraint ("No LLM calls anywhere in the
collection path") by construction: nothing in this inventory adds a network call, only
struct field reads and a sqlite write.

## 6. Backfill / autopsy-reproduction validation path

Acceptance (owned by `task_cmu92w`, phase 5) is: run backfill over the pinned 37h Hugo
corpus and match the manual autopsy within 10% on turns/polls/reports/heartbeats/ratio.
This inventory does not re-embed the corpus's absolute file paths (machine-specific,
outside the repo) — the **local research prerequisite** for whoever picks up phase 5 is:

> `C:/Users/User/.codeterm/general/research/orchestration-economics-bundle/` — README with
> the full pinned corpus definition (exact orchestrator rollout file, UTC time window,
> lane glob, inclusion/exclusion rules), raw evidence extracts, and the two reference
> scripts (`codex_rollout_extract.py`, `claude_lanes_stats.py`) whose counting-rule deltas
> vs the manual autopsy are documented in the bundle's own README and must be resolved
> toward the autopsy's rules, not the scripts'.

Phase 5's backfill parser is the second of exactly two places session-jsonl parsing is
allowed in this effort (the first being live token-usage ingestion, §3); it is a one-time
calibration against hand-counted ground truth, not the operating mode — the operating mode
is the live daemon counters at the choke points in §2.

## 7. Open decisions for later phases

- **Codex/OpenCode compaction detection** has no confirmed marker (Codex) or confirmed
  concept (OpenCode) yet — phase 3 needs to either find one or explicitly scope
  "compactions" as Claude-only in v1 of the catalog.
- **Spawn instrumentation layer**: recommended the three `Ok(SpawnResult ..)` sites over
  the shared low-level `spawn_pane_with_ready_and_state_change` (§2) — phase 3 implementer
  should confirm no other success path exists (e.g. resumed-session spawns) that bypasses
  all three.
- **Ledger location**: reuse `orchestration.db` (new table, same file) vs. a dedicated
  `kpi_ledger.db` under the same `data_dir` — both satisfy §4's reasoning; phase 2 should
  pick based on whether KPI schema migrations should be able to run independently of the
  report-ledger's.

## 8. Future focused verification (not run in this phase)

Once phases 2–5 land, the reviewer/implementer runs (each scoped, capped, lane-local —
never a full workspace build for this alone):

```bash
scripts/test-capped.sh cargo test -p codeterm --lib agent::ledger:: -- --nocapture
scripts/test-capped.sh cargo test -p codeterm --lib service::hierarchy::plan_reminder_nudge:: -- --nocapture
codeterm kpi show --plan pl_dwojf1 --json
codeterm kpi events --plan pl_dwojf1 --json
```
Phase 5's own gate is the automated within-10% comparison script described in
`task_cmu92w`'s test plan — not re-specified here since it belongs to that task.

<!-- revision: 1 -->
