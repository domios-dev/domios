# evals/agent-state — replay + score (RT2)

Replays captured agent PTY streams (asciicast v2 `.cast`) through **both**
detection paths and scores them:

1. **Core `SignalExtractor`** (`src-tauri/src/agent/signal_extractor.rs`, P1a) —
   vt100 grid diff → `StreamSignals` (two-clock quiescence, motion, regions).
2. **Provider `classify_pane_state`** — the bundled JS plugin
   (`plugins/provider-<id>/plugin.js`), loaded in-process via the QuickJS runtime.

One Rust runner exercises both: the extractor renders each chunk's screen, its
rendered tail becomes the `PaneStateSnapshot.screenText`, and the provider
classifies it. Per output chunk we record a **state timeline** entry.

It is **additive** — a new eval set only, no production behavior change.

## Run

```bash
evals/agent-state/run.sh          # dev-locked wrapper (sources evals/_lib/lib.sh)
# or directly:
cd src-tauri && cargo test --test agent_state_replay -- --nocapture
```

Offline: no daemon, no network, no live provider account. Requires the bundled
`plugins/provider-*/plugin.js` to exist (they are checked in). If missing locally
the test skips; in CI it fails.

## Layout

```
evals/agent-state/
  README.md            this file
  run.sh               dev-locked runner -> cargo test
src-tauri/tests/
  agent_state_replay.rs   the harness (cast parser + replay + oracles + scorecard)
tests/fixtures/agent-state/<provider>/<state>/
  recording.cast       asciicast v2: header, then [t,"o",data] + [t,"r","COLSxROWS"]
  expected.json        oracle (final_state, optional extracted, negatives)
```

## `.cast` format

asciicast v2 (NDJSON). Line 1 is the header object (`width`/`height` size the
vt100 grid). Each later line is an event row:

- `[t, "o", "<data>"]` — output bytes (`t` = seconds; `now_ms = round(t*1000)`).
- `[t, "r", "COLSxROWS"]` — terminal resize (drives `SignalExtractor::resize`).

Control bytes (ESC, CR, LF) must be JSON-escaped (``, `\r`, `\n`) — raw
control bytes are invalid JSON. Generate captures with the RT1 capture tap; never
hand-edit the byte stream of a real capture.

## `expected.json`

```json
{
  "provider": "codex",
  "final_state": "update_prompt",
  "extracted": { "keys": ["down", "enter"] },
  "negatives": ["waiting_on_auth", "rate_limited", "waiting", "turn_complete"]
}
```

- **`final_state`** — the `PaneState` the classifier must return at the final
  quiescent frame (GOLDEN assertion). Snake_case, per `pane_state.rs`.
- **`extracted.keys`** — optional; the classifier's recommended `action.keys`
  must match exactly (e.g. step OFF a dangerous default before Enter).
- **`negatives`** — the HARD gate. None of these may appear at any **mid-stream**
  step (a step with real output still to come). Two kinds are accepted:
  - classify states (`rate_limited`, `update_prompt`, …) — never that verdict mid-stream.
  - the coarse-liveness axis (`waiting`, `turn_complete`) — never read the pane as
    parked/finished while it is still producing output (the wait-FP / can't-tell-done bugs).

## Scorecard

Printed every run (baseline table):

- **per-state precision / recall** over fixtures (final-state labels).
- **false Waiting (mid-stream)** and **false turn_complete (mid-stream)** counts —
  the wait-FP and can't-tell-finished metrics. P2 must drive these to 0; record
  the current numbers as the baseline P2+ must strictly beat.

### The coarse-liveness gate (baseline stand-in)

The current `PaneState` enum has no `turn_complete`/`waiting` member, so the
harness derives a reference liveness verdict from the extractor's two-clock
quiescence (`Working` while bytes flow; `Waiting` after `WAIT_MS` byte-idle;
`turn_complete` after `DONE_MS` semantic-idle). Idle ticks are injected into
gaps (`TICK_MS`) so a mid-stream stall is actually sampled. This is a deliberate
stand-in for today's coarse gate; P2 replaces it with the semantic-veto gate and
the eval re-measures. Constants live at the top of `agent_state_replay.rs`.

## Fixtures (seed)

Synthetic seeds so the harness runs green NOW (real captures arrive from RT1 /
the orchestrator). Content guidance: `plugins/provider-codex/tests/fixtures/pane-state/*.txt`.

- `codex/working_streaming` — sustained streaming output; stays `working`, no
  false parked/done mid-stream. Includes a `"r"` resize row.
- `codex/update_prompt` — self-update menu; final `update_prompt` with
  `["down","enter"]` (skips the `Update now` default); never auth/rate-limited.

## Adding a fixture

1. Capture a real stream to `recording.cast` (RT1 `pane capture`).
2. Drop it under `tests/fixtures/agent-state/<provider>/<state>/`.
3. Write `expected.json` (final_state + negatives; extracted/keys when actionable).
4. `evals/agent-state/run.sh` — the new fixture is auto-discovered.
