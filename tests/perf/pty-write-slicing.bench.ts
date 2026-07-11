// Latency oracle: PTY replay/throughput floor imposed by
// writeTerminalBytesInFrames' one-slice-per-rAF cadence. Mocks term.write as
// instant to isolate the SCHEDULING cost (real xterm ANSI parse adds on top).
// Run: `npx tsx tests/perf/pty-write-slicing.bench.ts`.
import { writeTerminalBytesInFrames, TERMINAL_WRITE_SLICE_BYTES } from "../../src/hooks/terminal/terminalWriteFrames";

const FRAME_MS = 1000 / 60;

let rafQueue: Array<() => void> = [];
(globalThis as unknown as { requestAnimationFrame: (cb: () => void) => number }).requestAnimationFrame = (cb) => {
  rafQueue.push(cb);
  return rafQueue.length;
};
(globalThis as unknown as { cancelAnimationFrame: () => void }).cancelAnimationFrame = () => {};

function pump(maxFrames = 100000): number {
  let frames = 0;
  while (rafQueue.length && frames < maxFrames) {
    const batch = rafQueue;
    rafQueue = [];
    for (const cb of batch) cb();
    frames++;
  }
  return frames;
}

const term = { write: (_c: Uint8Array, cb?: () => void) => { cb?.(); } };

console.log(`slice = ${TERMINAL_WRITE_SLICE_BYTES / 1024}KB/frame, live cap = ${(TERMINAL_WRITE_SLICE_BYTES * 60 / 1024 / 1024).toFixed(2)} MB/s\n`);
console.log("Replay-on-show scheduling floor (real xterm parse adds on top):");
console.log(" payload   frames  wall-clock@60fps");
for (const kb of [64, 256, 1024, 2048, 4096]) {
  rafQueue = [];
  const payload = new Uint8Array(kb * 1024);
  let done = false;
  writeTerminalBytesInFrames(term as never, payload, () => { done = true; });
  const frames = pump();
  console.log(`${String(kb).padStart(5)}KB   ${String(frames).padStart(5)}   ${(frames * FRAME_MS).toFixed(0).padStart(6)}ms  ${done ? "" : "(incomplete!)"}`);
}
