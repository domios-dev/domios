import { performance } from "node:perf_hooks";
import { OutputRetention } from "../../src/lib/outputRetention";

const retained = new OutputRetention();
const cursor = { boot: "bench", ptyId: "pty", offset: 2 * 1024 * 1024 };
retained.append("tab", { ...cursor, bytes: new Uint8Array(cursor.offset).fill(65) });
const measure = (known: boolean) => {
  const start = performance.now();
  let copiedBytes = 0;
  for (let iteration = 0; iteration < 100; iteration++) {
    copiedBytes += retained.snapshot("tab", known ? cursor : undefined)!.bytes.length;
  }
  return { ms: performance.now() - start, copiedBytes };
};
const before = measure(false), after = measure(true);
retained.append("tab", { ...cursor, offset: cursor.offset + 5, bytes: new Uint8Array([66, 67, 68, 69, 70]) });
const delta = retained.snapshot("tab", cursor)!.bytes;
if (after.copiedBytes !== 0 || delta.length !== 5) throw new Error("Known-position replay copied consumed history");
console.log(JSON.stringify({ iterations: 100, retainedBytes: cursor.offset, before, after, deltaBytes: delta.length }, null, 2));
