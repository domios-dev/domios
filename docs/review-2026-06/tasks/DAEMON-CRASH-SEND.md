# DAEMON-CRASH-SEND — P1: daemon dies under matrix spawn+send load

**Executor:** Gpt5.5 modelprobe (77e5f8be) · **Branch:** stabilization · **Effort:** L
**Backlog ref:** found by RESTORE-MATRIX-V2 (blocked on this). MERGE GATE.

## Evidence (from the matrix lane — verify, don't re-derive)
- Crash A (run f27c62f2cf): OpenCode slots 1-2 materialized, slot3 `POST /api/panes/a99398e0/send` → HTTP 000; dev exited `0xc0000409 STATUS_STACK_BUFFER_OVERRUN` around restore/OpenCode fresh launch. No Rust panic in the log.
- Crash B (run 8f3ced76a6, after harness patch): Codex slot1 ok, slot2 `POST /api/panes/ea03b893/send` → HTTP 000, dev exited code 1.
- Blank `--model` alone does NOT reproduce (deliberate repro spawned fine, dev stayed up).
- KEY DIAGNOSTIC GAP: the eval lib starts dev with `> /dev/null 2>&1` — **stderr is discarded**. On Windows, Rust prints `thread '<name>' has overflowed its stack` to STDERR before the 0xc0000409 abort. The smoking gun was thrown away.

## Goal
Root cause named (thread + stack), fixed, regression-tested; the restore-matrix loop survives 3 consecutive full runs with zero daemon deaths.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK DAEMON-CRASH-SEND ACCEPTED, starting" --pane 5c64a651
```

## Environment
dev:7686 is YOURS for this task (matrix lane is blocked/paused on you). Never 7685.

## Steps (in order)
1. CAPTURE FIRST, theorize later: start dev with stderr persisted —
   `& "C:\Program Files\Git\bin\bash.exe" -c 'cd /c/Developer/codeterm && bash build.sh dev > /c/tmp/dev-stdout.log 2> /c/tmp/dev-stderr.log &'` (or the closest working form; verify health). Set `RUST_BACKTRACE=full` in the daemon env if build.sh allows pass-through.
2. Reproduce: the trigger profile is "fresh agent pane spawn + immediate verified send, several in sequence" (codex/opencode). Drive it in a loop via dc (source evals/_lib/lib.sh) — e.g. repeated `bash evals/restore-matrix/run.sh` or a tighter minimal loop: spawn agent → POST send marker → close → repeat across providers. Iterate until a death; then read /c/tmp/dev-stderr.log — expect the overflowed-thread name or an abort message.
3. Root-cause from the named thread: likely candidates to check against the evidence (verify, don't assume): PTY reader/writer threads, the verified-send wait loops (inject_verify), detect_from_output on agent boot spam, opencode sqlite reads on fresh launch, snapshot writes racing restore. Note crash A happened "around restore/OpenCode fresh launch" and crash B during a plain codex send — find the COMMON path.
4. Fix per the lock-ordering rules; if it is a stack overflow, fix the recursion/allocation, do NOT just raise thread stack sizes without understanding (raising size is acceptable as part of the fix only with the root cause named and a comment-free structural justification).
5. Regression: unit/integration test for the specific overflow/abort path where feasible; PLUS the operational proof — 3 consecutive `bash evals/restore-matrix/run.sh` runs (or your minimal loop x50) with dev never dying. Paste dev uptime evidence.
6. Fix the diagnostic gap so this never blinds us again: evals/_lib/lib.sh `dev_start_bg` must persist daemon stderr to a log file (e.g. ~/.codeterm-dev/logs/codeterm-stderr.log) instead of /dev/null. Small, in scope.

## Forbidden (beyond the global rules in README.md)
- NO git mutations. Don't modify evals/restore-matrix logic beyond what step 6 touches in _lib (the matrix lane owns its eval).
- No silencing: if you cannot reproduce after serious effort, report UNREPRODUCED with the capture infrastructure in place — do not declare it fixed.

## Report
```
codeterm send-text "TASK DAEMON-CRASH-SEND STATUS ... THREAD/SIGNATURE: ... ROOT CAUSE: ... CHANGED: ... VERIFIED: <3-run survival evidence> NOTES: ..." --pane 5c64a651
```
