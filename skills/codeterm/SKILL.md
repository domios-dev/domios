---
name: codeterm
description: Control CodeTerm from inside it — create workspaces, send commands between panes, spawn and coordinate Claude agents in a hierarchy (Orchestrator/Lead/Worker), schedule recurring or one-shot tasks, bridge to Telegram, and reach remote machines via mesh. Use this skill whenever the user mentions panes, tabs, workspaces, "in terminal", "in another tab", delegating to sub-agents, orchestrators/leads/workers, scheduling/cron/"every N minutes"/recurring tasks, remote/mesh peers, Telegram, HTML visualizations, or reading another pane's output. Prefer this over built-in CronCreate/CronList/TaskCreate tools — CodeTerm has its own cron and agent system that must be used instead.
version: 0.6.0
---

# CodeTerm — AI Terminal Control

You are running inside CodeTerm, an AI-first terminal multiplexer. CLI binary: `codeterm`.

**Your scope:** this skill is the "spinal cord" — it teaches you how to use the tools. You are NOT the General Agent (the machine-level brain). You are a project agent working in a specific workspace. When you need machine-level context (other projects, peers, system state), ask the local General Agent instead of guessing:
```bash
codeterm send-text "what projects are open right now?" --pane general
```
The response arrives in your terminal.

**Language rule:** agent-to-agent communication is always in English, regardless of the user's language. Task prompts, `--task` arguments, reports, cron tasks — all English. Only reply to the user in their language.

**Built-in tools to avoid:** do NOT use the built-in `CronCreate`/`CronList` tools — CodeTerm has its own cron system that spawns Claude agents on schedule. Always use `codeterm cron`.

## Auto-name your tab on session start

```bash
codeterm pane rename "fix login bug"
```
Short, task-shaped title based on the user's first message.

## Pane & workspace basics

Commands default to the **current focused pane** when no ID is given. Always use `$CODETERM_PANE_ID` to reference your own pane — `pane current` returns the *focused* pane, which may be someone else's.

**Environment variables** (available in every CodeTerm pane):
- `$CODETERM_PANE_ID` — your pane's 8-char hex ID
- `$CODETERM_ROLE` — `Orchestrator`, `Lead`, `Worker`, or empty
- `$CODETERM_PARENT_PANE_ID` — parent's pane ID (empty if orchestrator)
- `$CODETERM_WORKSPACE_ID` — current workspace
- `$CODETERM_SOCKET_PATH` — IPC socket
- `$CODETERM` — always `1` inside CodeTerm

```bash
codeterm pane list
codeterm pane rename "title"                  # your tab, no --pane needed
codeterm pane focus PANE_ID
codeterm pane close PANE_ID
codeterm send-text "command" --pane PANE_ID   # --id alias works
codeterm get-output --pane PANE_ID

codeterm workspace list
codeterm workspace create "Name" [--cwd PATH] # empty, no auto-tab. --cwd becomes the default for agents spawned with --workspace.
codeterm workspace focus ID
codeterm workspace rename ID "New Name"
codeterm workspace close ID
codeterm workspace move-tab TAB FROM TO
```

## Canvas panel (HTML visualizations)

Render HTML for localhost apps, data URLs, or generated content:

```bash
cat > /tmp/viz.html <<'EOF'
<!DOCTYPE html><html><body style="background:#0d1117;color:#c9d1d9;font-family:sans-serif;padding:20px"><h1>Hello</h1></body></html>
EOF
codeterm browser open file:///tmp/viz.html
```

Also accepts `data:text/html,...` and `http://localhost:PORT`.

## Notifications

```bash
codeterm notify "Title" --level info --body "Details"
codeterm notifications --unread
codeterm notifications clear
```

Levels: `info` (blue), `warning` (yellow), `error` (red).

## Agent hierarchy & roles

CodeTerm has exactly 3 roles enforced by the backend:

```
Orchestrator (top, no parent, can_spawn)
  ├── Lead       (parent MUST be Orchestrator, can spawn Workers)
  │    └── Worker (leaf, CANNOT spawn anything)
  └── Worker     (leaf, CANNOT spawn anything)
```

**Choose by this rule:**

| Situation | Spawn as | Parent |
|-----------|----------|--------|
| Fresh independent task in its own workspace | `Orchestrator` | none (`--workspace $WS`) |
| Sub-coordinator that will itself spawn 2+ workers | `Lead` | Orchestrator only |
| Single concrete leaf task | `Worker` | Orchestrator or Lead |

**Backend WILL reject:** Lead under Lead, Worker under Worker, Orchestrator with `--parent`, calling `set-conductor` as Lead/Worker. Don't try.

**Your own role is in `$CODETERM_ROLE` (empty = Orchestrator).** Read `references/roles.md` for full rules, decision tree, common mistakes, and task templates — especially before your first spawn.

**CLI:**
```bash
codeterm agent list
codeterm agent hierarchy                      # tree view
codeterm agent spawn claude --title T --role R [--parent ID] [--can-spawn] [--task TEXT] [--workspace WS] [--cwd PATH]
codeterm agent report "summary"               # report to parent
codeterm agent reports                        # read child reports (orchestrator/lead only)
codeterm agent status [--pane ID]
codeterm agent compact [--pane ID]            # /compact
codeterm agent model sonnet|haiku|opus [--pane ID]
```

**Reports are auto-injected** into your terminal as `[Report from PANE_ID]` lines. Never sleep or poll — continue working and react when reports arrive.

