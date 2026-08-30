# CodeTerm CLI reference

<!-- Do not document commands or flags that don't exist. Full generated
     reference: docs/CLI.md; the validation contract is docs/contributing/cli.md. -->

You are inside CodeTerm, an AI-first terminal multiplexer. CLI binary: `codeterm`. Detail is discoverable: run `codeterm <cmd> --help` for flags. Use codeterm docs <NAME>; it returns the complete document. Use codeterm docs or codeterm docs --help to discover available names. This section carries only invariants and the capability map.

Env vars ALWAYS set in every tab: `$CODETERM=1`, `$CODETERM_TAB_ID` (your internal tab id), `$CODETERM_SOCKET_PATH`, `$CODETERM_API_URL`, `$CODETERM_API_PORT`, `$CODETERM_SESSION_BINDING_NONCE`.

Env vars set CONDITIONALLY, only when the tab actually has that identity — test before you use one: `$CODETERM_PARENT_TAB_ID` (set for a tab with a parent; a root Orchestrator legitimately has none), `$CODETERM_ROLE`, `$CODETERM_TEAM_ID`, `$CODETERM_WORKER_ID`, `$CODETERM_PLAN_ID`, `$CODETERM_TASK_ID`, `$CODETERM_ATTEMPT_ID`, `$CODETERM_STARTUP_ATTEMPT_ID` (agent-created launch handshake), `$CODETERM_LANE_BASE` (task-bound launches), `$CODETERM_PAD_DIR`, `$CODETERM_API_TOKEN`. `codeterm plan record --help` and `codeterm agent report --help` render the ones they default from as `[env: …]`. There is NO `$CODETERM_WORKSPACE_ID` in a shell tab — resolve the workspace with `codeterm workspace list` or `codeterm tab list`.

## Hard invariants

- DISCOVERY HABIT: CLI FIRST, ALWAYS. For anything CodeTerm-related, run `codeterm <cmd> --help` and read the owning document BEFORE curling APIs, grepping source, or reading repo/doc files. If a `codeterm` verb exists for it, the verb is the answer — never raw `curl /api/...`, never spelunking `~/.codeterm*` internals.
- INTERACTIVE DISCOVERY STAYS HUMAN-READABLE. Use plain `codeterm workspace list` for the top-down workspace → Team/Plan → Tab map, including empty workspaces. Use `codeterm tab list` for the current workspace and preserve `(self)`. Inspect one selected tab with `codeterm tab inspect <id>`; add `--lifecycle` only for its journal telemetry. Never request whole-list JSON merely to orient yourself; it is an automation contract.
- ROUTING: THE PREFIX IS THE RETURN ADDRESS. Reply on the channel named by the `-=-codeterm:...-=-` marker (see `codeterm docs ROUTING`). The CLI command IS the reply — never echo the same content in your tab afterward.
- WORKSPACE INHERITANCE IS AUTOMATIC. `tab new`, `agent spawn`, and `cron create` land in YOUR workspace — the one that owns your tab — never in the workspace the user happens to have selected in the UI. Pass `--workspace <id>` only to place work in a different workspace on purpose.
- TEAM CONTAINMENT IS AUTOMATIC. From a live Team tab, ordinary `tab new` shells and `cron create` jobs inherit that Team. Use `--no-team` only when the work must deliberately outlive Team cleanup; `--team <id>` remains the explicit cross-check for a named Team.
- `chat_backend` TABS HAVE NO SHELL. Check `backing.kind` in `tab list` before instructing a tab to run commands; send chat text instead.
- DELAYED SEND IS NOT THE DEFAULT. `codeterm send` is immediate; `--delay`/`--at` are delay-only scheduling tools for user-requested future delivery.
- NEVER HARDCODE MODEL IDS. Discover with `codeterm agent providers` / `codeterm agent models <provider>`.
- Version/quota questions: `codeterm identify` (not `--version`, not curl) and `codeterm usage latest` (never `ccusage` or parsing `~/.claude`).
- System sleep: `codeterm keepawake ...` — never `caffeinate`/`powercfg`/`systemd-inhibit`.
- AGENT-TO-AGENT TRAFFIC IS ENGLISH (task bodies, reports, cron tasks, mesh messages); only user-facing channels carry the user's language.
- SECRETS NEVER GO TO MEM — `codeterm mem secret` / `codeterm docs SECRETS`.

## Capability map — where to look when the task mentions…

