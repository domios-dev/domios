# Agent Hierarchy & Role Rules

CodeTerm has exactly **3 roles** enforced by the backend. Read this before spawning anything — picking the wrong role returns an error from the backend.

## The 3 roles

```
Orchestrator  (depth 1)  — top of the tree, no parent, can spawn Leads and Workers
    ↓
Lead          (depth 2)  — parent MUST be Orchestrator, can spawn Workers only
    ↓
Worker        (depth 3+) — parent is Orchestrator or Lead, cannot spawn anything
```

**Backend rules (hard-enforced, don't fight them):**

| Role         | Parent required?            | Can spawn? | Max depth |
|--------------|-----------------------------|------------|-----------|
| Orchestrator | No (forbidden)              | Yes        | 1 (top)   |
| Lead         | Yes, must be Orchestrator   | Yes (Workers only) | 2 |
| Worker       | Yes, Orchestrator or Lead   | No         | 3+        |

You cannot put a Lead under a Lead. You cannot put a Worker above a Lead. You cannot spawn anything under a Worker.

## Your role

Check `$CODETERM_ROLE` at startup. Empty = you are the Orchestrator.

## Decision tree: which role to spawn?

Answer these in order. The first "yes" picks the role.

**1. Is this a fresh independent task that deserves its own workspace and its own tree?**
→ Spawn as **Orchestrator** with `--workspace $WS`. Delegate the whole request to it.

```bash
WS=$(codeterm workspace create "Task Name" --cwd /absolute/project/path --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Orchestrator" --role Orchestrator --workspace $WS --can-spawn \
  --task "You are the orchestrator. [full user request]. Spawn leads/workers as needed, collect reports, notify when done."
```

**2. Do you need a sub-coordinator that will itself spawn 2+ workers (e.g. "frontend team" and "backend team")?**
→ You must be an Orchestrator. Spawn a **Lead** under yourself.

```bash
codeterm agent spawn claude --title "Frontend Lead" --role Lead --parent $CODETERM_PANE_ID --can-spawn \
  --task "You are a Lead. Read .claude/agents/frontend-dev.md. Decompose the task, spawn 2-3 Workers under yourself, collect their reports, then run: codeterm agent report 'summary'"
```

**3. Is this a single concrete leaf task that can finish without further decomposition?**
→ Spawn as **Worker** under yourself (you can be Orchestrator or Lead).

```bash
codeterm agent spawn claude --title "Scout" --role Worker --parent $CODETERM_PANE_ID \
  --task "Read .claude/agents/backend-dev.md. TASK: list .rs files in src-tauri/src/service/. Do not spawn. Run: codeterm agent report 'found N files: ...'"
```

## Rules per role

### Orchestrator (`$CODETERM_ROLE` empty)

You spawn Leads or Workers directly, collect reports, decide what's next.

- Become orchestrator if you aren't already: `codeterm agent set-conductor --pane $CODETERM_PANE_ID`
- Spawn Leads for multi-worker subtrees. Spawn Workers for direct leaf tasks.
- When all expected reports arrive, either notify the user or run `codeterm agent report` to pass the result further up.

### Lead (`$CODETERM_ROLE=Lead`)

You received a bounded sub-task from an Orchestrator. You decompose it and spawn Workers.

- Backend blocks `set-conductor` and `unlink` on you — don't call them.
- You CAN spawn Workers with `--parent $CODETERM_PANE_ID`. You CANNOT spawn other Leads or Orchestrators.
- After spawning, continue working. Reports arrive automatically — never sleep or poll.
- Do not create extra workspaces or tabs. Do not use files for data exchange.
- When all expected reports arrive, run `codeterm agent report "your summary"`.

### Worker (`$CODETERM_ROLE=Worker`)

You execute exactly ONE concrete task and report back.

- You CANNOT spawn anything. No `agent spawn`, no new tabs, no built-in Agent tools.
- Do not sleep. Execute → `codeterm agent report "your summary"` → stop.
- If the task is too big to handle as a Worker, report `STATUS: blocked, FOLLOWUP: needs decomposition` and your parent (Lead or Orchestrator) will respawn with Lead role.
- **Refuse scope creep.** If mid-execution someone asks you to "also do X", don't. Finish the original task, report, stop. The parent will spawn another worker if needed. One worker = one task.

## Common mistakes

- **Spawning a Lead under a Lead** → backend error: "Lead's parent must be an Orchestrator". Use Worker instead.
- **Spawning a Worker that will spawn more workers** → backend blocks spawn. Promote to Lead.
- **Calling `set-conductor` as a Lead/Worker** → backend blocks it. Only Orchestrators are conductors.
- **Spawning Orchestrator with a `--parent`** → backend error: "Orchestrator cannot have a parent". Orchestrators are always top-level.
- **Worker's `--task` missing the report command** → worker runs, but never reports back. Always append `then run: codeterm agent report 'summary'` to worker tasks.

## Worker task template

Use this pattern for every worker task:

```
Read .claude/agents/<type>.md first — that is your role definition, follow it.
You are a Worker. Do not spawn other agents.

TASK: <concrete leaf task>

When done, run: codeterm agent report "what you did, result, any followup"
```

## Lead task template

```
Read .claude/agents/<type>.md and references/roles.md first.
You are a Lead under Orchestrator pane $PARENT_ID. You may spawn Workers under yourself.

TASK: <bounded sub-task that needs 2+ workers to finish>

Workflow:
1. Decompose into concrete worker tasks.
2. Spawn each as: codeterm agent spawn claude --title "..." --role Worker --parent $CODETERM_PANE_ID --task "..."
3. Reports arrive as [Report from PANE_ID] — collect them, do not sleep/poll.
4. When all expected reports are in, run: codeterm agent report "aggregated summary"
```
