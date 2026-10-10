# Renderer memory over a long session

Measures the WebView2 renderer while a user works across many busy tabs for a
long time: several workspaces of shell tabs stream agent-like output
(`stream.mjs`), and the scenario visits every tab round-robin through the
sidebar, the way a user switches. One CDP sample per full cycle shows what
grows with time rather than with load.

Named dev instance only; the runner refuses the default dev and prod ports and
verifies `identify --json` through the bench lib before creating anything.

```bash
# isolated instance (own data dir, API, vite and CDP ports)
CODETERM_INSTANCE=perfqa CODETERM_API_PORT=7741 ./build.sh dev --wait

CODETERM_INSTANCE=perfqa CODETERM_API_PORT=7741 \
  bash evals/perf/renderer-memory/run.sh --workspaces 3 --tabs 6 --cycles 6 \
    --dwell-lines 200 --snapshots --out /tmp/rm-run
```

Outputs in `--out`:

- `samples.jsonl`: after a forced GC, CDP `Performance.getMetrics` (JS heap,
  DOM nodes, JS listeners, documents), Tauri event-listener registry entries and
  IPC callbacks, mounted tabs, xterm instances and buffer lines, canvases, and on
  Windows the private bytes of each WebView2 child process (`renderer`,
  `gpu-process`, ...).
- `visits.jsonl`: click-to-ready latency per visit (`readyMs`: the tab's
  terminal is visible with scrollback replayed). Phase `cycle1` is first visits,
  later cycles are revisits, `pingpong` alternates two tabs (warm switches).
- `t0.heapsnapshot`, `cycleN.heapsnapshot` with `--snapshots`.

Other tools:

- `node summarize-snapshot.mjs a.heapsnapshot b.heapsnapshot` — growth by
  constructor between two snapshots (self size and count).
- `node cpu-profile.mjs --cdp PORT --vite PORT --ms 20000` — main-thread busy
  share and top functions by self time over a window.
- `node eval.mjs --cdp PORT --vite PORT 'expr'` — evaluate one expression in
  the renderer.

A flat curve across cycles is the pass condition; a column that rises every
cycle is an accumulation to chase with the snapshot diff.
