import assert from "node:assert/strict";
import { test } from "node:test";
import { ChatGestureTrace, packGesture, unpackGesture } from "./chatGestureTrace";

test("gesture flags round trip", () => {
  for (let bits = 0; bits < 16; bits++) {
    const state = unpackGesture(bits, 7);
    assert.equal(packGesture(state), bits);
    assert.equal(state.stationaryFrames, 7);
  }
});
test("trace ring retains bounded frames and edges with dropped counts", () => {
  const trace = new ChatGestureTrace("gesture-1");
  for (let i = 0; i < 1000; i++) { trace.sample([i, Infinity], true); trace.edge("scroll", [i]); }
  assert.equal(trace.frames.length, 600);
  assert.equal(trace.edges.length, 400);
  assert.equal(trace.droppedFrames, 400);
  assert.equal(trace.droppedEdges, 600);
  assert.deepEqual(trace.frames[0], [400, 400, 0]);
  for (let i = 0; i < 59; i++) assert.equal(trace.sample([], false), false);
  assert.equal(trace.sample([], false), true);
});
test("chunks preserve tuples and fit chat byte limit including envelope", () => {
  const trace = new ChatGestureTrace("gesture-2");
  for (let i = 0; i < 600; i++) { trace.sample(Array(16).fill(123456.78), true); trace.edge("resize", Array(34).fill(-123456.78)); }
  const chunks = trace.chunks(2);
  assert.deepEqual(chunks.flatMap(chunk => chunk.frames), trace.frames);
  const encodedEdges = chunks.flatMap(chunk => chunk.edges);
  assert.deepEqual(encodedEdges, trace.edges.slice(0, encodedEdges.length));
  assert.equal(encodedEdges.length + chunks[0].droppedEdges, trace.edges.length + trace.droppedEdges);
  assert.ok(chunks.length <= 32);
  chunks.forEach((chunk, index) => {
    assert.equal(chunk.index, index); assert.equal(chunk.count, chunks.length); assert.equal(chunk.droppedBatches, 2);
    assert.ok(new TextEncoder().encode(JSON.stringify({ scope: "trace", edge: "trace", at: 0, state: chunk })).length < 4000);
  });
});
test("trace identity and tuple widths remain bounded for malformed inputs", () => {
  const trace = new ChatGestureTrace("x".repeat(5000), Infinity);
  trace.sample(Array(100).fill(Infinity), true);
  trace.edge("write", Array(100).fill(NaN));
  const chunk = trace.chunks()[0];
  assert.equal(chunk.gestureId, "unknown");
  assert.equal(chunk.scopeId, 0);
  assert.equal(chunk.frames[0].length, 17);
  assert.equal(chunk.edges[0].length, 36);
  assert.ok(JSON.stringify(chunk).length < 4000);
});
