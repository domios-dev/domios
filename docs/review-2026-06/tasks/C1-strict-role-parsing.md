# C1 — Reject invalid --role values on agent spawn (CLI + daemon)

**Executor:** Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** C1 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK C1 ACCEPTED, starting" --pane 5c64a651
```

## Goal
`codeterm agent spawn <p> --role <anything but Orchestrator|Worker>` exits non-zero
with a message naming the two valid roles — on both the CLI parse side and the
daemon endpoint (whichever currently swallows the bad value into `role=None`).

## Evidence (run FIRST)
Live prod behavior (already proven 2026-06-11, re-confirm in CODE, not against prod):
`spawn --role Lead` → exit 0, pane created with `role=None`.
```bash
grep -n "role" cli/src/args/agent.rs | head           # find how role is declared (likely Option<String>)
grep -rn "fn.*spawn" src-tauri/src/api/ src-tauri/src/service/spawn.rs 2>/dev/null | head
grep -rn "\"Orchestrator\"\|\"Worker\"" src-tauri/src/ cli/src/ --include="*.rs" | grep -i "role\|parse" | head
```

## Steps
1. Trace the role string end-to-end: CLI arg → request body → daemon handler →
   stored pane role. Find the exact point where an unknown string becomes `None`.
2. CLI side: change the `--role` arg to a clap `ValueEnum` (Orchestrator, Worker) —
   bad values then fail at parse time with clap's standard message.
3. Daemon side: the endpoint must ALSO reject unknown role strings with 400 + body
   naming valid roles (defense in depth — other clients than the CLI exist: mesh,
   REST callers).
4. Add/extend a unit test on the daemon parse path: "Lead" → error, "Worker" → ok.
5. Verify live against DEV only: `./build.sh dev --wait`, then
   `CODETERM_API_PORT=7686 codeterm agent spawn claude --role Lead --title T` →
   must exit non-zero. Then `./build.sh kill`. NEVER run this against 7685.
6. Commit: `fix(agent): reject unknown --role values on spawn (CLI ValueEnum + daemon 400)`

## Verify
```bash
./build.sh check                                      # exit 0
# PowerShell foreground, 90s:
cargo test spawn                                      # new test passes
# paste the live dev rejection output from step 5
```

## Forbidden
- Anything on port 7685. Dev only (7686), and `./build.sh kill` when done.
- Do not resurrect a Lead role. Two roles exist. Period.

## Report
```
codeterm send-text "TASK C1 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <where the swallow was>" --pane 5c64a651
```
