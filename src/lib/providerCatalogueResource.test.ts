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
