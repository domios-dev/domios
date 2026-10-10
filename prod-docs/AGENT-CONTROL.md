# Agent control

Skill-driven agent integration, hierarchy, command palette, and session persistence.

## Agent Integration

External agents (personal assistants, gateways, CI/CD) can connect via REST API:

```bash
# Create workspace for agent
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"Agent"}'

# Create tab
curl -X POST http://localhost:7685/api/workspaces/$WS_ID/tabs

# Send command to a tab (enter: true is default, appends \r automatically)
curl -X POST http://localhost:7685/api/tabs/$TAB_ID/send \
  -H "Content-Type: application/json" \
  -d '{"text":"npm run build"}'

# Press terminal keys / navigation actions
curl -X POST http://localhost:7685/api/tabs/$TAB_ID/send \
  -H "Content-Type: application/json" \
  -d '{"actions":[{"type":"key","key":"esc"},{"type":"delay","ms":200},{"type":"key","key":"enter"}]}'

# Read tab output
curl http://localhost:7685/api/tabs/$TAB_ID/output

# Notify
curl -X POST http://localhost:7685/api/notifications \
  -H "Content-Type: application/json" \
  -d '{"title":"Build complete","severity":"info","level":"action_needed"}'
```

## Agent Hierarchy

Domios has a 2-level role hierarchy: Orchestrator (depth 1) -> Worker (depth 2). Each agent gets its own tab; the hierarchy is a logical chain, not a nested layout. IDs are 8-char hex (e.g. `a3f1b2c7`).

**Roles:**
- **Orchestrator** (was Conductor) -- top-level, spawns Workers.
- **Worker** -- executes one task, reports back. Cannot spawn. Cannot call `set-orchestrator`, `unlink`, or `agent spawn`.

Backward-compat alias: `Conductor` -> `Orchestrator`.

**Placement rules.** The backend refuses a parent for an Orchestrator (`Orchestrator cannot have a parent`), requires a Worker's `--parent-tab` to be a spawning parent such as its Orchestrator, and refuses to make a tab that already has a parent an Orchestrator (`already in a hierarchy`). There is no middle coordinator: a sub-task that needs its own tree goes to a fresh Orchestrator spawned with `--workspace` in its own workspace, which owns the whole sub-request; a concrete leaf goes to a Worker under you. Workers fan out in parallel, one task each.

**Briefs.** Pass a brief with `--task-file PATH` or `--task-file -` fed by a single-quoted heredoc. The CLI refuses a multiline or over-240-character inline `--task`, because the shell has already expanded its backticks, `$`, quotes and newlines; a short scalar such as `--task "run tests"` stays fine. Every Worker brief names its report command, or the Worker finishes without reporting.

**Worker discipline.** A Worker executes its one task, reports with `codeterm agent report`, and stops: no spawning, sleeping or polling, and no "also do X" scope creep. A task too big for one Worker is reported `partial` with the needed decomposition, so the Orchestrator splits it. A Worker `--status blocked` report proposes an owner decision for an external dependency, persists its reason and evidence, and leaves the task unchanged; only the Plan owner or Planner explicitly blocks the task.

**Push-based reports:** Reports go to the spawning Orchestrator, auto-injected into its PTY as `[Report from Name] message\r`, so Claude sees them as user input -- no polling needed.

```bash
# Mark current tab as orchestrator
codeterm agent set-orchestrator --tab $CODETERM_TAB_ID

# View the complete local workspace map (workspaces, Teams, Plans, and nested tabs); --workspace selects the existing one-workspace tree; --agents = agent tabs only
codeterm tab list

# Get one tab's state (working|question|idle|error|exited, as tab list shows), type, session, prompt
codeterm tab status --tab $WORKER

# Include pending_prompt question and numbered options for an active provider dialog
codeterm tab status --tab $WORKER --json

# Answer one-based option 1 from the displayed prompt; refuses if it left the screen
codeterm tab respond 1 --tab $WORKER

# Repair a wrong live provider label from the tab's verified session, no restart
codeterm tab repair --from-evidence

# The Plan owner can repair an assigned worker through the same evidence boundary
codeterm tab repair --from-evidence --tab $WORKER

# Adopt a live foreign PTY as an agent send target without restarting it
codeterm tab adopt-agent --tab $FOREIGN --provider <provider-id>
# A login shell is refused unless the owner passes --force
codeterm tab adopt-agent --tab $FOREIGN --provider <provider-id> --force

TEAM_ID=$CODETERM_TEAM_ID
# Clear role; a final Orchestrator retains its Team and active Plan for replacement
codeterm agent unlink --tab $CODETERM_TAB_ID

# Replace that missing Orchestrator while preserving the Team and Plan history
codeterm agent spawn claude --team "$TEAM_ID" --role Orchestrator
```

