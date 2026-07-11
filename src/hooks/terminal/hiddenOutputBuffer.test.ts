import { createHiddenOutputBuffer } from "./hiddenOutputBuffer";

function assert(cond: boolean, label: string) {
  if (!cond) throw new Error(`FAILED: ${label}`);
}

const bytes = (...vals: number[]) => new Uint8Array(vals);

// Under cap: everything replays in order, no resync needed.
(() => {
  const buf = createHiddenOutputBuffer(1024);
  buf.push(bytes(1, 2, 3));
  buf.push(bytes(4, 5));
  const { payload, dropped } = buf.drain();
  assert(payload.join(",") === "1,2,3,4,5", "coalesces chunks in FIFO order");
  assert(!dropped, "no drop while under cap");
  assert(buf.size === 0, "drain clears the buffer");
})();

// Over cap: oldest whole chunks are evicted and `dropped` flags a needed resync.
(() => {
  const buf = createHiddenOutputBuffer(6);
  buf.push(bytes(1, 2, 3));
  buf.push(bytes(4, 5, 6));
  buf.push(bytes(7, 8, 9)); // total 9 > 6 -> evict first chunk
  assert(buf.size === 6, "size bounded to the trailing chunks");
  const { payload, dropped } = buf.drain();
  assert(payload.join(",") === "4,5,6,7,8,9", "keeps the trailing bytes");
  assert(dropped, "eviction sets the dropped flag");
})();

// A single chunk larger than the cap is never evicted to empty (nothing to replay otherwise).
(() => {
  const buf = createHiddenOutputBuffer(4);
  buf.push(bytes(1, 2, 3, 4, 5, 6, 7, 8));
  assert(buf.size === 8, "lone oversized chunk is retained");
  const { payload } = buf.drain();
  assert(payload.length === 8, "oversized chunk drains intact");
})();

// Unbounded-growth guard: a long hidden session stays near the cap, not the total.
(() => {
  const cap = 1000;
  const buf = createHiddenOutputBuffer(cap);
  for (let i = 0; i < 10_000; i++) buf.push(bytes(0, 0, 0, 0, 0, 0, 0, 0, 0, 0));
  assert(buf.size <= cap, "buffer never exceeds the cap across many pushes");
  const { dropped } = buf.drain();
  assert(dropped, "long session marks dropped so the caller resyncs");
})();

// Empty drain is safe and reports no drop.
(() => {
  const buf = createHiddenOutputBuffer(10);
  const { payload, dropped } = buf.drain();
  assert(payload.length === 0 && !dropped, "empty drain is a no-op");
})();

console.log("hidden output buffer checks passed");
