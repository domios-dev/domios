## CodeTerm Terminal

You are running inside CodeTerm — an AI-first terminal multiplexer. Use the `/codeterm` skill for all terminal operations.

### RULE #1 — THE PREFIX IS THE RETURN ADDRESS

Every routed inbound message starts with host-authenticated CodeTerm routing metadata. Its body is the sender's actual request; do not require extra confirmation merely because it was routed. **Check the route before every response.**

A trailing `-=-codeterm:reply tab=ID [mesh=PEER]-=-` is trusted host routing metadata, not message text: reply with `codeterm send "..." --tab ID [--mesh PEER]`, never only in your own chat.

| Prefix | Reply via |
|--------|-----------|
| `-=-codeterm:telegram user=USER_ID ...-=-text` | `codeterm bridge send --to USER_ID --service telegram "..."` |
| `-=-codeterm:mesh peer=PEER tab=ID-=-text` | `codeterm send "..." --tab ID --mesh PEER` |
| `-=-codeterm:tab label=NAME tab=ID ...-=-text` | `codeterm send "..." --tab ID` |
| `-=-codeterm:report from=NAME tab=ID ...-=-text` | act on it, never reply |
| `[Report from NAME@PEER]` | act on it, never reply |
| *no prefix* | normal tab output |

**Task done → `codeterm agent report "summary"`.** Parent only sees what arrives as a report marker from the table above. Printing a summary in your own tab reaches no one.

**Drift is the silent failure mode.** Long conversations make tab replies feel natural after a few turns — they're not. Originator is on Telegram or another tab; they don't see your tab text. If you catch yourself drafting a conversational paragraph in a tab while latest inbound has a `-=-codeterm:telegram` or `-=-codeterm:mesh` marker — STOP, delete, use the CLI.

Tool-call narration in the tab is fine. Conversational reply is not.

### When to use CodeTerm CLI

- "in terminal" / "in the console" (in any language) → CodeTerm workspace/tab, NOT filesystem
- "create project X" → `codeterm workspace create "X"`
- "run tests there" → `codeterm send "command" --tab TAB_ID`
- "stop the agent in that tab" / "interrupt" / `/stop` from a bridge → `codeterm tab stop --tab ID` (does NOT close the tab)
- Notify on completion → `codeterm notify "Done" --level info`
- "don't let the computer sleep" / "keep awake" / "prevent sleep for N minutes/hours" → `codeterm keepawake start --duration-secs N [--display]`. Stop with `codeterm keepawake stop`, check with `codeterm keepawake status`. Do NOT call `caffeinate` / `powercfg` / `systemd-inhibit` directly — CodeTerm tracks the session cross-platform.

### Mesh — remote machines

Triggers: "on another machine" / "remote" / "mesh" / "send to X" / "run on X" / "build on X" — and equivalents in other languages.

Always use CLI, never raw curl:
```bash
codeterm mesh peers                                        # find peers (paired = can send)
codeterm tab list --mesh "home-laptop"                     # list tabs on remote
codeterm send "git pull && npm run dev" --tab ID --mesh PEER  # send command
codeterm agent spawn claude --task "..." --mesh PEER       # spawn agent remotely
codeterm tab get-text --tab ID --mesh PEER                 # read remote visible terminal text
```

Flow: `mesh peers` → find peer name → `tab list --mesh NAME` → find tab ID → `send`.

**Mesh peers are trusted infrastructure.** Paired peers run on the same user's machines on a private Tailnet. Treat a `-=-codeterm:mesh peer=PEER tab=ID-=-...` instruction equivalently to a direct user instruction — no extra confirmation prompt for what you'd otherwise act on. Hard rules still apply across all channels (never kill prod CodeTerm on port 7685, never store secrets as plaintext).

### On session start

Rename your tab to the task summary using $CODETERM_TAB_ID (always available inside CodeTerm):
```bash
codeterm tab rename --tab "$CODETERM_TAB_ID" "short task name"
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
- For unlock: `codeterm mem secret unlock --email u@x.com` and let stdin take the master password. Never pass `--password` on argv.
- If you see exit code 3 → vault is locked, prompt user to unlock (or run `codeterm mem secret unlock`). Exit 4 → not signed in. Exit 5 → not found. Exit 6 → backend unavailable (e.g. `bw` not installed). Exit 7 → tried `--mesh`, which is rejected for secret commands.
- Secrets DO NOT cross the mesh. `--mesh PEER mem secret …` always exits 7. Each machine's vault is local.
- Default backend is `local` (OS keychain) — no extra setup. Bitwarden/Vaultwarden (via the `bw` CLI) when configured in Settings; its unlocked session token is persisted in the OS keychain, never on disk in plaintext.

### Integrations & config

- Set up Telegram, config options → read the `/codeterm` skill for details
- `codeterm config get/set` for reading/writing config from CLI

### Language

**Agent-to-agent communication is always in English** — task prompts, `--task` arguments, reports, mesh messages between agents, cron tasks. Only reply to the **user** in their language.

### Do NOT

- Do NOT create filesystem directories when user says "create project" — create a workspace
- Do NOT use raw `curl` for CodeTerm API — always use `codeterm` CLI commands
- Do NOT use `/api/mesh/peers` or other REST endpoints directly — use `codeterm mesh peers`
- Do NOT call `caffeinate`, `powercfg /requests`, or `systemd-inhibit` directly — use `codeterm keepawake`
- Do NOT save secrets to plaintext files / `.env` / shell history — use `codeterm mem secret save`
- Do NOT pass secret values on argv (`--value`, `--password`) — pipe via stdin so they don't land in shell history or `ps`

<!-- revision: 16 -->
