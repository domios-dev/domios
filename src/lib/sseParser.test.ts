import assert from "node:assert/strict";
import { MAX_SSE_FRAME_BYTES, parseSseFrame, SseAccumulator, SseCapacityError, type SseMessage, type SseWork } from "./sseParser";

const encoder = new TextEncoder();
const messages = ['event: output\nid: boot:1\ndata: {"text":"é🙂漢字"}', 'data: first\r\ndata: second', ':keepalive', 'data: 7'];
const expected = messages.map(frame => parseSseFrame(frame)).filter(message => message !== null);

async function receive(chunks: Uint8Array[], work?: SseWork) {
  const received: SseMessage[] = [];
  const accumulator = new SseAccumulator(work);
  for (const chunk of chunks) {
    await accumulator.push(chunk, (frame, bytes) => {
      const message = parseSseFrame(frame, bytes, work);
      if (message) received.push(message);
    });
  }
  accumulator.finish();
  return received;
}

for (const separator of ["\n\n", "\r\n\r\n", "\n\r\n", "\r\n\n"]) {
  const wire = encoder.encode(messages.join(separator) + separator);
  for (let boundary = 0; boundary <= wire.length; boundary++) {
    assert.deepEqual(await receive([wire.subarray(0, boundary), wire.subarray(boundary)]), expected);
  }
  assert.deepEqual(await receive(Array.from(wire, (_, index) => wire.subarray(index, index + 1))), expected);
}

for (const ending of ["\n\n", "\r\n\r\n", "\n\r\n", "\r\n\n"]) {
  const frame = 'data: "' + "é".repeat((MAX_SSE_FRAME_BYTES - 8) / 2) + '"';
  const wire = encoder.encode(frame + ending);
  assert.equal((await receive([wire.subarray(0, 8), wire.subarray(8, -1), wire.subarray(-1)]))[0].data, frame.slice(6));
  await assert.rejects(receive([encoder.encode(frame + "x" + ending)]), SseCapacityError);
}
await assert.rejects(receive([encoder.encode("x".repeat(MAX_SSE_FRAME_BYTES + 1))]), SseCapacityError);
const unfinished = new SseAccumulator();
await unfinished.push(encoder.encode("x".repeat(MAX_SSE_FRAME_BYTES) + "\r"), () => assert.fail("unfinished frame delivered"));
assert.throws(() => unfinished.finish(), SseCapacityError);

assert.doesNotThrow(() => parseSseFrame("data: " + "[".repeat(32) + "0" + "]".repeat(32)));
assert.throws(() => parseSseFrame("data: " + "[".repeat(33) + "0" + "]".repeat(33)), SseCapacityError);
assert.throws(() => parseSseFrame("data: [" + "0,".repeat(256 * 1024) + "0]"), SseCapacityError);
for (const payload of [
  JSON.stringify({ value: '[{,\\"'.repeat(100), next: [1, 2] }),
  JSON.stringify({ even: "\\\\", odd: '\\"', next: [1] }),
  '"unterminated ' + "[".repeat(100),
]) {
  assert.equal(parseSseFrame("data: " + payload)?.data, payload);
}

const work: SseWork = { delimiterBytes: 0, decodedBytes: 0, encodedBytes: 0, structureCodeUnits: 0, structurePasses: 0 };
const payload = JSON.stringify({ output: "A".repeat(256 * 1024), escaped: '\\"'.repeat(32) });
const frame = "data: " + payload;
const wire = encoder.encode(frame + "\r\n\r\n");
const chunks = Array.from({ length: Math.ceil(wire.length / 113) }, (_, i) => wire.subarray(i * 113, (i + 1) * 113));
assert.equal((await receive(chunks, work))[0].data, payload);
assert.equal(work.delimiterBytes, wire.length);
assert.equal(work.decodedBytes, wire.length - 4);
assert.equal(work.encodedBytes, 0);
assert.equal(work.structurePasses, 1);
assert.ok(work.structureCodeUnits <= payload.length * 2);

const ordered = new SseAccumulator();
const order: string[] = [];
let release!: () => void;
const held = new Promise<void>(resolve => { release = resolve; });
let entered!: () => void;
const first = new Promise<void>(resolve => { entered = resolve; });
const pushing = ordered.push(encoder.encode("data: 1\n\ndata: 2\n\n"), async (frame) => {
  order.push(frame);
  if (order.length === 1) { entered(); await held; }
});
await first;
assert.deepEqual(order, ["data: 1"]);
release();
await pushing;
assert.deepEqual(order, ["data: 1", "data: 2"]);
await assert.rejects(new SseAccumulator().push(encoder.encode("data: 1\n\ndata: 2\n\n"), () => { throw new Error("cancel delivery"); }), /cancel delivery/);

console.log("SSE parser: fragmentation, UTF-8, bounds, linear work and ordered delivery passed");
