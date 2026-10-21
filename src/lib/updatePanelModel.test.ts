import assert from "node:assert/strict";
import { createUpdateIntent, groupUpdates, nextUpdateStep, readUpdateIntent, type UpdateItem } from "./updatePanelModel";

const items: UpdateItem[] = [
  { id: "plugin", group: "Plugins", name: "Plugin", currentVersion: "1.0.0", targetVersion: "2.0.0" },
  { id: "frontend", group: "Interface", name: "Interface", currentVersion: "1.0.0", targetVersion: "2.0.0" },
  { id: "app", group: "App", name: "Domios", currentVersion: "1.0.0", targetVersion: "2.0.0" },
];
assert.deepEqual(groupUpdates(items).map(section => section.group), ["App", "Interface", "Plugins"]);
const intent = readUpdateIntent(JSON.stringify(createUpdateIntent(items)))!;
assert.deepEqual(nextUpdateStep(intent, "1.0.0", items), { kind: "waitForApp" });
assert.deepEqual(nextUpdateStep(intent, "2.0.0", items), { kind: "plugin", id: "plugin" });
assert.deepEqual(nextUpdateStep(intent, "2.0.0", items.filter(item => item.group !== "Plugins")), { kind: "interface" });
assert.deepEqual(nextUpdateStep(intent, "2.0.0", []), { kind: "complete" });
assert.equal(readUpdateIntent('{"stage":"plugins"}'), null);
assert.equal(readUpdateIntent("broken"), null);
assert.deepEqual(nextUpdateStep(createUpdateIntent(items.filter(item => item.group === "Plugins")), "1.0.0", items), { kind: "plugin", id: "plugin" });
console.log("update panel grouping and persisted ordering: 7 assertions passed");

import { availabilityIntent, hasUpdateBadge, panelItems, updateableItems } from "./updatePanelModel";
import type { UpdateAvailabilityItem } from "./updateAvailability";
const plugin: UpdateAvailabilityItem = { id: "plugin", kind: "plugin", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", channel: "official", reason: "newer" };
assert.equal(hasUpdateBadge(plugin), true);
assert.equal(hasUpdateBadge({ ...plugin, latest: "1.0.0" }), false);
assert.equal(hasUpdateBadge({ ...plugin, compatible: false }), false);
assert.equal(hasUpdateBadge({ ...plugin, state: "core_silent" }), false);
assert.equal(hasUpdateBadge({ ...plugin, installed: null }), false);
assert.equal(panelItems([{ ...plugin, state: "core_silent" }]).length, 0);
const incompatible = { ...plugin, id: "future", compatible: false, state: "incompatible" as const };
const app: UpdateAvailabilityItem = { ...plugin, id: "app", kind: "app" };
const all = availabilityIntent([app, plugin, incompatible]);
assert.deepEqual(all.pluginIds, ["plugin", "future"]);
assert.deepEqual(nextUpdateStep(all, "2.0.0", panelItems([incompatible])), { kind: "complete" });
assert.deepEqual(nextUpdateStep({ ...all, pluginIds: ["future"] }, "2.0.0", panelItems([{ ...incompatible, compatible: true, state: "update_available" }])), { kind: "plugin", id: "future" });
console.log("availability badges, core exclusion, and post-restart compatibility: 9 assertions passed");

const staged: UpdateAvailabilityItem = { ...plugin, id: "frontend", kind: "frontend", installed: null, state: "ready_to_apply", reason: "staged_bundle" };
assert.equal(hasUpdateBadge(staged), true);
assert.equal(hasUpdateBadge({ ...staged, compatible: false }), false);
assert.equal(hasUpdateBadge({ ...staged, kind: "plugin" }), false);
assert.equal(panelItems([staged])[0].group, "Interface");
assert.deepEqual(nextUpdateStep(availabilityIntent([staged]), "1.0.0", panelItems([staged])), { kind: "interface" });
console.log("staged Interface availability: 5 assertions passed");

const failingChannelItem: UpdateAvailabilityItem = { ...plugin, id: "lark", channel: "community" };
const healthyChannelItem: UpdateAvailabilityItem = { ...plugin, id: "teams-client", channel: "codeterm-plugins" };
const coreItem: UpdateAvailabilityItem = { ...plugin, id: "claude", state: "core_silent", reason: "local_ahead", channel: "codeterm-core" };
const channelErrors = {
  community: "Plugin channel unavailable: network unavailable (example.com/community.git). Domios will retry.",
  "codeterm-core": "Plugin channel unavailable: repository not found or not readable without access (github.com/rollacode/codeterm.git). Domios will retry.",
};
const channelRows = panelItems([failingChannelItem, healthyChannelItem, coreItem], channelErrors);
assert.equal(channelRows.find(item => item.id === "lark")?.channelError, channelErrors.community);
assert.equal(channelRows.find(item => item.id === "teams-client")?.channelError, undefined);
assert.equal(channelRows.find(item => item.id === "claude"), undefined);
assert.deepEqual(availabilityIntent(updateableItems([failingChannelItem, healthyChannelItem, coreItem], channelErrors)).pluginIds, ["teams-client"]);
console.log("failed channel isolation with one healthy channel and a silent core channel: 4 assertions passed");


import { targetInstalled, updateRunRows } from "./updatePanelModel";
const offered = panelItems([plugin, { ...plugin, id: "second" }]);
const bulk = createUpdateIntent(offered);
const external = [plugin, { ...plugin, id: "second" }].map(item => ({ ...item, installed: "2.0.0", state: "up_to_date" as const, reason: "same_version" }));
assert.equal(targetInstalled(offered[0], external), true);
assert.deepEqual(updateRunRows(offered, [], external, bulk, "plugin", {}).map(row => [row.state, row.alreadyCurrent]), [["completed", true], ["completed", true]]);
assert.deepEqual(nextUpdateStep(bulk, "1.0.0", panelItems(external)), { kind: "complete" });
assert.deepEqual(updateRunRows(offered, [], external, null, null, {}).map(row => row.state), ["completed", "completed"]);
assert.deepEqual(updateRunRows(offered, offered, [plugin, { ...plugin, id: "second" }], bulk, "plugin", {}).map(row => row.state), ["active", "queued"]);
console.log("concurrent external updates reach terminal states: 5 assertions passed");

const newerOffer = [{ ...offered[0], currentVersion: "2.0.0", targetVersion: "3.0.0" }];
assert.equal(updateRunRows([offered[0]], newerOffer, external, null, null, { plugin: "completed" })[0].state, "available");
console.log("later offers remain actionable after a completed run: 1 assertion passed");

{
  const row = { id: "git", name: "git", group: "Plugins" as const, currentVersion: "0.1.6", targetVersion: "0.1.7" };
  assert.deepEqual(updateRunRows([row], [], [], null, null, {}), []);
  const history = updateRunRows([row], [], [], null, null, { git: "failed" });
  assert.equal(history[0].state, "failed");
}
