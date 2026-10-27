# Domios CLI reference

You are inside Domios, an AI-first terminal multiplexer. CLI binary: `domios`; `codeterm` remains a fully working alias without a deprecation warning. Detail is discoverable: run `domios <cmd> --help` for flags. Use domios docs <NAME>; it returns the complete document. Use `domios docs` or `domios docs --help` to discover available names. This section carries only invariants and the capability map.

Env vars ALWAYS set in every tab: `$CODETERM=1`, `$CODETERM_TAB_ID` (your internal tab id), `$CODETERM_SOCKET_PATH`, `$CODETERM_API_URL`, `$CODETERM_API_PORT`, `$CODETERM_SESSION_BINDING_NONCE`.

Env vars set CONDITIONALLY, only when the tab actually has that identity — test before you use one: `$CODETERM_PARENT_TAB_ID` (set for a tab with a parent; a root Orchestrator legitimately has none), `$CODETERM_ROLE`, `$CODETERM_TEAM_ID`, `$CODETERM_WORKER_ID`, `$CODETERM_PLAN_ID`, `$CODETERM_TASK_ID`, `$CODETERM_ATTEMPT_ID`, `$CODETERM_STARTUP_ATTEMPT_ID` (agent-created launch handshake), `$CODETERM_LANE_BASE` (task-bound launches), `$CODETERM_PAD_DIR`, `$CODETERM_API_TOKEN`. `domios plan record --help` and `domios agent report --help` render the ones they default from as `[env: …]`. There is NO `$CODETERM_WORKSPACE_ID` in a shell tab — resolve the workspace with `domios workspace list` or `domios tab list`.

With the local socket unavailable, credentialed fallback probes RPC first; a `starting` reply waits on readiness events for up to 30 seconds before sending the command. Without credentials, discovery checks loopback health first. REST listener recovery returns `starting` with the phase immediately. A deadline preserves the phase; `startup_failed` is terminal. Commands never replay.

Worker kinds add role prompts automatically: use `agent spawn --role Worker --kind qa` for verification, `--kind reviewer` for read-only code review or `--kind release_manager` for release coordination, with `--parent-tab`; `plan task dispatch` takes the same `--kind`. Both can perform assigned mutating work with or without `--plan-task`; task-less reports omit `--status`. For parent-authorized task-bound canonical placement, use `plan task dispatch <plan> <task> --execution-intent mutating --canonical-writer --kind qa` or set `--kind release_manager`. Dispatch preserves task/attempt reporting identity and enforces one live writer. Direct task-bound spawn cannot reserve the canonical attempt. Task-less kinds with Plan context retain parent-directed placement behavior and do not reserve task attempts or participate in canonical writer occupancy, so the parent coordinates those canonical writes. Publication and tags require owner GO.

Programmatic sends to agents are framed, including anonymous CLI sends. Host job reports use `domios send --system-context build --tab ID --from-file PATH`; system notices have no reply address.

## Hard invariants

