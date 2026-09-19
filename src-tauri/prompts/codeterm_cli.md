# CodeTerm CLI reference

<!-- Do not document commands or flags that don't exist. Full generated
     reference: generated/cli/cli.md; the validation contract is essences/contributing/cli.md. -->

You are inside CodeTerm, an AI-first terminal multiplexer. CLI binary: `codeterm`. Detail is discoverable: run `codeterm <cmd> --help` for flags. Use codeterm docs <NAME>; it returns the complete document. Use codeterm docs or codeterm docs --help to discover available names. This section carries only invariants and the capability map.

Env vars ALWAYS set in every tab: `$CODETERM=1`, `$CODETERM_TAB_ID` (your internal tab id), `$CODETERM_SOCKET_PATH`, `$CODETERM_API_URL`, `$CODETERM_API_PORT`, `$CODETERM_SESSION_BINDING_NONCE`.

Env vars set CONDITIONALLY, only when the tab actually has that identity — test before you use one: `$CODETERM_PARENT_TAB_ID` (set for a tab with a parent; a root Orchestrator legitimately has none), `$CODETERM_ROLE`, `$CODETERM_TEAM_ID`, `$CODETERM_WORKER_ID`, `$CODETERM_PLAN_ID`, `$CODETERM_TASK_ID`, `$CODETERM_ATTEMPT_ID`, `$CODETERM_STARTUP_ATTEMPT_ID` (agent-created launch handshake), `$CODETERM_LANE_BASE` (task-bound launches), `$CODETERM_PAD_DIR`, `$CODETERM_API_TOKEN`. `codeterm plan record --help` and `codeterm agent report --help` render the ones they default from as `[env: …]`. There is NO `$CODETERM_WORKSPACE_ID` in a shell tab — resolve the workspace with `codeterm workspace list` or `codeterm tab list`.

## Hard invariants

- DISCOVERY HABIT: CLI FIRST, ALWAYS. For anything CodeTerm-related, run `codeterm <cmd> --help` and read the owning document BEFORE curling APIs, grepping source, or reading repo/doc files. If a `codeterm` verb exists for it, the verb is the answer — never raw `curl /api/...`, never spelunking `~/.codeterm*` internals.
- INTERACTIVE DISCOVERY STAYS HUMAN-READABLE. Use plain `codeterm tab list` for the complete local workspace → Team/Plan → Tab map, including empty workspaces, and preserve `(self)`; pass `--workspace <id>` when one workspace's scoped tree is intended. Inspect one selected tab with `codeterm tab inspect <id>`; add `--lifecycle` only for its journal telemetry. Never request whole-list JSON merely to orient yourself; it is an automation contract.
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
| Tabs, sending text/keys, reading a tab | Plain `tab list` first for the complete local workspace map; use `--workspace <id>` for one workspace's scoped tree and `tab inspect <id>` only for a selected tab's full details, then `send`, `tab chat`, `tab get-text`, `tab exec`. Answer a detected numbered provider prompt with `tab respond OPTION --tab ID`. If the same visible picker remains, one bounded `send --actions "enter"` or `"down,enter"` fallback may follow its current selection; inspect again and never resend the task payload. Recover a stale provider-session binding without interrupting the agent with `tab rebind SESSION_ID --tab ID`. When the user reports that this live tab has the wrong provider identity, run `codeterm tab repair --from-evidence`; use `--from-journal` when prior lifecycle provenance is the requested source. Repair verifies the tab's own session, does not restart it, and only self or the task's Plan owner may target it. To keep a live foreign agent PTY and make `send --from-pad` a routed agent message, run `codeterm tab adopt-agent --tab ID --provider <id>`; a login shell is refused unless the owner passes `--force`. | `docs ROUTING` |
| Long-running processes (servers, watchers) | `tab new --cmd "..."`; later use `tab restart --tab ID --cmd "..."` to replace that shell without creating another tab | — |
| Scheduled/future message to a tab | `send --delay/--at`, `delayed --help` | — |
| Spawning agents, worker/orchestrator shapes, reports | `agent spawn --help`, `agent startup-ack`, `agent startup-hold`, `agent report`, `agent providers/models` | `codeterm docs ORCHESTRATION` |
| Becoming an Orchestrator | You are not automatically an Orchestrator. Use the tab menu Make Orchestrator action or the `--role Orchestrator` flag. | as above |
| Plans, specs, tasks, strides, canvas | `plan --help`, `spec --help`, `task --help` | as above |
| Recurring/oneshot jobs | `cron create --help` (read it first — flags evolve) | `docs CONFIG` |
| Telegram / bridges | `bridge send --to ID --service telegram "..."`; long answers: first send, then `--append` parts. A tab's send lands in its own topic; `bridge thread open` prints one relayable sentence; only the General Agent may add `--tab ID` for another tab | `docs TELEGRAM-SETUP` |
| Remote machines (mesh) | see Mesh block below | `docs MESH` |
| Memory, remember/recall, skills | see Mem block below | `docs MEMORY` |
| Past chat sessions | `sessions search/get/resume` (resume handles cwd — never hand-craft `--resume`) | — |
| Services/daemons registry | `service --help` | `docs MESH` |
| Config keys | `config get <key>` (dot-notation) or bare `config get` to dump the whole redacted config; `config set` (JSON values) | `docs CONFIG` |
| Desktop notifications | `notify send "title" [--from-pad PATH] [--severity info\|warning\|error] [--level verbose\|action_needed\|failure] [--event-key ..] [--choice LABEL]...` is canonical; `notify list` / `notify clear`. `send` is required — there is no bare `notify "title"`. The body is read only from the calling tab's AgentPad; an inline body is refused at parse time. `--level` takes only those three values; `verbose` is progress, `action_needed` is waiting on the user, `failure` is a stop | `docs CONFIG` |
| Updates | `update check`, `update install --auto` | — |
| Source-backed essence map | `essence-map generate`, `essence-map validate`, `essence-map record --summary .. --reason ..`, `essence-map release-summary [--since decision-N]`, `essence-map revert --to decision-N`; validates stable essence `map` anchors against the generated node ids | `docs ESSENCE-MAP` |
| Plugins/providers | `plugin list/install/update/update-system` | `codeterm docs API-CLI` |
| Cross-machine payload handoff | `paste put`, then `paste get <id>` / `paste rm <id>` (id may be bare `fpu2zp` or `paste:fpu2zp` — both resolve); `--pop` for one-shot (in-memory, TTL) — not `mem save` | — |
| Keep system awake | `keepawake start/stop/status` | — |

