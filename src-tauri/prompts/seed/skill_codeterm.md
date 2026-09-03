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
- `codeterm notify send "Done" --level action_needed` — user-attention escalation (rare; `send` is required). `--level` is the closed set verbose/action_needed/failure; naming no level files in the bell without alerting. An escalation may ask rather than announce: repeat `--choice "Label"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it. A short completion or progress note may be filed at `verbose`; keep it a periodic status, never a running commentary of one notification per step. A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried.
- `codeterm keepawake start --duration-secs N [--display]` / `stop` / `status` — prevent sleep cross-platform (do NOT call `caffeinate`, `powercfg`, `systemd-inhibit`)

Always use the CLI from inside CodeTerm via Bash. Do not curl the REST API directly.
Use `$CODETERM_TAB_ID` when a command needs the current tab's identity; do not use pane-era names.
