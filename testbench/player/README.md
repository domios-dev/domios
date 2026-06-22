# Detection Replay Player

Static review UI for detection replay timelines.

Open `testbench/player/index.html` in a browser and load a `timeline.json` file.
For a smoke check, load `sample-timeline.json`.

For large repo-local timelines, serve the repo root and pass the timeline path:

```bash
python3 -m http.server 8097 --bind 127.0.0.1
```

Then open:

```text
http://127.0.0.1:8097/testbench/player/index.html?src=/testbench/replays/claude/snake_incremental_build_browser_v1-canonical-20260621-085835/timeline.json
```

The player also accepts drag-and-drop of a `timeline.json` file.

The main view shows the reconstructed terminal on the left and the Chat UI
preview on the right: real state (`idle|working|question`), status message,
pending prompt card, last input anchor, and replay quality counters.

Use `frame=N` to open a permalink to a specific frame:

```text
http://127.0.0.1:8097/testbench/player/index.html?src=/testbench/replays/claude/snake_incremental_build_browser_v1-canonical-20260621-085835/timeline.json&frame=7348
```

The `Copy Frame Link` button writes the current frame URL to the clipboard.

Playback controls:

- `Space` — play/pause
- `ArrowLeft` / `ArrowRight` — previous/next frame
- `Shift+ArrowLeft` / `Shift+ArrowRight` — jump 10 frames
- `Home` / `End` — first/last frame
- speed selector — `0.25x`, `1x`, `2x`, `5x`, `10x`

Expected generated timeline shape:

```json
{
  "provider": "claude",
  "scenario": "snake_incremental_build_browser_v1",
  "frames": [
    {
      "t_ms": 100,
      "screen": ["rendered vt100 row 1", "rendered vt100 row 2"],
      "cursor": { "row": 10, "col": 2 },
      "decset": { "alt_screen": false, "cursor_hidden": true },
      "changed_cells": 42,
      "detector": {
        "state": "working",
        "working": true,
        "status_line": "Thinking... 1s · esc to interrupt",
        "last_turn_summary": null,
        "phase": "thinking"
      }
    }
  ],
  "inputs": [
    { "t_ms": 50, "kind": "input", "turn": 1, "text": "..." }
  ],
  "expected_phases": [
    { "from_ms": 50, "to_ms": 5000, "state": "working" }
  ]
}
```

The player intentionally treats expected phases and detector output as separate
lanes. The detector output is what is under test.
