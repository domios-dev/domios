# Long-paste submit bench

Run only in canonical DEV under the Orchestrator's execution authority, with
its build SHA equal to the clean checkout SHA:

```bash
bash evals/long-paste-submit/run.sh
```

The default provider matrix is Claude and Codex. The runner reads each provider's
advertised `transport`, launches that provider, and refuses a mismatching tab
provider or backing. Runtime tabs never receive terminal actions or capture
requests. All CLI and HTTP operations use the DEV-locked `evals/_lib/lib.sh`.

Each provider retains 4096, 16384, 36435 and 65536 byte ASCII payloads at idle
and mid-turn: eight size cases. There is also a short mid-turn control. PTY
backings retain the original long-mid-turn-next-send recovery-cancellation
case, whose second send follows the first observed successful submit key.
`PROVIDERS`, `PAYLOAD_SIZES`, `LONG_PAYLOAD_CHARS`, `EVENT_TIMEOUT`,
`LOG_EVENT_TIMEOUT` and `TURN_START_TIMEOUT` can narrow a diagnostic invocation;
a narrowed invocation is not a complete matrix receipt.

The mid-turn control asks the agent to run `gate.py` in the foreground. A
bench-owned loopback connection plus authoritative Working status establishes
the busy precondition. Initial readiness requires AgentReady, idle status,
and an acknowledged own-session nonce. Waits consume daemon events or the
existing PTY log stream; deadlines only bound failures. No screen, collapsed
paste rendering or elapsed time proves delivery.

Before a measurement send, `evidence-anchor.json` records the exact session,
nonce, provider, backing, PTY identity, normalized transcript count and existing
row IDs. The final observer reads the suffix after that count and refuses a
changed binding before or after the read. It requires one exact complete user
payload row, excluding inherited and pre-anchor rows, followed by an assistant
reply, plus one delivered ledger receipt with an own-session source claim.
Runtime receipts must name a structured item on that exact thread. PTY cases
additionally require exactly one raw or closed bracketed payload frame in
`trace.cast`. Duplicate payload writes or rows fail. This normalized append
boundary is bench evidence; it does not establish persistence of the product's
native JSONL byte anchor or runtime cursor across restart.

PTY screen snapshots, submit-key timestamps, collapsed paste rendering and
recovery diagnostics remain artifacts. They are not readiness or delivery
oracles. The runtime path uses transcript and ledger evidence without a PTY.
Missing evidence fails rather than replaying input. The runner retains all
failure artifacts and closes only its created tabs and workspace.

Receipts are under `target-check/evals/long-paste-submit/<run>/<provider>/<case>/`.
`source-identity.txt` records execution and build SHAs; `providers.json` records
the capability inventory. Each case retains `spawn.json`, `inspect.json`, the
append boundary, the post-read binding, `assert-transcript.json`,
`assert-ledger.json` and send response. PTY cases also retain capture and log
observations. Exit zero requires explicit completion and executed assertions.
These receipts do not replace the separate ten-message-per-condition composer
and CLI confirmation drill or remote interactive-input acceptance.

Offline oracle fixtures (no daemon or live agent):

```bash
python3 -m unittest discover -s evals/long-paste-submit -p 'test_*.py' -v
bash -n evals/long-paste-submit/run.sh
```

For an isolation-first red/green demonstration, run
`OutcomeTests.test_runtime_needs_no_pty_capture` with the payload frame check
temporarily made unconditional; it must fail on zero captured payload writes.
Restore the transport-aware assertion and run that selector,
then the file. Run `OutcomeTests.test_second_payload_write_fails` with duplicate
write rejection removed; it must fail because the second write was accepted.
Neither demonstration has been executed in the Worker lane.

<!-- revision: 7 -->
