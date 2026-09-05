# Long-paste submit bench

This bench drives the dev daemon's Rust PTY delivery path with a real Codex TUI.
It compares a short mid-turn send, a 36,435-character idle send, a 4,096 / 16,384
/ 36,435 / 65,536-character mid-turn curve, and a mid-turn 36,435-character send
followed by a second send after the first successful CR write is observed.

Production evidence restored the reported screenshot to the CodeTerm-send
evidence table: its source was a 36,458-byte CodeTerm send that normalized to
Codex's `[Pasted Content 36435 chars]`. The production audit recorded bracketed
`MultilineTui` encoding, successful CR writes at approximately 0.15s, 1s, and
2s, then exact-turn confirmation and an `Idle -> Working` transition several
seconds after the third key. This bench measures the missing relationship:
collapsed-frame render time versus that key burst.

The mid-turn cases ask Codex to run `gate.py` as a foreground tool command. The
helper connects to a bench-owned loopback socket and blocks on that connection,
so `Working` is an observed provider state rather than a timed approximation.
Measurement sends use the same `/chat-send` ledger seam as the Chat panel. The
bench follows the dev log until the first and third successful `key-write`
events, taking visible-screen snapshots around the burst. It releases the
control turn only after those observations and waits on both `Idle -> Working`
and the send's exact-turn confirmation, with bounded failure deadlines, to show
whether the queued payload starts without another input. No readiness sleep or
polling loop is used.

PTY capture is armed only after the control turn reaches `Working`. Each
`sequence.txt` lists the actual input and output timestamps. A bracketed paste
must appear as one frame ending in `ESC [ 201 ~`; every Enter must be a separate
`0d` frame. The summary timestamps the first `[Pasted Content N chars]` output
against the paste write and includes submit-cycle, submit-audit, recovery,
idle-poll, and status log events.

`recovered=false` is printed only as diagnostic context. It is not an oracle:
production telemetry reports it on nearly all transactions, including tiny
sends, because the transaction samples a lagging status synchronously. The
informative outcomes are collapsed-frame timing, `Idle -> Working`, exact-turn
confirmation, and the visible composer state.

Each case also preserves `inspect.json` and prints `provider-selection.txt`.
That distinction matters because public-send recovery resolves
`verifySubmit` from the tab record's `agent_type`; `MultilineTui` alone cannot
distinguish Codex from Claude because both providers declare
`tui_input_drained`. Codex adds a 150ms initial-key offset, while Claude does
not. The public readiness gate itself checks `BootStage::AgentReady` and does
not run provider prompt detection.

`transcript-summary.txt` records whether each unique payload token is already
present in Codex's parsed transcript after the third key write.
It supplements the visible screen; it does not replace the composer-state
observation.

Run only against a matching dev build from the canonical integration checkout:

```bash
bash evals/long-paste-submit/run.sh
```

Optional bounds:

```bash
PAYLOAD_SIZES="4096 16384 36435 65536" LONG_PAYLOAD_CHARS=36435 \
  EVENT_TIMEOUT=120 LOG_EVENT_TIMEOUT=15 TURN_START_TIMEOUT=15 \
  bash evals/long-paste-submit/run.sh
```

The script sources `evals/_lib/lib.sh`; every CodeTerm CLI operation uses `dc`,
which refuses to fall through to prod. Artifacts are written beneath
`target-check/evals/long-paste-submit/` and the temporary workspace and tabs are
closed on exit.

Interpret the cases from observed evidence:

- The first mid-turn size whose collapsed-frame render lands after CR #3 is the
  measured ingestion threshold. If that case stays in the composer, the key
  burst ended before Codex finished ingesting the paste.
- If 65,536 characters submit with exact turn evidence and the collapsed frame
  renders before CR #3, the byte-scaled-delay candidate is refuted and must not
  be proposed.
- If a size submits before its collapsed frame renders, Codex did not require the
  collapsed-frame signal for that run; do not infer a render gate from it.
- If a single mid-turn case submits but `long-mid-turn-next-send` does not, and its first
  cycle has fewer recovery CR frames after the second generation claim, the new
  send cancelled the older recovery cycle.
- If the 36,435-character idle case also sticks, mid-turn is not required. If
  only the mid-turn curve shifts, busy Codex changes ingestion or queues the CR.
- If the capture shows CR in the bracketed-paste frame, framing is the cause. If
  it shows a closed paste frame followed by a separate `0d`, that hypothesis is
  falsified for the captured run.
- If no `key-write` follows the paste within the transaction deadline, inspect
  the captured write result: the write-queue acknowledgement bound, not Codex's
  handling of CR, prevented submission.

This is a measurement bench. A non-reproduction is valid: preserve and report
the printed screens, send exits, input-frame sequence, and audit events for all
cases.

<!-- revision: 4 -->
