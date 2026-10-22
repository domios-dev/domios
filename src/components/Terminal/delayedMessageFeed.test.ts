import assert from "node:assert/strict";
import { createDelayedMessageFeed } from "./delayedMessageFeed";
import type { DelayedMessage } from "./delayedSend";

async function main() {
  const message: DelayedMessage = { id: "future", tabId: "tab", text: "body", scheduledAtMs: 200, createdAtMs: 1, state: "queued", sessionChanged: false };
  const publishes: DelayedMessage[][] = [];
  const publishLog: { at: number; entries: { id: string; state: string }[] }[] = [];
  let load = async () => [message];
  const feed = createDelayedMessageFeed("tab", {
    load: () => load(), publish: messages => { publishes.push(messages); publishLog.push({ at: performance.now(), entries: messages.map(({ id, state }) => ({ id, state })) }); }, subscribe: async () => () => {},
  });
  await feed.ready;
  assert.deepEqual(publishes, [[message]]);
  let resolve!: (messages: DelayedMessage[]) => void;
  load = () => new Promise<DelayedMessage[]>(done => { resolve = done; });
  const waking = feed.wake();
  assert.deepEqual(publishes, [[message]], "wake preserves the current list until refreshed");
  resolve([{ ...message }]);
  await waking;
  assert.equal(publishes.length, 1, "unchanged results are not published");
  load = async () => { throw new Error("offline"); };
  await feed.wake();
  assert.equal(publishes.length, 1, "transport failures preserve the current list");
  load = async () => [{ ...message, state: "failed" }];
  await feed.refresh();
  assert.equal(publishes.length, 2);
  assert.equal(publishes[1][0].state, "failed");
  const second = { ...message, id: "second" };
  load = async () => [message, second];
  await feed.refresh();
  assert.equal(publishes.length, 3);
  load = async () => [second, { ...message }];
  await feed.refresh();
  assert.equal(publishes.length, 3, "reordering does not publish or remount chips");
  load = async () => [];
  await feed.refresh();
  assert.equal(publishes.length, 4, "a genuinely emptied queue is published");
  assert.deepEqual(publishes[3], []);
  feed.dispose();
  console.log("delayedMessageFeed: all assertions passed", JSON.stringify(publishLog));

}
void main();

