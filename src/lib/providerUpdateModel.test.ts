import assert from "node:assert/strict";
import { providerUpdateModel } from "./providerUpdateModel";
import type { ProviderCliStatus } from "./providerCli";

const status: ProviderCliStatus = { required: true, installed: true, binary: "fixture", resolved_path: "/tools/fixture", availability: "available", searched: [], installer: null, updater: "opaque", latest_version: "1.2.3" };
assert.equal(providerUpdateModel("Fixture", status)?.canUpdate, true);
assert.equal(providerUpdateModel("Fixture", { ...status, latest_version: null }), null);
assert.equal(providerUpdateModel("Fixture", { ...status, installed: false }), null);
const unsupported = providerUpdateModel("Fixture", { ...status, updater: null, update_refusal: "opaque refusal" });
assert.equal(unsupported?.canUpdate, false);
assert.equal(unsupported?.state, "unsupported");
assert.ok(unsupported?.message.includes("opaque refusal"));
for (const state of ["running", "succeeded", "failed"] as const) {
  const model = providerUpdateModel("Fixture", { ...status, update_job: { tab_id: "utility", state, summary: "opaque result" } });
  assert.equal(model?.state, state);
  assert.equal(model?.message, "opaque result");
  assert.equal(model?.canUpdate, state === "failed");
}
console.log("providerUpdateModel: 15 assertions passed");
