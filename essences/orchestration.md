# Orchestration: intent, authority and continuity

A Team carries the owner's intent through a goal, specification, plan, bounded tasks, reports and review. Planner state is the source of truth. The [core](core.md#core:plans-and-orchestration) owns this responsibility; the [capability map](capabilities.md) records its entry points and delivery status.

## Roles and authority

The Orchestrator coordinates, dispatches and verifies. An optional Planner shapes tasks; when planning is waived, the Orchestrator does so. Adopting a plan into a Team binds its unique live Planner. Only the Orchestrator approves a plan, dispatches work, accepts, bounces or holds a task in review, and marks a task done.

Workers execute bounded tasks and never spawn. Only two hierarchy levels exist. Every task carries acceptance criteria and an executable test plan; executors read its applicable plan skills. The Planner assigns the task's executor role, role guidance, one-word personality and model, or the Orchestrator does when planning is waived. Executor role, personality and role guidance are required and cannot be blanked. The spawned Worker keeps its persona name; a runtime role overlay cannot grant spawn or Team authority.

A Team may attach one optional Cartographer Manager for stable product contracts and the generated map. The Cartographer is a non-anchoring singleton with no Team-management, plan or spawn authority, and is removed with its Team. [Operator and agent access](permissions.md) are separate from these role responsibilities.

The owner can start the Cartographer from the workspace Map view with Create map or Refresh map, choosing focus sources, provider and model, and an optional typed or dictated project description. The request prefers a workspace Team with live agents, then its most recent Team, and creates a Team with an Orchestrator when neither exists. A retained Team without a staffing manager receives an Orchestrator. An attached Cartographer receives the brief instead of a second attachment. Asking the Orchestrator remains available.

## Dispatch, reports and review

Dispatch assigns the task and delivers its brief to the Worker. Local reassignment requires verified live binding and a fresh acknowledgement. Dispatch and respawn explicitly reopen blocked tasks while recording the previous reason; respawn preserves prior review evidence. A reopened task reuses its retained lane. For tool-less Workers, the host attaches declared editable/read-only task files and the attempt's exact skill snapshots through provider capabilities, and delivers the complete brief as one user turn. Unsupported or unreadable context refuses dispatch. The Orchestrator can inspect bounded completed turns from the Worker's authenticated session without reading provider-specific files.

Every Worker turn ends in a report. The report is pushed to the Orchestrator, including across the mesh for a remote Worker. A refused report is a diagnostic, not a recorded task report. A report is input and is never replied to; acceptance, bounce or hold is the Orchestrator's answer. A submitted result is not an accepted result. Tool-less Workers acknowledge receipt through exact own-session user-turn evidence. Their completed report-marked replies become reports; unmarked completed replies remain routed answers. Completion is provider-authoritative and never inferred from the terminal screen.

A bounce carries its reason and failing evidence. It returns work to a live Worker when continuation is available; otherwise it reopens the task as pending, preserving the note, attempt history and lane for redispatch. Ambiguous attempts refuse rather than assigning ownership by guess.

A delivered task-bound report ends with one verdict reminder naming its plan, task, accept/bounce command and report time. The reminder is advisory and never blocks dispatch, integration or other work.

## Isolated work and verification

A mutating Worker writes only its own isolated lane unless the Orchestrator explicitly opts one Worker into canonical. That opt-in is exclusive, and lane acceptance waits for its verdict. Lane Workers may run isolated checks that do not rebuild the whole project; full rebuilds, full suites, dev services, packaging and self-install stay out of lanes. The Orchestrator accepts on review and lands accepted lanes by real ancestry-preserving merges, then reruns each merged wave's focused selectors as one canonical batch; a canonical failure becomes a bounce or fix task rather than holding the acceptance. The broad gate runs once, on the release SHA. A lane is removed only after its ancestry is proven integrated.

## Pause and continuity

Plan pause is the only agent-issued pause. Paused tasks are system holds; there is no separate pause command for an individual task. Reminders never resume a paused plan.

A Worker that dies while holding an in-progress task produces an immediate structured event to its Orchestrator naming the task, lane, dirty state and recovery command. Recovery preserves the task's evidence and ownership boundaries. After daemon recovery, continuation follows the exact session and task-attempt evidence required by the [crash-recovery contract](core.md#core:crash-recovery-supervisor); a restored view alone does not authorize resuming work.

## Team teardown

Team close inventories every Team-owned task lane and reports removed, retained-with-reason and worktree-less branch entries. It removes only clean lanes whose branch tip is an ancestor of the recorded destination; unsafe lanes remain as cleanup debt. It removes the Team's members, finalizes open attempts and removes its crons. Attached Cartographer membership ends with the Team; [Blueprints](sentinels.md) follow the same Team lifetime.

Close refuses while a Team task is in progress or a Worker is working unless forced. A forced close names every killed Worker and task; a plan with unfinished tasks stays active and flagged rather than being archived. A confirmed human may close a Team, and coordinator agents may close their Team but cannot close their own workspace. A confirmed human may close a workspace containing coordinators. Workspace close uses this same Team teardown. Close refusals remain visible.

## Coordination feedback

Team and workspace closes keep a durable inventory. Recently Closed restores each close as one action; Undo uses the same record until its recorded deadline or the next close. Restoration keeps workspace folders, Team identities and membership, manager pairing and plan binding, exact per-tab sessions, tab order and panel state, and the enabled states of crons and Blueprints. Repeating a restore, including after restart, creates no duplicates. Utility shells return stopped and require an explicit rerun. Finalized Worker assignments stay finalized; retained dirty or unmerged lanes remain available, while removed clean merged lanes and their attempts are never recreated.

With plan reminders enabled, an active execution plan with actionable work and no live task-bound Worker may nudge its idle Orchestrator. Lifecycle and idle events trigger the check. Like every automatic plan reminder, the nudge is sent only on the connected [Supporter's](supporter-agent.md) approval. The same stall episode produces at most one delivery attempt: a suppressed nudge leaves it eligible, and live work or a changed actionable set opens a new one. The nudge leaves dispatch and task decisions with the Orchestrator and never resumes a paused plan.

A validated blocked verdict sends no reminder to that recipient. It creates one host-authored owner notice per distinct blocker, with choices to wait, pause the plan or decide. The notice proposes pausing when its blocker stops every actionable task. After a reminder is accepted, unrelated plan changes do not repeat it for the same recipient; that recipient's task status, assignment, report or new owner message must change first.

Team utility work reports back to its coordinating agent through the message gateway. Readiness requires observed health; command completion carries its outcome and failure evidence. A failed command or an empty test selection cannot become a passing QA report. Changing the development instance being checked must not redirect the report away from its caller. These are repository toolchain responsibilities, not a new core service.

```map
anchors:
  - core:plans-and-orchestration
  - capability:teams-and-roles
  - capability:task-fitted-personality
  - capability:goals-specs-plans-and-tasks
  - capability:dispatch-and-reports
  - capability:review-and-decisions
  - capability:reminders-milestones-and-pause
  - capability:isolated-lanes
  - capability:integration
```

Task-bound tool-less Workers retain their original brief and pending completed-turn
windows across daemon restart. A provider declares whether resume restores its
conversation. Restoring providers receive no repeated brief; non-restoring ones
receive one new-launch admission only after authenticated binding and only when
exact current-session evidence has not already proved delivery. Attempt identity,
file context, skills, and shared class instructions continue through resume.
