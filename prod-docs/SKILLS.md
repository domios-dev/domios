# CodeTerm Skills for Claude Code

CodeTerm ships skills that teach Claude Code how to control the terminal.

## Installation

CodeTerm installs the `codeterm` skill itself on every start — a manual copy is
only for a checkout you are not running. See *Seeding, rendering and mirroring*
below for what the app does.

```bash
cp -r skills/codeterm ~/.claude/skills/
```

Or symlink:
```bash
ln -s /path/to/codeterm/skills/codeterm ~/.claude/skills/codeterm
```

## Seeding, rendering and mirroring

`commands/skills.rs` owns three steps, all on startup:

1. **Seed** — bundled `skills/<id>/` → the canonical dir
   (`<data_dir>/skills/<id>/`), whole directory, `references/` included.
2. **Render** — the `codeterm` skill declares placeholders filled from the
   in-binary prompt constants, so the skill and the General Agent prompt share
   one source instead of two drifting copies:

   | Placeholder | Source constant | Prompt file |
   |---|---|---|
   | `{codeterm_cli}` | `CODETERM_CLI` | `src-tauri/prompts/codeterm_cli.md` |
   | `{agent_pad_rule}` | `AGENT_PAD_RULE` | `src-tauri/prompts/agent_pad_rule.md` |

   Adding a fragment to the skill means adding a placeholder plus a render-map
   entry — never pasting the text into `SKILL.md`. Rendering happens on copy, so
   the canonical and provider copies are post-render.
3. **Mirror** — canonical `codeterm` → `~/.claude/skills/` and
   `~/.agents/skills/`. Provider dirs hold exactly `{codeterm}`; every other
   entry is pruned. All other skills stay canonical-only and are discovered
   through `codeterm mem search`.

**The install gate is content, directory-scoped and render-aware.** Both the seed
and the mirror copy when the destination is missing or differs from the *rendered*
source anywhere in the skill directory — an edited `references/` file counts, and a
changed prompt constant reinstalls at the same revision. Comparing raw bundled
bytes would be permanently unequal (source pre-render, destination post-render),
which silently degrades the gate to always-true.

`<!-- revision: N -->` at the bottom of `SKILL.md` is a **never-downgrade guard
only**: a destination declaring a strictly newer revision is left alone. It is not
the copy decision, so bumping it is not how you ship an edit — though you should
still bump it when you change the file.

Every agent's own pad rule (write payloads to the pad, pass `--from-pad`) reaches
spawned agents through their launch prompt and reaches skill readers through
`{agent_pad_rule}`; `prod-docs/PROMPTS.md` maps all three carriers.

## Available Skills

### `codeterm` — Core terminal control
- Create tabs, manage workspaces
- Send commands to other tabs
- List tabs

## How it works

Skills are markdown files that Claude Code loads automatically when relevant. When Claude detects it's running in CodeTerm (via `$CODETERM=1` env var and `$CODETERM_TAB_ID` for self-identification), it can use the `codeterm` CLI to:

