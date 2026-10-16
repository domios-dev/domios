import { test } from "node:test";
import assert from "node:assert/strict";
import { decodeChatScrollTrace } from "../chat-scroll-trace.mjs";

test("decoder reconstructs out of order chunks and five-frame edge context per client", () => {
  const line = (client, index, frames, edges) => `[chat-scroll] client=${client} build_sha=Some("abc1234") ${JSON.stringify({ edge: "trace", state: { gestureId: "gesture-1", index, count: 2, frames, edges, droppedFrames: 0, droppedEdges: 0, droppedBatches: 0 } })}`;
  const log = [line("a", 1, [[7, 20, 0, 0, 0, 0, 0, 0, 0, 0, 42, 0]], [[6, 5, 0, 20, 1]]), line("a", 0, [[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 42, 0]], [[1, 7], [2, 6]]), line("b", 0, [[0, 1]], [])].join("\n");
  const decoded = decodeChatScrollTrace(log);
  assert.equal(decoded[0].complete, true);
  assert.equal(decoded[1].complete, false);
  assert.equal(decoded[0].jumps[0].delta, 20);
  assert.deepEqual(decoded[0].jumps[0].context.map(edge => edge.edge), ["absorb", "write"]);
});


test("same-anchor oracle rejects a known layout shift and accepts native scroll motion", async () => {
  const { sameAnchorLayoutResidual } = await import("../chat-scroll-residual.mjs");
  const before = { anchor: 42, anchorTop: 100, scrollTop: 500 };
  assert.deepEqual(sameAnchorLayoutResidual(before, { ...before, anchorTop: 285 }), { residual: 185, failed: true });
  assert.deepEqual(sameAnchorLayoutResidual(before, { ...before, anchorTop: 120, scrollTop: 480 }), { residual: 0, failed: false });
  assert.deepEqual(sameAnchorLayoutResidual(before, { ...before, anchorTop: 108 }), { residual: 8, failed: false });
  assert.deepEqual(sameAnchorLayoutResidual(before, { ...before, anchorTop: -157 }), { residual: -257, failed: true });
  assert.equal(sameAnchorLayoutResidual(before, { ...before, anchor: 43 }), null);
  assert.equal(sameAnchorLayoutResidual({ ...before, anchor: 0 }, { ...before, anchor: 0 }), null);
});


test("segmented traces evaluate the seam motion once without losing layout shifts", () => {
  const frame = (index, top, anchorTop) => [index, top, 1000, 400, 2, 0, 0, 0, 0, 10, 42, anchorTop];
  const line = (gestureId, frames) => `[chat-scroll] client=a build_sha=abc ${JSON.stringify({ edge: "trace", state: {
    gestureId, index: 0, count: 1, frames, edges: [], droppedFrames: 0, droppedEdges: 0, droppedBatches: 0 } })}`;
  const decoded = decodeChatScrollTrace([
    line("gesture-1", [frame(598, 500, 100), frame(599, 480, 120)]),
    line("gesture-1.1", [frame(0, 480, 120), frame(1, 460, 325)]),
  ].join("\n"));
  assert.equal(decoded.flatMap(trace => trace.residuals).length, 2);
  assert.deepEqual(decoded.flatMap(trace => trace.jumps).map(jump => jump.delta), [185]);
  assert(decoded.every(trace => trace.complete && trace.droppedFrames === 0 && trace.droppedEdges === 0));
});