- DISCOVERY HABIT: CLI FIRST, ALWAYS. For anything Domios-related, run `domios <cmd> --help` and read the owning document BEFORE curling APIs, grepping source, or reading repo/doc files. If a `domios` verb exists for it, the verb is the answer — never raw `curl /api/...`, never spelunking `~/.domios*` internals.
- INTERACTIVE DISCOVERY STAYS HUMAN-READABLE. Use plain `domios tab list` for the complete local workspace → Team/Plan → Tab map, including empty workspaces, and preserve `(self)`; pass `--workspace <id>` when one workspace's scoped tree is intended. Inspect one selected tab with `domios tab inspect <id>`; add `--lifecycle` only for its journal telemetry. Never request whole-list JSON merely to orient yourself; it is an automation contract.
- `domios tab release-ownership` frees a local tab from stale remote control.
- LAUNCH SPEED: `agent spawn`, `plan task dispatch`, and `plan task respawn` accept `--speed <TIER>` from the provider's declared service tiers. `plan task update --speed <TIER>` stores the task default; dispatch and respawn inherit it unless overridden. `default` explicitly selects the provider default.
- TAB IDENTITY: a tab title is one word with no whitespace. `plan task dispatch` derives a short Worker description from the task name; `--description` overrides it. Direct `agent spawn` still needs `--description`; use `domios tab describe` for an existing tab.
- ROUTING: Follow the host-supplied return hint inside `<tip>…</tip>` (see `domios docs ROUTING`): agent senders use its command; shell senders get a reply in your own chat/output.
- WORKSPACE INHERITANCE IS AUTOMATIC. `tab new`, `agent spawn`, and `cron create` land in YOUR workspace — the one that owns your tab — never in the workspace the user happens to have selected in the UI. Pass `--workspace <id>` only to place work in a different workspace on purpose.
- UTILITY SHELL PURPOSE: `tab new --purpose build|dev|qa|deploy|logs|db|shell` labels the shell in the sidebar (default `shell`). Use build for builds, dev for servers, qa for checks, deploy for releases, logs for tails, db for database consoles. Purpose survives `tab restart` and desk restore. Restart with `--cmd` returns on submission to the fresh shell; `command_submission.status: submit_unconfirmed` means uncertainty: never replay. Use `tab exec` to wait for completion. Restart retains the per-command bound; `--timeout` overrides it. Running/idle and last exit come only from shell command marks; no evidence stays unknown.
- LINUX TOOLING: when a task needs Linux tooling or reproduces a Linux-only failure, open a utility shell with `tab new --shell wsl:<distro>` (`tab new --list-shells` lists options); agents themselves run natively.
- TEAM CONTAINMENT IS AUTOMATIC. From a live Team tab, ordinary `tab new` shells and `cron create` jobs inherit that Team. Use `--no-team` only when the work must deliberately outlive Team cleanup; `--team <id>` remains the explicit cross-check for a named Team.
- `chat_backend` TABS HAVE NO SHELL. Check `backing.kind` in `tab list` before instructing a tab to run commands; send chat text instead.
- Remote task locations come from provider `remote_task` metadata. Use `agent spawn <provider> --location cloud --environment <id>` only when its capability reports available. These are one-task tabs, with no assumed follow-up, cancellation or Worker authority. Never replay unconfirmed creation; inspect the existing provider task list.
- DELAYED SEND IS NOT THE DEFAULT. `domios send` is immediate; `--delay`/`--at` are for user-requested future delivery. `send --every` repeats into an EXISTING tab (monitor/watcher); cron spawns a FRESH agent per scheduled run. With `--mesh`, recurring delays require the target to advertise `delayed_recurrence`; an incapable peer is refused, so update it or send a one-shot explicitly.
- SEND RESULT: Immediate `send` exits 1 with a reason on stderr when `ok:false` or `bytes_written:false`, including retained pre-ready input. JSON failures retain the daemon receipt in `data`. A retained `send_id` or queued delivery is pending, not delivered: inspect its status or exact own-session transcript and never replay held, written or uncertain input.
- NEVER HARDCODE MODEL IDS. Discover with `domios agent providers` / `domios agent models <provider>`, which refreshes live-discovery catalogues before listing.
- MODEL SELECTION: `--model` is validated through each provider's catalogue capability. Authoritative catalogues refuse unknown ids before creating a tab and show close ids; free-form providers keep provider-owned ids. If a required live catalogue is unavailable, the requested id is preserved and the response carries a warning.
- CUSTOM ENDPOINTS: configured endpoint models appear under their owning harness in `agent models <harness>`. Select with `agent spawn <harness> --model <endpoint>/<model>`; the endpoint owns model validation. An endpoint id colliding with an installed harness is refused with `harness_id_collision`.
- LIVE MODEL SWITCH: `domios agent model --tab TAB [MODEL]` switches a model or changes settings on that exact tab. `--reasoning-effort LEVEL` and `--runtime-mode MODE` must match the selected model/provider catalogue; omit MODEL to keep the current model. Refused values exit non-zero and report supported choices. JSON retains `mode_used`, `model_confirmation`, and `evidence`; `pending` means no model was reported before its deadline, and the label follows the first turn.
- SERVICE TIER: `domios agent model --tab TAB --list-speeds` lists that tab's session-bound model tiers; `--speed TIER` selects one and `--speed default` clears it. Never infer tiers from provider names or static labels.
- SESSION FORK: `domios agent fork --tab TAB` opens a separate tab when its provider declares forking. Its session stays pending until that new launch supplies its own evidence. With `background_work_guard` enabled, model switches refuse running background work; an explicit `--force` override reports the affected items.
- Version/quota questions: `domios identify` (not `--version`, not curl) and `domios usage latest` (never `ccusage` or parsing `~/.claude`).
- System sleep: `domios keepawake ...` — never `caffeinate`/`powercfg`/`systemd-inhibit`.
- AGENT-TO-AGENT TRAFFIC IS ENGLISH (task bodies, reports, cron tasks, mesh messages); only user-facing channels carry the user's language.
- SECRETS NEVER GO TO MEM — `domios mem secret` / `domios docs SECRETS`.
- Message and pad file options accept `-` for UTF-8 stdin: `send --from-file -`, `--from-pad -`, `plan task remote-report --message-file -`.

