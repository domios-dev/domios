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

## Attachment & visibility model

**The board is workspace-scoped; tasks are owned by their requester pane.**
A task = (workspace, requester pane, assignee pane). Rationale: workspaces are
already the project boundary (cwd, agent roster); "the swarm working on project X"
is exactly one workspace today. The Tasks tab shows the ACTIVE workspace's board.

Role-based visibility (daemon-filtered, not UI-filtered):
- **Orchestrator pane** (requester): sees every task it created — full board,
  all states, all reports. `task list` from an orchestrator pane defaults to
  `--requester self`.
- **Worker pane** (satellite): sees ONLY tasks assigned to it — `task list` from
  a Worker pane is forcibly scoped to `--assignee self`. A worker cannot read
  another worker's brief or report (keeps briefs self-contained by construction).
- User in the UI: sees everything (it's their machine).

## Orchestrator ergonomics — the system reminds the orchestrator it orchestrates

Live finding: over a 30h session the orchestrator's own context compacts and
drifts; nothing in the system re-anchors the role. Fixes, all daemon-driven:

1. **Digest injection.** On events (report arrived, lane Dead, task Blocked) and
   on a quiet-period timer (default 20 min with non-terminal tasks outstanding),
   the daemon injects into the orchestrator's pane a one-liner digest message
   carrying a `-=-codeterm:task-digest` marker:
   `[Tasks] 2 working · 1 REPORT UNREAD (F7) · 1 lane silent 9m (Spark/E4) — task list`
   This is the anti-amnesia device: the role reminder arrives as workflow, not as
   prompt text the agent must remember to obey. Survives compaction by definition
   (it keeps arriving).
2. **Session re-anchor.** When an orchestrator pane's agent session restarts or
   compacts (`SessionStart` hook already exists), the daemon re-injects a board
   snapshot: "You are the requester of N open tasks: ...".
3. **Unread-report discipline.** Reports land as messages AND set an unread flag;
   the digest nags until `task show ID` (or UI open) clears it. No more reports
   scrolling away between user messages.
4. **Orchestrator action-nudges (symmetric watchdogs).** The queue must drain;
   stagnation that only the requester can resolve gets escalating, action-phrased
   nudges into the orchestrator's pane (not mere info):
   - report unread > `T_review` (default 10 min) → "verify and close F7 — its
     lane is idle waiting on you";
   - task Blocked > `T_decide` → "decision needed on E4: unblock, reassign or
     cancel";
   - task Dead → immediate "re-dispatch or cancel X";
   - queued unassigned tasks while lanes sit idle → "2 queued, 3 idle lanes —
     dispatch".
   Escalation chain mirrors the worker side: nudge ×2 → if the orchestrator pane
   itself stays unresponsive, a warning NOTIFICATION to the human user
   ("your orchestrator is stalled: 3 undispatched tasks, 1 unread report").
   The human is the final watchdog; the system never silently waits on a
   forgotten orchestrator.

## Worker ergonomics

1. **Envelope** (delivery message) carries the brief + the two commands the worker
   must run (`task accept`, `task report ...`) — self-contained, works for agents
   with zero CodeTerm knowledge.
2. **Pinned current task.** Pane status bar shows a task chip (id + state + age);
   `$CODETERM_TASK_ID` env is set on the pane while a task is Accepted/Working.
3. **Restart/compaction re-anchor.** On worker session restart with a held task,
   the daemon re-injects: "You hold task X (state Working, heartbeat overdue
   3m). Brief: <path>. Continue or report blocked." — kills the
   finished-but-never-reported and restarted-and-forgot classes.
4. **Status-check pings** are the nudge machinery from the state machine — same
   message shape, marker-tagged, so ChatUI renders them as part of the task card
   thread, not as loose text.

## Self-teaching messages — every queue message re-teaches the codeterm way

