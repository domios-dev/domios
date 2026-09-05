# pty-paste-trace

Observation bench for one question: **does a CodeTerm-originated send of a long
payload into a codex tab end up unsubmitted, and if so, at which hop is the
carriage return lost?**

This bench observes. It never edits `service/pty_io/`,
`chat_send_transaction.rs`, `pty/write_queue.rs` or their tests.

## What it measures

1. **Ingestion vs burst window.** The submit transaction writes the paste, then
   up to `MAX_SUBMIT_KEY_WRITES = 3` carriage returns at 0 / 1s / 2s
   (`submit_transaction.rs:18`, `:628-644`). The whole burst closes at ~2.05s.
   If codex needs longer than that to ingest a large bracketed paste, every CR
   lands mid-ingestion. `run.sh` sweeps 4k / 16k / 36k / 64k and prints the
   `paste-encoding`, `key-write` and `submit-recovery` lines against the moment
   the composer renders its collapsed `[Pasted Content N chars]` frame.
2. **CR while `Working`.** Each size runs twice, once into an idle TUI and once
   into a tab that is mid-turn, so the screen after CR #3 says whether codex
   queued the CR, swallowed it, or submitted.
3. **Which branch the send took.** `initial-submit` in the log means the
   provider resolved to a zero `initial_key_delay` (Claude's strategy: the CR is
   glued to the paste in one frame batch). `paste-encoding` means a non-zero
   delay (codex's 150ms). On a codex tab, an `initial-submit` line is a
   provider-mistyping symptom, not a normal send.

## Running

Dev only, and dev must be up. Every CLI call goes through `dc`.

```bash
./evals/pty-paste-trace/run.sh <codex-tab-id>
./evals/pty-paste-trace/run.sh <codex-tab-id> 4000 16000 36000 64000
```

## Offline corpus analysis

`analyze-submit-bursts.py` needs no daemon. It reads any `codeterm.log` and
groups `submit-audit` lines into transactions, reporting payload size against
submit-key count, the recovery verdict and the following turn transition.

```bash
python3 evals/pty-paste-trace/analyze-submit-bursts.py ~/.codeterm/logs/codeterm.log
```

Use it first: it answers the size-vs-outcome question from production
telemetry alone, without driving a live tab.
