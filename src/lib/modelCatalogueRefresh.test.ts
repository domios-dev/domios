import assert from "node:assert/strict";
import { test } from "node:test";
import { refreshFailureMessage, refreshOutcomeFromResponse } from "./modelCatalogueRefresh";

test("refresh responses keep the daemon's failure reason", () => {
  assert.deepEqual(refreshOutcomeFromResponse(true), { refreshed: true });
  assert.deepEqual(refreshOutcomeFromResponse({ refreshed: true, reason: null }), { refreshed: true });
  assert.deepEqual(refreshOutcomeFromResponse({ refreshed: false, reason: " opaque-reason " }), { refreshed: false, reason: "opaque-reason" });
  assert.deepEqual(refreshOutcomeFromResponse({ refreshed: false, reason: "" }), { refreshed: false });
  assert.deepEqual(refreshOutcomeFromResponse(null), { refreshed: false });
});

test("the failure message names the reason only when one exists", () => {
  assert.match(refreshFailureMessage("opaque-reason"), /opaque-reason/);
  assert.doesNotMatch(refreshFailureMessage(undefined), /:/);
});
