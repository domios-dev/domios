import { test } from "node:test";
import assert from "node:assert/strict";
import { decodeChatScrollTrace } from "../chat-scroll-trace.mjs";

test("decoder reconstructs out of order chunks and five-frame edge context per client", () => {
  const line = (client, index, frames, edges) => `[chat-scroll] client=${client} build_sha=Some("abc1234") ${JSON.stringify({ edge: "trace", state: { gestureId: "gesture-1", index, count: 2, frames, edges, droppedFrames: 0, droppedEdges: 0, droppedBatches: 0 } })}`;
  const log = [line("a", 1, [[7, 20]], [[6, 5, 0, 20, 1]]), line("a", 0, [[6, 0]], [[1, 7], [2, 6]]), line("b", 0, [[0, 1]], [])].join("\n");
  const decoded = decodeChatScrollTrace(log);
  assert.equal(decoded[0].complete, true);
  assert.equal(decoded[1].complete, false);
  assert.equal(decoded[0].jumps[0].delta, 20);
  assert.deepEqual(decoded[0].jumps[0].context.map(edge => edge.edge), ["absorb", "write"]);
});