## Capability map — where to look when the task mentions…

Each command's `--help` has the flags; the Docs column has the depth.

| Task mentions… | Use | Docs |
|---|---|---|
| Version, abilities, documents | `identify`, `capabilities`, `docs` | — |
| Tabs: list, read, search, send, todos | `tab list`, then `tab inspect` / `tab get-text --tab <ID>`; `tab search <PATTERN>` spans every workspace; `tab todo list --tab <CHILD_ID>` inspects a child checklist through the authorized parent API; `send`; chat backends: `tab chat`; a numbered prompt the provider classifier confirmed: `tab respond OPTION --tab <ID>` | `docs ROUTING` |
| A tab with a wrong or stale agent binding | `tab rebind`, `tab repair --from-evidence`, `tab adopt-agent` (read `--help` first). Never type task or ACK text into an unverified shell or picker | `docs ROUTING` |
| Servers, watchers | `tab new --purpose dev --cmd "..."`; replace it with `tab restart --tab ID --cmd "..."` | — |
| Agents: spawn, restart, switch provider, report | `agent spawn`, `agent restart`, `agent switch-provider`, `agent switch-provider-status`, `agent report`, `agent providers` / `agent models` | `docs ORCHESTRATION` |
| Agent usage, reset credits, child transcript | `agent usage read|reset`, `agent child-history` (all require exact tab ids) | `docs API-CLI` |
| Becoming an Orchestrator | Tab menu Make Orchestrator, or `--role Orchestrator` | `docs ORCHESTRATION` |
| Plans, specs, tasks | `plan --help`, `spec --help`, `task --help`; `plan lanes <plan> --clean` previews merged lanes and unregistered build residue, `--confirm` executes, `--fetch` refreshes refs; dirty lanes stay protected | `docs PLAN-EDITING` |
| Delayed or repeated message to a tab | `send --delay` / `--at` / `--every`, `delayed --help` | — |
| Scheduled jobs (fresh agent per run) | `cron create --help`; a short meaningful name is required | `docs CONFIG` |
| Watch CI, builds, files, agents or receive webhooks and act | `blueprint new --intent`, typed call when a card fits, else author/check/apply; per-graph webhook credentials: `blueprint webhook`; `blueprint keep` returns the saved ID and any `evicted_ids`, which the CLI also prints | `docs BLUEPRINTS` |
| Telegram | `bridge send --to ID --service telegram "..."`; long answers continue with `--append` | `docs TELEGRAM-SETUP` |
| Remote machines | Mesh block below | `docs MESH` |
| Memory, skills | Memory block below | `docs MEMORY` |
| Spinal Cord (Supporter Agent): explain, connect, verify, disconnect | `supporter --help` (`spinal-cord` alias) | `domios docs SPINAL-CORD` |
| Past chats | `sessions search` / `get` / `resume`; read-only `sessions backups export latest --sanitize --out <file>` captures a desk | — |
| Config | `config get [key]`, `config set` | `docs CONFIG` |
| Notifying the user | `notify send "title" --from-pad PATH --level verbose\|action_needed\|failure` | `docs CONFIG` |
| Plugins, provider CLIs | `plugin list` / `install` / `update`, `agent cli status <provider>` | `docs API-CLI` |
| Plugin app tabs (surfaces) | `surface list`, `surface open <plugin>:<surface>`, `surface close --tab <ID>`; surfaces take no `send` | `docs SURFACES` |
| Model endpoints, presets | `model-provider --help`; keys go in on stdin only | `docs INTEGRATIONS` |
| Account sign-in and identity | `account login|status|logout` (local machine; hosted sign-in opens the system browser) | `domios docs CONFIG` |
| Encrypted Neighbors, gateway permissions and delivery status | `account neighbors`; only the authenticated General Agent authors grants and denials | `domios docs NEIGHBORS` |
| Updates | `update check`, `update install --auto`; automatic installation requires an attached native desktop host. A headless server returns a typed refusal with recovery instructions; on macOS/Windows use `update install --from-file <installer-path>` with the official installer on the target machine, and on Linux use the package manager or official package. | — |
| Payload between machines | `paste put`, `paste get <id>` (not `mem save`) | — |
| Services registry | `service --help` | `docs MESH` |
| REST listener status/recovery while HTTP is down | `rest-api status`, `rest-api restart` through the local socket | `docs CONFIG` |
| Essence map | `essence-map --help` | `docs ESSENCE-MAP` |
| Provider launch leftovers | `integration sweep-profiles --provider <id>` is a dry run; only the owner adds `--confirm` | — |
| Keep the machine awake | `keepawake start` / `stop` / `status` | — |

