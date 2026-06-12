# Spec: `codeterm task` — native task queue for agent orchestration

Status: PROPOSED · Author: Lamarr (arbiter session 2026-06-11/12) · Phase target: v1.5

## Why (every requirement below was a live incident, not a hypothesis)

During the 2026-06 stabilization program a single orchestrator ran 7 worker lanes
for ~30h over raw `send-text`. The protocol that made it work was enforced only by
prompt text (`docs/review-2026-06/tasks/README.md`) and the orchestrator's
attention. Failures observed:

| Incident | Root cause | Queue feature that kills the class |
|---|---|---|
| Task F6 sat undelivered ~30 min, looked like "long work" | message landed after the codex session ended its turn; no ack tracking | delivery→ack state machine + auto-nudge |
| Worker finished 8 min of work, went idle, never reported | report discipline lived in prompt text only | report-due state + heartbeat timeout |
| Worker died silently to provider rate limits | no quota awareness in dispatch | limit-aware routing (phase 2) |
| Commit dropped by concurrent `reset`; foreign staged files swept into commits; scratch-branch switch orphaned a peer's commit | N writers, one working tree | worktree isolation (phase 3) |
| Orchestrator hand-verified every ack/nudge/report | no machine-readable task state | the queue itself |

Design principle: **the daemon enforces what prompts can only request.** A dumb
worker must be unable to hold a task in a wrong state silently.

## Data model

```
Task {
  id:          "task:" + 8-hex
  brief:       inline text | path to brief file (repo-relative)
  assignee:    pane_id | "auto"
  requester:   pane_id            // orchestrator; reports route here
  state:       Queued | Delivered | Accepted | Working | ReportDue | Done | Blocked | Dead
  created_at / delivered_at / accepted_at / last_heartbeat_at / closed_at
  report:      Option<TaskReport> // structured, see below
  nudges:      u8                 // auto-nudge count
  tags:        Vec<String>        // e.g. "llm", "docs", wave labels
}
TaskReport { status: done|blocked|partial, changed_paths: Vec<String>,
             verified: Vec<String>, notes: String, proposed_commit: Option<String> }
```

Persistence: same store as panes/sessions (survives daemon restart; tasks in
non-terminal states re-arm their watchdogs on boot).

## State machine (daemon-enforced)

```
Queued ── deliver (send-text w/ task envelope) ──► Delivered
Delivered ── worker `task accept` ──► Accepted        (watchdog: no accept in
Delivered ── T_ack expired ──► auto-NUDGE (max 2) ──► Dead + notify requester)
Accepted ── first heartbeat/progress ──► Working
Working ── T_heartbeat expired ──► auto-NUDGE ──► (no reply) Dead + notify
Working ── worker `task report` ──► Done | Blocked
Working ── worker announces finishing ──► ReportDue (grace T_report, then nudge)
any ── requester `task cancel` ──► Dead
```

Defaults (config `[tasks]`): `T_ack = 120s`, `T_heartbeat = 1500s`,
`T_report = 300s`, `max_nudges = 2`. Nudge = templated send-text into the
assignee pane referencing the task id; Dead = warning notification + board flag.
States change ONLY via daemon (CLI/REST/socket) — there is no "the orchestrator
believes" state.

## Surfaces

CLI (mirrors REST `/api/tasks/*` and socket `task.*`):

```bash
codeterm task create --brief <text|@file> [--assignee PANE|--auto-route]
                     [--tags llm,docs] [--ack-timeout 120]
codeterm task list [--state working|dead|...] [--assignee P] [--json]   # the board
codeterm task show ID                          # full state + report
codeterm task accept ID                        # worker-side
codeterm task progress ID "step 3 done"        # heartbeat + log line
codeterm task report ID --status done --changed a.rs,b.rs \
                     --verified "cargo test x -> ok" --notes "..." \
                     [--proposed-commit "fix(x): ..."]
codeterm task nudge ID | cancel ID             # requester-side manual overrides
```

The task envelope delivered into the worker pane is self-contained (mirrors the
proven brief protocol): brief body + "FIRST: codeterm task accept <id>" + report
command template. Workers without the codeterm skill still function — the
envelope carries everything.

UI: a Tasks board (sidebar tab next to Mesh): rows = tasks, columns = state,
assignee, age, nudges; Dead rows highlighted. Pane chips show a small badge when
the pane holds an Accepted/Working task.

Mesh: `--mesh PEER` on create/list — task envelope relays over the existing mesh
send path; state lives on the REQUESTER's daemon (single source of truth).

## Integration points (all existing subsystems)

- **Hierarchy/roles**: `task create` from a Worker is rejected (Workers don't
  spawn work — existing rule, now enforced); reports auto-route to requester
  pane like `agent report` does today. `agent report` stays for free-form;
  task reports are the structured lane.
- **G4 blocker classifier**: a Working task whose pane hits a known blocker menu
  flips the task to Blocked with the blocker name in notes — the board shows WHY
  a lane stalled (today: get-output + grep spinners).
- **Notifications**: Dead/Blocked → warning notification (existing surface).
- **Cron**: cron-spawned agents get their task created automatically, so
  scheduled work appears on the same board.

## Phase 2 — limit-aware auto-routing (`--assignee auto`)

Router inputs, all existing: registered providers + models (`agent models`),
usage-probe quota snapshots (`/usage` parsing already ships), pane busyness
(does the pane hold an Accepted/Working task), tag constraints (`llm` tasks may
not route to a quota-exhausted provider). Algorithm v1 is dumb on purpose:
filter eligible lanes → prefer idle → prefer largest remaining quota → round-robin
tie-break. A provider that just produced a rate-limit-shaped failure (detector:
provider TUI error fingerprints, structural per E6 rules) is cooled down for
`cooldown_secs`. No learned scheduling in v1.x.

## Phase 3 — worktree isolation (`task create --worktree`)

Spawn/assign gains: create `git worktree add .codeterm/worktrees/<task-id>` from
the current branch; worker cwd = the worktree; report gains `diffstat`; requester
gets `task merge ID` (fast-forward/cherry the worktree branch, delete worktree)
and `task discard ID`. Kills the shared-tree race class entirely. Out of scope
here: merge-conflict UX beyond "report conflict, leave worktree for a human".

## Testing strategy (per docs/contributing/writing-tests.md)

- State machine: pure Rust unit tests (all transitions + watchdog timing via
  injected clock — NOT real sleeps; this is the TTL-clock lesson, the task
  watchdogs MUST take the debug clock from day one).
- API e2e (non-llm): create→deliver→accept→report against eager shell panes;
  ack-timeout → nudge → dead path with debug clock advance; persistence across
  daemon restart (session-restore pattern).
- One llm-tier e2e: real claude worker accepts and reports through the envelope.
- Component: Tasks board rendering from fixture states (happy-dom pattern).
- Vacuity rules apply: nudge tests assert the nudge text arrived in the pane
  (split marker in template), not that a counter incremented.

## Out of scope (explicitly)

- No task dependencies/DAGs in v1 (waves stay orchestrator-side).
- No automatic retries of Dead tasks.
- No cross-requester task transfer.
- No replacement of `agent report` (coexists).
- No learned/LLM-driven routing.

## Phasing

1. **v1.5.0**: data model + state machine + CLI/REST + envelope + watchdogs +
   notifications + board (read-only) + tests. ~the value of the whole program.
2. **v1.5.x**: auto-routing + quota cooldowns + board actions (nudge/cancel).
3. **v1.6**: worktree isolation + merge queue.
4. Later: mesh-distributed boards, cron deep-integration, G4-blocker auto-unblock
   hooks.

<!-- revision: 1 -->
