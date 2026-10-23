---
name: codeterm
description: Control Domios from inside it — create workspaces, send commands between tabs, spawn and coordinate Claude/Codex agents in a 2-level hierarchy (Orchestrator/Worker), schedule recurring or one-shot tasks, bridge to Telegram, reach remote machines via mesh, and use the persistent vector + keyword memory store (mem). Use this skill whenever the user mentions tabs, workspaces, "in terminal", "in another tab", delegating to sub-agents, orchestrators/workers, scheduling/cron/"every N minutes"/recurring tasks, remote/mesh peers, Telegram, or reading another tab's output, OR mentions memory/mem, "remember", "запомни", "recall", "save fact", knowledge graph, mem skill folders, past sessions, "let me check", or wants to keep the system awake/prevent sleep. Prefer this over built-in CronCreate/CronList/TaskCreate tools — Domios has its own cron and agent system that must be used instead.
---

# Domios — AI Terminal Control

<!-- codeterm-version: {codeterm_version} -->
If `domios --version` differs from this marker, trust `domios docs`.

**Your scope:** This skill is the CLI reference for project agents. Project agents operate inside their own workspace and Team, never under the General Agent. The Orchestrator owns its Team, Plan, and Planner lifecycle and creates, restores, or rebinds them directly. Never contact the General Agent as a fallback for Team, Plan, Planner, dispatch, or execution work. Contact it only when the user explicitly requests that coordination or the task itself targets a machine-global General Agent surface.

When a user says this live tab has the wrong provider identity, repair it in place with `domios tab repair --from-evidence`; use `--from-journal` only when lifecycle provenance is the intended source. The verb verifies session evidence and never restarts the tab. To keep a live foreign agent PTY and make send a routed agent message, use `domios tab adopt-agent --tab ID --provider <id>`; a login shell is refused unless the owner passes `--force`.

Provider switching and self-switch status: `domios agent switch-provider --help`, `domios agent switch-provider-status --help`, and `domios docs API-AGENTS`.

**Built-in tools to avoid:** do NOT use the built-in `CronCreate`/`CronList`/`TaskCreate` tools — Domios has its own cron and agent system that must be used instead.

**Give the tab a single word name and put the activity in its description:**
```bash
domios tab rename "Scout"
domios tab describe "Reviews the login flow and records findings"
```
Choose a stable name with a single word. For a new agent, pass the activity as
`--description`; on an existing tab, use `domios tab describe`.
The protected General Agent home tab (`main` / `general`) is the exception: its stable title is machine chrome, so never rename it.

---

{codeterm_cli}

---

{interactive_child_pty_operation}

---

{agent_pad_rule}

---

## Spawning open agent shells (only if one is installed)

Besides PTY agents (claude/codex/...), `agent spawn` can open an **open agent
shell** — a `chatBackend` plugin that chats with a local LLM and runs its own
tool loop. This only works **if such a plugin is installed**, so check first:

```bash
domios agent providers          # transport=chat_backend entries are open shells
domios agent spawn <shell-id> [--system "custom system prompt"] [--preset ID]
```

`--system` overrides the shell's system prompt for that session; `--preset` picks
a configured preset; omit both to use the plugin's default. If no entry has transport
`chat_backend`, no shell plugin is installed — don't assume one (e.g. never hardcode a
specific shell id).

## Pairing across API ports

Use `domios mesh connect <instance-id-or-name> --addr <ip:port>` when UDP discovery cannot reach a different API port. This reads and records the answering identity, refuses an ID mismatch, and sends the ordinary request. The receiving owner still approves with `domios mesh approve <request-id>`. Paired means trusted; a manual sighting never grants access. See `domios docs MESH` for listener readiness and identity binding.

## References

The [embedded document index](references/docs.md) lists whole-document entry points.

Model endpoints and owned presets are managed through `domios model-provider`.
Read `domios docs INTEGRATIONS` before adding an endpoint; pipe its key on stdin,
never in an argument. A capable PTY provider accepts `agent spawn --preset NAME`.

Full references live in `domios docs <NAME>`; the `references/` files only point there:
- Memory — `domios docs MEMORY-USAGE` (commands, kinds, search fallbacks, wake-up, calibration), `domios docs MEMORY`, `domios docs MEMORY-SERVICES` (skill folders, Librarian, dreaming)
- Agent roles — `domios docs AGENT-CONTROL` (hierarchy, placement, briefs) and `domios docs ORCHESTRATION`
- Blueprints — `domios docs BLUEPRINTS` (full reference) and `domios docs BLUEPRINT-RECIPES` (use-case patterns and recipes)
- Cron — `domios docs API-INTEGRATIONS` (CLI, oneshot, lifecycle) and `domios docs RUNTIME-CONFIG` (job schema)
- Telegram — `domios docs TELEGRAM-SETUP` (setup, media, formatting, per-tab topic threads) and `domios docs MESH-BRIDGE` (poller migration)
- Themes — `domios docs RUNTIME-CONFIG` (custom theme TOML format)
- Models — `domios agent models <provider>` for the provider-owned catalogue and available choices
- Essence maps — `domios docs ESSENCE-MAP` for source-backed map generation, validation, and decision records

Local models use the bundled OpenCode provider: `domios model-provider add|remove|list|test` manages owned connections. The optional OpenCode decision adapter is selected through `domios supporter`; TypeSafe Jev remains the default.

## Memory protocol (always-on)

The always-on protocol lives in your provider rules and fires regardless of which skill is loaded. Save only durable facts. Instructions for the current session or task (model or worker choice for now, temporary priorities, 'this time', 'for this release') stay in the conversation or plan, never in memory. If durability is uncertain, do not save; ask only when the user says remember. Even an explicit remember of a session-scoped instruction requires the user to say it is permanent. Explicit "remember" requests for durable facts still mean save now.

<!-- revision: 84 -->
