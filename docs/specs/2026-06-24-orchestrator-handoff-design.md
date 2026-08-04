# Orchestrator Handoff — directed context reset attached to a plan

<!-- revision: 2 -->

**Design proposal only.** A DIRECTED HANDOFF capability: an orchestrator composes a
focused, goal-directed handoff aimed at its active plan, attaches it to the plan as
provenance, renders it on the canvas, and reseeds a FRESH context in the same Tab
that resumes from the handoff. Far sharper than a generic `/compact` summary.

Grounding (verified in code):
- `Plan` already carries `owner: OrchestratorRef` + `owner_history: Vec<TransferRecord>`
  (`{from,to,ts,reason}`) — an EXISTING provenance/audit trail (`plan/types.rs:70,258`).
- `CanvasElement { id, kind: String, canvas{x,y,collapsed}, content: serde_json::Value,
  links: Vec<String> }` is a generic plan-level board node, rendered by `nodeRegistry`'s
  open-kind fallback (`plan/types.rs:207`). New node kinds need NO new persistence.
- `Milestone { from, kind, text, ts, links, loop_id, iteration }` is task-interior; `kind`
  must be declared in `plan.milestone_kinds` (validated, `cli/cmd/plan.rs:93`).
- `agent.report` RPC already accepts `tab_id, message, kind, plan_id, task_id, status,
  branch, sha, changed_files, test_cmd, test_result, skipped` (`cli/cmd/agent.rs:299`).
- `agent compact` sends `/compact\n` to a Tab via the send path (`cli/cmd/agent.rs:505`).
- §17 externalized bodies: large text lives in sibling `.md`, hydrated by the single-writer
  + watcher; `INDEX.md` regenerated on write.

**Core decision: reuse, do not invent.** A handoff = a `CanvasElement` of kind `handoff`
(structured fields in `content`, large body in a `.md` sidecar) + a `TransferRecord` in
`owner_history` (context generation changed, same Tab) + one declared `handoff` color
lane. Zero new persistence path; renders for free; archive-cascades for free.

---

## 1. Handoff doc schema

A handoff must let a zero-context agent resume the plan. Fields (structured part lives in
`CanvasElement.content`; the narrative body lives in the `.md` sidecar):

```jsonc
// CanvasElement.content for kind:"handoff"
{
  "seq": 3,                       // handoff #N for this plan -> timeline ordering
  "mode": "new" | "compact",      // continuation mode used
  "from": { "tab_id", "session_id", "title" },   // the context being retired
  "ts": 0,
  "goal": "one-to-two lines: what we are building",
  "plan_id": "...", "plan_title": "...", "plan_handle": "...",
  "state": { "coverage_pct": 72, "done": 18, "total": 24, "focus_section": "sec_..", "phase": "v3 layout" },
  "in_flight": [                  // live agents + branch state at reset
    { "task_handle": "nav-pan-zoom", "status": "in_progress",
      "assigned_tab_id": "..", "branch": "feat/canvas-nav", "sha": "abc1234", "dirty": true }
  ],
  "next_actions": ["...", "..."], // ordered resume queue (the sharp part)
  "foundational_rules": ["§22.1 zoom=camera, bounds fixed", "..."],  // non-negotiables for THIS plan
  "open_decisions": ["scroll-vs-pan on W2 still open"],              // unresolved decision milestones
  "branch_state": { "branch": "feat/canvas", "head_sha": "1c22b98e", "dirty": false },
  "links": ["plan:<id>", "doc:docs/specs/2026-06-24-canvas-v2-design.md",
            "handoff:<prev_seq>", "task:<handle>", "section:<id>"],
  "body_path": ".codeterm/plans/<plan_id>/handoffs/0003-canvas-orch.md",  // §17 sidecar
  "resume_prompt": "...",         // generated; the exact seed for the fresh agent
  "reseed_state": "pending" | "sent" | "acked"   // §3 idempotency
}
```

The `.md` sidecar body = the human/agent-readable narrative (free-text from the
orchestrator): why we're here, what just happened, traps to avoid, nuance the structured
fields can't hold. `RichMarkdown`-rendered on the canvas (W3).

