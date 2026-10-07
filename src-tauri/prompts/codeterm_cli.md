# Domios CLI reference

<!-- Do not document commands or flags that don't exist. Full generated
     reference: generated/cli/cli.md; the validation contract is .agents/skills/cli-contributor/SKILL.md. -->

You are inside Domios, an AI-first terminal multiplexer. CLI binary: `codeterm`. Detail is discoverable: run `codeterm <cmd> --help` for flags. Use codeterm docs <NAME>; it returns the complete document. Use `codeterm docs` or `codeterm docs --help` to discover available names. This section carries only invariants and the capability map.

Env vars ALWAYS set in every tab: `$CODETERM=1`, `$CODETERM_TAB_ID` (your internal tab id), `$CODETERM_SOCKET_PATH`, `$CODETERM_API_URL`, `$CODETERM_API_PORT`, `$CODETERM_SESSION_BINDING_NONCE`.

Env vars set CONDITIONALLY, only when the tab actually has that identity — test before you use one: `$CODETERM_PARENT_TAB_ID` (set for a tab with a parent; a root Orchestrator legitimately has none), `$CODETERM_ROLE`, `$CODETERM_TEAM_ID`, `$CODETERM_WORKER_ID`, `$CODETERM_PLAN_ID`, `$CODETERM_TASK_ID`, `$CODETERM_ATTEMPT_ID`, `$CODETERM_STARTUP_ATTEMPT_ID` (agent-created launch handshake), `$CODETERM_LANE_BASE` (task-bound launches), `$CODETERM_PAD_DIR`, `$CODETERM_API_TOKEN`. `codeterm plan record --help` and `codeterm agent report --help` render the ones they default from as `[env: …]`. There is NO `$CODETERM_WORKSPACE_ID` in a shell tab — resolve the workspace with `codeterm workspace list` or `codeterm tab list`.

## Hard invariants

- DISCOVERY HABIT: CLI FIRST, ALWAYS. For anything Domios-related, run `codeterm <cmd> --help` and read the owning document BEFORE curling APIs, grepping source, or reading repo/doc files. If a `codeterm` verb exists for it, the verb is the answer — never raw `curl /api/...`, never spelunking `~/.codeterm*` internals.
- INTERACTIVE DISCOVERY STAYS HUMAN-READABLE. Use plain `codeterm tab list` for the complete local workspace → Team/Plan → Tab map, including empty workspaces, and preserve `(self)`; pass `--workspace <id>` when one workspace's scoped tree is intended. Inspect one selected tab with `codeterm tab inspect <id>`; add `--lifecycle` only for its journal telemetry. Never request whole-list JSON merely to orient yourself; it is an automation contract.
- `codeterm tab release-ownership` frees a local tab from stale remote control.
- TAB IDENTITY: a tab title is one word with no whitespace. Put activity prose in `description`, using `--description` when spawning an agent or `codeterm tab describe` for an existing tab.
- ROUTING: Follow the host-supplied return hint (see `codeterm docs ROUTING`): agent senders use its command; shell senders get a reply in your own chat/output.
- WORKSPACE INHERITANCE IS AUTOMATIC. `tab new`, `agent spawn`, and `cron create` land in YOUR workspace — the one that owns your tab — never in the workspace the user happens to have selected in the UI. Pass `--workspace <id>` only to place work in a different workspace on purpose.
- UTILITY SHELL PURPOSE: `tab new --purpose build|dev|qa|shell` labels the shell in the sidebar (default `shell`). Use build for builds, dev for servers, qa for checks. Purpose survives `tab restart` and desk restore. Running/idle and last exit come only from shell command marks; no evidence stays unknown.
- TEAM CONTAINMENT IS AUTOMATIC. From a live Team tab, ordinary `tab new` shells and `cron create` jobs inherit that Team. Use `--no-team` only when the work must deliberately outlive Team cleanup; `--team <id>` remains the explicit cross-check for a named Team.
- `chat_backend` TABS HAVE NO SHELL. Check `backing.kind` in `tab list` before instructing a tab to run commands; send chat text instead.
- DELAYED SEND IS NOT THE DEFAULT. `codeterm send` is immediate; `--delay`/`--at` are for user-requested future delivery. `send --every` repeats into an EXISTING tab (monitor/watcher); cron spawns a FRESH agent per scheduled run. With `--mesh`, recurring delays require the target to advertise `delayed_recurrence`; an incapable peer is refused, so update it or send a one-shot explicitly.
- NEVER HARDCODE MODEL IDS. Discover with `codeterm agent providers` / `codeterm agent models <provider>`, which refreshes live-discovery catalogues before listing.
- MODEL SELECTION: `--model` is validated through each provider's catalogue capability. Authoritative catalogues refuse unknown ids before creating a tab and show close ids; free-form providers keep provider-owned ids. If a required live catalogue is unavailable, the requested id is preserved and the response carries a warning.
- LIVE MODEL SWITCH: `codeterm agent model --tab TAB MODEL` relaunches that idle tab on its own verified session; `model_confirmation: "pending"` means no model was reported before the deadline, and the label follows the first turn.
- SESSION FORK: `codeterm agent fork --tab TAB` opens a separate tab when its provider declares forking. Its session stays pending until that new launch supplies its own evidence. With `background_work_guard` enabled, model switches refuse running background work; an explicit `--force` override reports the affected items.
- Version/quota questions: `codeterm identify` (not `--version`, not curl) and `codeterm usage latest` (never `ccusage` or parsing `~/.claude`).
- System sleep: `codeterm keepawake ...` — never `caffeinate`/`powercfg`/`systemd-inhibit`.
- AGENT-TO-AGENT TRAFFIC IS ENGLISH (task bodies, reports, cron tasks, mesh messages); only user-facing channels carry the user's language.
- SECRETS NEVER GO TO MEM — `codeterm mem secret` / `codeterm docs SECRETS`.

