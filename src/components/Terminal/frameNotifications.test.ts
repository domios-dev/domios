import assert from "node:assert/strict";
import { frameNotifications } from "./frameNotifications";

const frames = new Map<number, () => void>();
let next = 0;
const events: string[] = [];
const notifications = frameNotifications((key) => events.push(key), {
  request(callback) { frames.set(++next, callback); return next; },
  cancel(handle) { frames.delete(handle); },
});
function frame() {
  const callbacks = [...frames.values()];
  frames.clear();
  callbacks.forEach((callback) => callback());
}
for (let i = 0; i < 600; i++) notifications.enqueue("first");
notifications.enqueue("second");
assert.equal(frames.size, 1);
assert.deepEqual(events, []);
frame();
assert.deepEqual(events, ["first", "second"]);
notifications.enqueue("first");
notifications.enqueue("second");
notifications.immediate("first");
frame();
assert.deepEqual(events, ["first", "second", "first", "second"]);
notifications.enqueue("first");
notifications.remove("first");
assert.equal(frames.size, 0);
notifications.enqueue("first");
notifications.clear();
frame();
assert.equal(events.length, 4);
console.log("Frame notifications coalesce, flush and cancel without duplicate delivery");
