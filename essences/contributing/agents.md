# Agent Roles And Orchestration

CodeTerm uses a two-level agent hierarchy: **Orchestrator -> Worker**.
The shape is intentionally flat. An Orchestrator owns decomposition and
integration; Workers own bounded execution tasks. Workers do not create
sub-workers.

The runtime stores the role as `AgentRole`, with two variants:

- `Orchestrator`
- `Worker`

Structural roles are deliberately closed. Behavioral specialization layers on
top as a **role profile** — a free-form label on the spawn spec/tab that picks
the onboarding prompt, default model routing, and presence label without
changing hierarchy semantics. The first named profile is **Planner** (1.6.2): a
high-reasoning Worker the Orchestrator spawns (`agent spawn claude --model
fable --role Planner`) to author or refine GOAL/Spec/Plan/Tasks and sequencing
in the CodeTerm planner, then report back with `in_review`. The Planner never
spawns agents, assigns Tabs, or accepts work — execution and acceptance stay
with the Orchestrator. Per-task free-form role guidance lives on the task
(`role_guidance`) and is prepended to the worker brief at dispatch; there is no
persistent custom role-template registry. `plan task add` requires it and
`plan task update` refuses to blank it. Presence alone is the machine contract;
prose quality remains a human judgment. `plan doctor` warns, without blocking,
when tasks carry identical guidance. Each task also carries an
`executor_role` field — a short slug (e.g. `rust-backend-dev`, `frontend-dev`,
`code-reviewer`) that lets the Orchestrator name the expected agent personality
for that task without altering the two-value `AgentRole` hierarchy. Execution
tasks without `executor_role` are flagged by `codeterm plan doctor --kind quality`.
The field is a contract, enforced where the paths converge: `plan task add`
requires it, `plan task update` refuses to blank it, and approval refuses a plan
holding a roleless task. Enforcement is presence only — no allowlist, minimum
length, or banned-word list. Tasks that predate the contract are reported and
never filled in; an invented role is worse than an absent one, because the
Orchestrator would route on it.

## Role Model

An **Orchestrator** is the top-level coordinator for a user request or an
independent project lane. It turns ambiguous work into scoped tasks, chooses the
verification gates, integrates completed work, and keeps track of branch,
worktree, and process state.

A **Worker** is a leaf executor. It receives one concrete task, works in the
assigned scope, verifies that scope, and reports the result to its parent. If a
Worker discovers that the task needs decomposition, the result is a blocked or
partial report rather than a new child agent.

This split keeps the tree shallow and makes ownership clear:

- Planning and sequencing belong to the Orchestrator.
- Implementation, investigation, review, and docs-sync tasks can be delegated to
  Workers when they are scoped enough to run independently.
- Cross-lane tradeoffs, final readiness, and user-facing status remain with the
  Orchestrator.

## Role Assignment

CodeTerm records the structural role when an agent Tab is spawned. Prompt text
can also describe the intended role, but the product-level hierarchy is the
`AgentRole` value.

For Codex/GPT agents, the cross-vendor behavior source is
[`AGENTS.md`](../../AGENTS.md). Claude reads [`CLAUDE.md`](../../CLAUDE.md),
which imports the same rules with Claude-specific additions. This document
describes the architecture and operating model; it is not a prompt template.

Role-specific launch text is kept inline with the spawned task. The repo docs
and prompt templates are the source of truth for CodeTerm orchestration
behavior.

## Delegation Model

Delegation is useful when a task has clear boundaries, can run in parallel, or
benefits from a visible Tab. Common Worker lanes include:

- Backend implementation or investigation.
- Frontend implementation or investigation.
- Code review of a branch diff.
- Documentation synchronization after behavior changes.
- Release or build verification work.

Small single-file edits, architectural decisions, and broad cross-cutting
changes often stay with the Orchestrator because they require a shared view of
the whole request.

## Reports

Worker completion flows through structured reports. A report is an event, not a
chat conversation. The parent Orchestrator consumes reports, updates the plan,
and decides whether more work, review, or integration is needed.

The expected report shape is:

```text
STATUS: done | blocked | partial
CHANGES: files touched
BUILD/TYPECHECK/TESTS: result
FOLLOWUP: anything the Orchestrator should know
```

Blocked work uses the normal `blocked` status plus a label. Use
`--status-label human-decision` when the unblocker is an escalated choice
outside agent authority. Use `--status-label human-verify` when the unblocker is
an external leg the agent cannot physically drive, such as OAuth, real
email/payment, or a production side effect.

Reports are designed to avoid active polling. A delayed heartbeat can be used as
a fallback for long-running work, but normal progress should come from reports
arriving naturally.

## Verification Gates

The Orchestrator defines the readiness bar for each lane before treating it as
complete. Typical gates include:

- Targeted unit or integration tests for the changed code.
- `./build.sh check` for Rust/Tauri changes.
- `npm run build` or targeted TypeScript tests for frontend changes.
- Playwright/web-mode checks for UI behavior.
- Manual UI oracle checks when the bug is visual or depends on real session
  data.
- Code review before merging a feature branch into its integration branch.

A lane is not complete just because its implementation compiles. The relevant
behavior must be exercised at the layer where the user-visible bug occurs.
Tests should assert the true end state of the user journey, not only the
intermediate API or status response. `POST 200` and "email sent" are setup
evidence; passing evidence is the observable far end, such as stable DOM,
recipient landed on the right thread, or persisted visible state.

Before an autonomous fix, ask whether it is small, well-understood, and
low-risk. If yes, ship one logical fix with a regression test that fails before
and passes after, then re-run the scenario and adjacent journeys. Otherwise
escalate with what is broken, options plus trade-offs, and a recommendation.

## Worktrees And Branches

Independent lanes can use separate worktrees and branches. This keeps diffs
reviewable and protects unrelated work. A later integration branch can combine
the lane diffs for full-system verification.

The Orchestrator works from the canonical integration checkout. Workers own
isolated lanes; merges, reviews, broad QA, and release gates run warm there.

Worktrees with uncommitted changes are source artifacts. They should be kept
until their diffs have been integrated, reviewed, or intentionally discarded.

## Tab Type Awareness

CodeTerm Tabs can have different backing kinds:

| `backing.kind` | Meaning | Shell | Interaction model |
| --- | --- | --- | --- |
| `pty` | Terminal or PTY-backed agent Tab | Yes | Shell commands and terminal input |
| `chat_backend` | LLM chat Tab backed by a provider plugin | No | Chat messages only |

This distinction matters for agent-to-agent communication. A PTY Tab can
execute shell commands; a `chat_backend` Tab cannot.

## Dogfooding

Agent orchestration is also product testing. Spawning workers, receiving
reports, switching Tabs, and running dev servers through CodeTerm exercises the
same UX being built. Friction in that loop is useful product feedback and should
be captured as a bug, follow-up, or docs gap when it affects the work.

## Agent Prompts On Disk

Prompt templates compiled into the binary live under `src-tauri/prompts/`; the
full source and loader map is `prod-docs/PROMPTS.md`.

