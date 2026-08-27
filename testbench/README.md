# Snapshot Testbench

Fast dev/CI-only snapshot tests for isolated units. The first unit is agent
terminal parsing: saved terminal output is replayed through the real Rust
extractors and real provider plugins in-process. No live agent, daemon, UI, or
parser duplicate is used by the runner.

An isolated run is a small `input -> real library -> expected output` loop. The
input is checked in beside its oracle, so parser and unit work can iterate in
milliseconds without booting CodeTerm or waiting for a provider. Live agents are
used only to refresh the corpus; the normal bench is offline and deterministic.

The core rule: the runner is a thin harness. It may discover cases, normalize
fixture file names, and print diffs, but it must not reimplement the behavior
under test. A green case must mean the production parser/unit returned the
expected result on the saved input.

## Run

```bash
export CARGO_TARGET_DIR=/Users/user/Developer/.psetup-target
bash testbench/run.sh
```

The runner auto-discovers `testbench/snapshots/**/expected.json`.

Current units:

- `provider_screen` — loads the real provider plugin with Rust `load_one` and
  calls `parsePrompt`, `classifyTabState`, `hasEventMarkers`, and
  `detectEvents` in-process.
- `agent_state_replay` — replays `raw.cast` through the Rust
  `SignalExtractor`, provider `classify_tab_state`, semantic marker parser,
  and command detection.

## Refresh The Claude Demo Snapshots

This talks to the already-running dev daemon on port 7686 through `dc`. It does
not rebuild, restart, or kill the daemon.

```bash
bash testbench/capture-claude-demo.sh
bash testbench/run.sh
```

The script captures:

- `claude/options-abc`
- `claude/compound-wizard`
- `claude/risk-tradeoff`

Each case contains `raw.cast`, `screen.txt`, `meta.json`, and `expected.json`.

## Case Format

Provider screen cases:

```json
{
  "provider": "claude",
  "unit": "provider_screen",
  "expected": {
    "parsePrompt": {
      "question": "Question text",
      "options": [
        { "label": "Option label", "value": "A" }
      ]
    }
  }
}
```

The expected object is a subset match against the real parser output. Include
only fields the case is meant to pin.

Agent-state replay cases:

```json
{
  "provider": "codex",
  "unit": "agent_state_replay",
  "final_state": "update_prompt",
  "extracted": { "keys": ["down", "enter"] },
  "negatives": ["waiting_on_auth", "rate_limited", "waiting", "turn_complete"]
}
```

Use `gating: false` for real baseline captures that should print findings but
not fail CI.

## Add A Case

1. Create `testbench/snapshots/<provider>/<situation>/`.
2. Add `screen.txt` for provider-screen parsing, or `raw.cast` for replay.
3. Add `expected.json`.
4. Run `bash testbench/run.sh`.

For parser work, capture the real output first, write the expected parse, run
the isolated bench, then iterate the real parser until the saved snapshot is
green.

## Build A New Isolated Unit

1. Add a new `unit` variant to `testbench/rust/harness.rs`.
2. Keep the unit adapter tiny: read fixture input, call the real production
   function, serialize its actual output.
3. Compare expected JSON as a subset unless the full output is intentionally
   stable.
4. Document the fixture shape here before adding cases.

Avoid daemon calls, sleeps, network, clocks, and UI automation inside isolated
units. If a case needs any of those, capture the result into a fixture first and
replay the fixture offline.
