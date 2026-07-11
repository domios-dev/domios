# UI responsiveness latency oracles

Bounded, deterministic micro-benches for the two reported stalls: composer
input freezing every few seconds, and PTY output scrolling slowly during a long
Codex session. Named `*.bench.ts` (not `*.test.ts`) so they stay out of the
`pnpm test:unit` gate — they are diagnostic oracles, run on demand.

```bash
npx tsx tests/perf/transcript-churn.bench.ts
npx tsx tests/perf/pty-write-slicing.bench.ts
```

## transcript-churn.bench.ts

Per-streamed-chunk cost of the transcript recompute pipeline (`mergeById` in the
reducer, then `attachToolResults` / `trimAfterSessionReset` /
`matchedOptimisticEchoes` in the `allMessages` useMemo) as transcript size N
grows. Frame budget = 16.7ms.

Baseline (33c28409, macOS, Node/V8):

```
   N      p50      p95      max    frames>16.7ms
 5000    1.33     1.56     1.59     0/60
10000    2.26     2.78     2.94     0/60
```

**Finding: the JS transcript pipeline is NOT the freeze cause** — even a 10k
message transcript recomputes in <3ms/chunk, zero dropped frames. Do not target
`chat-core` for the composer stall; the cost is in React reconciliation / DOM
(`MessageBubble` list) or in main-thread contention from the PTY write path,
which this oracle does not exercise. Confirm real keystroke→paint with the
Playwright oracle below.

## pty-write-slicing.bench.ts

Frame-scheduling floor imposed by `writeTerminalBytesInFrames` (one
`TERMINAL_WRITE_SLICE_BYTES` = 64KB slice per rAF). `term.write` is mocked
instant, so this is the pure scheduling floor — real xterm ANSI parse adds on
top of every frame.

Baseline (33c28409):

```
slice = 64KB/frame, live cap = 3.75 MB/s
 payload   frames  wall-clock@60fps
  4096KB      64     1067ms
```

**Finding: quantified "scrolls slowly".** Visible live render is hard-capped at
64KB × 60fps ≈ 3.75 MB/s; a Codex burst faster than that catches up at that
rate. A full 4MB hidden-buffer replay on tab-show takes ≥1067ms of scheduling
alone (`HIDDEN_BUFFER_MAX_BYTES` in `usePtyConnection.ts`). Both the live path
(`flushPendingWrites`) and replay path write exactly one slice per frame.

## Real keystroke→paint (QA lane, needs a running dev on 7686)

The composer ships a built-in perf harness: `window.__composerPerf`
(`inputMarks` / `paintLatencies`) and `window.__renderCounts`. Drive it from a
single Playwright microtest against dev (web-mode on macOS) to capture
keystroke→paint under a long live session — that is the missing measurement the
JS oracles above cannot produce headless.