## Capability map — where to look when the task mentions…

Each command's `--help` has the flags; the Docs column has the depth.

| Task mentions… | Use | Docs |
|---|---|---|
| Version, abilities, documents | `identify`, `capabilities`, `docs` | — |
| Tabs: list, read, send | `tab list`, then `tab inspect` / `tab get-text --tab <ID>`; `send`; chat backends: `tab chat`; a numbered prompt the provider classifier confirmed: `tab respond OPTION --tab <ID>` | `docs ROUTING` |
| A tab with a wrong or stale agent binding | `tab rebind`, `tab repair --from-evidence`, `tab adopt-agent` (read `--help` first). Never type task or ACK text into an unverified shell or picker | `docs ROUTING` |
| Servers, watchers | `tab new --purpose dev --cmd "..."`; replace it with `tab restart --tab ID --cmd "..."` | — |
| Agents: spawn, restart, switch provider, report | `agent spawn`, `agent restart`, `agent report`, `agent providers` / `agent models` | `docs ORCHESTRATION` |
| Becoming an Orchestrator | Tab menu Make Orchestrator, or `--role Orchestrator` | `docs ORCHESTRATION` |
| Plans, specs, tasks | `plan --help`, `spec --help`, `task --help` | `docs PLAN-EDITING` |
| Delayed or repeated message to a tab | `send --delay` / `--at` / `--every`, `delayed --help` | — |
| Scheduled jobs (fresh agent per run) | `cron create --help`; a short meaningful name is required | `docs CONFIG` |
| Watch CI, builds, files, agents and act | `blueprint recipe`, then `--dry-run` | `docs BLUEPRINTS` |
| Telegram | `bridge send --to ID --service telegram "..."`; long answers continue with `--append` | `docs TELEGRAM-SETUP` |
| Remote machines | Mesh block below | `docs MESH` |
| Memory, skills | Memory block below | `docs MEMORY` |
| Past chats | `sessions search` / `get` / `resume` (never hand-craft `--resume`) | — |
| Config | `config get [key]`, `config set` | `docs CONFIG` |
| Notifying the user | `notify send "title" --from-pad PATH --level verbose\|action_needed\|failure` | `docs CONFIG` |
| Plugins, provider CLIs | `plugin list` / `install` / `update`, `agent cli status <provider>` | `docs API-CLI` |
| Model endpoints, presets | `model-provider --help`; keys go in on stdin only | `docs INTEGRATIONS` |
| Updates | `update check`, `update install --auto` | — |
| Payload between machines | `paste put`, `paste get <id>` (not `mem save`) | — |
| Services registry | `service --help` | `docs MESH` |
| Essence map | `essence-map --help` | `docs ESSENCE-MAP` |
| Provider launch leftovers | `integration sweep-profiles --provider <id>` is a dry run; only the owner adds `--confirm` | — |
| Keep the machine awake | `keepawake start` / `stop` / `status` | — |

## Plan/task quick flags — the intuitive guesses that are WRONG

These are the plan/task flags agents most often guess by analogy and get wrong.
Verified against the clap tree; `codeterm <cmd> --help` has the rest. The command
words below resolve; the flags are the corrections.