| Domain / trigger | Commands | Depth |
|---|---|---|
| Environment, version, what-can-this-do | `codeterm identify`, `capabilities`, `docs` | — |
| Tabs, sending text/keys, reading a tab | Plain `tab list` first; use `tab inspect <id>` only for a selected tab's full details, then `send`, `tab chat`, `tab get-text`, `tab exec`. Answer a detected numbered provider prompt with `tab respond OPTION --tab ID`. If the same visible picker remains, one bounded `send --actions "enter"` or `"down,enter"` fallback may follow its current selection; inspect again and never resend the task payload. Recover a stale provider-session binding without interrupting the agent with `tab rebind SESSION_ID --tab ID`. | `docs ROUTING` |
| Long-running processes (servers, watchers) | `tab new --cmd "..."`; later use `tab restart --tab ID --cmd "..."` to replace that shell without creating another tab | — |
| Scheduled/future message to a tab | `send --delay/--at`, `delayed --help` | — |
| Spawning agents, worker/orchestrator shapes, reports | `agent spawn --help`, `agent startup-ack`, `agent startup-hold`, `agent report`, `agent providers/models` | `codeterm docs ORCHESTRATION` |
| Becoming an Orchestrator | You are not automatically an Orchestrator. Use the tab menu Make Orchestrator action or the `--role Orchestrator` flag. | as above |
| Plans, specs, tasks, strides, canvas | `plan --help`, `spec --help`, `task --help` | as above |
| Recurring/oneshot jobs | `cron create --help` (read it first — flags evolve) | `docs CONFIG` |
| Telegram / bridges | `bridge send --to ID --service telegram "..."`; long answers: first send, then `--append` parts | `docs TELEGRAM-SETUP` |
| Remote machines (mesh) | see Mesh block below | `docs MESH` |
| Memory, remember/recall, skills | see Mem block below | `docs MEMORY` |
| Past chat sessions | `sessions search/get/resume` (resume handles cwd — never hand-craft `--resume`) | — |
| Services/daemons registry | `service --help` | `docs MESH` |
| Config keys | `config get <key>` (dot-notation) or bare `config get` to dump the whole redacted config; `config set` (JSON values) | `docs CONFIG` |
| Desktop notifications | `notify send "title" [--from-pad PATH] [--severity info\|warning\|error] [--level verbose\|action_needed\|failure] [--event-key ..] [--choice LABEL]...` is canonical; `notify list` / `notify clear`. `send` is required — there is no bare `notify "title"`. The body is read only from the calling tab's AgentPad; an inline body is refused at parse time. `--level` takes only those three values; `verbose` is progress, `action_needed` is waiting on the user, `failure` is a stop. Repeat `--choice "Label"` once per option, no cap, to offer tappable answers — labels only, never free text; the tapped label returns to the tab that asked, and blank or duplicate labels (compared trimmed and case-insensitively) are refused before the notification is sent. The result carries `unseen` with a reason per closed surface when the bell is hidden and neither a toast nor a push channel will show it; the record is still written and the exit is still 0 | `docs CONFIG` |
| Updates | `update check`, `update install --auto` | — |
| Plugins/providers | `plugin list/install/update/update-system` | `codeterm docs API-CLI` |
| Cross-machine payload handoff | `paste put`, then `paste get <id>` / `paste rm <id>` (id may be bare `fpu2zp` or `paste:fpu2zp` — both resolve); `--pop` for one-shot (in-memory, TTL) — not `mem save` | — |
| Keep system awake | `keepawake start/stop/status` | — |

## Plan/task quick flags — the intuitive guesses that are WRONG

These are the plan/task flags agents most often guess by analogy and get wrong.
Verified against the clap tree; `codeterm <cmd> --help` has the rest. The command
words below resolve; the flags are the corrections.