**Generation = template + live plan state + orchestrator free-text.** A pure
`compose_handoff(plan, from_tab, narrative)` in `service/plan/handoff.rs`:
- AUTO from the in-memory plan index: `state` (coverage/done/total), `in_flight` (tasks
  not done/cancelled + their latest branch/sha pulled from `agent.report` milestone
  provenance), `open_decisions` (open `decision`-kind milestones), `branch_state` (git),
  `links` (prev handoff -> timeline chain).
- FREE-TEXT from the orchestrator via `--from-file`/`--stdin`: `goal`, `next_actions`,
  narrative body. Flags override individual fields.
- `resume_prompt` synthesized from a fixed template (see §3).

**Storage & attachment.** Structured fields -> `plan.elements` (in the plan JSON, single
writer). Body -> `.md` sidecar, listed in `INDEX.md` by handle. Appears in HISTORY two
ways: (a) the element's `seq`+`ts` form a timeline on the board; (b) a `TransferRecord`
appended to `owner_history` (`from`=retired context, `to`=fresh context, `reason="handoff
#3: <goal>"`) so the existing audit trail records every reset.

---

## 2. CLI surface

```
codeterm plan handoff create <plan_id> [--mode new|compact]
        [--goal <text>] [--next <text>]* [--rule <text>]*
        [--from-file <f> | --stdin]          # narrative body
        [--auto]                             # auto-fill state/in_flight/branches (default on)
        [--json]                             # returns {handoff_id, seq, body_path, resume_prompt}
codeterm plan handoff show  <plan_id> [<selector>|--seq <n>|--latest] [--json]
codeterm plan handoff list  <plan_id> [--json]
codeterm orchestrator handoff [--plan <id>] [--mode new|compact] [--reseed] [--from-file|--stdin]
        # one-shot for the CALLING tab: compose + attach + render + (optionally) reseed self
```

- `--auto` composes the live/computed fields from plan state; `--from-file`/`--stdin`
  supplies the narrative; explicit flags win over both. With no narrative it still produces
  a valid handoff from plan state alone (graceful).
- `plan handoff create` does the COMPOSE+ATTACH only. `orchestrator handoff --reseed` is the
  full self-reset (compose -> attach -> `/new` -> seed), so the orchestrator triggers it
  whenever it wants, mid-process, in ONE command.
- `handoff show --latest` is what the FRESH agent runs first (§3) — pulls the handoff on
  demand instead of bloating the seed message.

**Reuse vs new entity — reuse, justified.** A handoff is a `CanvasElement` (kind `handoff`)
+ an `owner_history` `TransferRecord` + a `.md` sidecar + a declared color lane. That reuses:
storage, single-writer, the watcher/hydration, `plan-changed` fan-out, archive cascade
(plan elements hide with the plan), and the open-kind canvas renderer. A new top-level
entity would re-implement every one of those. The only new code is the COMPOSE function and
the render component — both genuinely new behavior.

**Native Rust + JS parity.** Compose/attach/reseed live behind ONE daemon RPC
(`plan.handoff.create`, `plan.handoff.reseed`). CLI and any JS drop-in both call the RPC —
no provider-id branching, capability-shaped (`supports_handoff()`), so parity is structural
(one code path). `parity_test.rs` asserts native and JS produce the byte-identical handoff
element + sidecar for the same plan fixture.

---

## 3. The `/new` self-reseed flow (mode `new`, PREFERRED)

`orchestrator handoff --reseed --mode new` drives, in the daemon:

1. **Compose + attach** the handoff (`reseed_state:"pending"`), write element + `.md`, emit
   `plan-changed` (marker appears on the board immediately, pulsing).
2. **`/new` the same Tab** — send `/new\n` to the orchestrator's OWN Tab via the existing
   send-to-tab path (the one `agent compact` uses). Fresh zero-context session, same tab.
3. **Detect fresh session ready** — wait for the Tab's new session id / prompt-ready signal
   (reuse tab-status plumbing), flip `reseed_state:"sent"`.
