# Implementation plan: `codeterm task` (spec: task-queue.md v2)

Status: READY TO EXECUTE after PR #143 merges · Branch: `task-queue` off `develop`
Process: the proven arbiter pipeline (docs/review-2026-06/tasks/README.md protocol —
brief files, arbiter-only git, fail-first tests, vacuity rules). Executor tiers:
codex lanes (Gpt5.5 ×2, Spark ×2 when un-limited), free-model probes for seam maps,
Claude lanes stay parked.

## Ground rules carried from the spec

- Watchdog timers take the injectable clock FROM DAY ONE (the TTL-xfail lesson):
  reuse the debug-clock plumbing cron already uses (`src-tauri/src/cron/scheduler.rs`,
  `/api/debug/clock`).
- State transitions only through the daemon; CLI/REST/socket are thin.
- Every wave lands with its tests (per docs/contributing/writing-tests.md); no wave
  merges red.

## Wave 0 — core domain (pure Rust, no I/O)

| Task | Scope | Tier | Effort |
|---|---|---|---|
| TQ-01 | `src-tauri/src/tasks/` module: Task/TaskReport types, state enum, transition fn table (pure: `(state, event) -> Result<state>`), watchdog deadline calculus on injected clock | codex | M |
| TQ-02 | TDD-first transition tests: every legal edge, every illegal edge rejected, nudge counting, deadline arithmetic across clock advances — written BEFORE TQ-01 lands (same lane, fail-first) | codex | M |
| TQ-03 | Persistence: schema + store (same SQLite home as sessions; reuse `session/store.rs` patterns), non-terminal tasks re-arm watchdogs on boot — test with store round-trip + simulated restart | codex | M |

## Wave 1 — daemon surfaces + delivery

| Task | Scope | Tier | Effort |
|---|---|---|---|
| TQ-10 | REST `/api/tasks/*` (routes.rs conventions, `(StatusCode, String)` errors) + socket `task.*` (socket/server pattern) + role-based visibility filter (Worker→assignee-only; reuse role from pane state) | codex | M |
| TQ-11 | CLI `codeterm task ...` (args/cmd split per cli conventions; mesh relay via api_map like cron) + cli-manifest.json + docs/CLI.md rows (cli-doctor gate will enforce) | codex | M |
| TQ-12 | Envelope delivery: render brief+commands template, deliver via existing verified-send path into assignee pane (eager-spawn if pane lacks PTY — LAZY-1 path); embed `-=-codeterm:task` marker | codex | M |
| TQ-13 | Watchdogs live: scheduler ticks (cron scheduler timer pattern), nudge send, Dead + notification (existing notify surface); e2e with debug clock: deliver→no accept→2 nudges→Dead, split-marker asserts on nudge text | codex | L |

## Wave 2 — ergonomics

| Task | Scope | Tier | Effort |
|---|---|---|---|
| TQ-20 | Digest engine: event-driven + quiet-timer; `-=-codeterm:task-digest` marker into requester pane; unread-report flags; suppress when zero open tasks (test the negative!) | codex | M |
| TQ-21 | Re-anchors: orchestrator + worker session-restart hooks (SessionStart path), `$CODETERM_TASK_ID` env on assignee panes; e2e via session-restore harness | codex | M |
| TQ-22 | G4 bridge: blocker classification on a pane holding a Working task → task Blocked with reason (unit on classifier output mapping; no new detection) | codex | S |

## Wave 3 — ChatUI + board

| Task | Scope | Tier | Effort |
|---|---|---|---|
| TQ-30 | Marker kinds in `message_markers.rs` + chat-ui `recognize` (the cross-pane marker pattern + its tests as template); raw markers hidden | codex | M |
| TQ-31 | Cards: TaskCard / ReportCard / BoardCard / nudge row in packages/chat-ui (CodetermCard family); component tests per card on the happy-dom harness, copy-button interactions asserted | codex | M |
| TQ-32 | Tasks board tab (Sidebar, MeshPanel as the structural template): board table, unread badges, pane chips; component-level tests for board rendering from fixture states | codex | M |

## Wave 4 — system proof + docs

| Task | Scope | Tier | Effort |
|---|---|---|---|
| TQ-40 | E2E pack: full lifecycle vs eager shell panes (non-llm); visibility negatives; restart persistence; one Playwright smoke: envelope→accept→report cards render in both panes' ChatUI | codex | M |
| TQ-41 | One llm-tier e2e: real claude worker accepts + reports through the envelope (deliberate run only) | codex | S |
| TQ-42 | Docs: API.md rows (check-api-docs gate enforces), codeterm skill section (B10 source + sync), CLAUDE.md one-paragraph + AGENTS.md regen, TESTING.md runbook row | codex | S |
| TQ-43 | Dogfood gate: the arbiter (me) runs ONE real wave of any follow-up work entirely through `codeterm task` instead of send-text — exit criterion for v1.5.0: zero protocol violations the queue failed to catch | arbiter | — |

## Wave 5 — phase 2 (separate milestone, do not start until v1.5.0 ships)

TQ-50 auto-routing (usage-probe quotas + busyness + cooldowns); TQ-51 board actions
(nudge/cancel from UI); then phase-3 worktrees per spec.

## Risks

- **Send-path coupling**: envelope delivery rides verified-send; W0's fire-and-forget
  contract means delivery≠read. Mitigation: Delivered state means "send ok", the
  ack watchdog covers the rest (that IS the design).
- **Marker pipeline churn**: chat-ui recognize is shared with cross-pane chat —
  TQ-30 must keep existing marker tests green (they exist and are honest).
- **Scope creep via ergonomics**: digest/re-anchor tuning is endless; v1.5.0 ships
  with defaults, tuning is data-driven later (the dogfood gate produces the data).

## Estimate shape

13 dispatchable tasks; waves 0–1 are the daemon spine (likely the long pole),
waves 2–4 parallelize across lanes. With the current pipeline cadence this is
single-digit days of arbiter sessions, not weeks.

<!-- revision: 1 -->
