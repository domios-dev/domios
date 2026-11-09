import assert from "node:assert/strict";
import type { TransportClient } from "../../lib/transportClient";
import { subscribeAgentRunOutput } from "./agentRunOutput";

type Options = NonNullable<Parameters<TransportClient["subscribe"]>[2]>;
const channels: Array<{ path: string; emit: Parameters<TransportClient["subscribe"]>[1]; options: Options; closed: boolean }> = [];
const client = {
  subscribe(path, emit, options = {}) {
    const channel = { path, emit, options, closed: false };
    channels.push(channel);
    return () => { channel.closed = true; };
  },
} as TransportClient;

async function main() {
  const output: string[] = [];
  const connected: boolean[] = [];
  const endings: Array<{ status?: number; offset: number }> = [];
  const controller = new AbortController();
  const stop = subscribeAgentRunOutput(client, "tab/id", controller.signal, {
    output: text => output.push(text), connected: value => connected.push(value),
    ended: (end, offset) => endings.push({ status: end.status, offset }),
  });
  assert.equal(channels[0].path, "/api/tabs/tab%2Fid/output/stream?from=0");
  channels[0].options.onReady?.();
  channels[0].emit("message", { output: "雪", offset: 3 });
  channels[0].emit("message", { output: "雪", offset: 3 });
  channels[0].options.onEnd?.({ reason: "error" });
  await Promise.resolve();
  assert.equal(channels[1].path, "/api/tabs/tab%2Fid/output/stream?from=3");
  channels[1].emit("message", { output: "!", offset: 4 });
  assert.deepEqual(output, ["雪", "!"]);
  assert.deepEqual(connected, [true, false]);
  stop();
  assert.equal(channels[1].closed, true);
  channels[1].options.onEnd?.({ reason: "error" });
  await Promise.resolve();
  assert.equal(channels.length, 2);

  const fallbackStart = channels.length;
  subscribeAgentRunOutput(client, "missing", controller.signal, {
    output() {}, connected() {}, ended: (end, offset) => endings.push({ status: end.status, offset }),
  });
  channels[fallbackStart].options.onEnd?.({ reason: "http", status: 404 });
  assert.deepEqual(endings, [{ status: 404, offset: 0 }]);
  controller.abort();
  channels[fallbackStart].options.onEnd?.({ reason: "http", status: 404 });
  assert.equal(endings.length, 1);
  console.log("agent run mux resume, duplicate suppression, 404 fallback and cancellation passed");
}
void main().catch(error => { console.error(error); process.exitCode = 1; });