Plans, tasks, Teams: `domios plan --help` / `domios task --help` / `domios team --help`, with the full rules in `domios docs PLAN-EDITING` and `domios docs ORCHESTRATION`. Orchestrator, Planner and Worker launches carry their own flag contract.

Workers default to full machine access for either execution intent. For explicit confinement pass `--sandbox read-only` or `--sandbox workspace-write` to `domios agent spawn` or local `domios plan task dispatch`. Execution intent still limits task and report authority; saved sandbox policies remain immutable.

Every agent tab, Orchestrator included, keeps its own checklist with `domios todo write --from-pad todos.json` instead of the built-in todo tool. The JSON array declares the whole list as `{ "content", "status": "pending|in_progress|completed|blocked|skipped", "activeForm", "id?", "link?", "reason?" }`: the first write creates it; later writes match `id` first, then exact title, append new content and remove omitted items you added, while omitted locked items remain unchanged. Entries with `id` may omit `content` and `activeForm`; they change only status/reason. At most one item is `in_progress`; `blocked`/`skipped` need a `reason`; completed back to `in_progress` is a restart.

After `domios tab rebind SESSION_ID --tab ID` on an `agentRuntime` Chat tab, use `domios agent restart --tab ID` to resume the recorded session and its own history. `sessions resume` refuses an unusable already-bound tab and names that restart command.

The owner's explicit `domios agent restart --tab <ID> --fresh` starts a new session in the same tab with its recorded model and effort. An existing exact restored session is refused; a live agent requires `--force`. Restore never makes this choice automatically.

For an in-place provider handoff, use `domios agent switch-provider TARGET --tab TAB`. The target builds its summary from bounded recent messages and host context. Full history is not transferred. Optional `--model` and `--reasoning-effort` follow the target catalogue; a busy tab requires waiting or explicit `--force` to stop first. Self-switch is daemon-owned and returns an operation id immediately; read it with `agent switch-provider-status ID --tab TAB`. Switching `main` also saves the General’s provider/model/effort defaults.

Plan-locked Blueprint owners hold their automations with `plan_limit_locked`. Apply, edit and run-now refuse; delete and export remain available. Paid unlock resumes without replaying missed changes.

Choose 1-3 catalogue tags from `domios blueprint tags` and pass each with `--tag` when keeping any solution. Typed callable Blueprints are saved with `domios blueprint keep --file callable.yaml --when "Reuse this typed helper" --tag data-transform`; inputs stay their signature and no scheduler job is created. Reuse the returned `mem:` identity through `call.ref`. `domios blueprint show mem:<id> --json` exposes the pinned callee graph; show on a caller also includes bounded nested results and stops. Call admission uses the host checker and caller Team/workspace. The CLI freezes local import bytes before sending them; source labels never authorize daemon file reads.

Remote provider sign-in: `domios agent login --tab ID --device` starts a declared code flow; `domios agent login --tab ID --show --mesh PEER --json` reads its link, code and expiry for the owner. Never save or log login values. The sign-in card can send one phone notification. See `domios docs API-CLI`.

## Mesh (remote peers)

Adopt an unbound remote Worker into a local Team with `domios team adopt-agent <team> <worker> --peer <peer>`, or re-parent it in place with `domios agent reparent <worker> --to <local-orchestrator> --peer <peer>`; omit `--peer` for an unbound local Worker. Only the current parent or local owner may transfer, and both peers require Full access. The destination cannot pull it. Retry an uncertain request to recover the same durable operation. Task-bound Workers keep their Plan ownership path. `--mesh` executes on another daemon and is refused for these origin-side operations.

