# UI responsiveness latency oracles

Bounded, deterministic oracles for the two reported stalls: composer input
freezing every few seconds, and PTY output scrolling slowly during a long Codex
session. `*.bench.ts` files are diagnostic (run on demand, not in the unit
gate); the render-count regression test lives next to the component and DOES run
in `pnpm test:unit`.

```bash
npx tsx tests/perf/transcript-churn.bench.ts
npx tsx tests/perf/pty-write-slicing.bench.ts
npx tsx src/components/Terminal/PaneSessionChat.planref-stability.test.tsx
```

## Root causes & fixes

**Composer freeze.** Not the chat-core JS pipeline (see transcript-churn below —
<3ms/chunk at 10k msgs). It was React re-rendering the whole visible
`MessageBubble` list (each re-parsing markdown) on every streamed chunk:
`MessageBubble` is `React.memo` (default shallow), and `resolvePlanRef`'s
identity churned per chunk because `visiblePlanRefs` returned a fresh array
(→ `orderedRefs` → resolver output → new `resolvePlanRef`), plus
`usePlanIndexOptional`'s no-provider fallback allocated a fresh object each
render. Fix: `visiblePlanRefs` reuses the previous array when the deduped
ref-set is unchanged (`PaneSessionChat.tsx`); `usePlanIndexOptional` returns a
stable shared fallback (`PlansProvider.tsx`). Guarded by
`PaneSessionChat.planref-stability.test.tsx`.

**Slow PTY scroll.** `writeTerminalBytesInFrames` wrote exactly one 64KB slice
per rAF → hard 3.75 MB/s cap and a ~1067ms floor to replay a 4MB buffer. Fix:
drain multiple slices per frame under an 8ms budget, then yield
(`terminalWriteFrames.ts`); the live path hands the drainer up to 256KB
(`usePtyConnection.ts`). Above 8ms/slice parse cost it degrades to 1 slice/frame
== the old cadence, so no burst-freeze regression.

## transcript-churn.bench.ts

Per-streamed-chunk cost of the transcript recompute pipeline vs. transcript size
N. Baseline (macOS, Node/V8): 10k msgs = 2.3ms p50 / 2.9ms max, 0/60 frames over
the 16.7ms budget. **Rules chat-core OUT as the freeze cause.**

## pty-write-slicing.bench.ts

Deterministic (mocked `performance.now` + synthetic per-slice parse cost).
`before` = old 1-slice/frame; `after` = current budgeted drainer.

```
payload   parse/slice   before   after   wall before/after @60fps
 4096KB       1ms          64       8    1067ms / 133ms   (8x faster)
 4096KB       4ms          64      32    1067ms / 533ms   (2x faster)
 4096KB      12ms          64      64    1067ms / 1067ms  (== before, no regression)
```

## PaneSessionChat.planref-stability.test.tsx

happy-dom render-count oracle. Streams 12 tail-delta chunks into a mounted
`PaneSessionChat` and asserts unchanged `MessageBubble` rows re-render ≤2× (not
~1×/chunk). Uses a test-only `window.__bubbleRenderCounts` opt-in counter added
to `MessageBubble` (no-op in prod). Fails at 13× if either fix is reverted.

## Real keystroke→paint (optional QA lane, dev on 7686)

The composer also ships `window.__composerPerf` (`inputMarks` /
`paintLatencies`). A Playwright microtest against dev (web-mode on macOS) can
capture end-user keystroke→paint under a live session; the headless oracles
above are the fast pre-screen.