4. **Seed ONE message** via `codeterm send --tab <self>` — the generated `resume_prompt`,
   deliberately LEAN (does not inline the whole handoff):
   > You are the orchestrator resuming plan `<handle>` after a directed context reset
   > (handoff #3). Do this IN ORDER, do not re-plan:
   > 1. `codeterm plan handoff show <plan_id> --latest` — read it fully (goal, in-flight,
   >    next actions, rules, open decisions).
   > 2. Load the orchestration role (skill `codeterm-orchestrator`).
   > 3. `codeterm plan get <plan_id>` — current plan state.
   > 4. Resume from `next_actions`. Report progress as milestones (`plan record`).
5. **Guaranteed read** — step 1 IS the first instruction and the daemon withholds nothing
   else, so the fresh agent must pull the handoff before acting. The agent's first
   provenance write (`agent report --kind handoff-ack --plan <id>`) flips
   `reseed_state:"acked"` -> the marker stops pulsing, renders "resumed".

**Idempotency / safety.**
- `reseed_state` gates `/new`: only sent once per handoff (pending->sent). A re-fire is a
  no-op while sent/acked.
- One in-flight reseed per Tab (a lock) + a short cooldown -> no double-`/new`.
- NEVER auto-triggered by context-fill — always agent/operator-initiated, so no reseed loop.
- If `/new` fails (Tab busy), abort the send but KEEP the attached handoff (still valuable
  provenance); operator resumes manually. The handoff is durable independent of the reseed.

**Mode (a) compact-with-handoff** (`--mode compact`): send the composed handoff to the Tab
as a message, THEN `/compact`, so the summarizer is biased toward the goal instead of a
generic recap. Pro: preserves in-context working memory; no cold start. Con: `/compact` is
still lossy and costs tokens to re-summarize, and the auto-summary competes with the
handoff for the resulting context.

**Recommendation: mode `new` (b).** The plan is the source of truth (§2 #1 — plans stay
JSON, authoritative); context is disposable. A fresh context pointed at a sharp,
plan-anchored handoff is cheaper and more focused than re-summarizing a bloated window.
Keep `--mode compact` as the fallback for when the orchestrator wants to preserve
in-context working memory mid-task without a full reset.

---

## 4. Canvas render — orchestrator handoff timeline lane

The handoff is plan/orchestrator-level provenance, NOT task-interior — so it renders in the
**orchestrator region** (§2.5 / §23.3), not in the SPEC|PLAN|task columns.

- **Marker = a distinct `handoff` node** (a pill/coin, not a doc or task node): unmistakable
  glyph (⎌ reset for `new` / ✂ for `compact`), `#seq`, ts, from-context title, mode badge.
- **Timeline lane:** handoffs sit on a horizontal rail along the orchestrator-region header,
  chronological L->R, joined by a thin `links` edge (`handoff[n-1] -> handoff[n]`) so a run
  of resets reads as a literal TIMELINE of context generations.
- **Content-LOD (§26 W0–W3), fixed footprint:** W0 = coin + `#seq` + status ring; W1 = +
  goal one-liner + from→to; W2 = + `next_actions` + in-flight agents/branches; W3 = full
  `.md` body (`RichMarkdown`) with node-internal scroll. Same appW-driven grammar as every
  other node — no clicks, zoom reveals.
- **Edges:** handoff -> plan (provenance) + handoff -> the in-flight tasks it referenced (you
  see which tasks were live at the reset). Coverage/edges still drawn to dimmed neighbors.
- **Status emphasis (§24.5):** the LATEST handoff pulses while `reseed_state` is
  pending/sent (a reset is in progress); once `acked` it dims to ~0.7 like settled history.
- **Unmissable but not dominant:** at overview it's a coin on the region header (always
  visible as a marker); it only reads in full when you zoom to it. It never competes with the
  plan/spec columns for space.

Alternatives: (A) milestone of kind `handoff` on a synthetic orchestrator task — REJECTED:
buries a plan-level event inside a task interior. (B) a full handoff doc-node column —
REJECTED: too dominant. (C) **orchestrator timeline lane of handoff coins** — RECOMMENDED:
right altitude (orchestrator region), scales as a timeline, compact yet unmissable.

---

## 5. Dogfood + test plan

**Dogfood.** This feature is itself a plan in this canvas: lanes = {schema/compose,
batch-record + endpoint, CLI surface, reseed flow, canvas render}, each a task with model +
rationale, reports chaining as milestones. Once built, the orchestrator runs
`orchestrator handoff` on its OWN plan and the marker appears on the board — extending §11's
self-rendering requirement to handoffs. (Do NOT fire a real reseed in the build session —
feature-build + separate testing only.)

