import assert from "node:assert/strict";
import { channelDiagnostics, classifyUpdateFailure, classifyUpdateStatus } from "./updateStatusModel";
import type { UpdateAvailabilitySnapshot } from "./updateAvailability";

const ahead = "system plugin 'codex' is not installable: local_ahead";
const community: UpdateAvailabilitySnapshot = { items: [{
  id: "lark", kind: "plugin", installed: "2.0.0", latest: "1.0.0", compatible: true,
  state: "up_to_date", reason: "local_ahead", channel: "community",
}], errors: {} };
assert.deepEqual(classifyUpdateStatus(community), {
  errors: {}, information: ["lark: installed version 2.0.0 is ahead of channel version 1.0.0"],
});
const core: UpdateAvailabilitySnapshot = { items: [{ ...community.items[0], id: "codex", state: "core_silent", channel: "codeterm-core" }],
  errors: { "codeterm-core": ahead } };
assert.deepEqual(classifyUpdateStatus(core), { errors: {}, information: [] });
assert.deepEqual(classifyUpdateStatus({ items: [], errors: { core: `${ahead}; system plugin 'other' is not installable: incompatible`, app: "download failed" } }).errors,
  { core: "system plugin 'other' is not installable: incompatible", app: "download failed" });
for (const message of ["local_ahead", "download failed: local_ahead", `${ahead}: download failed`, "system plugin 'codex' is not installable: same_version"]) {
  assert.deepEqual(classifyUpdateStatus({ items: [], errors: { core: message } }).errors, { core: message });
}
const unavailable = "Plugin channel unavailable: repository not found or not readable without access (github.com/rollacode/codeterm.git). Domios will retry.";
assert.deepEqual(classifyUpdateStatus({ items: [], errors: { "codeterm-core": unavailable } }).errors, { "codeterm-core": unavailable });
for (const reason of ["same_version", "newer_version", "incompatible"]) {
  assert.deepEqual(classifyUpdateStatus({ items: [{ ...community.items[0], reason }], errors: {} }), { errors: {}, information: [] });
}
assert.deepEqual(classifyUpdateStatus({ items: [], errors: {} }), { errors: {}, information: [] });
console.log("update status classification: 11 cases passed");

assert.deepEqual(channelDiagnostics({ "codeterm-core": unavailable, community: "x", app: "y", frontend: "z" }, new Set(["community"])),
  [{ channel: "codeterm-core", message: unavailable }]);
console.log("channel diagnostics exclude shown rows and app discovery: 1 case passed");

for (const detail of [new Error("InvalidJsonResponseError: raw daemon failure"), "channel fetch failed"]) {
  assert.equal(classifyUpdateFailure("background", detail), null);
  for (const origin of ["check", "update"] as const) {
    const failure = classifyUpdateFailure(origin, detail)!;
    assert.equal(failure.message, origin === "check" ? "Couldn't check for updates. Try again later." : "Couldn't update. Try again later.");
    assert.equal(failure.details, String(detail));
    assert.equal(failure.channelUnavailable, false);
  }
}
const channelFailure = classifyUpdateFailure("update", new Error(`community: ${unavailable}`))!;
assert.deepEqual(channelFailure, { message: unavailable, details: `Error: community: ${unavailable}`, channelUnavailable: true });
assert.equal(classifyUpdateFailure("check", unavailable)?.message, unavailable);
console.log("update failure classification names channel causes: 9 cases passed");
