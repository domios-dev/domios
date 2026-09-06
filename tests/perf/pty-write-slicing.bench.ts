// Latency oracle: PTY replay/throughput floor of writeTerminalBytesInFrames.
// Deterministic — a mocked performance.now advances by a synthetic per-slice
import {
  writeTerminalBytesInFrames,
  TERMINAL_WRITE_SLICE_BYTES,
  TERMINAL_WRITE_FRAME_BUDGET_MS,
} from "../../src/hooks/terminal/terminalWriteFrames";

const FRAME_MS = 1000 / 60;

// Deterministic clock: term.write advances it by PARSE_MS per 64KB slice.
let clock = 0;
let parseMs = 0;
Object.defineProperty(globalThis, "performance", {
  configurable: true,
  value: { now: () => clock },
});

let rafQueue: Array<() => void> = [];
(globalThis as unknown as { requestAnimationFrame: (cb: () => void) => number }).requestAnimationFrame = (cb) => {
  rafQueue.push(cb);
  return rafQueue.length;
};
(globalThis as unknown as { cancelAnimationFrame: () => void }).cancelAnimationFrame = () => {};

function pump(maxFrames = 1_000_000): number {
  let frames = 0;
  while (rafQueue.length && frames < maxFrames) {
    const batch = rafQueue;
    rafQueue = [];
    for (const cb of batch) cb();
    frames++;
  }
  return frames;
}

const term = { write: (_c: Uint8Array, cb?: () => void) => { clock += parseMs; cb?.(); } };

function measure(bytes: number, perSliceMs: number): number {
  clock = 0;
  parseMs = perSliceMs;
  rafQueue = [];
  const payload = new Uint8Array(bytes);
  let done = false;
  writeTerminalBytesInFrames(term as never, payload, () => { done = true; });
  const frames = pump();
  if (!done) throw new Error("drain incomplete");
  return frames;
}

const slices = (bytes: number) => Math.ceil(bytes / TERMINAL_WRITE_SLICE_BYTES);

console.log(
  `slice=${TERMINAL_WRITE_SLICE_BYTES / 1024}KB, budget=${TERMINAL_WRITE_FRAME_BUDGET_MS}ms/frame\n`,
);
console.log("Frames to drain (lower = faster catch-up). before = 1 slice/frame.\n");
console.log("payload   parse/slice   before   after   wall before/after @60fps");
for (const kb of [256, 1024, 4096]) {
  const bytes = kb * 1024;
  for (const perSlice of [1, 4, 12]) {
    const before = slices(bytes);
    const after = measure(bytes, perSlice);
    console.log(
      `${String(kb).padStart(5)}KB   ${String(perSlice).padStart(5)}ms      ${String(before).padStart(5)}   ${String(after).padStart(5)}   ${(before * FRAME_MS).toFixed(0).padStart(5)}ms / ${(after * FRAME_MS).toFixed(0)}ms`,
    );
  }
}
console.log(
  `\nAt >=${TERMINAL_WRITE_FRAME_BUDGET_MS}ms/slice the budget caps at 1 slice/frame (== before): no burst-freeze regression.`,
);
