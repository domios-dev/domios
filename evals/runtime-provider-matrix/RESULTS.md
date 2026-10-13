# DEV run at 7dfc864ac

Executed once on DEV build `7dfc864acc9b02df97b46b133a68c949b0abe9e3` with
`PROVIDER=codex` and the retained pre-restart Gaugeprobe fixture. Exit 1:
2 scenario PASSes, 10 FAILs; owned-resource cleanup PASS. No Rust compile.

Command:

```bash
MATRIX_RESUME_FIXTURE=target-check/evals/gauge-resume.json PROVIDER=codex \
  bash evals/runtime-provider-matrix/run.sh
```

Raw command exits, outputs, results JSON and table are retained locally under
`target-check/evals/runtime-provider-matrix/matrix-1791923528506-95214/`.
The matrix ran the initial driver; observation repairs below were made afterward.
No result has been changed to PASS based on an unexecuted repair.

| Row | Result | Executed evidence / limitation |
|---|---|---|
| 1 | PASS | Exact pre-restart assistant ID and codeword survived in the owned thread's history |
| 2 | FAIL | Build changed and original session persisted; driver incorrectly required a new binding nonce, so neutral question was not reached |
| 3 | FAIL | Blocked by row 2; owned legacy pre-state adapter is also missing |
| 4 | PASS | CLI, self and peer changes each confirmed by tab list: gpt-6-astra, gpt-6.1-sol, gpt-6-astra |
| 5 | FAIL | Worker ACK and in_progress observed inside data.task; driver parsed the wrong envelope and hit its deadline |
| 6 | FAIL | Driver retained first streaming fragment: “I’m using the codeterm” |
| 7 | FAIL | Driver retained first streaming fragment: “Assigned”; General fixture not reached and isolated adapter missing |
| 8 | FAIL | Compaction control accepted; driver searched message type instead of the transcript compaction marker and hit its deadline |
| 9 | FAIL | Restart returned agent_ready=true and session_action=resumed with retained Worker session; driver retained partial identity; fresh path not reached |
| 10 | FAIL | Working Chat tab observed; CLI stop returned 404 “Tab not found or has no PTY” |
| 11 | FAIL | Cron report deadline: run remained Running; agent final answer reports completion failed because run had no active watcher; hidden worker and dreamer not reached |
| 12 | FAIL | No native pending question appeared before deadline in full-access mode; no answer was injected |
| Cleanup | PASS | Owned plan deleted; cron canceled/deleted; workspace and tabs absent in readback |

The failed interrupt's long-turn tab was explicitly closed before remaining
rows finished. Only the owned Gaugeprobe workspace was driven and removed.

## Source repairs and executed unit results

After this run, the driver uses the changed build plus the exact loaded Chat
thread for the retained restart fixture, unwraps data.task for ACK, reads final
assistant content again after idle, and recognizes the compaction timeline
marker. Self/peer shell commands now explicitly use Bash; their first-run
transcripts showed zsh sourcing errors for the Bash-only lib even though the
model identity changes were subsequently confirmed.

`node --test evals/runtime-provider-matrix/observations.test.mjs`: 4 passed,
0 failed, exit 0. Shell syntax checks and Node syntax check: exit 0.
Affected-test mapper selected no additional commands for these eval files.
These offline results do not establish live identity, compaction or restart PASS.

## Remaining slices

- Rerun corrected observations on canonical DEV with an operator restart
  checkpoint; the retained first-run workspace has been cleaned up.
- Add an owned legacy-tab pre-state seed and verify its session/history restore.
- Supply safe isolated General Agent and provider-scoped dreamer fixtures.
- Diagnose the CLI Chat stop 404 and cron completion watcher refusal.
- Supply a provider-supported native question/approval mode and execute its
  complete round-trip. The current full-access question probe is insufficient.
