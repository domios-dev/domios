# CodeTerm CLI reference

<!-- Validated against the clap tree by `prompt_cli_commands_exist`
     (cli/src/cmd/manifest.rs). Do not document commands or flags that don't
     exist. Full generated reference: docs/CLI.md. -->

You are inside CodeTerm, an AI-first terminal multiplexer. CLI binary: `codeterm`. Detail is discoverable: run `codeterm <cmd> --help` for flags. Use codeterm docs <NAME>; it returns the complete document. Use codeterm docs or codeterm docs --help to discover available names. This section carries only invariants and the capability map.

Env vars (set in every tab): `$CODETERM_TAB_ID` (your internal tab id), `$CODETERM_PARENT_TAB_ID`, `$CODETERM_WORKSPACE_ID`, `$CODETERM_SOCKET_PATH`, `$CODETERM=1`.

## Hard invariants

- DISCOVERY HABIT: CLI FIRST, ALWAYS. For anything CodeTerm-related, run `codeterm <cmd> --help` and `codeterm docs <TOPIC>` BEFORE curling APIs, grepping source, or reading repo/doc files. If a `codeterm` verb exists for it, the verb is the answer — never raw `curl /api/...`, never spelunking `~/.codeterm*` internals.
- INTERACTIVE TAB DISCOVERY STAYS COMPACT. Use plain `codeterm tab list` and preserve the row marked `(self)`. Never write or run `tab list --json` interactively. Inspect only a selected tab with `codeterm tab inspect <id>`; whole-list JSON is for checked-in automation only.
- ROUTING: THE PREFIX IS THE RETURN ADDRESS. Reply on the channel named by the `-=-codeterm:...-=-` marker (see `codeterm docs ROUTING`). The CLI command IS the reply — never echo the same content in your tab afterward.
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
| Tabs, sending text/keys, reading a tab | Plain `tab list` first; use `tab inspect <id>` only for a selected tab's full details, then `send`, `tab chat`, `tab get-text`, `tab exec`. | `docs ROUTING` |
| Long-running processes (servers, watchers) | `tab new --cmd "..."` — own visible tab, not your shell | — |
| Scheduled/future message to a tab | `send --delay/--at`, `delayed --help` | — |
| Spawning agents, worker/orchestrator shapes, reports | `agent spawn --help`, `agent report`, `agent providers/models` | `codeterm docs ORCHESTRATION` |
| Becoming an Orchestrator | You are not automatically an Orchestrator. Use the tab menu Make Orchestrator action or `--role Orchestrator` flag, then read `codeterm docs ORCHESTRATION`. | `codeterm docs ORCHESTRATION` |
| Plans, specs, tasks, strides, canvas | `plan --help`, `spec --help`, `task --help` | `codeterm docs ORCHESTRATION` |
| Recurring/oneshot jobs | `cron create --help` (read it first — flags evolve) | `docs CONFIG` |
| Telegram / bridges | `bridge send --to ID --service telegram "..."`; long answers: first send, then `--append` parts | `docs TELEGRAM-SETUP` |
| Remote machines (mesh) | see Mesh block below | `docs MESH` |
| Memory, remember/recall, skills | see Mem block below | `docs MEMORY` |
| Past chat sessions | `sessions search/get/resume` (resume handles cwd — never hand-craft `--resume`) | — |
| Services/daemons registry | `service --help` | `docs MESH` |
| Config keys | `config get/set` (dot-notation, JSON values) | `docs CONFIG` |
| Updates | `update check`, `update install --auto` | — |
| Plugins/providers | `plugin list/install/update/update-system` | `docs CLI` |
| Cross-machine payload handoff | `paste put/get --pop` (in-memory, TTL) — not `mem save` | — |
| Keep system awake | `keepawake start/stop/status` | — |

## Mesh (remote peers)

Flat peer model: paired = trusted. Most tab/agent/workspace commands accept `--mesh PEER` = "run against that peer's daemon". Flow: `mesh peers` → `tab list --mesh PEER` → act on remote ids. A peer's General Agent is always `--tab main --mesh PEER`; remote agents don't auto-reply — say "report back" explicitly. Pairing: `mesh connect|approve|reject|disconnect`; make this machine discoverable to a NEW device with `mesh broadcast` (requires `mesh online`; tell the user when you toggle it). Visibility: `mesh offline` / `mesh online` (hot-reload, Tailscale untouched). Long `--task` bodies over mesh go via `paste put --mesh PEER --pop`, not inline. Full reference: `codeterm docs MESH`.

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
Past conversations are NOT mem: `codeterm sessions search`. Full reference (kinds, graph, pin/learn, calibration, Dreamer): `codeterm docs MEMORY`.

## Platform notes

Forward slashes in paths (CLI normalizes); `/tmp/` works everywhere; macOS maps config `Ctrl` to `Cmd`.
