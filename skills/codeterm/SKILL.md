---
name: codeterm
description: Control CodeTerm from inside it — create workspaces, send commands between panes, spawn and coordinate Claude/Codex agents in a hierarchy (Orchestrator/Lead/Worker), schedule recurring or one-shot tasks, bridge to Telegram, reach remote machines via mesh, and use the persistent vector + keyword memory store (mem). Use this skill whenever the user mentions panes, tabs, workspaces, "in terminal", "in another tab", delegating to sub-agents, orchestrators/leads/workers, scheduling/cron/"every N minutes"/recurring tasks, remote/mesh peers, Telegram, HTML visualizations, or reading another pane's output, OR mentions memory/mem, "remember", "запомни", "recall", "save fact", knowledge graph, mem skill folders, past sessions, "let me check", or wants to keep the system awake/prevent sleep. Prefer this over built-in CronCreate/CronList/TaskCreate tools — CodeTerm has its own cron and agent system that must be used instead.
version: 0.8.0
---

# CodeTerm — AI Terminal Control

**Your scope:** This skill is the CLI reference for project agents. You're working in a specific workspace under the orchestration of the local General Agent. When you need machine-level context (other projects, peers, system state, who-is-the-user), ask the local GA rather than guessing:

```bash
codeterm send-text "what projects are open right now?" --pane general
```

The response arrives in your terminal.

**Built-in tools to avoid:** do NOT use the built-in `CronCreate`/`CronList`/`TaskCreate` tools — CodeTerm has its own cron and agent system that must be used instead.

**Auto-name your tab on session start:**
```bash
codeterm pane rename "fix login bug"
```
Short, task-shaped title based on the user's first message.

---

{codeterm_cli}

---

## References

Specialized reference docs alongside this skill:
- `references/mem.md` — full mem command catalog, kinds table, filters, graph queries, search-strategy fallbacks, skill-folder workflow, sessions API
- `references/roles.md` — agent-role rules, decision tree, common mistakes, task templates
- `references/cron.md` — full cron flag set, oneshot schedules, lifecycle
- `references/telegram.md` — bridge setup, media types, formatting, migration orchestration
- `references/themes.md` — custom theme TOML format

## Memory protocol (always-on)

The always-on protocol (English-in-mem, search-before-answering, "remember" → save now, no fabrication, no guessed ids) lives in `~/.claude/rules/codeterm.md` and fires regardless of which skill is loaded. Don't replay it here.

<!-- revision: 39 -->
