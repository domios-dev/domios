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
- ROUTING: Follow the host-supplied return hint inside `<tip>…</tip>` (see `codeterm docs ROUTING`): agent senders use its command; shell senders get a reply in your own chat/output.
- WORKSPACE INHERITANCE IS AUTOMATIC. `tab new`, `agent spawn`, and `cron create` land in YOUR workspace — the one that owns your tab — never in the workspace the user happens to have selected in the UI. Pass `--workspace <id>` only to place work in a different workspace on purpose.
- UTILITY SHELL PURPOSE: `tab new --purpose build|dev|qa|deploy|logs|db|shell` labels the shell in the sidebar (default `shell`). Use build for builds, dev for servers, qa for checks, deploy for releases, logs for tails, db for database consoles. Purpose survives `tab restart` and desk restore. Running/idle and last exit come only from shell command marks; no evidence stays unknown.
- LINUX TOOLING: when a task needs Linux tooling or reproduces a Linux-only failure, open a utility shell with `tab new --shell wsl:<distro>` (`tab new --list-shells` lists options); agents themselves run natively.
- TEAM CONTAINMENT IS AUTOMATIC. From a live Team tab, ordinary `tab new` shells and `cron create` jobs inherit that Team. Use `--no-team` only when the work must deliberately outlive Team cleanup; `--team <id>` remains the explicit cross-check for a named Team.
- `chat_backend` TABS HAVE NO SHELL. Check `backing.kind` in `tab list` before instructing a tab to run commands; send chat text instead.
- DELAYED SEND IS NOT THE DEFAULT. `codeterm send` is immediate; `--delay`/`--at` are for user-requested future delivery. `send --every` repeats into an EXISTING tab (monitor/watcher); cron spawns a FRESH agent per scheduled run. With `--mesh`, recurring delays require the target to advertise `delayed_recurrence`; an incapable peer is refused, so update it or send a one-shot explicitly.
- SEND RESULT: Immediate `send` exits 1 with a reason on stderr when `ok:false` or `bytes_written:false`, including retained pre-ready input. JSON failures retain the daemon receipt in `data`. A retained `send_id` or queued delivery is pending, not delivered: inspect its status or exact own-session transcript and never replay held, written or uncertain input.
- NEVER HARDCODE MODEL IDS. Discover with `codeterm agent providers` / `codeterm agent models <provider>`, which refreshes live-discovery catalogues before listing.
- MODEL SELECTION: `--model` is validated through each provider's catalogue capability. Authoritative catalogues refuse unknown ids before creating a tab and show close ids; free-form providers keep provider-owned ids. If a required live catalogue is unavailable, the requested id is preserved and the response carries a warning.
- LIVE MODEL SWITCH: `codeterm agent model --tab TAB MODEL` relaunches that idle tab on its own verified session; `model_confirmation: "pending"` means no model was reported before the deadline, and the label follows the first turn.
- SESSION FORK: `codeterm agent fork --tab TAB` opens a separate tab when its provider declares forking. Its session stays pending until that new launch supplies its own evidence. With `background_work_guard` enabled, model switches refuse running background work; an explicit `--force` override reports the affected items.
- Version/quota questions: `codeterm identify` (not `--version`, not curl) and `codeterm usage latest` (never `ccusage` or parsing `~/.claude`).
- System sleep: `codeterm keepawake ...` — never `caffeinate`/`powercfg`/`systemd-inhibit`.
- AGENT-TO-AGENT TRAFFIC IS ENGLISH (task bodies, reports, cron tasks, mesh messages); only user-facing channels carry the user's language.
- SECRETS NEVER GO TO MEM — `codeterm mem secret` / `codeterm docs SECRETS`.
- Message and pad file options accept `-` for UTF-8 stdin: `send --from-file -`, `--from-pad -`, `plan task remote-report --message-file -`.

## Capability map — where to look when the task mentions…

Each command's `--help` has the flags; the Docs column has the depth.

