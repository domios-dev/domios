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

<!-- revision: 8 -->
