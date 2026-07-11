import {
  TERMINAL_WRITE_SLICE_BYTES,
  writeTerminalBytesInFrames,
  writeTerminalStringInFrames,
} from "./terminalWriteFrames";

function assert(cond: boolean, label: string) {
  if (!cond) throw new Error(`FAILED: ${label}`);
}

const rafQueue: FrameRequestCallback[] = [];
(globalThis as { requestAnimationFrame?: (cb: FrameRequestCallback) => number }).requestAnimationFrame = (cb) => {
  rafQueue.push(cb);
  return rafQueue.length;
};
(globalThis as { cancelAnimationFrame?: (id: number) => void }).cancelAnimationFrame = () => {};

function drainFrames(limit = 20) {
  let count = 0;
  while (rafQueue.length > 0 && count < limit) {
    const cb = rafQueue.shift()!;
    cb(performance.now());
    count += 1;
  }
}

(() => {
  const writes: number[] = [];
  let complete = false;
  const term = {
    write(data: Uint8Array | string, cb?: () => void) {
      writes.push(typeof data === "string" ? data.length : data.byteLength);
      cb?.();
    },
  };
  const payload = new Uint8Array(TERMINAL_WRITE_SLICE_BYTES * 2 + 7);
  writeTerminalBytesInFrames(term, payload, () => {
    complete = true;
  });
  drainFrames();
  assert(writes.length === 3, "large byte payload is split across frames");
  assert(writes[0] === TERMINAL_WRITE_SLICE_BYTES, "first byte slice respects cap");
  assert(writes[2] === 7, "last byte slice carries remainder");
  assert(complete, "byte completion fires after final slice");
})();

(() => {
  const writes: number[] = [];
  let complete = false;
  const term = {
    write(data: Uint8Array | string, cb?: () => void) {
      writes.push(typeof data === "string" ? data.length : data.byteLength);
      cb?.();
    },
  };
  writeTerminalStringInFrames(term, "x".repeat(TERMINAL_WRITE_SLICE_BYTES + 3), () => {
    complete = true;
  });
  drainFrames();
  assert(writes.join(",") === `${TERMINAL_WRITE_SLICE_BYTES},3`, "string payload is sliced");
  assert(complete, "string completion fires after final slice");
})();

(() => {
  const writes: number[] = [];
  const term = {
    write(data: Uint8Array | string, cb?: () => void) {
      writes.push(typeof data === "string" ? data.length : data.byteLength);
      cb?.();
    },
  };
  const cancel = writeTerminalBytesInFrames(term, new Uint8Array(TERMINAL_WRITE_SLICE_BYTES + 1));
  cancel();
  drainFrames();
  assert(writes.length === 0, "cancel before first frame prevents writes");
})();

console.log("terminal write frame scheduler checks passed");
