import assert from "node:assert/strict";
import { mountNotificationHistory } from "./notificationHistory";
import type { Notification } from "../../lib/types";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

async function run() {
  const first = deferred<Notification[]>();
  const recovered = deferred<void>();
  let refresh!: () => void;
  let reads = 0;
  let subscribed = false;
  const applied: Notification[][] = [];
  const notice = { id: "missed" } as Notification;
  const stop = mountNotificationHistory(
    () => {
      assert(subscribed, "recovery must subscribe before the initial read");
      reads++;
      return reads === 1 ? first.promise : Promise.resolve([notice]);
    },
    (rows) => { applied.push(rows); if (rows.length) recovered.resolve(); },
    (request) => { subscribed = true; refresh = request; return () => { subscribed = false; }; },
    (error) => { throw error; },
  );
  for (let i = 0; i < 20; i++) refresh();
  assert.equal(reads, 1);
  first.resolve([]);
  await recovered.promise;
  assert.equal(reads, 2, "a reconnect burst coalesces into one follow-up read");
  assert.deepEqual(applied, [[], [notice]]);
  stop();
  refresh();
  assert.equal(subscribed, false);
  assert.equal(reads, 2, "disposal removes recovery and prevents later reads");

  const late = deferred<Notification[]>();
  let adopted = false;
  const dispose = mountNotificationHistory(
    () => late.promise,
    () => { adopted = true; },
    () => () => {},
    () => {},
  );
  dispose();
  late.resolve([notice]);
  await late.promise;
  assert.equal(adopted, false, "an old page cannot hydrate after disposal");
  console.log("Notification history: recovery coalesces and disposal fences old reads");
}

void run();