| Task mentions… | Use | Docs |
|---|---|---|
| Version, abilities, documents | `identify`, `capabilities`, `docs` | — |
| Tabs: list, read, search, send | `tab list`, then `tab inspect` / `tab get-text --tab <ID>`; `tab search <PATTERN>` spans every workspace; `send`; chat backends: `tab chat`; a numbered prompt the provider classifier confirmed: `tab respond OPTION --tab <ID>` | `docs ROUTING` |
| A tab with a wrong or stale agent binding | `tab rebind`, `tab repair --from-evidence`, `tab adopt-agent` (read `--help` first). Never type task or ACK text into an unverified shell or picker | `docs ROUTING` |
| Servers, watchers | `tab new --purpose dev --cmd "..."`; replace it with `tab restart --tab ID --cmd "..."` | — |
| Agents: spawn, restart, switch provider, report | `agent spawn`, `agent restart`, `agent report`, `agent providers` / `agent models` | `docs ORCHESTRATION` |
| Becoming an Orchestrator | Tab menu Make Orchestrator, or `--role Orchestrator` | `docs ORCHESTRATION` |
| Plans, specs, tasks | `plan --help`, `spec --help`, `task --help`; `plan lanes <plan> --clean` previews cleanup, `--confirm` executes, `--fetch` refreshes refs | `docs PLAN-EDITING` |
| Delayed or repeated message to a tab | `send --delay` / `--at` / `--every`, `delayed --help` | — |
| Scheduled jobs (fresh agent per run) | `cron create --help`; a short meaningful name is required | `docs CONFIG` |
| Watch CI, builds, files, agents and act | `blueprint recipe`, then `--dry-run` | `docs BLUEPRINTS` |
| Telegram | `bridge send --to ID --service telegram "..."`; long answers continue with `--append` | `docs TELEGRAM-SETUP` |
| Remote machines | Mesh block below | `docs MESH` |
| Memory, skills | Memory block below | `docs MEMORY` |
| Spinal Cord (Supporter Agent): explain, connect, verify, disconnect | `supporter --help` (`spinal-cord` alias) | `codeterm docs SPINAL-CORD` |
| Past chats | `sessions search` / `get` / `resume`; read-only `sessions backups export latest --sanitize --out <file>` captures a desk; `sessions backups import --file <file>` requires a disposable named upgrade-restore DEV instance | — |
| Config | `config get [key]`, `config set` | `docs CONFIG` |
| Notifying the user | `notify send "title" --from-pad PATH --level verbose\|action_needed\|failure` | `docs CONFIG` |
| Plugins, provider CLIs | `plugin list` / `install` / `update`, `agent cli status <provider>` | `docs API-CLI` |
| Plugin app tabs (surfaces) | `surface list`, `surface open <plugin>:<surface>`, `surface close --tab <ID>`; surfaces take no `send` | `docs SURFACES` |
| Model endpoints, presets | `model-provider --help`; keys go in on stdin only | `docs INTEGRATIONS` |
| Account sign-in and identity | `account login|status|logout` (local machine; hosted sign-in opens the system browser) | `codeterm docs CONFIG` |
| Updates | `update check`, `update install --auto` | — |
| Payload between machines | `paste put`, `paste get <id>` (not `mem save`) | — |
| Services registry | `service --help` | `docs MESH` |
| REST listener status/recovery while HTTP is down | `rest-api status`, `rest-api restart` through the local socket | `docs CONFIG` |
| Essence map | `essence-map --help` | `docs ESSENCE-MAP` |
| Provider launch leftovers | `integration sweep-profiles --provider <id>` is a dry run; only the owner adds `--confirm` | — |
| Keep the machine awake | `keepawake start` / `stop` / `status` | — |

Plans, tasks, Teams: `codeterm plan --help` / `codeterm task --help` / `codeterm team --help`, with the full rules in `codeterm docs PLAN-EDITING` and `codeterm docs ORCHESTRATION`. Orchestrator, Planner and Worker launches carry their own flag contract.

The owner's explicit `codeterm agent restart --tab <ID> --fresh` starts a new session in the same tab with its recorded model and effort. An existing exact restored session is refused; a live agent requires `--force`. Restore never makes this choice automatically.

## Mesh (remote peers)

Flat peer model: paired = trusted. Most tab/agent/workspace commands accept `--mesh PEER` = "run against that peer's daemon". Flow: `mesh peers` → `tab list --mesh PEER` → act on remote ids. A peer's General Agent is always `--tab main --mesh PEER`; remote agents don't auto-reply — say "report back" explicitly. Pairing: `mesh connect|approve|reject|disconnect`; for differing API ports use `codeterm mesh connect <instance-id-or-name> --addr <ip:port>` to read the identity and send an owner-approved request; disconnect revokes the peer (only the local owner lifts it with `mesh unrevoke <instance-id>`). Make this machine discoverable to a NEW device with `mesh broadcast` (requires `mesh online`; tell the user when you toggle it). Visibility: `mesh offline` / `mesh online` (hot-reload, Tailscale untouched). Mesh spawn requires peer `--cwd`; it sends locally read `--task-file` contents.

## Memory (mem)

The Domios memory protocol is in your provider rules. Commands: `codeterm mem --help`; depth: `codeterm docs MEMORY`. Past conversations are not mem: `codeterm sessions search`.

Use `codeterm mem taxonomy` for area counts and `codeterm mem area merge <from> --into <to> --dry-run` to preview a reversible area move; omit `--dry-run` to apply. Undo per item with `codeterm mem restore <id> --version <history-row>`.

## Platform notes

Paths: use forward slashes; the CLI normalizes them on every OS.

Blueprints use deterministic patterns by default; unmatched output remains `other` without a model call. Use `--model-classification` without patterns only when model judgment is wanted. Blueprint terminal adapters: `codeterm blueprint create --observe tab:<id> --output-settled 300 --session` watches same-Team utility or agent PTY output without polling; supply the required name, question, buckets and routes. `send:<tab>` delivers the stage text body. `codeterm blueprint input <id> --from-pad <path>` delivers another text body to an input stage. Follow `codeterm docs BLUEPRINT-RECIPES` for the Terminal adapter recipe.

Opt-in product analytics is local: `codeterm analytics status`, `codeterm analytics enable`, and `codeterm analytics disable`. Disabling clears queued events and identifiers.

Account and subscriptions: `codeterm account plan` shows cached entitlements and a stale marker; `codeterm account upgrade pro` opens checkout; `codeterm account portal` opens subscription management. See `codeterm docs ACCOUNT`.

Utility shell lifetimes: `tab new --timeout 10m` for tests, QA and builds; leave unset for servers. Reset or extend from now with `tab timeout --tab <ID> --timeout 20m`; disable with `--timeout none`. Only the creating agent can change the lifetime. See `codeterm docs TAB-LIFECYCLE`.

Relocate a workspace after its folder moves: `codeterm workspace set-cwd <workspace-id> <absolute-folder>`. This preserves its identity and reports any paths it could not rebase.

Repair a moved Plan project without its old folder: `codeterm plan set-project <plan> <path>`. It validates the target host folder and preserves Plan, task, Team and attempt identities.

Voice notes arrive as `<domios type="attachment" kind="voice" path="<file>" label="voice" />` plus `<transcript>…</transcript>`: the transcript is the message, the path its audio. `status="untranscribed"` carries a failure note instead. Older notes (bare text after the frame, or `-=-codeterm:voice file=<path>-=-`) are read-only legacy.