The `agent.report` webhook event fires when any agent sends a report, enabling async orchestration patterns.

### One-call agent spawn

The `task` field is baked into the launch command via the `-p` flag, so the agent starts working immediately with no delay. Spawn answers once the launch reaches a verdict, bounded at 20 s: `launch_state` `ready`, or `starting` with a warning while the agent is still coming up; a provider that exits before readiness fails the spawn with its output tail (`codeterm docs API-AGENTS`). Each spawned agent gets its own tab (does NOT auto-switch active tab). Uses socket API for reliability.

```bash
# Spawn a Worker under the Orchestrator
codeterm agent spawn claude --title "Worker-1" --description "Runs focused tests" --role Worker --parent-tab $CODETERM_TAB_ID --task "run tests"

# Spawn defaults to claude if no agent specified
codeterm agent spawn --title "Helper" --description "Helps with repository work" --role Worker --parent-tab $CODETERM_TAB_ID

# Cross-workspace spawn
codeterm agent spawn claude --workspace WS_ID --title "Remote" --description "Audits the remote workspace" --role Worker --parent-tab $CODETERM_TAB_ID
```

Returns: tab_id, tab_id, pty_id, agent, title, role.

The `--agent-name` option (for loading `.claude/agents/<name>.md`) is only available via the REST API (`POST /api/agents/spawn`), not the CLI.

### Batch API (REST and Tauri IPC)

Spawn an Orchestrator and its Workers in one call. The batch API is available via both `POST /api/batch` (REST) and the `mux_batch` Tauri IPC command, with identical semantics and $ref resolution. Both delegate to the unified ServiceContext.

```bash
curl -X POST http://localhost:7685/api/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "description": "Coordinates repository work", "role": "Orchestrator", "can_spawn": true}},
      {"method": "spawn", "params": {"title": "W1", "description": "Analyzes the code", "role": "Worker", "parent_tab_id": "$boss", "task": "analyze code", "agent_name": "reviewer"}},
      {"method": "spawn", "params": {"title": "W2", "description": "Runs focused tests", "role": "Worker", "parent_tab_id": "$boss", "task": "run tests"}}
    ]
  }'
```

Available batch methods: `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`. Use `$id` references to chain command results (e.g. `"parent_tab_id": "$boss"` resolves to the tab_id from the "boss" spawn).

## General Agent recovery

A connected Supporter Agent helps Domios notice when the General Agent is stuck. It works automatically, with no feature switches. Without a connected adapter, the existing dead-process recovery and normal tab behavior remain. Connect through Settings > Agents > Supporter Agent or `codeterm supporter use <adapter>`; required adapters first need a token through `token set --adapter <adapter>`. Saved connections keep their own models and tokens when switching. `codeterm supporter disconnect <adapter>` removes one; removing the active adapter stops the Supporter. Memory ranking, tab observations, the shared budget and sent text are explained in `codeterm docs MEMORY` and the Guide (`@ds-guide supporter-agent` on its own paragraph).

The General Agent watchdog reacts to PTY death, provider exit to a shell and launch settlement. It respects shutdown, launch grace and restart limits; screen text alone never authorizes restarting a live agent. Model-authorized recovery remains notify-only until a separate held-out accuracy check passes.

