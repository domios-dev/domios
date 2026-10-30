# Domios Skills for Claude Code

Domios ships skills that teach Claude Code how to control the terminal.


Shell profiles are available through `domios tab new --list-shells`; select one with `domios tab new --shell <profile-id>`. Profiles include native shells and installed WSL distributions on Windows. For Linux tooling or reproducing a Linux-only failure, open a utility shell with `domios tab new --purpose qa --shell wsl:Ubuntu --cmd "uname -a"` (use the distribution id from the list). WSL shells map host drive paths to `/mnt/<drive>/...` and report Linux working directories back as host paths; the selected profile survives restore. Agents continue running natively; agent launches inside WSL are not supported. Choose a default shell in Settings > General; Automatic preserves the existing OS shell choice. Refresh shells there or in the New Terminal menu after installing a shell.

## Installation

Domios installs its control skill automatically on every start; register other skills through `domios mem skill` and find them with `domios mem search`. A skill copy whose `<!-- revision: N -->` is strictly newer than the bundled one is never downgraded; the provider copies are generated, so edit the canonical skill rather than a mirror.

## Available Skills

### `codeterm` — Core terminal control
- Create tabs, manage workspaces
- Send commands to other tabs
- List tabs

## How it works

Skills are markdown files that Claude Code loads automatically when relevant. When Claude detects it's running in Domios (via `$CODETERM=1` env var and `$CODETERM_TAB_ID` for self-identification), it can use the `domios` CLI to:

1. **Send commands**: `domios send "npm test"` (sends immediately to the current tab, newline appended automatically). The CLI refuses task-shaped prose for a raw **shell** tab unless `--to-shell` is explicit; short single-line commands remain eligible. A shell receives only the raw text/keys, without a route tag; an anonymous CLI send receives `<domios from="cli">`; `--system-context build` delivers a system notice without a reply address. Other sends to an agent tab receive a host-authored `<domios from="tab" ...>` route tag and closing `</domios>` so it can identify the sender. The Chat UI `/chat-send` route refuses PTY tabs without a live agent identity before writing. For quote-heavy or multiline text, use `domios send --stdin --tab ID` or `domios send --from-file - --tab ID`; message and pad file options accept `-` to read UTF-8 stdin on every platform. For terminal keys, use `domios send --actions "esc,delay:200ms,up,enter" --tab ID` (`enter`, arrows, `tab`, `backspace`, `ctrl-c`, `key*3`, and `delay:250ms` are supported) — these deliver as raw key bytes to a shell, with no wrapping. To spawn a bare terminal that runs a command on start, use `domios tab new --purpose dev --cwd /repo --cmd "npm run dev"`; inside a Team it becomes a Team utility shell unless `--no-team` is explicit. Replace that process later with `domios tab restart --tab ID --cwd /repo --cmd "npm run dev"`; it preserves the tab and Team metadata instead of creating another tab. To schedule a future tab message without blocking yourself, use `domios send --delay 10m --tab ID "text"`, `--delay 4h`, `--delay 1d`, or `--at 2026-06-15T09:00`; an absolute `--at` in the past is due immediately and sends as soon as the scheduler can deliver it. Manage queued items with `domios delayed list|edit|send-now|delete`; failed messages remain listed with their failure reason. Use `send --every` to repeat into an EXISTING tab (a monitor/watcher); cron spawns a FRESH agent per scheduled run. With `--mesh`, the remote target Team owns the series; delete it on that peer because closing the caller Team will not cancel it.

