import assert from "node:assert/strict";
import { test } from "node:test";
import { createProviderCatalogueResource } from "./providerCatalogueResource";

test("many catalogue consumers share cached loads and one invalidation per event", async () => {
  let requests = 0;
  const scheduled: Array<() => void> = [];
  const resource = createProviderCatalogueResource(async () => ++requests, callback => scheduled.push(callback));
  const first = resource.load();
  for (let consumer = 0; consumer < 100; consumer++) assert.equal(resource.load(), first);
  assert.equal(await first, 1);
  assert.equal(await resource.load(), 1);
  assert.equal(requests, 1);
  for (let consumer = 0; consumer < 100; consumer++) resource.invalidate();
  scheduled.splice(0).forEach(callback => callback());
  const refreshed = resource.load();
  for (let consumer = 0; consumer < 100; consumer++) assert.equal(resource.load(), refreshed);
  assert.equal(await refreshed, 2);
  resource.invalidate();
  assert.equal(await resource.load(), 3);
  assert.equal(requests, 3);
});

test("reconnect invalidations coalesce behind an in-flight catalogue request", async () => {
  const requests: Array<(value: number) => void> = [];
  const resource = createProviderCatalogueResource(() => new Promise<number>(resolve => requests.push(resolve)));
  const first = resource.load();
  for (let event = 0; event < 1_000; event++) resource.invalidate();
  for (let consumer = 0; consumer < 100; consumer++) assert.equal(resource.load(), first);
  assert.equal(requests.length, 1, "readiness train does not start overlapping requests");
  requests[0](1);
  await Promise.resolve();
  assert.equal(requests.length, 2, "all invalidations during one request schedule one refresh");
  for (let event = 0; event < 1_000; event++) resource.invalidate();
  requests[1](2);
  await Promise.resolve();
  assert.equal(requests.length, 3, "a second invalidation train also coalesces");
  requests[2](3);
  assert.equal(await first, 3, "all waiters receive the response for the latest revision");
  assert.equal(await resource.load(), 3);
  assert.equal(requests.length, 3, "settled revision remains cached");
});
