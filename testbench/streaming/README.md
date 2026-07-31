# Streaming-preview quality runs

Measures how well the ephemeral Chat streaming preview (motion decoder turn
buffer + canonical prefix matcher) reproduces the in-flight assistant message,
per provider. The recorded artifacts are deterministic; every stage after the
recording is offline and repeatable.

Goal metrics (see `simulate.ts` output):

- **lead** — how much earlier the preview shows content than the canonical
  JSONL message lands (bigger is better),
- **leftovers** — preview rows still displayed after the turn's canonical
  messages arrived (must trend to 0: nothing stale sticks around),
- **jank** — frame transitions that are not "cut from top / append at bottom /
  extend last row" (must trend to 0: no visual jumping).

## 1. Record a session (dev daemon, port 7686)

One tab per provider, the same question set for all providers
(`questions.json`). For each provider (claude / opencode / codex / model
variants):

```bash
source evals/_lib/lib.sh                       # dev-locked `dc` wrapper
dc agent spawn claude --title streaming-probe  # note the tab id + pty tab id
# arm the PTY cast capture:
dev_post "/api/tabs/<TAB>/capture/start" '{}'
# start the canonical-arrival sidecar (records wall-clock per message batch):
python testbench/streaming/record-chat-timeline.py --tab <TAB> \
    --out captures/streaming/<provider>/chat-timeline.ndjson &
# drive the conversation: send each prompt from questions.json via
#   dc tab send --tab <TAB> "<prompt>"
# waiting for the tab to settle idle between prompts.
dev_post "/api/tabs/<TAB>/capture/stop" '{}'   # returns the .cast path
```

Keep the capture's `captured_at_ms` (meta from capture/stop or meta.json) —
it aligns the sidecar's wall clock with the cast clock.

## 2. Render the motion timeline

```bash
cd src-tauri
cargo run --example render_timeline -- <session.cast> timeline-motion.json
```

(Use an isolated target dir; see `build.ps1 worktree-check` notes in
docs/BUILD.md.)

## 3. Simulate the Chat UI and score

```bash
npx tsx testbench/streaming/simulate.ts \
  --timeline timeline-motion.json \
  --chat chat-timeline.ndjson \
  --epoch-ms <captured_at_ms> \
  --out streaming-report.json
```

The simulator replays per frame exactly what the Chat streaming block would
display — decoder turn buffer minus canonical coverage, using the REAL
production matcher (`src/components/Terminal/streamPreviewMatch.ts`) — so a
matcher fix immediately changes the score with no UI in the loop.

## 4. Inspect visually

Load `timeline-motion.json` in `testbench/player/index.html` — the
"In-flight Message" panel shows the reconstructed preview per frame; the
report's worst turns (`leftover_sample`) tell you which frames to scrub to.

## Cross-provider matrix

Repeat step 1 per provider with the same `questions.json`, keep artifacts
under `captures/streaming/<provider>/`, and compare `streaming-report.json`
summaries side by side. Regressions in the matcher or decoder show up as
falling lead, rising leftovers, or rising jank on the SAME recordings —
re-run steps 2-3 only, no re-recording needed.

<!-- revision: 1 -->
