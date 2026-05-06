## CodeTerm Terminal

You are running inside CodeTerm — an AI-first terminal multiplexer. Use the `/codeterm` skill for all terminal operations.

### When to use CodeTerm CLI

- "in terminal" / "в терминале" / "в консоли" → CodeTerm workspace/pane, NOT filesystem
- "create project X" → `codeterm workspace create "X"`
- "run tests there" → `codeterm send-text PANE_ID "command"`
- Any visual content (diagrams, dashboards) → `codeterm browser open file:///path`
- Notify on completion → `codeterm notify "Done" --level info`
- "don't let the computer sleep" / "keep awake" / "prevent sleep for N minutes/hours" → `codeterm keepawake start --duration-secs N [--display]`. Stop with `codeterm keepawake stop`, check with `codeterm keepawake status`. Do NOT call `caffeinate` / `powercfg` / `systemd-inhibit` directly — CodeTerm tracks the session cross-platform.

### Mesh — remote machines

Triggers: "on another machine" / "remote" / "mesh" / "send to X" / "run on X" / "build on X" — and equivalents in other languages.

Always use CLI, never raw curl:
```bash
codeterm mesh peers                                        # find peers (paired = can send)
codeterm pane list --mesh "home-laptop"                    # list panes on remote
codeterm send-text "git pull && ./build.sh dev" --pane ID --mesh PEER  # send command
codeterm agent spawn claude --task "..." --mesh PEER       # spawn agent remotely
codeterm get-output --pane ID --mesh PEER                  # read remote output
```

Flow: `mesh peers` → find peer name → `pane list --mesh NAME` → find pane ID → `send-text`.

### On session start

Rename your tab to the task summary using $CODETERM_PANE_ID (always available inside CodeTerm):
```bash
codeterm pane rename --pane "$CODETERM_PANE_ID" "short task name"
```

### Secrets — pluggable secret store

When the user wants to save, look up, or rotate a credential (API key, OAuth token, master password, license key, etc.) → use the secret store, NOT plaintext files / env vars / shell history.

```bash
codeterm mem secret status                                  # check backend + auth state
echo -n "$VALUE" | codeterm mem secret save --name openai-prod  # save (pipe value via stdin)
codeterm mem secret get --name openai-prod                  # raw value to stdout (no newline)
codeterm mem secret get --name openai-prod --json           # {item_id, name, value, notes}
codeterm mem secret search "openai"                         # metadata-only search
codeterm mem secret list --org ORG_ID --limit 20
codeterm mem secret delete --name openai-prod
```

Rules:
- ALWAYS pipe values via stdin (`echo -n "$V" | …`). Never `--value V` — that puts the plaintext in shell history.
- For unlock: `codeterm mem secret unlock --email u@x.com`, stdin = master password. Never pass `--password` on argv.
- Non-zero exit codes are descriptive — surface to user. Common: 3 = vault locked (run `mem secret unlock`), 7 = `--mesh` rejected (secrets don't cross mesh, vault is local).

### Project map — code search by symbol/concept

When working inside a project workspace (any non-`protected` workspace with a cwd), CodeTerm auto-indexes the codebase into a structural map: files, classes, functions, methods, plus edges (calls, contains, Tauri-IPC ↔ HTTP bridges). PageRank ranks by code centrality. Use this BEFORE grep/find for symbol/concept lookups — it's faster across hundreds of files, knows kinds, and supports semantic search.

```bash
codeterm map status                                       # what's indexed, when, counts
codeterm map search "spawn_agent"                         # hybrid: substring + semantic (default)
codeterm map search "MultiplexerState" --kind class       # filter by kind: function|method|class|file|module
codeterm map search "create a new pty terminal" --semantic-only   # concept queries
codeterm map search "workspace_create" --json             # machine-readable rows
codeterm map rank --path src-tauri/src/multiplexer/       # top-N nodes by PageRank (centrality) in subtree
codeterm map files --limit 15                             # top-N files by importance — what to read first
codeterm map bridges --kind tauri                         # one row per distinct command, sorted by invoke-site count
codeterm map bridges --kind tauri --all-pairs             # full per-invoke-site listing (TS callsite → Rust handler)
codeterm map bridges --kind http                          # all HTTP routes: METHOD path → handler:line
codeterm map edges --node 1816 --direction out            # what does this node call
codeterm map edges --name TelegramService --direction in  # who references this symbol (resolves by name)
codeterm map deps src-tauri/src/multiplexer/state.rs      # files THIS file pulls in
codeterm map dependents src-tauri/src/pty/manager.rs      # files that reference THIS file
codeterm map index                                        # daemon auto-watches; manual only if stale
```

`map search` is the swiss-army knife. The graph queries (`rank`, `bridges`, `edges`, `deps`, `dependents`, `files`) replace 5–10 `rg` calls for structural questions ("most important file in X", "all IPC handlers", "who depends on Y"). All support `--json`.

**Trigger phrases → command** (use these patterns mechanically; do NOT default to `rg` when one fits):

- "where is X / find class X / locate function Y" → `map search "X"` (single keyword, exact symbol)
- "code that does Z conceptually" / "find handler for jitter" → `map search "<phrase>" --semantic-only`
- "most important / most central / hot path / top function in X" → `map rank --path X --kind function|class|method`
- "biggest files / what to read first / most central files in X" → `map files --path X --limit N`
- "all Tauri commands" / "list IPC handlers" → `map bridges --kind tauri --limit 200 [--json | jq …]`
- "all HTTP routes" / "every endpoint" → `map bridges --kind http --limit 200`
- "top-N tauri commands by invoke sites" → `map bridges --kind tauri --limit N` (already sorted by invoker_count desc; first column is the count)
- "group Tauri commands by prefix / count distinct mux_*" → `map bridges --kind tauri --json | jq -r '.tauri[] | select(.command | startswith("mux_")) | .command' | wc -l` (one row per command, no dedup needed)
- "who calls X / where is FOO referenced / blast radius of rename Y" → `map edges --name X --direction in`
- "what does this file/symbol pull in" → `map deps PATH` (file) or `map edges --node ID --direction out` (symbol)
- "what depends on this file / who imports this module" → `map dependents PATH` (incoming file-level)

**Aggregate / count-by / group-by questions are MAP territory, not rg.** When the shape is "count distinct X grouped by Y" or "which N has the most Z", reach for `bridges --json | jq` or `rank` first.

Map is blind on these — fall back to `rg` immediately:
- **Literal tokens / escape sequences / ANSI codes** — map indexes named symbols, not raw text.
- **Anonymous closures** (e.g. `useEffect(() => {...})`) — no name → no node.
- **Code in string literals / `include_str!`, comments, READMEs** — not AST-parsed.

### Config & language

- `codeterm config get/set` for CLI reads/writes
- **Agent-to-agent: English always** (task prompts, reports, cron tasks). Reply to the user in their language.

### Do NOT

- Do NOT use `start` or `open` for browser — use `codeterm browser open`
- Do NOT create filesystem directories when user says "create project" — create a workspace
- Do NOT use raw `curl` / REST endpoints for CodeTerm API (`/api/mesh/peers` etc) — use `codeterm` CLI
- Do NOT call `caffeinate`, `powercfg /requests`, or `systemd-inhibit` directly — use `codeterm keepawake`
- Do NOT save secrets to plaintext files / `.env` / shell history — use `codeterm mem secret save`
- Do NOT pass secret values on argv (`--value`, `--password`) — pipe via stdin so they don't land in shell history or `ps`
- Do NOT default to `rg`/`grep`/`find` for symbol lookups inside a project workspace — try `codeterm map search` first

<!-- revision: 15 -->
