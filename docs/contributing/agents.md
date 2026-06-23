# Agent Roles And Orchestration

CodeTerm uses a two-level agent hierarchy: **Orchestrator -> Worker**.
The shape is intentionally flat. An Orchestrator owns decomposition and
integration; Workers own bounded execution tasks. Workers do not create
sub-workers.

The runtime stores the role as `AgentRole`, with two variants:

- `Orchestrator`
- `Worker`

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

CodeTerm records the structural role when an agent pane is spawned. Prompt text
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
benefits from a visible pane. Common Worker lanes include:

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
- Code review before merging a feature branch back toward `develop`.

A lane is not complete just because its implementation compiles. The relevant
behavior must be exercised at the layer where the user-visible bug occurs.

## Worktrees And Branches

Independent lanes can use separate worktrees and branches. This keeps diffs
reviewable and protects unrelated work. A later integration branch can combine
the lane diffs for full-system verification.

Worktrees with uncommitted changes are source artifacts. They should be kept
until their diffs have been integrated, reviewed, or intentionally discarded.

## Pane Type Awareness

CodeTerm panes can have different backing kinds:

| `backing.kind` | Meaning | Shell | Interaction model |
| --- | --- | --- | --- |
| `pty` | Terminal or PTY-backed agent pane | Yes | Shell commands and terminal input |
| `chat_backend` | LLM chat pane backed by a provider plugin | No | Chat messages only |

This distinction matters for agent-to-agent communication. A PTY pane can
execute shell commands; a `chat_backend` pane cannot.

## Dogfooding

Agent orchestration is also product testing. Spawning workers, receiving
reports, switching panes, and running dev servers through CodeTerm exercises the
same UX being built. Friction in that loop is useful product feedback and should
be captured as a bug, follow-up, or docs gap when it affects the work.

## Agent Prompts On Disk

Prompt templates compiled into the binary live under `src-tauri/prompts/`:

- `general_agent.md`
- `bootstrap_instructions.md`
- `worker_suffix.md`
- `orchestrator_suffix.md`
- `codeterm_rule.md`
- `cron_complete_suffix.md`

The Rust loader is `src-tauri/src/prompts.rs`.

<!-- revision: 6 -->