1. **Send commands**: `codeterm send "npm test"` (sends immediately to the current tab, newline appended automatically). The CLI refuses task-shaped prose for a raw **shell** tab unless `--to-shell` is explicit; short single-line commands remain eligible. A shell receives only the raw text/keys, with no `-=-codeterm:tab...-=-` return-address marker; the marker is added only when the target is an **agent** tab (so it knows who to reply to). The Chat UI `/chat-send` route refuses PTY tabs without a live agent identity before writing. For quote-heavy or multiline text, use `codeterm send --stdin --tab ID` or `codeterm send --from-file - --tab ID`; message and pad file options accept `-` to read UTF-8 stdin on every platform. For terminal keys, use `codeterm send --actions "esc,delay:200ms,up,enter" --tab ID` (`enter`, arrows, `tab`, `backspace`, `ctrl-c`, `key*3`, and `delay:250ms` are supported) — these deliver as raw key bytes to a shell, with no wrapping. To spawn a bare terminal that runs a command on start, use `codeterm tab new --cmd "cd /repo && ./build.sh dev"`; inside a Team it becomes a Team utility shell unless `--no-team` is explicit. Replace that process later with `codeterm tab restart --tab ID --cwd /repo --cmd "./build.sh dev"`; it preserves the tab and Team metadata instead of creating another tab. To schedule a future tab message without blocking yourself, use `codeterm send --delay 10m --tab ID "text"`, `--delay 4h`, `--delay 1d`, or `--at 2026-06-15T09:00`; an absolute `--at` in the past is due immediately and sends as soon as the scheduler can deliver it. Manage queued items with `codeterm delayed list|edit|send-now|delete`; failed messages remain listed with their failure reason. Use `send --every` to repeat into an EXISTING tab (a monitor/watcher); cron spawns a FRESH agent per scheduled run. With `--mesh`, the remote target Team owns the series; delete it on that peer because closing the caller Team will not cancel it.
2. **Send notifications**: `codeterm notify send "Done" --level action_needed` — `--level` (verbose|action_needed|failure) is what lets it alert; without one it only files in the bell. An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. A short completion or progress note may be filed at `verbose` — a periodic status, never a running commentary of one notification per step; Worker and Planner tabs still report through their reporting channel instead of notifying. A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried
3. **Create workspaces**: `codeterm workspace create "Feature X"`
4. **Read visible terminal text**: `codeterm tab get-text` (reads what the user sees). If a tab is showing an abandoned provider session, recover the live transcript without restarting the agent with `codeterm tab rebind SESSION_ID --tab ID`; the command refuses a session that has not materialized. If the tab's provider label is wrong, repair it from its own verified evidence with `codeterm tab repair --from-evidence`; use `--from-journal` to select the newest restorable provenance record. Repair never restarts the tab and is authorized only for the tab itself or its task's Plan owner. To keep a live foreign agent PTY and make send a routed agent message, use `codeterm tab adopt-agent --tab ID --provider <id>`; a login shell is refused unless the owner passes `--force`.
5. **Name tab**: `codeterm tab rename "Scout"` or `codeterm tab rename "Scout" --tab ID`. Tab titles are one word; put activity prose in `codeterm tab describe "..."`. A task-bound tab keeps its planned name and records progress in its description
6. **Create theme**: `codeterm theme save my-theme /tmp/theme.toml --apply`
7. **Agent hierarchy**: `codeterm agent set-orchestrator` (mark self as orchestrator)
8. **Compact context**: `codeterm agent compact [--tab ID]` (sends /compact to Claude)
9. **Choose model**: use `codeterm agent models <provider>` to list ids and `codeterm agent spawn [provider] --model MODEL --task "..."` to select one. Authoritative catalogues and free-form providers with a published live model list refuse unknown ids before creating a tab and show valid ids. A unique bare id selects its provider; when several providers advertise it, specify `--agent` for task dispatch or pass the provider positionally to spawn. Free-form providers without a live list pass provider-owned ids through. To change the model of one idle live tab, `codeterm agent model --tab TAB MODEL` relaunches only that tab on its own session. Success confirms the model from the current launch report or first post-relaunch turn; if neither reports before the deadline, success says `model_confirmation: "pending"` with ready relaunch evidence, and the tab identity follows the first turn. Providers without a live switch refuse it, so respawn those with `--model`.
10. **Send report**: `codeterm agent report "task done"` (report to direct parent, auto-injected into parent's PTY)
11. **Message a running agent**: `codeterm send --tab ID "status?"` (immediate agent-directed message via the normal send transport)
12. **List tabs / hierarchy**: `codeterm tab list` (all local workspaces with tab counts, paths, Teams and active Plans, sub-agents nested, caller marked `(self)`; `--workspace <id>` selects the existing scoped tree and `--agents` filters to agent tabs). Use `codeterm tab inspect <id>` for one tab's full payload, or add `--lifecycle` for its journal telemetry.
13. **Spawn agent**: `codeterm agent spawn claude --title "Worker" --description "Reviews tab routing" --role Worker --parent-tab ID --task "do work"` (title is a single word; description says what the tab does; creates a tab without switching active tab; task delivery is provider-owned and is not an inter-tab `-=-codeterm:tab...-=-` message)
14. **Spawn with agent definition**: via REST API `POST /api/agents/spawn` with `agent_name` field (loads `.claude/agents/<name>.md`; not available in CLI)
15. **Cross-workspace spawn**: `codeterm agent spawn claude --workspace WS_ID --title "Remote" --description "Reviews API contracts" --role Worker --parent-tab ID --task "work"`
16. **Create cron job**: `codeterm cron create "name" --schedule "0 9 * * * *" --task "Review TODOs"` (lands in your own workspace and inherits your Team; use `--no-team` only for a deliberately independent job; add `--once` for oneshot: `--schedule "in 30m" --once`)
17. **Edit cron job**: `codeterm cron edit ID --schedule "*/15 * * * *"` (prefer over delete+recreate to preserve history)
18. **Complete cron run**: `codeterm cron complete --run-id UUID --summary "result"`
19. **List cron jobs**: `codeterm cron list`, **trigger manually**: `codeterm cron trigger ID`
20. **Read config**: `codeterm config get integrations.telegram_enabled` (dot-notation); bare `codeterm config get` (no key) dumps the whole redacted config
21. **Write config**: `codeterm config set integrations.telegram_enabled true` (JSON values)
22. **Send to Telegram**: `codeterm bridge send --to USER_ID --service telegram "text"` (auto-edits within 60s, `--append` to concatenate)
23. **Keep system awake**: `codeterm keepawake start --duration-secs 3600 [--display]` (cross-platform: caffeinate on macOS, SetThreadExecutionState on Windows). Stop: `codeterm keepawake stop`. Status: `codeterm keepawake status`. Use this instead of calling `caffeinate` / `powercfg` / `systemd-inhibit` directly.
24. **Search past agent sessions**: `codeterm sessions search "QUERY" [--provider claude|codex|all] [--project /path] [--limit N]` — content search across every registered provider's sessions. Each hit carries its `provider` and `session_id`; a `warning:` line names any provider store that could not be fully searched, so an empty result is not proof of absence.
25. **Inspect a past session**: `codeterm sessions get SESSION_ID` — provider, recorded cwd, file path, timestamps. Use before `resume` to verify the JSONL is still on disk.
26. **Resume a past session atomically**: `codeterm sessions resume SESSION_ID [--title T] [--workspace-id ID] [--workspace-name N]` — `--title` is a tab name with a single word. The command locates the session, opens the right workspace at the recorded cwd, spawns the agent tab, and hands the resume id to the provider with the correct CLI shape. Use this instead of the brittle "create workspace at guessed cwd → `agent spawn --args=--resume=UUID`" dance.
    - **Recover a reduced/wiped desk**: `codeterm sessions backups list` shows rotated full-desk snapshots (name, saved time, workspace/tab counts); `codeterm sessions backups restore NAME` additively recreates workspaces/tabs missing from the live desk — it replaces nothing and never closes what exists.
27. **Manage local services (daemon registry)**: `codeterm service list [--kind cron|daemon]`, `codeterm service add NAME --health-mode http|tcp|process|script [...] [--push]` (registers a daemon for the Mesh tab; `--push` returns a per-service Bearer token printed ONCE for the tab send endpoint), `codeterm service show ID`, `codeterm service edit ID --enabled true|false`, `codeterm service delete ID`, `codeterm service token ID --rotate` (old token invalidated immediately, also auto-enables push).
28. **Memory pin / unpin**: `codeterm mem pin <id>` and `codeterm mem unpin <id>` toggle `is_pinned_wakeup` so the item is always loaded into the L1 wake-up payload (subject to budget). `codeterm mem pinned [--limit N]` lists every pinned item.
29. **Atomic pinned save**: `codeterm mem save --kind fact --title "T" --pin --workspace $CODETERM_WORKSPACE_ID "<body>"` creates a pinned, workspace-scoped item in a single round-trip — no follow-up PATCH.
30. **Workspace-aware wake-up**: `codeterm mem wake-up` inherits the calling tab's workspace, boosts items scoped there by `+0.5` in the L1 score, and admits them alongside global rows. Use `--workspace <id>` only to override the caller. Add `--explain` to append a per-item score breakdown for debugging selection.
31. **Calibration audit / manual run**: `codeterm mem calibration log [--since ISO] [--limit N] [--count]` shows the dreamer's importance / archive sweep history. `codeterm mem calibration sweep [--force]` runs it manually — server-gated to Sunday unless `--force`, hard-capped at 30 mutations per session.
32. **Audit-trail actor**: `codeterm mem update <id> --changed-by <actor> ...` sets the `X-CodeTerm-Actor` header so `mem_item_history.changed_by` carries the actor label (e.g. `dreamer-calibration:promote`). Default is `"user"` when the flag is omitted.

## CLI Flags

All CLI commands support the `--json` flag for checked-in machine-readable automation. Agents should not request whole-list JSON merely to discover their surroundings; use the human maps, then inspect one selected record:
```bash
codeterm tab list
codeterm tab inspect <tab-id>
codeterm workspace list
```

The human workspace list is the compact top-down map: authoritative tab counts,
working directories, empty/protected state, Teams and active Plan ids, then live
tabs in stable creation order. Reserve `workspace list --json` for checked-in
automation that specifically needs the workspace-only payload.
`workspace current` prints a bare id for shell use. Its point-query `--json`
form adds `source: explicit|caller-tab|workspace-env|focus-fallback` when
automation must distinguish the caller's own workspace from the globally active
fallback. Resolution is never positional: `--workspace`, then `$CODETERM_TAB_ID`
joined through workspace membership, then a validated `$CODETERM_WORKSPACE_ID`,
then the active workspace. A caller with no identity at all gets an empty stdout
and exit 1, so a script must not treat an empty result as success.

## Integrations

### Telegram Bridge
CodeTerm routes messages between Telegram and the General Agent:
- **Inbound:** Messages arrive as `[Telegram: user_id] text` in the agent's PTY. Rapid messages within 300ms are debounced. Typing indicator is sent automatically.
- **Outbound:** Agent sends replies via CLI: `codeterm bridge send --to USER_ID --service telegram "text"`. Within 60s, messages auto-edit the previous one. Use `--append` to concatenate text.

Setup via CLI:
```bash
# Save token to secrets dir
mkdir -p ~/.codeterm/secrets
echo -n "BOT_TOKEN" > ~/.codeterm/secrets/telegram.token

# Enable
codeterm config set integrations.telegram_enabled true
codeterm config set integrations.telegram_allowed_users '["USER_ID"]'

# Restart CodeTerm (hot-reload not yet supported)
```

Sending messages from the agent:
```bash
# Send a new message
codeterm bridge send --to 123456789 --service telegram "Hello from CodeTerm"

# Append to the previous message (edits in-place)
codeterm bridge send --to 123456789 --service telegram --append "...continued"
```

Connected tabs show service icons in the sidebar (`connected_services` field).

#### Hot-relocating the poller

Bridge migration, its flags, exit codes, rollback, and split-brain recovery are owned by `codeterm docs MESH`.

### Provider Hook Integration
Session binding needs each provider's CodeTerm hooks installed and trusted. No Settings click is required:
```bash
codeterm integration status --provider codex            # read install/trust state
codeterm integration install --provider codex           # records consent, installs, grants trust
codeterm integration install --provider claude --mesh PEER   # same on a paired peer
codeterm integration install --provider claude --precompact-memory-export       # enable memory export
codeterm integration install --provider claude --no-precompact-memory-export    # disable memory export
codeterm integration sweep-profiles --provider codex    # preview dead profiles and legacy files
codeterm integration sweep-profiles --provider codex --confirm  # delete this instance's dead profiles
codeterm integration sweep-profiles --provider codex --confirm --include-legacy  # also delete unmarked files
```
Omitting either PreCompact flag preserves the current CodeTerm hook choice or its recorded setting. Once consent is recorded, daemon start and provider plugin reload repair a missing, stale, or untrusted CodeTerm hook by themselves (reinstall plus trust grant). An existing CodeTerm-owned install without a consent record is recorded as consent before reconciliation; foreign hooks are never rewritten or trusted.
Profile cleanup is dry-run by default. `--confirm` deletes only files marked for this instance whose tab ids are not live here. `--include-legacy` with `--confirm` deletes every unmarked profile, regardless of tab liveness, so review the preview before opting in.

### Config Management
Read/write CodeTerm config at runtime:
```bash
codeterm config get integrations.telegram_enabled
codeterm config set integrations.telegram_enabled true
codeterm config get general_agent                    # read entire section
```
Dot-notation for nested keys. Values are JSON (true, false, 42, "string", ["array"]).

## Related documents

Agent hierarchy and session integration: `codeterm docs AGENT-CONTROL`.

For `send --every --mesh` and `delayed create/edit --every --mesh`, the peer must advertise `delayed_recurrence` in `codeterm identify` features. If it does not, the request is refused with the peer name and version; update that peer or send a one-shot explicitly.

<!-- revision: 74 -->