Drift risk (live-observed class): agents have INTERNAL work-tracking tools
(Claude's TodoWrite/Task tool, codex planners) and will silently route work
through them — invisible to the board, the state machine, and the user. Prompt
rules against this fade with context; therefore **every queue-generated message
embeds its own one-line counter-drift hint + the exact next command**:

- Envelope footer: `Track THIS task only via codeterm: accept -> codeterm task
  accept <id>; close -> codeterm task report <id> ... Internal todo/task tools
  do not count as done.`
- Worker nudge: `Reply via codeterm task report <id> — work tracked anywhere
  else is invisible and will be marked Dead.`
- Orchestrator digest/action-nudge footer: `Dispatch and close ONLY via
  codeterm task create / task show — subagents or internal todos bypass the
  board and lose watchdog protection.`
- Re-anchors include the full command pair verbatim.

Implementation detail: all message templates live as prompt files in
`src-tauri/prompts/task_*.md` (envelope, nudge_worker, nudge_requester, digest,
reanchor) — automatically covered by the prompts 3-way source-of-truth CI gate,
and A/B-testable via the prompt-evaluator skill like any other prompt.

## ChatUI integration — tasks are cards, not text

Reuse the proven marker→card pipeline (`-=-codeterm:*` markers are already parsed
by packages/chat-ui recognize/CodetermCard and hidden-raw — the cross-pane chat
tests cover the pattern). New marker kinds, all hidden-raw in transcripts:

| Marker | Rendered card | Where it appears |
|---|---|---|
| `-=-codeterm:task` (envelope) | TaskCard: id chip, brief summary (first line), state badge, accept-command copy button | worker pane chat |
| `-=-codeterm:task-progress` | thin progress row appended to the same TaskCard thread | both panes |
| `-=-codeterm:task-report` | ReportCard: status pill (done/blocked), changed-paths list, verified lines, proposed-commit block with copy | orchestrator pane chat |
| `-=-codeterm:task-digest` | BoardCard: counts row + per-task chips, each chip click = `task show` | orchestrator pane chat |
| `-=-codeterm:task-nudge` | amber reminder row in the TaskCard thread | worker pane chat |

Component-test surface is the same harness as MessageBubble.providers.test.tsx —
fixtures per marker kind, structure/interaction asserts, no Playwright needed for
the rendering layer.

## Gaps closed in v4 (second design pass)

1. **Brief immutability.** A brief passed as `@file` is SNAPSHOTTED into the task
   record at create time (the file is a convenience input, not a live reference) —
   otherwise branch switches/edits silently change a delivered task's contract.
   `task show` always renders the snapshot.
2. **Telegram escalation.** The existing bridge becomes an optional escalation
   transport: Dead/Blocked/stalled-orchestrator events and the daily digest can
   mirror to the paired Telegram user (`[tasks] notify_telegram = true`).
   Managing the swarm from the phone was already proven by bridge usage; this
   closes the loop for away-from-keyboard supervision.
3. **Per-lane serial queue + reassignment.** One ACTIVE task per pane; further
   tasks assigned to the same pane queue FIFO and auto-deliver on close (the
   proven Haiku pattern). `task reassign ID --assignee P` re-delivers a Dead/
   Blocked task's snapshot to another lane, preserving history (re-dispatch was
   manual copy-paste today).
4. **Transition audit log + retention.** Every state change/nudge appends to a
   per-task timeline (who/what/when) — post-mortems read the timeline, not pane
   scrollback. Done/Dead tasks auto-archive off the default board after
   `archive_after_days` (default 7), queryable via `task list --archived`.
5. **Nudge economy.** Nudges into LLM panes consume the worker's context and the
   user's tokens: coalesce multiple pending nudges into one message, and respect
   `[tasks] quiet_hours` (e.g. 01:00-08:00 — no nudge delivery, watchdog clocks
   pause; dreamer hours stay silent).
6. **UI modes.** Desktop: full board tab + pane badges + chat cards. Web mirror:
   same board, read-only actions in v1.5.0. Mobile web: board collapses to a
   status-chip list (state + age), cards in chat render normally; no create/
   reassign on mobile in v1. Requesters are always agent panes — the user supervises
   via board/notifications/Telegram, never creates tasks directly (cut by user).

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
- ChatUI cards: component tests per marker kind (TaskCard/ReportCard/BoardCard/
  nudge row) on the happy-dom harness — fixture markers in, DOM structure +
  copy-button interactions asserted; raw marker MUST be hidden (the cross-pane
  test pattern). One Playwright smoke: envelope→accept→report round-trip renders
  both cards in both panes' ChatUI.
- Ergonomics e2e (debug clock): quiet-period digest fires only when non-terminal
  tasks exist; unread-report nag repeats until shown; worker restart re-anchor
  message arrives after a simulated session restart (session-restore harness).
- Visibility e2e: `task list` from a Worker-role pane never returns a foreign
  task (negative assert, not presence-only).

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

<!-- revision: 6 -->
