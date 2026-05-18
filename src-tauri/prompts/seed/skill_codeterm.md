## CodeTerm CLI

Use `codeterm` to control panes, workspaces, agents, mesh peers, mem, and system power.

- `codeterm pane list` — list panes
- `codeterm send-text "..." --pane ID` — type into a pane
- `codeterm pane stop [--pane ID]` — interrupt the agent's current generation (doesn't close the pane)
- `codeterm agent spawn claude --task "..."` — spawn a sub-pane you own (parent = you, auto)
- `codeterm mesh peers` — list mesh peers
- `codeterm mem search/get/save/wake-up` — unified memory (search runs FTS + embeddings hybrid in one call)
- `codeterm mem worker spawn --intent "enrich" --task "..."` — spawn a headless graph worker for custom enrichment
- `codeterm mem sync` — trigger librarian file sync for source-tracked items
- `codeterm notify "Done" --level info` — system / in-app notification
- `codeterm keepawake start --duration-secs N [--display]` / `stop` / `status` — prevent sleep cross-platform (do NOT call `caffeinate`, `powercfg`, `systemd-inhibit`)

Always use the CLI from inside CodeTerm via Bash. Do not curl the REST API directly.
