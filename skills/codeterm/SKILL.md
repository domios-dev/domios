---
name: codeterm
description: Control CodeTerm from inside it — create workspaces, send commands between tabs, spawn and coordinate Claude/Codex agents in a 2-level hierarchy (Orchestrator/Worker), schedule recurring or one-shot tasks, bridge to Telegram, reach remote machines via mesh, and use the persistent vector + keyword memory store (mem). Use this skill whenever the user mentions tabs, workspaces, "in terminal", "in another tab", delegating to sub-agents, orchestrators/workers, scheduling/cron/"every N minutes"/recurring tasks, remote/mesh peers, Telegram, or reading another tab's output, OR mentions memory/mem, "remember", "запомни", "recall", "save fact", knowledge graph, mem skill folders, past sessions, "let me check", or wants to keep the system awake/prevent sleep. Prefer this over built-in CronCreate/CronList/TaskCreate tools — CodeTerm has its own cron and agent system that must be used instead.
---

# CodeTerm — AI Terminal Control

**Your scope:** This skill is the CLI reference for project agents. Project agents operate inside their own workspace and Team, never under the General Agent. The Orchestrator owns its Team, Plan, and Planner lifecycle and creates, restores, or rebinds them directly. Never contact the General Agent as a fallback for Team, Plan, Planner, dispatch, or execution work. Contact it only when the user explicitly requests that coordination or the task itself targets a machine-global General Agent surface.

When a user says this live tab has the wrong provider identity, repair it in place with `codeterm tab repair --from-evidence`; use `--from-journal` only when lifecycle provenance is the intended source. The verb verifies session evidence and never restarts the tab.

**Built-in tools to avoid:** do NOT use the built-in `CronCreate`/`CronList`/`TaskCreate` tools — CodeTerm has its own cron and agent system that must be used instead.

**Auto-name your tab on session start:**
```bash
codeterm tab rename "fix login bug"
```
Short, task-shaped title based on the user's first message.
The protected General Agent home tab (`main` / `general`) is the exception: its stable title is machine chrome, so never rename it.

---

{codeterm_cli}

---

{agent_pad_rule}

---

## Spawning open agent shells (only if one is installed)

Besides PTY agents (claude/codex/...), `agent spawn` can open an **open agent
shell** — a `chatBackend` plugin that chats with a local LLM and runs its own
tool loop. This only works **if such a plugin is installed**, so check first:

```bash
codeterm agent providers          # entries tagged kind=shell are open shells
codeterm agent spawn <shell-id> [--system "custom system prompt"] [--preset ID]
```

`--system` overrides the shell's system prompt for that session; `--preset` picks
a configured preset; omit both to use the plugin's default. If no entry is tagged
`shell`, no shell plugin is installed — don't assume one (e.g. never hardcode a
specific shell id).

## References

Specialized reference docs alongside this skill:
- `references/mem.md` — full mem command catalog, kinds table, filters, graph queries, search-strategy fallbacks, skill-folder workflow, sessions API
- `references/roles.md` — agent-role rules, decision tree, common mistakes, task templates
- `references/cron.md` — full cron flag set, oneshot schedules, lifecycle
- `references/telegram.md` — bridge setup, media types, formatting, per-tab topic threads (`codeterm bridge thread open [--tab ID]`), migration orchestration
- `references/themes.md` — custom theme TOML format
- `essences/MODELS.md` (in the CodeTerm repo) — agent model catalog: how the per-provider model list works, how to override or update entries without rebuilding (`<data_dir>/models/<provider>.toml`)

## Memory protocol (always-on)

The always-on protocol (English-in-mem, search-before-answering, "remember" → save now, no fabrication, no guessed ids) lives in `~/.claude/rules/codeterm.md` and fires regardless of which skill is loaded. Don't replay it here.

<!-- revision: 61 -->