**Tests.**
1. **Compose unit (Rust):** plan fixture with coverage+tasks+branches -> `handoff create
   --auto` populates every required field; `.md` sidecar written + re-hydrated; `INDEX.md`
   updated; `TransferRecord` appended.
2. **Resume sufficiency (the key eval, model-judged like `evals/cli-agent`):** feed ONLY the
   handoff doc to a fresh agent (no other context) and assert it can correctly answer goal /
   in-flight / next-action / branch. This proves the handoff captures enough to resume — add
   as `evals/orchestrator-handoff/`.
3. **Reseed integration (dev daemon):** `orchestrator handoff --reseed` against a dev Tab;
   assert `/new` sent, NEW session id (fresh context), seed message sent, fresh agent's first
   action is `plan handoff show` (transcript/report assert), `reseed_state`->`acked`.
   Idempotency: fire twice -> exactly one `/new`.
4. **Canvas render (Playwright, §26.7 matrix):** seed a plan with 2 handoffs; assert handoff
   nodes on the orchestrator lane, timeline link drawn, body reveals at W2/W3, latest-pending
   pulses, fixed footprint across zoom, no overlap with plan/spec/task columns.
5. **Batch-record (§6):** one `plan record` equals the N-call sequence it replaces (assert
   resulting plan state identical, call-count = 1, single `plan-changed` emit).

---

## 6. Dogfood-recording ergonomics — collapse N calls to one

**Friction today.** Recording one "decision + create task + set body + milestone":
- (declare milestone kind if new) `plan kinds set` — 0–1
- `plan task add` — 1
- set body `plan task update --body` (or write `.md`) — 1
- `plan milestone add` (the decision) — 1
- often a second milestone (e.g. testing) — 1
- branch/sha provenance via `agent report` — 1

≈ **4–6 HTTP round-trips for ONE logical provenance write**, each its own `plan-changed`
emit. The orchestrator does this constantly -> verbose, high-friction, fan-out churn.

**Consolidated surface — `codeterm plan record` over a batch endpoint.**

```

<!-- revision: 1 -->
POST /api/plans/:id/batch  { "ops": [ {op,...}, ... ] }   # atomic, single-writer, ONE emit
  ops: upsert_task | set_body | add_milestone | declare_kind | link | add_handoff
```

```
codeterm plan record <plan_id> \
  --task <handle|new> [--name .. --type .. --section .. --model .. --status ..] \
  --body-file <f> | --body-stdin \
  --milestone "kind:text" [--milestone ...]*      # auto-declares the kind if missing \
  --link doc:<path> --branch <b> --sha <s> \
  [--json]                                         # returns {task_id, milestone_ids}
```

One call = upsert task (create-or-update by handle, **idempotent**) + set body (`.md`
sidecar) + attach N milestones (auto-declaring kinds) + link docs + record branch/sha,
applied atomically under the single-writer lock with ONE `plan-changed` emit (not N).

**Fold existing commands onto it (smallest surface, most calls removed):**
- `agent report` becomes a thin preset of `plan record` (report = upsert_task(status) +
  add_milestone(kind) + branch/sha) — same batch path.
- `plan handoff create` is just `{op:"add_handoff"}` in the batch — **a handoff IS a dogfood
  write**, sharing the atomic path and single emit.

So the whole provenance surface lowers to ONE endpoint: `POST /api/plans/:id/batch`, with
`plan record` (general), `agent report` (preset), and `plan handoff` (special op) as sugar.
6 calls -> 1, structured provenance fully preserved, single fan-out.

---

## Recommendations (summary)

1. **Schema/storage:** `CanvasElement` kind `handoff` (structured `content`) + `.md` sidecar
   (§17) + `owner_history` `TransferRecord`. Reuse, no new entity.
2. **CLI:** `plan handoff create|show|list` + `orchestrator handoff [--reseed]`, all lowering
   to the batch endpoint; capability-shaped RPC for native+JS parity.
3. **Continuation:** **mode `new` (/new self-reseed)** preferred; `--mode compact` fallback.
4. **Render:** orchestrator-region **timeline lane of handoff coins**, content-LOD W0–W3,
   latest-pending pulses.
5. **Ergonomics:** **`POST /api/plans/:id/batch` + `codeterm plan record`**, fold
   `agent report` and `plan handoff` onto it — 6 calls to 1.
```
