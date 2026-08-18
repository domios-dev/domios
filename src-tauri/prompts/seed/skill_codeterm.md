## CodeTerm CLI

Use `codeterm` to control tabs, workspaces, agents, mesh peers, mem, and system power.

- `codeterm tab list` — always use this plain compact view for interactive discovery/cleanup; `(self)` marks your tab
- `codeterm tab inspect --tab ID` — full details for one selected tab; add `--lifecycle` only for that tab's journal history
- `codeterm send "..." --tab ID` — type into a tab
- `codeterm tab stop [--tab ID]` — interrupt the agent's current generation (doesn't close the tab)
- `codeterm agent spawn claude --task "..."` — spawn a child tab you own (parent = you, automatic)
- `codeterm mesh peers` — list mesh peers
- `codeterm mem search/get/save/wake-up` — unified memory (search runs FTS + embeddings hybrid in one call)
- `codeterm mem worker spawn --intent "enrich" --task "..."` — spawn a headless graph worker for custom enrichment
- `codeterm mem sync` — trigger librarian file sync for source-tracked items
- `codeterm notify send "Done" --level info` — system / in-app notification (rare user-attention escalation or explicit user request only; `send` is required)
- `codeterm keepawake start --duration-secs N [--display]` / `stop` / `status` — prevent sleep cross-platform (do NOT call `caffeinate`, `powercfg`, `systemd-inhibit`)

Always use the CLI from inside CodeTerm via Bash. Do not curl the REST API directly.
Use `$CODETERM_TAB_ID` when a command needs the current tab's identity; do not use pane-era names.
