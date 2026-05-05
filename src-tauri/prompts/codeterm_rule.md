## CodeTerm Terminal

You are running inside CodeTerm — an AI-first terminal multiplexer. Use the `/codeterm` skill for all terminal operations.

### When to use CodeTerm CLI

- "in terminal" / "в терминале" / "в консоли" → CodeTerm workspace/pane, NOT filesystem
- "create project X" → `codeterm workspace create "X"`
- "run tests there" → `codeterm send-text PANE_ID "command"`
- Any visual content (diagrams, dashboards) → `codeterm browser open file:///path`
- Notify on completion → `codeterm notify "Done" --level info`
- "don't let the computer sleep" / "keep awake" / "prevent sleep for N minutes/hours" → `codeterm keepawake start --duration-secs N [--display]`. Stop with `codeterm keepawake stop`, check with `codeterm keepawake status`. Do NOT call `caffeinate` / `powercfg` / `systemd-inhibit` directly — CodeTerm tracks the session cross-platform.

### Output truncation — `codeterm snip` for noisy commands

Wrap any build / test / log command with `codeterm snip <cmd>` to drop known noise (passing tests, compile spam, hunk middles) while keeping failures, errors, and diagnostic boundaries. Cold start ~17 ms, ~97% byte reduction on a green cargo run, ~80% on `git log -200`. Use it WHENEVER you would otherwise spend a Bash turn reading more than a screenful of output.

```bash
codeterm snip cargo test                     # cargo test/build/check/clippy/doc/bench
codeterm snip go test ./...                  # go test/vet/build
codeterm snip git log -200                   # deterministic 35-line output
codeterm snip git diff
codeterm snip pytest -x
codeterm snip pnpm test                      # also npm/yarn/jest/vitest
codeterm snip RUST_LOG=debug cargo test      # leading KEY=val applies to the child
```

Rules:
- Default to `codeterm snip` for cargo, go, git log/diff, npm/pnpm/yarn, jest/vitest, pytest. Unknown tools pass through unchanged — wrapping never hurts, so when in doubt, wrap.
- Pipe operators: `codeterm snip <cmd> | head` works (snip wraps only the head). Top-level chains (`&&`, `||`, `;`) wrap each segment independently.
- Refuses shell builtins (`cd`, `source`, `export`, `eval`, `exec`, …) with exit 2 — they don't have output to truncate.
- Local-only — `--mesh PEER snip` exits 7. Run snip on the target peer.
- Don't pipe to `tail -N` as a substitute — snip preserves the full failure block plus diagnostic context, while `tail` blindly drops the start (where `running N tests` and the early failures often live).

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
- For unlock: `codeterm mem secret unlock --email u@x.com` and let stdin take the master password. Never pass `--password` on argv.
- If you see exit code 3 → vault is locked, prompt user to unlock (or run `codeterm mem secret unlock`). Exit 4 → not signed in. Exit 5 → not found. Exit 6 → backend unavailable (e.g. `bw` not installed). Exit 7 → tried `--mesh`, which is rejected for secret commands.
- Secrets DO NOT cross the mesh. `--mesh PEER mem secret …` always exits 7. Each machine's vault is local.
- Default backend is Bitwarden (uses the `bw` CLI). The unlocked session token is persisted in the OS keychain, never on disk in plaintext.

### Integrations & config

- Set up Telegram, config options → read the `/codeterm` skill for details
- `codeterm config get/set` for reading/writing config from CLI

### Language

**Agent-to-agent communication is always in English** — task prompts, `--task` arguments, reports, mesh messages between agents, cron tasks. Only reply to the **user** in their language.

### Do NOT

- Do NOT use `start` or `open` for browser — use `codeterm browser open`
- Do NOT create filesystem directories when user says "create project" — create a workspace
- Do NOT use raw `curl` for CodeTerm API — always use `codeterm` CLI commands
- Do NOT use `/api/mesh/peers` or other REST endpoints directly — use `codeterm mesh peers`
- Do NOT call `caffeinate`, `powercfg /requests`, or `systemd-inhibit` directly — use `codeterm keepawake`
- Do NOT save secrets to plaintext files / `.env` / shell history — use `codeterm mem secret save`
- Do NOT pass secret values on argv (`--value`, `--password`) — pipe via stdin so they don't land in shell history or `ps`
- Do NOT pipe to `tail -N` / `head -N` to manage long build/test output — use `codeterm snip <cmd>` instead, it's pattern-aware and keeps the failure block intact

<!-- revision: 10 -->
