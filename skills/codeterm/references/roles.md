# Agent Hierarchy & Role Rules

CodeTerm has a 3-level hierarchy: **Orchestrator → Lead → Worker**. Your role is in `$CODETERM_ROLE` (empty = Orchestrator).

## Common rules (all roles)

- Your own pane ID is `$CODETERM_PANE_ID`, NOT what `codeterm pane current` returns — `current` is the *focused* pane.
- Reports from children arrive automatically as `[Report from PANE_ID]` lines in your terminal. Never sleep/poll — just continue working and react when you see them.
- Agent-to-agent communication is always in English, regardless of user language.
- Always include `codeterm agent report '...'` in the worker's `--task` so it knows how to report back.

## Orchestrator (`$CODETERM_ROLE` empty)

Top of the hierarchy. You spawn Leads or Workers, collect reports, decide what to do.

```bash
codeterm agent set-conductor --pane $CODETERM_PANE_ID        # become orchestrator
codeterm agent spawn claude --title "Team-A" --role Lead --parent $CODETERM_PANE_ID --task "..."
codeterm agent spawn claude --title "Scout" --role Worker --parent $CODETERM_PANE_ID --task "..."
```

When all expected reports arrive, either notify the user or run `agent report` to pass the result up.

## Lead (`$CODETERM_ROLE=Lead`)

You are under an Orchestrator. You decompose your sub-task and spawn Workers.

- Backend blocks `set-conductor` and `unlink` on you — don't call them.
- You CAN spawn Workers with `--parent $CODETERM_PANE_ID`.
- After spawning, continue working. Do NOT sleep or poll for reports.
- Do NOT create extra workspaces or tabs. Do NOT use files to exchange data.
- When all expected reports arrive, run `codeterm agent report "your summary"`.

## Worker (`$CODETERM_ROLE=Worker`)

You execute exactly ONE concrete task and report back.

- You CANNOT spawn anything. No `agent spawn`, no new tabs, no built-in Agent tools.
- Do NOT sleep. Execute → `codeterm agent report "your summary"` → stop.
- Use `codeterm agent report` CLI only — never the built-in reporting tools.

## Separate workspace shortcut

If the user says "in a separate workspace", "another workspace", "new workspace" — delegate with exactly these 2 commands and stop:

```bash
WS=$(codeterm workspace create "Name" --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Orchestrator" --role Orchestrator --workspace $WS --can-spawn \
  --task "You are the orchestrator. [full task]. Set yourself as conductor, spawn leads/workers, collect reports, notify when done."
```

Do NOT call `set-conductor` on yourself. Do NOT spawn Leads. Do NOT rename your pane. The spawned Orchestrator handles everything.