Flat peer model: paired = trusted. Most tab/agent/workspace commands accept `--mesh PEER` = "run against that peer's daemon". Flow: `mesh peers` → `tab list --mesh PEER` → act on remote ids. A peer's General Agent is always `--tab main --mesh PEER`; remote agents don't auto-reply — say "report back" explicitly. New machines come only from the owner's account: `mesh link <installation-id>` with both machines signed in to the same account, online and reachable (no broadcast, scan or address). The receiver must choose Approve on its Mesh canvas or connection notification; Decline or ten minutes without approval cancels the request. Sign-out blocks the mesh without unpairing. `mesh disconnect` removes the pair on both machines through a signed notice; an offline peer learns it on next contact. Reconnecting requires a new approval. Visibility: `mesh offline` / `mesh online` (hot-reload, Tailscale untouched). Mesh spawn requires peer `--cwd`; it sends locally read `--task-file` contents.

## Memory (mem)

The Domios memory protocol is in your provider rules. Commands: `domios mem --help`; depth: `domios docs MEMORY`. Past conversations are not mem: `domios sessions search`.

Use `domios mem taxonomy` for area counts and `domios mem area merge <from> --into <to> --dry-run` to preview a reversible area move; omit `--dry-run` to apply. Undo per item with `domios mem restore <id> --version <history-row>`.

One embedder serves all of mem: `domios mem embedder status` shows it and its re-embed progress; `domios mem embedder use <provider> --model <m>` switches it in the background. Store its key with `domios mem embedder key set <provider>` (piped stdin only, local owner-only file). A remote embedder also needs the owner's opt-in (`domios docs MEMORY`).

## Platform notes

Paths: use forward slashes; the CLI normalizes them on every OS.

Blueprint files use native `schema_version: 2` YAML with stable node ids, explicit literal/input/bind values and effect nodes. Start with `domios blueprint new --intent "<purpose>" --json`; reuse a fitting typed card by call, otherwise author/check/apply. `domios docs BLUEPRINT-AUTHORING` is the shared cookbook; check returns the sealed definition for calls, so save its real pins/dependencies before apply. Open full bodies through `domios blueprint show <id>`. Watch `ask` nodes use deterministic `matches` by default; unmatched output remains `other` without a model call. Set the node's `model_classification: true` without patterns when model judgment is wanted. A `terminal` source watches same-Team PTY output without polling, and a `send` effect delivers the stage text body. `domios blueprint input <id> --from-pad <path>` supplies another text body to an input stage. Read `domios docs BLUEPRINTS` and `domios docs BLUEPRINT-RECIPES` for native node parameters and examples.

Each Blueprint code or command step runs with the user's own permissions, like an agent in a tab, without Domios credentials in its environment; foreign Blueprints are gated by folder trust (1.15.4).

Opt-in product analytics is local: `domios analytics status`, `domios analytics enable`, and `domios analytics disable`. Disabling clears queued events and identifiers.

Account and subscriptions: `domios account plan` shows cached entitlements and a stale marker; `domios account upgrade pro` opens checkout; `domios account portal` opens subscription management. See `domios docs ACCOUNT`.

Utility shell command bounds: `tab new --timeout 60m` for tests, QA and builds; leave unset for dev servers. Each command-start mark starts a fresh deadline; idle shells never expire by it. Use `tab timeout --tab <ID> --timeout 60m` to change current and future commands from their own starts; disable with `--timeout none`. Expiry stops the command, keeps the tab open and reports to its owner. Only the creating agent can change the bound. See `domios docs TAB-LIFECYCLE`.

Relocate a workspace after its folder moves: `domios workspace set-cwd <workspace-id> <absolute-folder>`. This preserves its identity and reports any paths it could not rebase.

Repair a moved Plan project without its old folder: `domios plan set-project <plan> <path>`. It validates the target host folder and preserves Plan, task, Team and attempt identities.

Voice notes arrive as `<domios type="attachment" kind="voice" path="<file>" label="voice" />` plus `<transcript>…</transcript>`: the transcript is the message, the path its audio. `status="untranscribed"` carries a failure note instead. Older notes (bare text after the frame, or `-=-codeterm:voice file=<path>-=-`) are read-only legacy.

For cold replacement, `domios app quit --exact --json` must confirm cleanup and pinned process exits.

The plan defaults to Curator mode. Inspect it with `domios plan control-loop show <plan>`, request a round now with `domios plan control-loop run <plan>`, or choose `domios plan control-loop set <plan> curator` / `domios plan control-loop set <plan> heartbeat --interval 30m`. A Curator mirrors its Orchestrator's model, receives a bounded digest, and may ask one message with up to three questions. Its grants are limited to the active round and session; see `domios docs ORCHESTRATION-OPERATIONS`.
