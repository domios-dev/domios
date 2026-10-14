import assert from "node:assert/strict";
import { classifyUpdateFailure, classifyUpdateStatus } from "./updateStatusModel";
import type { UpdateAvailabilitySnapshot } from "./updateAvailability";

const ahead = "system plugin 'codex' is not installable: local_ahead";
const snapshot: UpdateAvailabilitySnapshot = { items: [{
  id: "codex", kind: "plugin", installed: "2.0.0", latest: "1.0.0", compatible: true,
  state: "up_to_date", reason: "local_ahead", channel: "codeterm-core",
}], errors: { "codeterm-core": ahead } };
assert.deepEqual(classifyUpdateStatus(snapshot), {
  errors: {}, information: ["codex: installed version 2.0.0 is ahead of channel version 1.0.0"],
});
assert.deepEqual(classifyUpdateStatus({ items: [], errors: { core: ahead } }), {
  errors: {}, information: ["codex: installed version is ahead of channel"],
});
assert.deepEqual(classifyUpdateStatus({ ...snapshot, errors: { core: `${ahead}; system plugin 'other' is not installable: incompatible`, app: "download failed" } }).errors,
  { core: "system plugin 'other' is not installable: incompatible", app: "download failed" });
assert.deepEqual(classifyUpdateStatus({ items: [], errors: { core: `${ahead}; system plugin 'other' is not installable: local_ahead` } }).errors, {});
for (const message of ["local_ahead", "download failed: local_ahead", `${ahead}: download failed`, "system plugin 'codex' is not installable: same_version"]) {
  assert.deepEqual(classifyUpdateStatus({ items: [], errors: { core: message } }).errors, { core: message });
}
for (const reason of ["same_version", "newer_version", "incompatible"]) {
  assert.deepEqual(classifyUpdateStatus({ items: [{ ...snapshot.items[0], reason }], errors: {} }), { errors: {}, information: [] });
}
assert.deepEqual(classifyUpdateStatus({ items: [], errors: {} }), { errors: {}, information: [] });

for (const detail of [new Error("InvalidJsonResponseError: raw daemon failure"), "channel fetch failed"]) {
  assert.equal(classifyUpdateFailure("background", detail), null);
  for (const origin of ["check", "update"] as const) {
    const failure = classifyUpdateFailure(origin, detail)!;
    assert.equal(failure.message, origin === "check" ? "Couldn't check for updates. Try again later." : "Couldn't update. Try again later.");
    assert.equal(failure.details, String(detail));
  }
}

console.log("update status classification: 18 cases passed");
