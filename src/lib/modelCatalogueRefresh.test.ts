import assert from "node:assert/strict";
import { test } from "node:test";
import { refreshFailureToast, refreshOutcomeFromResponse } from "./modelCatalogueRefresh";

test("refresh responses keep the daemon's failure reason", () => {
  assert.deepEqual(refreshOutcomeFromResponse(true), { refreshed: true });
  assert.deepEqual(refreshOutcomeFromResponse({ refreshed: true, reason: null }), { refreshed: true });
  assert.deepEqual(refreshOutcomeFromResponse({ refreshed: false, reason: " opaque-reason " }), { refreshed: false, reason: "opaque-reason" });
  assert.deepEqual(refreshOutcomeFromResponse({ refreshed: false, reason: "" }), { refreshed: false });
  assert.deepEqual(refreshOutcomeFromResponse(null), { refreshed: false });
});

test("a refresh failure toast is one line and copies the full reason", async () => {
  const copied: string[] = [];
  const toast = refreshFailureToast("vendor", "opaque-reason", (text) => { copied.push(text); });
  assert.equal(toast.detail, null);
  assert.equal(toast.tabId, null);
  assert.doesNotMatch(toast.title, /opaque-reason/);
  assert.deepEqual(toast.actions?.map((action) => action.label), ["Copy"]);
  await toast.actions![0].run();
  assert.deepEqual(copied, ["opaque-reason"]);
  assert.equal(refreshFailureToast("vendor", undefined, () => {}).actions, undefined);
  assert.equal(refreshFailureToast("vendor", "a", () => {}).id, refreshFailureToast("vendor", "b", () => {}).id);
});