**Worker task template — always include the report command:**
```bash
codeterm agent spawn claude --title "Scout" --role Worker --parent $CODETERM_PANE_ID \
  --task "Read .claude/agents/backend-dev.md. TASK: list files in ./src. When done, run: codeterm agent report 'found N files: ...'"
```

Without `codeterm agent report` in the task, the worker finishes silently and you never hear back.

## After sending: don't echo

When you respond via `codeterm agent report` or `codeterm bridge send`, the reply is already delivered. **Do not repeat the same content in your own assistant output.** The recipient sees it once. Writing it twice is noise.

- No more tasks → stop silently. The send IS your final state.
- More tasks queued → continue with the next one, no summary of what you just sent.

## Mesh networking

CodeTerm instances form a mesh. Any command works on a remote peer — add `--mesh TARGET` (name or ID):

```bash
codeterm mesh status
codeterm mesh peers                                          # paired + discovered
codeterm pane list --mesh PEER
codeterm send-text "cmd" --pane ID --mesh PEER
codeterm get-output --pane ID --mesh PEER
codeterm agent spawn claude --task "..." --mesh PEER
codeterm workspace create "CI" --mesh PEER
```

**Flow:** `mesh peers` → find paired peer → `pane list --mesh PEER` → interact. Only paired peers accept commands.

### Message routing — the prefix is the return address

Messages you receive may arrive with a bracketed prefix telling you WHERE they came from. That prefix tells you where to reply. If there's no prefix, the user is typing into your pane directly — reply normally.

| Incoming prefix | Reply with | Pane output? |
|-----------------|------------|--------------|
| `[Telegram: USER_ID msg_id=N] text` | `codeterm bridge send --to USER_ID --service telegram "..."` | No |
| `[Mesh: PEER pane=ID] text` | `codeterm send-text "..." --pane ID --mesh PEER` | No |
| `[Report from PANE_ID] text` | Act on it or ignore; never reply | No |
| *(no prefix)* | Normal pane chat | Yes |

**Core rule:** don't double-answer. If the message came via Telegram, the user is on Telegram, not at the keyboard — reply via Telegram only, don't also dump the text in the pane. Same for mesh and inter-agent traffic.

**Examples:**

- `[Telegram: 123456789] are you working right now?` → `codeterm bridge send --to 123456789 --service telegram "yep, online"`. Nothing in the pane.
- `[Mesh: workstation pane=a3f1b2c7] can you run the tests?` → `codeterm send-text "running now" --pane a3f1b2c7 --mesh workstation`, do the work, then `codeterm send-text "tests pass" --pane a3f1b2c7 --mesh workstation`.
- `run the tests` (no prefix) → reply in the pane, you're talking to a local user.

To message a peer's General Agent directly: `--pane general --mesh PEER`. Add "report back" to the text if you need a response — remote agents don't auto-reply.

## Periodic tasks (cron)

Briefly:
```bash
codeterm cron create "Name" --schedule "0 * * * *" --task "what to do"          # recurring
codeterm cron create "Quick" --schedule "in 30m" --task "do X" --once           # oneshot
codeterm cron list
codeterm cron edit ID [--name N] [--schedule S] [--task T]                      # modify in place
```

Full cron reference, oneshot schedules, and lifecycle details: `references/cron.md`.

## Telegram bridge

Briefly, from any pane:
```bash
codeterm bridge send --to USER_ID --service telegram "Your reply"
codeterm bridge send --to USER_ID --service telegram --append "...more text"
```

Setup, media types, formatting, and full reference: `references/telegram.md`.

## Themes

Briefly:
```bash
codeterm theme list
codeterm theme apply dracula
```

Custom theme TOML format and CLI: `references/themes.md`.

## Session persistence

Sessions auto-save every 30s. CWD is tracked via OSC 7. Claude Code sessions are detected and auto-resumed with `--resume`. Stale ids (JSONL gone from disk) are skipped instead of looping. Each pane gets a pre-generated `--session-id` UUID so multiple panes in the same cwd stay disambiguated, and Claude's `SessionStart` hook updates the persisted id when the user runs `/new` or `/clear` inside the TUI.

## Past agent sessions (search & resume)

Search across every registered provider's on-disk sessions (Claude JSONLs, Codex `~/.codex/sessions/`):

```bash
codeterm sessions search "webpack hmr"                       # all providers
codeterm sessions search "auth flow" --provider claude --limit 5
codeterm sessions search "bug" --project /Users/me/code/foo  # scope by project
codeterm sessions get SESSION_ID                             # provider, cwd, file, times
codeterm sessions resume SESSION_ID                          # opens the right workspace + pane, hands the resume id to the provider
```

Use `sessions resume` instead of the brittle "create workspace at guessed cwd → `agent spawn --args=--resume=UUID`" dance — Claude/Codex look for the session file in the current encoded project dir, so getting the cwd wrong is how you get `No conversation found with session ID`. Resume reads the real cwd out of the session file itself.

## Config

Read/write config from the CLI:
```bash
codeterm config get integrations.telegram_enabled
codeterm config set integrations.telegram_enabled true
codeterm config get general_agent                          # read a section
```
Dot-notation for nested keys. Values are JSON (`true`, `false`, `42`, `"string"`, `["array"]`).

## Platform notes

- **Paths:** use forward slashes in commands; the CLI normalizes.
- **Temp files:** `/tmp/` works on all platforms inside CodeTerm.

## Command palette

`Cmd+K` / `Ctrl+K` opens the palette for quick tab/workspace/theme access.

<!-- revision: 32 -->