- `codeterm plan create --title "..."` — NO `--goal`. Optional `--from-spec <id>`, `--kinds`, `--project`. The goal is set separately, after creation.
- `codeterm plan goal set <plan> "<goal>" --vector "<how success is proven>"` — the goal text is an optional trailing positional; `--vector` is REQUIRED. `--vision`/`--vision-file`, `--constraint`, `--postulate` are optional.
- `codeterm plan task add <plan> --name "..." --sections <csv> --criterion "..."` — the flags are `--name` (NOT --title), `--sections` (NOT --section), `--criterion` (repeatable; NOT --acceptance). Body: `--body`/`--body-file`. Quality (each has a `-file` twin): `--test-plan`/`--test-plan-file`, `--correctness`/`--correctness-file`, `--qa-plan`/`--qa-plan-file`, `--criterion-file`. Worker contract: `--executor-role` and `--role-guidance`/`--role-guidance-file` are REQUIRED, and `update` refuses to blank either one. Routing/persona stays optional: `--model <provider>:<model>`, `--why`, `--personality`, `--intent`. Here `--execution-intent` is the enum `repository_mutation|non_repository`.
- `codeterm plan task dispatch <plan> <task> --execution-intent read_only|mutating` — NO `--brief`. The brief IS the task body (delivered on dispatch); to add more, pass `--from-pad` on the spawned agent. `--execution-intent` is a REQUIRED enum (`read_only|mutating`, distinct from task-add's enum), not free text.
- `codeterm plan task update <plan> <task> --model <provider>:<model>` — `--model` is a free-form routing string stored verbatim (e.g. `claude:haiku`); a bare provider like `claude` is accepted but only resolves to a concrete model later, at dispatch. Same quality/persona flags as add.
- `codeterm plan task show <plan> <task>` (or `codeterm task show <task>`) — the task-body reader (body, criteria, test_plan, correctness, executor_role, sections).
- `codeterm plan mode <plan> <mode>` — the mode is a trailing positional OR `--set <mode>`; values `planning|approved|execution|complete`.
- `codeterm plan review accept <plan> <task> --note "..."`, `codeterm plan review bounce <plan> <task> --note "..."`, `codeterm plan review hold <plan> <task> --note "..."` — every review verdict requires `--note`, never `--reason`; `bounce` returns work to the Worker and `hold` preserves its assignment while the review remains open.
- `codeterm plan in-progress [PLAN_ID]` — the bare form scopes to the current directory's project; a named plan ignores that ambient project, and `--all` lists every project.
- `codeterm tab search <PATTERN>` — searches every workspace and labels each hit with its workspace; `--mesh PEER` applies the same rule on that peer.
- `codeterm team pair --orchestrator <provider[:model]> --planner <provider[:model]>` — there is NO `team create`; a Team is minted at plan creation or via `team pair`. Team verbs: `list|show|rename|close|adopt-plan|adopt-agent|pair`.
- Agents CAN join a Team. `codeterm agent spawn <provider> --team <team-id>` attaches a fresh roleless agent (mirrors `tab new --team`); `codeterm team adopt-agent <team-id> <tab-id>` attaches one that is already running. Both authorize on YOUR OWN membership — `$CODETERM_TEAM_ID` must already be that Team — and refuse a non-member caller. `--team` is rejected with `--role`/`--plan-task` (those bindings carry their own Team) and is local-only, never with `--mesh`. Adopting a tab that already has a Team or a role is refused; `codeterm team show <team-id>` lists the member tabs.

Source of truth: this file (`src-tauri/prompts/codeterm_cli.md`) plus the generated `docs/CLI.md`; `~/.claude/skills/codeterm` mirrors the repo `skills/codeterm`.

## Mesh (remote peers)

Flat peer model: paired = trusted. Most tab/agent/workspace commands accept `--mesh PEER` = "run against that peer's daemon". Flow: `mesh peers` → `tab list --mesh PEER` → act on remote ids. A peer's General Agent is always `--tab main --mesh PEER`; remote agents don't auto-reply — say "report back" explicitly. Pairing: `mesh connect|approve|reject|disconnect`; make this machine discoverable to a NEW device with `mesh broadcast` (requires `mesh online`; tell the user when you toggle it). Visibility: `mesh offline` / `mesh online` (hot-reload, Tailscale untouched). Long `--task` bodies over mesh go via `paste put --mesh PEER --pop`, not inline.

## Memory (mem)

Persistent vector + keyword store; hybrid search fuses FTS5 + embeddings on every query. Protocol:
- All mem content is English (titles, bodies, queries) — the embedder is English-dominant; translate before saving, reply to the user in their language.
- "Remember" (in any language) = run `mem save` now — acknowledging without saving loses it. Background workers handle classification/graph/dedup.
- Search before answering about any person, project, or past event: `mem search "<query>"`; reformulate once (keyword → natural phrase) before giving up. Identity questions: `mem wake-up`.
- Never guess ids — search first, then `mem get <real-id>`.
- Facts change → save the new fact; no dates in bodies (timestamps are tracked).

```bash
codeterm mem search "<query>" [--kind K] [--limit N]
codeterm mem save --kind fact|note|learning|skill|rule|preference --title "..." "<body>"
codeterm mem get <id> · mem wake-up · mem save-preference --title "..." --replace "<body>"
codeterm mem skill new <slug>        # skills are FOLDERS with SKILL.md; never write to ~/.claude/skills
```
Past conversations are NOT mem: `codeterm sessions search`. Kinds, graph, pin/learn, calibration, and the Dreamer are in the document the capability map already names.

## Platform notes

Forward slashes in paths (CLI normalizes); `/tmp/` works everywhere; macOS maps config `Ctrl` to `Cmd`.
