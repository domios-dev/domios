# Detection Replay Scenarios

Scenario scripts generate full-session console replays for detection work. The
recording is the deterministic artifact; the scenario is only the generator.

## Full capture runbook

Use this when creating a new replay fixture. A replay is not trusted until every
step below passes.

### 1. Ensure dev is the target

The capture runner targets the dev daemon (`7686`) and must never write through
prod (`7685`). Start or refresh dev first:

```bash
./build.sh dev --wait
```

### 2. Record the full session

The scenario is only the generator. The recorded `raw.cast` is the artifact we
replay forever.

```bash
./testbench/replay-capture.py \
  testbench/detect-scenarios/snake_incremental_build_browser_v1.yaml \
  --provider claude
```

To test geometry robustness, record separate real sessions at separate PTY
sizes. Do not rely on post-factum re-rendering alone: TUIs may wrap, move, or
hide status rows differently when the real terminal size changes.

```bash
./testbench/replay-capture.py \
  testbench/detect-scenarios/snake_incremental_build_browser_v1.yaml \
  --provider claude \
  --cols 100 \
  --rows 30
```

Output:

```text
testbench/replays/<provider>/<scenario>-<run>/
  raw.cast
  screen.txt
  meta.json
  phases.json
  capture.log.json
  scenario.json
```

Reject partial runs: a usable replay has multiple input anchors, a full
turn/phase list, and a final idle/settled window. Do not promote a run that only
captured a picker frame or a single prompt.

### 3. Build the renderer fresh

The renderer shares production detection code. If Rust changed, the old binary
is stale.

```bash
cd src-tauri && cargo build --example render_timeline
cd ..
```

### 4. Render timeline + metrics

Use `75ms` for review-quality playback. For a quick smoke, `250ms` is enough.

```bash
src-tauri/target/debug/examples/render_timeline \
  testbench/replays/claude/<run>/raw.cast \
  testbench/replays/claude/<run>/timeline.json \
  --sample-ms 75 \
  --meta testbench/replays/claude/<run>/meta.json \
  --phases testbench/replays/claude/<run>/phases.json

./testbench/replay-analyze.py testbench/replays/claude/<run>/timeline.json
```

For matrix capture across providers and real PTY sizes:

```bash
./testbench/replay-matrix.py \
  --scenario testbench/detect-scenarios/geometry_smoke_v1.yaml \
  --provider claude \
  --provider opencode \
  --provider codex \
  --provider cursor \
  --geometry 120x32 \
  --geometry 100x30 \
  --geometry 160x40
```

For frame-local development, render only the window you are debugging. The
renderer still replays from the start to reconstruct the terminal correctly, but
it emits only the requested frames, so the JSON and player loop stay small.

```bash
src-tauri/target/debug/examples/render_timeline \
  testbench/replays/claude/<run>/raw.cast \
  /tmp/ct-window-7300-7380.json \
  --sample-ms 75 \
  --from-frame 7300 \
  --to-frame 7380 \
  --meta testbench/replays/claude/<run>/meta.json \
  --phases testbench/replays/claude/<run>/phases.json
```

### 5. Run replay doctor

This is the fidelity gate: byte rows, input anchors, lossy-byte rows,
phase coverage, detector summary, and final VT render equality.

```bash
./testbench/replay-doctor.py testbench/replays/claude/<run> \
  --screen-rows 50 \
  --screen-cols 120
```

Fast iteration gate:

```bash
./testbench/replay-doctor.py testbench/replays/claude/<run> \
  --fast-full \
  --screen-rows 50 \
  --screen-cols 120
```

Window gate for a suspected regression:

```bash
./testbench/replay-doctor.py testbench/replays/claude/<run> \
  --from-frame 7300 \
  --to-frame 7380 \
  --screen-rows 50 \
  --screen-cols 120
```

`--fast-full` and window gates are development accelerators. Before promoting a
detector change, run the dense full doctor and inspect the player on the
affected windows.

`50x120` is the current `ScreenState` oracle geometry. Once `ScreenState` tracks
the tab's real PTY winsize, switch this to the cast-header geometry instead.

### 6. Open the replay player

```bash
python3 -m http.server 8097 --bind 127.0.0.1
```

Then open:

```text
http://127.0.0.1:8097/testbench/player/index.html?src=/testbench/replays/claude/<run>/timeline.json
```

Link to a specific frame:

```text
http://127.0.0.1:8097/testbench/player/index.html?src=/testbench/replays/claude/<run>/timeline.json&frame=7348
```

The player shows reconstructed terminal on the left and Chat UI preview on the
right: real state (`idle|working|question`), status message, pending card, last
input anchor, and quality counters.

Rules:

- target dev only, never prod;
- record full sessions, not single frames;
- record important scenarios at multiple real PTY sizes; post-render geometry
  stress is useful but is not a substitute for real differently-sized captures;
- keep input events in the same cast clock as output events;
- use `phases.json` as expected lanes, not detector truth;
- promote a replay to gating only after Dump Doctor passes.