Immediate `domios send` exits 1 with its reason on stderr whenever the result is `ok:false` or `bytes_written:false`, including retained pre-ready input. With `--json`, the error envelope preserves the daemon receipt in `data`. A retained `send_id` or queued delivery requires status or exact own-session transcript inspection; never replay held, written or uncertain input.
2. **Send notifications**: `domios notify send "Done" --level action_needed` — `--level` (verbose|action_needed|failure) is what lets it alert; without one it only files in the bell. An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. A question closes when the owner taps a choice or sends the next chat message in the asking tab. After receiving an answer elsewhere, close your own question with `domios notify resolve <id>`. Resolved questions leave Needs you and resume eligible Curator rounds. A short completion or progress note may be filed at `verbose` — a periodic status, never a running commentary of one notification per step; Worker and Planner tabs still report through their reporting channel instead of notifying. A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried
3. **Create workspaces**: `domios workspace create "Feature X"`
4. **Read visible terminal text**: `domios tab get-text` (reads what the user sees). If a tab is showing an abandoned provider session, recover the live transcript without restarting the agent with `domios tab rebind SESSION_ID --tab ID`; the command refuses a session that has not materialized. If the tab's provider label is wrong, repair it from its own verified evidence with `domios tab repair --from-evidence`; use `--from-journal` to select the newest restorable provenance record. Repair never restarts the tab and is authorized only for the tab itself or its task's Plan owner. To keep a live foreign agent PTY and make send a routed agent message, use `domios tab adopt-agent --tab ID --provider <id>`; a login shell is refused unless the owner passes `--force`.
5. **Name tab**: `domios tab rename "Scout"` or `domios tab rename "Scout" --tab ID`. Tab titles are one word; put activity prose in `domios tab describe "..."`. A task-bound tab keeps its planned name and records progress in its description
6. **Create theme**: `domios theme save my-theme /tmp/theme.toml --apply`
7. **Agent hierarchy**: `domios agent set-orchestrator` (mark self as orchestrator)
8. **Compact context**: `domios agent compact [--tab ID]` (sends /compact to Claude)
9. **Choose model**: use `domios agent models <provider>` to list ids and `domios agent spawn [provider] --model MODEL --task "..."` to select one. Authoritative catalogues and free-form providers with a published live model list refuse unknown ids before creating a tab and show valid ids. A unique bare id selects its provider; when several providers advertise it, specify `--agent` for task dispatch or pass the provider positionally to spawn. Free-form providers without a live list pass provider-owned ids through. To switch a live tab, run `domios agent model --tab TAB MODEL`: Domios uses the provider's Live capability only when that tab's bound server advertises its session switch route, and confirms from that same session record. Otherwise it uses RestartResume, reports `mode_used` plus `fallback_reason`, and requires the tab to be idle. A Live response without matching session-record confirmation is reported as pending. Providers with neither mode refuse the switch. `domios agent restart --tab TAB` reports `action=resumed` or `action=fresh_launch`; a known provider in a confirmed `LaunchBindingUnconfirmed` shell with no verified session is started fresh from its recorded launch. To change a live tab's provider, use `domios agent restart --tab TAB --provider PROVIDER`; this starts a fresh provider session. Add `--handoff-history` to include bounded, redacted transcript context in the provider's first launch task. History handoff is opt-in and never resumes the source provider's session.
10. **Send report**: `domios agent report "task done"` (report to direct parent, auto-injected into parent's PTY)
11. **Message a running agent**: `domios send --tab ID "status?"` (immediate agent-directed message via the normal send transport)
12. **List tabs / hierarchy**: `domios tab list` (all local workspaces with tab counts, paths, Teams and active Plans, sub-agents nested, caller marked `(self)`; `--workspace <id>` selects the existing scoped tree and `--agents` filters to agent tabs). Use `domios tab inspect <id>` for one tab's full payload, or add `--lifecycle` for its journal telemetry.
13. **Spawn agent**: `domios agent spawn claude --title "Worker" --description "Reviews tab routing" --role Worker --parent-tab ID --task "do work"` (title is a single word; description says what the tab does; creates a tab without switching active tab; task delivery uses the provider-owned launch transport, with a host attribution frame in the initial task when the sender is a routed tab)
14. **Spawn with agent definition**: via REST API `POST /api/agents/spawn` with `agent_name` field (loads `.claude/agents/<name>.md`; not available in CLI)
15. **Cross-workspace spawn**: `domios agent spawn claude --workspace WS_ID --title "Remote" --description "Reviews API contracts" --role Worker --parent-tab ID --task "work"`
16. **Create cron job**: `domios cron create review-todos --schedule "0 9 * * *" --task "Review TODOs"` (name jobs by what they do; lands in your own workspace and inherits your Team; use `--no-team` only for a deliberately independent job; add `--once` for oneshot: `--schedule "in 30m" --once`)
17. **Edit cron job**: `domios cron edit ID --schedule "*/15 * * * *"` (prefer over delete+recreate to preserve history)
18. **Complete cron run**: `domios cron complete --run-id UUID --summary "result"`
19. **List cron jobs**: `domios cron list`, **trigger manually**: `domios cron trigger ID`
20. **Read config**: `domios config get integrations.telegram_enabled` (dot-notation); bare `domios config get` (no key) dumps the whole redacted config
21. **Write config**: `domios config set integrations.telegram_enabled true` (JSON values)
22. **Send to Telegram**: `domios bridge send --to USER_ID --service telegram "text"` (auto-edits within 60s, `--append` to concatenate)
23. **Keep system awake**: `domios keepawake start --duration-secs 3600 [--display]` (cross-platform: caffeinate on macOS, SetThreadExecutionState on Windows). Omit `--duration-secs` to stay awake until an explicit stop: `domios keepawake stop`. Status: `domios keepawake status`. Use this instead of calling `caffeinate` / `powercfg` / `systemd-inhibit` directly.
24. **Search past agent sessions**: `domios sessions search "QUERY" [--provider claude|codex|all] [--project /path] [--limit N]` — content search across every registered provider's sessions. Each hit carries its `provider` and `session_id`; a `warning:` line names any provider store that could not be fully searched, so an empty result is not proof of absence.
25. **Inspect a past session**: `domios sessions get SESSION_ID` — provider, recorded cwd, file path, timestamps. Use before `resume` to verify the JSONL is still on disk.
26. **Resume a past session atomically**: `domios sessions resume SESSION_ID [--title T] [--workspace-id ID] [--workspace-name N]` — `--title` is a tab name with a single word. The command locates the session, opens the right workspace at the recorded cwd, spawns the agent tab, and hands the resume id to the provider with the correct CLI shape. Use this instead of the brittle "create workspace at guessed cwd → `agent spawn --args=--resume=UUID`" dance, which breaks because `claude --resume` needs the JSONL under the current encoded project directory.
    - **Recover a reduced/wiped desk**: `domios sessions backups list` shows rotated full-desk snapshots (name, saved time, workspace/tab counts); `domios sessions backups restore NAME` additively recreates workspaces/tabs missing from the live desk — it replaces nothing and never closes what exists.
27. **Manage local services (daemon registry)**: `domios service list [--kind cron|daemon]`, `domios service add NAME --health-mode http|tcp|process|script [...] [--push]` (registers a daemon for the Mesh tab; `--push` returns a per-service Bearer token printed ONCE for the tab send endpoint), `domios service show ID`, `domios service edit ID --enabled true|false`, `domios service delete ID`, `domios service token ID --rotate` (old token invalidated immediately, also auto-enables push).
28. **Memory pin / unpin**: `domios mem pin <id>` and `domios mem unpin <id>` toggle `is_pinned_wakeup` so the item is always loaded into the L1 wake-up payload (subject to budget). `domios mem pinned [--limit N]` lists every pinned item.
29. **Atomic pinned save**: `domios mem save --kind fact --title "T" --pin --workspace $CODETERM_WORKSPACE_ID "<body>"` creates a pinned, workspace-scoped item in a single round-trip — no follow-up PATCH.
30. **Workspace-aware wake-up**: `domios mem wake-up` inherits the calling tab's workspace, boosts items scoped there by `+0.5` in the L1 score, and admits them alongside global rows. Use `--workspace <id>` only to override the caller. Add `--explain` to append a per-item score breakdown for debugging selection.
31. **Calibration audit / manual run**: `domios mem calibration log [--since ISO] [--limit N] [--count]` shows the dreamer's importance / archive sweep history. `domios mem calibration sweep [--force]` runs it manually — server-gated to Sunday unless `--force`, hard-capped at 30 mutations per session. `domios mem calibration stale --dry-run` lists the year-untouched items the gated nightly stale archive would take, with reasons; `domios mem merge` and `domios mem restore` keep Dreamer dedup reversible.
32. **Audit-trail actor**: `domios mem update <id> --changed-by <actor> ...` sets the `X-CodeTerm-Actor` header so `mem_item_history.changed_by` carries the actor label (e.g. `dreamer-calibration:promote`). Default is `"user"` when the flag is omitted.

## CLI Flags

All CLI commands support the `--json` flag for checked-in machine-readable automation. Agents should not request whole-list JSON merely to discover their surroundings; use the human maps, then inspect one selected record:
```bash
domios tab list
domios tab inspect <tab-id>
domios workspace list
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
Domios routes messages between Telegram and the General Agent:
- **Inbound:** Messages arrive as `[Telegram: user_id] text` in the agent's PTY. Rapid messages within 300ms are debounced. Typing indicator is sent automatically.
- **Outbound:** Agent sends replies via CLI: `domios bridge send --to USER_ID --service telegram "text"`. Within 60s, messages auto-edit the previous one. Use `--append` to concatenate text.

Setup via CLI:
```bash
# Save token to secrets dir
mkdir -p ~/.domios/secrets
echo -n "BOT_TOKEN" > ~/.domios/secrets/telegram.token

# Enable
domios config set integrations.telegram_enabled true
domios config set integrations.telegram_allowed_users '["USER_ID"]'

# Restart Domios (hot-reload not yet supported)
```

Sending messages from the agent:
```bash
# Send a new message
domios bridge send --to 123456789 --service telegram "Hello from Domios"

# Append to the previous message (edits in-place)
domios bridge send --to 123456789 --service telegram --append "...continued"
```

Connected tabs show service icons in the sidebar (`connected_services` field).

#### Hot-relocating the poller

Bridge migration, its flags, exit codes, rollback, and split-brain recovery are owned by `domios docs MESH`.

### Provider Hook Integration
Session binding needs each provider's Domios hooks installed and trusted. No Settings click is required:
```bash
domios integration status --provider codex            # read install/trust state
domios integration install --provider codex           # records consent, installs, grants trust
domios integration install --provider claude --mesh PEER   # same on a paired peer
domios integration sweep-profiles --provider codex    # preview dead profiles and legacy files
domios integration sweep-profiles --provider codex --confirm  # owner deletes reclaimable marked profiles
domios integration sweep-profiles --provider codex --confirm --include-legacy  # also delete eligible dead legacy profiles
```
Once consent is recorded, daemon start and provider plugin reload repair a missing, stale, or untrusted Domios hook by themselves (reinstall plus trust grant). An existing Domios-owned install without a consent record is recorded as consent before reconciliation; foreign hooks are never rewritten or trusted.
Profile cleanup is dry-run by default. Only the machine-owner instance may delete. `--confirm` removes this instance's dead marked profiles and foreign marked profiles only when that registered instance's exclusive lifetime lock is held throughout cleanup. Unknown and possibly live instances are kept. `--include-legacy` includes unmarked profiles only with an exact generated-tab filename and a tab id that is not live here. Project trust is carried over before removal; each pass removes at most 200 profiles. Owner startup performs this legacy migration once, continues capped passes while removals make progress, and reports removed and retained counts once. Live tabs and user files are kept.

### Config Management
Read/write Domios config at runtime:
```bash
domios config get integrations.telegram_enabled
domios config set integrations.telegram_enabled true
domios config get general_agent                    # read entire section
```
Dot-notation for nested keys. Values are JSON (true, false, 42, "string", ["array"]).

## Related documents

Agent hierarchy and session integration: `domios docs AGENT-CONTROL`.

For `send --every --mesh` and `delayed create/edit --every --mesh`, the peer must advertise `delayed_recurrence` in `domios identify` features. If it does not, the request is refused with the peer name and version; update that peer or send a one-shot explicitly.

<!-- revision: 91 -->