| Observed condition | Required evidence and response |
|---|---|
| Update prompt | An agent-owned tab may answer with one calibrated choice at 0.80 or above among provider-declared options. Owner-launched tabs keep the prompt for the owner. See `codeterm docs CONFIG`. |
| Crash to shell | The provider is absent from this tab's process tree and its recovery session is verified. Resume in the same tab. |
| Startup failure after its deadline | One restart within the overall episode limit. |
| Error loop | One restart, then an owner notification if the condition continues. |
| Sign-in or unavailable model/quota | Notify the owner with choices; never answer or switch the model automatically. |
| Folder trust | Use the existing trust responder: provenance, the shared trust cache or a live same-provider tab in the exact directory can authorize the declared option. Agent-owned launch and cron prompts may also use one calibrated declared choice. |
| Command/tool permission | An agent-owned tab may use one calibrated provider-declared choice; otherwise the tab's own permission policy applies. |
| `other` | Record the verdict; no recovery or hint. |

Model-authorized automatic actions require two agreeing observations, each at least 0.90 confidence, plus the deterministic evidence above. Owner notifications require two agreeing observations at or above 0.80. Below 0.60 is ignored. The separate held-out check must achieve at least 0.95 precision on automatic-action labels before those actions are enabled; an unqualified row notifies instead.

There are at most two automatic restarts per General Agent episode (the configured maximum is capped at two). After that, Domios sends a failure notification and waits for owner action. Every automatic action is journaled and sends a brief activity notification. All Supporter model calls share the hourly budget; an exhausted budget produces no model observation or model-authorized action.

Trust, permission and login notifications are suppressed for tabs the owner opened directly from the UI; the journal still records the condition. The General Agent, agent-spawned, Team, cron and hidden-worker tabs keep their notifications. After a Domios restart, restored tabs with the same condition produce one grouped notification per condition at restore completion. A durable key for tab, condition and launch prevents reporting the same condition twice across restarts.

Agent-, Team-, worker- and cron-owned tabs, visible or hidden, may answer folder-trust, tool-permission and update prompts with one calibrated Supporter choice at 0.80 or above among provider-declared options. Domios uses the existing prompt response surface and never submits model-authored text. If the model is absent, fails, is capped, selects `other` or is below 0.80, the structural responder decides and unresolved prompts notify the owner. Folder-trust prompts at launch or in cron can use this choice without host provenance; provenance still decides elsewhere. Login, authentication, unavailable-model and quota prompts are never answered automatically. Tabs launched directly by the owner without agent or Team ownership keep owner-facing prompts. Details: `codeterm docs TAB-LIFECYCLE`.

## Command Palette

`Cmd+K` (macOS) / `Ctrl+K` (Windows/Linux) opens the command palette for quick access to tabs, workspaces, settings, and theme switching.

## Session Persistence

Domios tracks and restores terminal state across sessions:

- **CWD tracking**: Working directory is tracked via OSC 7 escape sequences and saved/restored per tab on restart.
- **Claude Code auto-resume**: Claude Code sessions are detected and automatically resumed with `--resume`. Before injecting the resume flag, Domios checks that the session still exists on disk — missing sessions (cleared cache, machine-synced config without sessions) launch fresh instead of looping on `No conversation found with session ID`.
- **Pre-generated session ids**: For providers that accept an injected session id (Claude today), Domios pre-generates a UUID and passes it via `--session-id` at launch. Each tab gets a unique id even when several tabs share the same cwd.
- **SessionStart hook**: Claude's `SessionStart` matcher POSTs back to `/api/tabs/{id}/hook/session_start`, so `/new` and `/clear` inside the TUI rotate the persisted `agent_session_id` immediately.
- **Cross-provider session search**: `codeterm sessions search` walks every registered provider's sessions (Claude JSONLs, Codex `~/.codex/sessions/YYYY/MM/DD/*.jsonl`, OpenCode's shared `opencode.db`) and names any provider store it could not fully search. `codeterm sessions resume` opens the tab in the correct cwd and hands the resume id to the right provider.
- **Auto-save**: Session state is persisted every 30 seconds.

<!-- revision: 23 -->
