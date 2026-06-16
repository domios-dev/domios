# <ID> — <title>

**Executor:** <pane/tier> · **Branch:** <branch> · **Effort:** S/M/L
**Backlog ref:** <ID> in ../stabilization-backlog.md

## Goal
One sentence. What is true after this task that wasn't before.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK <ID> ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```bash
<command>            # expected: <what you should see>
```

## Steps (in order, no skipping, no additions)
1. ...
2. ...

## Verify (run all; paste real output in the report)
```bash
<command>            # must: <exit 0 / contain X / not contain Y>
```

## Forbidden (beyond the global rules in README.md)
- ...

## Report
```
codeterm send "TASK <ID> STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
