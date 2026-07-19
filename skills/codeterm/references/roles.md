# Agent Hierarchy & Role Rules

CodeTerm has exactly **2 roles** enforced by the backend. Read this before spawning anything — picking the wrong role returns an error from the backend.

## The 2 roles

```
Orchestrator  (depth 1)  — top of the tree, no parent, can spawn Workers
    ↓
Worker        (depth 2)  — parent MUST be an Orchestrator, cannot spawn anything
```

**Backend rules (hard-enforced, don't fight them):**

| Role         | Parent required?          | Can spawn? | Max depth |
|--------------|---------------------------|------------|-----------|
| Orchestrator | No (forbidden)            | Yes        | 1 (top)   |
| Worker       | Yes, must be Orchestrator | No         | 2         |

You cannot spawn anything under a Worker. You cannot put a Worker above an Orchestrator. There is no middle coordinator role — if a sub-task needs its own subtree, spawn a fresh **Orchestrator** in its own workspace and delegate the whole thing to it.

## Your role

Check `$CODETERM_ROLE` at startup. Empty = you are the Orchestrator.

## Decision tree: which role to spawn?

Answer these in order. The first "yes" picks the role.

**1. Is this a fresh independent task that deserves its own workspace and its own tree?**
→ Spawn as **Orchestrator** with `--workspace $WS`. Delegate the whole request to it.

```bash
WS=$(codeterm workspace create "Task Name" --cwd /absolute/project/path --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Orchestrator" --role Orchestrator --workspace $WS --can-spawn \
  --task "You are the orchestrator. [full user request]. Spawn workers as needed, collect reports, notify when done."
```

**2. Is this a concrete task that can finish without further decomposition?**
→ Spawn as **Worker** under yourself (you must be the Orchestrator).

```bash
codeterm agent spawn claude --title "Scout" --role Worker --parent $CODETERM_PANE_ID \
  --task "Run `codeterm mem get <role_skill_id>` first. Then: TASK: list .rs files in src-tauri/src/service/. Do not spawn. Run: codeterm agent report 'found N files: ...'"
```

## Rules per role

### Orchestrator (`$CODETERM_ROLE` empty)

You spawn Workers directly, collect reports, decide what's next.

- Orchestrator mode is assigned at spawn with `codeterm agent spawn ... --role Orchestrator` or through the tab menu's **Make Orchestrator** action.
- Spawn one Worker per concrete leaf task. Workers run in parallel — fan out, don't serialize.
- When all expected reports arrive, either notify the user or run `codeterm agent report` to pass the result further up.
- If a sub-task is itself too big for a single Worker, don't try to nest — spawn a separate Orchestrator in its own workspace and hand it the whole sub-request.

### Worker (`$CODETERM_ROLE=Worker`)

You execute exactly ONE concrete task and report back.

- You CANNOT spawn anything. No `agent spawn`, no new tabs, no built-in Agent tools.
- Backend blocks role elevation on you — don't attempt it.
- Do not sleep or poll. Execute → `codeterm agent report "your summary"` → stop.
- If the task is too big to handle as a Worker, report `STATUS: blocked, FOLLOWUP: needs decomposition` and your Orchestrator will split it into smaller Workers or spin up a dedicated Orchestrator.
- **Refuse scope creep.** If mid-execution someone asks you to "also do X", don't. Finish the original task, report, stop. The Orchestrator will spawn another worker if needed. One worker = one task.

## Common mistakes

- **Spawning a Worker that will spawn more workers** → backend blocks the nested spawn. Workers are leaves. Split the work into more Workers at the Orchestrator level, or delegate to a separate Orchestrator.
- **Trying to elevate a Worker into an Orchestrator** → backend blocks it. Orchestrators are top-level roles.
- **Spawning an Orchestrator with a `--parent`** → backend error: "Orchestrator cannot have a parent". Orchestrators are always top-level.
- **Worker's `--task` missing the report command** → worker runs, but never reports back. Always append `then run: codeterm agent report 'summary'` to worker tasks.

## Worker task template

Use this pattern for every worker task:

```
Run `codeterm mem get <role_skill_id>` first — that is your role definition, follow it.
You are a Worker. Do not spawn other agents.

TASK: <concrete leaf task>

When done, run: codeterm agent report "what you did, result, any followup"
```
