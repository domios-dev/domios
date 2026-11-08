import assert from "node:assert/strict";
import { channelDiagnostics, classifyUpdateFailure, classifyUpdateStatus, unavailableSourceKeys, updateIndicator, updateRestartDescription } from "./updateStatusModel";
import type { UpdateRow } from "./updatePanelModel";
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

const interfaceRow: UpdateRow = { id: "frontend", name: "Interface", group: "Interface", currentVersion: "1.0.0", targetVersion: "1.0.1", state: "available", alreadyCurrent: false };
const appRow: UpdateRow = { ...interfaceRow, id: "app", name: "Domios", group: "App" };
const idle = { busy: false, checking: false, failed: false };
assert.equal(updateIndicator([interfaceRow], idle).description, "Updates available. Interface update needs no restart.");
for (const rows of [[appRow], [interfaceRow, appRow]]) {
  assert.equal(updateIndicator(rows, idle).description, "Updates available. App update restarts Domios.");
  assert.equal(updateIndicator(rows, idle).state, "available");
}
assert.equal(updateIndicator([{ ...interfaceRow, state: "active" }], idle).state, "active");
assert.equal(updateIndicator([{ ...interfaceRow, state: "failed" }], { ...idle, busy: true }).state, "failed");
assert.equal(updateIndicator([], { ...idle, completed: true }).state, "completed");
assert.equal(updateIndicator([], idle).state, "idle");
console.log("unified update indicator: frontend, app, both, applying, failed and completed passed");

// The restart line follows a staged newer version, never a row's group alone.
assert.equal(updateRestartDescription(appRow, true), "Restart required");
assert.equal(updateRestartDescription(appRow, false), "Update restarts Domios");
assert.equal(updateRestartDescription(interfaceRow, false), "No restart required");
assert.equal(updateRestartDescription({ ...appRow, targetVersion: null, channelError: "HTTP 504", state: "unavailable" }, true), null);
assert.equal(updateRestartDescription({ ...appRow, targetVersion: null }, true), null);
assert.equal(updateRestartDescription({ ...appRow, channelError: "HTTP 504", state: "unavailable" }, true), null);
assert.equal(updateRestartDescription({ ...appRow, state: "completed" }, true), null);
assert.equal(updateRestartDescription({ ...appRow, state: "failed" }, false), null);
assert.equal(updateRestartDescription({ ...interfaceRow, targetVersion: null, channelError: "HTTP 504", state: "unavailable" }, false), null);
assert.equal(updateRestartDescription({ ...interfaceRow, group: "Plugins" }, true), null);

// A source outage is not a failed update and names its source.
const sourceDown = { ...idle, unavailableSources: ["App update channel"] };
const unavailableRow: UpdateRow = { ...appRow, targetVersion: null, channelError: "HTTP 504", state: "unavailable" };
assert.equal(updateIndicator([unavailableRow], sourceDown).state, "unavailable");
assert.equal(updateIndicator([unavailableRow], sourceDown).description, "App update channel unavailable. Open updates for details.");
assert.equal(updateIndicator([unavailableRow], sourceDown).offers, false);
assert.equal(updateIndicator([unavailableRow, interfaceRow], sourceDown).state, "available");
assert.equal(updateIndicator([unavailableRow], { ...sourceDown, failed: true }).state, "failed");
assert.equal(updateIndicator([unavailableRow], { ...sourceDown, unavailableSources: ["a", "b"] }).description, "2 update sources unavailable. Open updates for details.");

assert.deepEqual(unavailableSourceKeys({ app: "HTTP 504", community: "offline", "codeterm-core": unavailable }), ["app", "community"]);
assert.deepEqual(unavailableSourceKeys({}), []);
console.log("restart line, source outage indicator and named check result: passed");
