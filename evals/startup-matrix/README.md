# Curated startup matrix

This live bench adopts the owner's real DEV window (default instance, API 7686,
WebView2 CDP from health). It spends provider tokens: one minimal PONG prompt per
tab. It never closes tabs, replaces drafts, or changes product code.

Reserve serialized QA with the Orchestrator first. Run from a Team-bound QA shell:

```bash
bash evals/startup-matrix/run.sh --dependencies C:/Developer/codeterm/codeterm --expect-build 3b7bf7ea2
```

`--dependencies` selects a checkout with installed `@playwright/test`; omit when
installed in this lane. `--out DIR` selects the single results directory. Wait for
`ARMED <dir>`, then have the Orchestrator run in canonical:

```powershell
.\build.ps1 kill
.\build.ps1 dev-prebuilt --wait
```

The driver observes log changes, checks health for a new boot ID, and verifies a
new PID and kernel start time on the explicitly expected measured build. It subscribes with the new boot's
replay cursor, recording every frame. A 120 s failure deadline bounds restart,
boot readiness, click wake, and reply observation. Log and DOM events drive waits;
the three-second timer samples health latency only.

If restart preflight interrupts the run before any prompts, preserve that directory
and use `--resume <old-dir>` with a fresh `--out`. This reuses its pre-restart
inventory and listener proof and attaches to the new boot's replay ring. It is
only for a run that sent no prompts; it does not repeat a completed probe.

The inventory classifies every visible agent tab. The action matrix covers all
General tabs, every Endure fixture, an outside recent tab when available, and a
frozen tab. Missing activity means asleep outside General. Expected policy is
computed from the pre-action state; selection may update activity on later runs.
The first frozen chat fixture gets an agent-send wake without a preceding click;
other fixtures get real accessible sidebar clicks. Chat tabs use their own named
composer; PTY tabs receive a dev-locked `dc send`. Replies reuse core-journeys'
exact new assistant transcript and own-session oracle, including idle completion.
Input is sent immediately after opening, without waiting for agent readiness.
Chat input must accept within 1000 ms of visible content; PTY readiness must
arrive within 5000 ms of boot or the wake action. A refused send, missing reply,
missing readiness event or missed latency bound fails. Every row shows its SLA
measurement and verdict. The agent-send wake uses a frozen PTY fixture when one
is available so chat fixtures retain the click-and-type journey.

Artifacts in `target-check/evals/startup-matrix/<stamp>/`:

- `results.json`: schema 1, inventory, per-tab policy/state, boot/wake event,
  ready/reply timings, delivery receipt, UI render latency, health samples,
  restart listener proof, coverage, errors and screenshot paths.
- `results.md`: generated tab table, byte-identical to the JSON rendering.
- `events.jsonl`, `before-state.json`, `boot-state.json`, `final-state.json`:
  exact API evidence; `transcript-<tab>.json`: baseline IDs and resulting chat.
- `dev.log` and `lock-holds.md`: only this run's appended log bytes, every lock
  hold above 500 ms and every SLOW-IPC line, grouped and sorted by total time.
- `screenshots/`: failure captures. A late capture can show the current selected
  tab; per-tab click-time failures are captured immediately.

`ready_ms` measures receipt of `agent_ready` relative to the new listener's kernel
start. SSE has no emission timestamp: replay gives an upper bound, and overflow
frames explicitly preserve gaps. Frozen tabs have no boot-ready duration; their
separate `wake_ms` is relative to the action. No screenshot implies agent readiness.
`reply_ms` records the first new assistant content observed through the API;
`completed_ms` separately includes waiting for idle after the exact PONG transcript.

Validate with `node evals/startup-matrix/contract.mjs <dir>`: exit 0 means complete
passing evidence; 3 means complete evidence with observed failures; 1 means an
incomplete evidence contract. The driver uses 3 for observed failures. Offline
checks: `node --test evals/startup-matrix/model.test.mjs`.