## Plan/task quick flags — the intuitive guesses that are WRONG

These are the plan/task flags agents most often guess by analogy and get wrong.
Verified against the clap tree; `codeterm <cmd> --help` has the rest. The command
words below resolve; the flags are the corrections.

- `codeterm plan create --title "..."` — NO `--goal`. Optional `--from-spec <id>`, `--kinds`, `--project`. The goal is set separately, after creation.
- `codeterm plan goal set <plan> "<goal>" --vector "<how success is proven>"` — the goal text is an optional trailing positional; `--vector` is REQUIRED. `--vision`/`--vision-file`, `--constraint`, `--postulate` are optional.
- `codeterm plan task add <plan> --name "..." --sections <csv> --criterion "..."` — the flags are `--name` (NOT --title), `--sections` (NOT --section), `--criterion` (repeatable; NOT --acceptance). Body: `--body`/`--body-file`. Quality (each has a `-file` twin): `--test-plan`/`--test-plan-file`, `--correctness`/`--correctness-file`, `--qa-plan`/`--qa-plan-file`, `--criterion-file`. Worker contract: `--executor-role`, `--personality`, and `--role-guidance`/`--role-guidance-file` are REQUIRED, and `update` refuses to blank any of them. Routing stays optional: `--model <provider>:<model>`, `--why`, `--intent`. Here `--execution-intent` is the enum `repository_mutation|non_repository`.
- `codeterm plan task dispatch <plan> <task> --execution-intent read_only|mutating` — NO `--brief`. The brief IS the task body (delivered on dispatch); to add more, pass `--from-pad` on the spawned agent. `--execution-intent` is a REQUIRED enum (`read_only|mutating`, distinct from task-add's enum), not free text. Repeat `--authorized-gate <token>` to grant exact gate-claim tokens; omit it to grant none. `--canonical-writer` (mutating only) is the explicit opt-in that places this one Worker in the canonical checkout as its exclusive writer: never inferred, refused while another live attempt occupies canonical, and lane acceptance waits for its verdict.
- `codeterm plan task update <plan> <task> --model <provider>:<model>` — `--model` is a free-form routing string stored verbatim (e.g. `claude:haiku`); a bare provider like `claude` is accepted but only resolves to a concrete model later, at dispatch. Same quality/persona flags as add, EXCEPT `--criterion`/`--criterion-file`: on `update` they APPEND to the existing acceptance-criteria array by default — pass `--replace-criteria` to explicitly replace it wholesale instead (reports before/after counts).
- `codeterm agent spawn <provider> --role Worker --plan <plan> --execution-intent read_only --authorized-gate "<command>"` — the canonical verification Worker. `--plan-task` is NOT required and must NOT be passed: the plan's implementation task keeps its own owner. It owns no task, so it reports to `--parent-tab`/`--report-to` WITHOUT `--status` (a report naming a task it does not hold is refused and writes nothing), and repeated `--authorized-gate` is its whole command scope, delivered in its brief. A Worker with `--execution-intent mutating` and no task binding is REFUSED; a task-bound Worker declares gates on `plan task dispatch --authorized-gate`, not here.
- `codeterm plan review authorize-reporter <plan> <tab> --class review|canonical_qa` — the live Plan owner grants a task-less tab evidence-only reporting authority tied to its current session and binding nonce. That tab reports with `codeterm agent report --plan <plan>` and no `--task`/`--status`; success appends a plan stride and reaches the Orchestrator without granting Planner edits, task ownership, or a transition. A rebound session must be authorized again.
- `codeterm agent spawn --role Planner --plan <plan>` — `--parent-tab` is optional here: when omitted, the parent is resolved from the plan's live Orchestrator owner instead of being refused. An explicit `--parent-tab` still wins when given.
- `codeterm plan task show <plan> <task>` (or `codeterm task show <task>`) — the task-body reader (body, criteria, test_plan, correctness, executor_role, sections). Every `<task>` argument accepts the `task_xxxxxx` id, the handle, or the human key (`COD-12`); an ambiguous reference is refused, never guessed.
- `codeterm plan mode <plan> <mode>` — the mode is a trailing positional OR `--set <mode>`; values `planning|approved|execution|complete`.
- `codeterm plan review accept <plan> <task> --note "..."`, `codeterm plan review bounce <plan> <task> --note "..."`, `codeterm plan review hold <plan> <task> --note "..."` — every review verdict requires `--note`, never `--reason`; `bounce` returns work to the Worker and `hold` preserves its assignment while the review remains open.
- `codeterm plan in-progress [PLAN_ID]` — the bare form scopes to the current directory's project; a named plan ignores that ambient project, and `--all` lists every project.
- `codeterm tab search <PATTERN>` — searches every workspace and labels each hit with its workspace; `--mesh PEER` applies the same rule on that peer.
- `codeterm team pair --orchestrator <provider[:model]> --planner <provider[:model]>` — there is NO `team create`; a Team is minted at plan creation or via `team pair`. Team verbs: `list|show|rename|close|adopt-plan|adopt-agent|pair`. A Manager or Worker tab (a Team leaf) is refused unless it also passes `--acknowledge-leaf`, which attributes the new Team to that calling tab rather than to some other intermediary.
- Agents CAN join a Team. `codeterm agent spawn <provider> --team <team-id>` attaches a fresh roleless agent (mirrors `tab new --team`); `codeterm team adopt-agent <team-id> <tab-id>` attaches one that is already running. Both authorize on YOUR OWN membership — `$CODETERM_TEAM_ID` must already be that Team — and refuse a non-member caller. `--team` is rejected with `--plan-task` (that binding carries its own Team) and is local-only, never with `--mesh`. Adopting a tab that already has a Team or a role is refused; `codeterm team show <team-id>` lists the member tabs.
- A Team whose Orchestrator is gone gets a new one with `codeterm agent spawn <provider> --team <team-id> --role Orchestrator`. Removing the final Orchestrator retains the discoverable Team and active Plan identity/history so that command can still name them. It is the ONLY `--role` `--team` accepts, needs no `--workspace`, and carries no other spawn flag. The new Orchestrator takes the Team's active Plan over — owner and owner history move; descendant role bindings follow ordinary unlink rules. A takeover failure removes the spawned replacement and its lane without changing that retained Team or Plan history. A Team that still HAS a live Orchestrator refuses the second one by error code (`team_orchestrator_already_live`); a removed or dead Orchestrator does not count as live.

Source of truth: this file (`src-tauri/prompts/codeterm_cli.md`) plus the generated `generated/cli/cli.md`; `~/.claude/skills/codeterm` mirrors the repo `skills/codeterm`.

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
