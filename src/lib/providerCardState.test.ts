import assert from "node:assert/strict";
import { test } from "node:test";
import { providerCardState } from "./providerCardState";

test("provider card distinguishes pending observation from every completed availability result", () => {
  assert.deepEqual(providerCardState({ availability: "unknown", availability_checking: true }), { state: "loading", label: "Checking installation…" });
  assert.deepEqual(providerCardState({ availability: "available" }), { state: "available", label: "Installed" });
  assert.deepEqual(providerCardState({ availability: "unavailable" }), { state: "unavailable", label: "Not installed" });
  assert.deepEqual(providerCardState({ availability: "unknown" }), { state: "unknown", label: "Availability unknown" });
  assert.deepEqual(providerCardState({}), { state: "unknown", label: "Availability unknown" });
  assert.deepEqual(providerCardState({ availability: "available" }, true), { state: "signIn", label: "Sign in needed" });
});