Task-bound Workers use `codeterm agent ack` to move their assigned task to in-progress. A repeated acknowledgement reports already acknowledged for the same bound task and attempt, without changing the assignment. A refusal `plan_skill_attempt_binding_missing` goes to the Orchestrator for binding repair. An authenticated ACK also returns the current attempt’s plan skill manifest. Read applicable skills with `codeterm plan skill show <slug>` (default: your Team’s active plan; `--plan` reads another without rebinding).

- `codeterm plan create --title "..."` — NO `--goal`. Optional `--from-spec <id>`, `--kinds`, `--project`. The goal is set separately, after creation. A Team-bound caller without `--project` uses its Team workspace cwd.
- Message and pad file options accept `-` to read UTF-8 stdin, including `plan task dispatch --message-file -`, `send --from-file -`, and `--from-pad -`.
- `codeterm plan goal set <plan> "<goal>" --vector "<how success is proven>"` — the goal text is an optional trailing positional; `--vector` is REQUIRED. `--vision`/`--vision-file`, `--constraint`, `--postulate` are optional.
- `codeterm plan task add <plan> --name "..." --sections <csv> --criterion "..."` — the flags are `--name` (NOT --title), `--sections` (NOT --section), `--criterion` (repeatable; NOT --acceptance). Body: `--body`/`--body-file`. Quality (each has a `-file` twin): `--test-plan`/`--test-plan-file`, `--correctness`/`--correctness-file`, `--qa-plan`/`--qa-plan-file`, `--criterion-file`. Worker contract: `--executor-role`, `--personality`, and `--role-guidance`/`--role-guidance-file` are REQUIRED, and `update` refuses to blank any of them. Routing stays optional: `--model <provider>:<model>`, `--why`, `--intent`. Here `--execution-intent` is the enum `repository_mutation|non_repository`.
- `codeterm plan task dispatch <plan> <task> --execution-intent read_only|mutating` — NO `--brief`. The brief IS the task body (delivered on dispatch); to add more, pass `--from-pad` on the spawned agent. `--execution-intent` is a REQUIRED enum (`read_only|mutating`, distinct from task-add's enum), not free text. Repeat `--authorized-gate <token>` to grant exact gate-claim tokens; omit it to grant none. `--canonical-writer` (mutating only, explicit) makes this Worker the exclusive canonical writer: refused while another live attempt occupies canonical; its report must match canonical `HEAD` with no dirt.
- For a paired peer, add global `--mesh <peer>` to `plan task dispatch` with `--execution-intent mutating`. The attempt, ACK, report, and review stay on the origin; after accepting the report, run `codeterm plan task merge-remote-lane <plan> <task> <attempt>` on the origin to fetch the authenticated lane bundle and create a real merge commit.
- `codeterm agent spawn <provider> --role Worker --unplanned --reason "<why>" [--task "<brief>"]` — exceptional dispatch for small, urgent, or exploratory work. The daemon creates a visible `unplanned: <reason>` task in the caller's bound Plan and dispatches it through the task lifecycle. A reason is required; a Team without a bound Plan and non-Orchestrator callers are refused.
- `codeterm plan task update <plan> <task> --model <provider>:<model>` — `--model` is a free-form routing string stored verbatim (e.g. `claude:haiku`); a bare provider like `claude` is accepted but only resolves to a concrete model later, at dispatch. Same quality/persona flags as add, EXCEPT `--criterion`/`--criterion-file`: on `update` they APPEND to the existing acceptance-criteria array by default — pass `--replace-criteria` to explicitly replace it wholesale instead (reports before/after counts).
- `codeterm agent spawn <provider> --role Worker --plan <plan> --execution-intent read_only --authorized-gate "<command>"` — the canonical verification Worker. `--plan-task` is NOT required and must NOT be passed: the plan's implementation task keeps its own owner. It owns no task, so it reports to `--parent-tab`/`--report-to` WITHOUT `--status` (a report naming a task it does not hold is refused and writes nothing), and repeated `--authorized-gate` is its whole command scope, delivered in its brief. A plain Worker with `--execution-intent mutating` and no task binding is REFUSED; `--unplanned --reason` is the exceptional dispatched-task path.
- `codeterm plan review authorize-reporter <plan> <tab> --class review|canonical_qa` — the live Plan owner grants a task-less tab evidence-only reporting authority tied to its current session and binding nonce. That tab reports with `codeterm agent report --plan <plan>` and no `--task`/`--status`; success appends a plan stride and reaches the Orchestrator without granting Planner edits, task ownership, or a transition. A rebound session must be authorized again.
- `codeterm agent spawn --role Planner --plan <plan>` — `--parent-tab` is optional here: when omitted, the parent is resolved from the plan's live Orchestrator owner instead of being refused. An explicit `--parent-tab` still wins when given.
- `codeterm plan task show <plan> <task>` (or `codeterm task show <task>`) — the task-body reader (body, criteria, test_plan, correctness, executor_role, sections). Every `<task>` argument accepts the `task_xxxxxx` id, the handle, or the human key (`COD-12`); an ambiguous reference is refused, never guessed.
- `codeterm plan task split <plan> <task> --child "name" [--child "name" ...]` or `--from-file children.json` — the file is a JSON array of child objects with `name`, optional `body`, `criteria` string array, `test_plan`, `sections` (array or CSV), and routing overrides; omitted fields inherit from the parent. Dispatch children separately. Workers propose splits in reports and never spawn sub-Workers.
- `codeterm plan mode <plan> <mode>` — the mode is a trailing positional OR `--set <mode>`; values `planning|approved|execution|complete`.
- `codeterm plan review accept <plan> <task> --note-from-pad review.md` (also `bounce`/`hold`). Scalar `--note` allowed; authored inline refused. See `codeterm docs BOUNCE`.
- `codeterm plan in-progress [PLAN_ID]` — the bare form scopes to the current directory's project; a named plan ignores that ambient project, and `--all` lists every project.
- `codeterm tab search <PATTERN>` — searches every workspace and labels each hit with its workspace; `--mesh PEER` applies the same rule on that peer.
- `codeterm team pair --orchestrator <provider[:model]> --planner <provider[:model]>` — there is NO `team create`; a Team is minted at plan creation or via `team pair`. Team verbs: `list|show|rename|close|adopt-plan|adopt-agent|attach-cartographer|pair`. `codeterm team close <team>` tears the Team down: removes clean ancestry-merged lanes, keeps dirty or unmerged ones as cleanup debt, removes members, finalizes attempts, deletes Team crons and clears routing; closing a workspace runs the same teardown. A Manager or Worker tab (a Team leaf) is refused unless it also passes `--acknowledge-leaf`, which attributes the new Team to the calling tab.
- Agents CAN join a Team. `codeterm agent spawn <provider> --team <team-id>` attaches a fresh roleless agent; `codeterm team adopt-agent <team-id> <tab-id>` attaches one that is already running. Both require YOUR OWN membership (`$CODETERM_TEAM_ID` must already be that Team). `--team` is rejected with `--plan-task` (that binding carries its own Team) and is local-only, never with `--mesh`. Adopting a tab that already has a Team or a role is refused; `codeterm team show <team-id>` lists the member tabs.
- A Team whose Orchestrator is gone gets a new one with `codeterm agent spawn <provider> --team <team-id> --role Orchestrator`. The Team and its active Plan survive the final Orchestrator's removal, so that command can still name them. It is the ONLY `--role` `--team` accepts, needs no `--workspace`, and carries no other spawn flag. The new Orchestrator takes over the Team's active Plan, owner history included. A failed takeover removes the replacement and its lane and changes no Team or Plan history. A Team that still HAS a live Orchestrator refuses the second one by error code (`team_orchestrator_already_live`); a removed or dead Orchestrator does not count as live.

## Mesh (remote peers)

Flat peer model: paired = trusted. Most tab/agent/workspace commands accept `--mesh PEER` = "run against that peer's daemon". Flow: `mesh peers` → `tab list --mesh PEER` → act on remote ids. A peer's General Agent is always `--tab main --mesh PEER`; remote agents don't auto-reply — say "report back" explicitly. Pairing: `mesh connect|approve|reject|disconnect`; disconnect revokes the peer (only the local owner lifts it with `mesh unrevoke <instance-id>`). Make this machine discoverable to a NEW device with `mesh broadcast` (requires `mesh online`; tell the user when you toggle it). Visibility: `mesh offline` / `mesh online` (hot-reload, Tailscale untouched). Mesh spawn requires peer `--cwd`; it sends locally read `--task-file` contents.

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

Structured task context: `codeterm plan task files PLAN TASK --edit-file PATH --read-file PATH` replaces the task's file lists. Repeat flags for multiple paths; omit both to clear. Paths resolve against the Worker cwd at dispatch. Read files must exist; editable files may be new. A file cannot belong to both lists.

Read completed tool-less Worker turns with `codeterm agent turns --tab TAB --last N`. This uses exact session evidence. Results include outcomes and a bounded-window continuation cursor; see `codeterm docs PLAN-EDITING`.
