import assert from "node:assert/strict";
import { appIntentSatisfied, createUpdateIntent, restoredAppIntentInterrupted, groupUpdates, nextUpdateStep, readUpdateIntent, type UpdateItem } from "./updatePanelModel";

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

import { availabilityIntent, hasUpdateBadge, panelItems, updateableItems, updateSourceMessage } from "./updatePanelModel";
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

{
  const unavailable = panelItems([plugin], { official: "opaque channel error" });
  const installed = [{ ...plugin, installed: plugin.latest }];
  assert.equal(updateRunRows(unavailable, unavailable, installed, null, null, {})[0].state, "unavailable");
}

import { appIntentFinished, rowSourceKey, summarizeUpdatePanel, unavailableUpdateSources, updateSourceLabel, type UpdateRow } from "./updatePanelModel";
{
  const row = (id: string, state: UpdateRow["state"], channelError?: string): UpdateRow =>
    ({ id, name: id, group: "Plugins", currentVersion: "1.0.0", targetVersion: "2.0.0", state, alreadyCurrent: false, channelError });
  const idle = { busy: false, failed: false };
  assert.deepEqual(summarizeUpdatePanel([row("a", "available"), row("b", "available")], idle),
    { tone: "accent", label: "Updates available", sentence: "2 updates available" });
  assert.equal(summarizeUpdatePanel([row("a", "completed"), row("b", "active"), row("c", "queued")], { busy: true, failed: false }).sentence, "Updating 2 of 3");
  assert.equal(summarizeUpdatePanel([], { busy: true, failed: false }).sentence, "Updating components");
  const failed = summarizeUpdatePanel([row("a", "failed"), row("b", "unavailable", "opaque")], { busy: false, failed: true });
  assert.deepEqual([failed.tone, failed.label, failed.sentence], ["red", "Update failed", "1 update failed"]);
  const sourceDown = summarizeUpdatePanel([row("b", "unavailable", "opaque")], { busy: false, failed: false, unavailableSources: ["official plugin channel"] });
  assert.deepEqual([sourceDown.tone, sourceDown.label, sourceDown.sentence], ["warning", "Update source unavailable", "official plugin channel unavailable"]);
  assert.equal(summarizeUpdatePanel([], { busy: false, failed: false, unavailableSources: ["a", "b"] }).sentence, "2 update sources unavailable");
  assert.equal(summarizeUpdatePanel([row("a", "available"), row("b", "unavailable", "opaque")], { busy: false, failed: false, unavailableSources: ["b"] }).sentence, "1 update available");
  assert.equal(summarizeUpdatePanel([row("a", "completed")], idle).tone, "green");
  assert.equal(summarizeUpdatePanel([], idle).sentence, "Everything is up to date");
  assert.equal(summarizeUpdatePanel([row("a", "available", "opaque")], idle).label, "No applicable updates");
}
console.log("update panel summary: passed");

// The owner's 1.19.0 popover: cached same-version check, then the feed failed.
{
  const feedDown = { app: "Failed to fetch release: domios-dev/domios: manifest fetch returned HTTP 504" };
  const release: UpdateAvailabilityItem = { id: "app", kind: "app", installed: "1.19.0", latest: "1.19.0", compatible: true, state: "up_to_date", reason: "cached_check", channel: "official" };

  // Same version plus a stale marker: no target, nothing to apply, nothing to restart for.
  assert.equal(hasUpdateBadge({ ...release, state: "update_available" }), false);
  assert.equal(hasUpdateBadge({ ...release, kind: "frontend", state: "ready_to_apply" }), false);
  assert.equal(hasUpdateBadge({ ...release, latest: "1.18.9", state: "update_available" }), false);
  assert.equal(hasUpdateBadge({ ...release, latest: "1.19.0+rebuilt", state: "update_available" }), false);
  assert.equal(panelItems([{ ...release, state: "update_available" }]).length, 0);
  assert.equal(appIntentFinished({ appTarget: "1.19.0", stage: "app", pluginIds: [], interfaceRequested: false }, "1.19.0"), true);
  assert.equal(appIntentFinished({ appTarget: "1.18.9", stage: "app", pluginIds: [], interfaceRequested: false }, "1.19.0"), true);
  assert.equal(appIntentFinished({ appTarget: "1.19.1", stage: "app", pluginIds: [], interfaceRequested: false }, "1.19.0"), false);
  assert.equal(appIntentFinished({ appTarget: "1.19.0", stage: "app", pluginIds: ["git"], interfaceRequested: false }, "1.19.0"), false);
  assert.equal(appIntentFinished({ appTarget: "1.19.0", stage: "app", pluginIds: [], interfaceRequested: true }, "1.19.0"), false);
  assert.equal(appIntentFinished({ appTarget: "1.19.0", stage: "app", pluginIds: [], interfaceRequested: false }, ""), false);

  // Source failure with nothing staged: the row keeps its last-known state and the named source.
  const [row] = panelItems([release], feedDown);
  assert.equal(row.currentVersion, "1.19.0");
  assert.equal(row.targetVersion, null);
  assert.equal(row.channelError, feedDown.app);
  assert.equal(row.lastKnown, "Up to date at last check");
  assert.equal(updateSourceLabel(rowSourceKey(row)), "App update channel");
  assert.equal(updateSourceMessage(feedDown.app), "domios-dev/domios: manifest fetch returned HTTP 504");
  assert.deepEqual(updateRunRows([], [row], [release], null, null, {}).map(run => run.state), ["unavailable"]);
  assert.deepEqual(updateableItems([release], feedDown), []);

  // Never checked: the target is not borrowed from the running version.
  const [unchecked] = panelItems([{ ...release, latest: null, reason: "not_checked" }], { app: "offline" });
  assert.equal(unchecked.targetVersion, null);
  assert.equal(unchecked.lastKnown, "Not checked yet");

  // A cached manifest older than the running version never reads as a downgrade offer.
  const [older] = panelItems([{ ...release, latest: "1.18.9", reason: "cached_check" }], feedDown);
  assert.equal(older.targetVersion, null);
  assert.equal(older.lastKnown, "Up to date at last check");

  // A newer manifest for another platform is reported, not offered.
  const [foreign] = panelItems([{ ...release, latest: "1.20.0", compatible: false, state: "incompatible", reason: "cached_check" }], feedDown);
  assert.equal(foreign.targetVersion, null);
  assert.equal(foreign.lastKnown, "1.20.0 is not compatible with this install");

  // Source failure plus a staged newer update: the cached offer stays real and actionable.
  const cachedOffer: UpdateAvailabilityItem = { ...release, latest: "1.19.1", state: "update_available", reason: "cached_manifest" };
  const [staged] = panelItems([cachedOffer], feedDown);
  assert.equal(staged.targetVersion, "1.19.1");
  assert.equal(staged.channelError, undefined);
  assert.equal(staged.sourceWarning, feedDown.app);
  assert.equal(staged.lastKnown, undefined);
  assert.deepEqual(updateableItems([cachedOffer], feedDown), [cachedOffer]);
  assert.deepEqual(updateRunRows([], [staged], [cachedOffer], null, null, {}).map(run => run.state), ["available"]);

  // A plugin channel failure keeps a real last-known offer but cannot apply it.
  const pluginDown = { community: "Plugin channel unavailable: network unavailable (example.com/community.git). Domios will retry." };
  const [channelRow] = panelItems([{ ...plugin, id: "lark", channel: "community" }], pluginDown);
  assert.equal(channelRow.targetVersion, "2.0.0");
  assert.equal(channelRow.lastKnown, "Found at last check");
  assert.equal(updateSourceLabel(rowSourceKey(channelRow)), "community plugin channel");
  assert.equal(updateSourceMessage(pluginDown.community), "network unavailable (example.com/community.git).");
  const [samePlugin] = panelItems([{ ...plugin, id: "lark", channel: "community", latest: "1.0.0", state: "up_to_date" }], pluginDown);
  assert.equal(samePlugin.targetVersion, null);
  assert.equal(samePlugin.lastKnown, "Up to date at last check");

  // Each source is named once, whether it comes from rows or diagnostics.
  const runRow = (over: Partial<UpdateRow>): UpdateRow => ({ ...channelRow, state: "unavailable", alreadyCurrent: false, ...over });
  assert.deepEqual(unavailableUpdateSources(
    [runRow({ id: "lark" }), runRow({ id: "second" }), { ...runRow({ id: "app", group: "App", channel: "official" }), channelError: "HTTP 504" }, runRow({ id: "ok", channelError: undefined, state: "available" })],
    [{ channel: "community", message: "x" }, { channel: "store", message: "y" }, { channel: "codeterm-core", message: "z" }, { channel: "discovery", message: "w" }],
  ), ["community plugin channel", "App update channel", "store plugin channel", "Update discovery"]);
  assert.deepEqual(unavailableUpdateSources([], []), []);
}
console.log("Same version, source failure and staged update combinations: passed");

const sourceFailure = { app: "GitHub rate limit, retry at 14:30" };
const uncheckedApp: UpdateAvailabilityItem = { ...app, latest: null, state: "up_to_date", reason: "not_checked" };
assert.equal(panelItems([uncheckedApp], sourceFailure)[0].channelError, sourceFailure.app);
assert.deepEqual(updateableItems([uncheckedApp], sourceFailure), []);
assert.equal(panelItems([app], sourceFailure)[0].channelError, sourceFailure.app);
assert.deepEqual(updateableItems([app], sourceFailure), []);
const cachedApp: UpdateAvailabilityItem = { ...app, reason: "cached_manifest" };
assert.equal(panelItems([cachedApp], sourceFailure)[0].channelError, undefined);
assert.deepEqual(updateableItems([cachedApp], sourceFailure), [cachedApp]);

assert.equal(panelItems([cachedApp], sourceFailure)[0].sourceWarning, sourceFailure.app);
assert.equal(updateSourceMessage("Failed to fetch release: source: GitHub rate limit, retry at 14:30; mirror: access denied"), "GitHub rate limit, retry at 14:30");
assert.equal(updateSourceMessage("offline"), "offline");

assert.equal(restoredAppIntentInterrupted(intent, "1.0.0", { observed: false, phase: "idle", installing: false }), false);
assert.equal(restoredAppIntentInterrupted(intent, "1.0.0", { observed: true, phase: "idle", installing: false }), true);
assert.equal(restoredAppIntentInterrupted(intent, "1.0.0", { observed: true, phase: "idle", installing: true }), false);
assert.equal(restoredAppIntentInterrupted(intent, "2.0.0", { observed: true, phase: "idle", installing: false }), false);
for (const phase of ["checking", "downloading", "applying", "success", "error"]) {
  assert.equal(restoredAppIntentInterrupted(intent, "1.0.0", { observed: true, phase, installing: false }), false);
}
assert.equal(restoredAppIntentInterrupted({ ...intent, stage: "plugins" }, "1.0.0", { observed: true, phase: "idle", installing: false }), false);
assert.equal(restoredAppIntentInterrupted({ ...intent, appTarget: null }, "1.0.0", { observed: true, phase: "idle", installing: false }), false);
console.log("Restored app intent reconciliation: 11 decisions passed; initial idle/current request/active native work/installed target never interrupt.");

{
  const idle = { observed: true, phase: "idle", installing: false };
  const cases = [
    ["1.18.2", "1.18.2", true, false],
    ["1.18.3", "1.18.2", true, false],
    ["1.18.10", "1.18.9", true, false],
    ["1.19.0", "1.18.10", true, false],
    ["2.0.0", "1.99.0", true, false],
    ["1.18.1", "1.18.2", false, true],
    ["1.18.2-rc.2", "1.18.2-rc.10", false, true],
    ["1.18.2", "1.18.2-rc.10", true, false],
    ["1.18.2-rc.1", "1.18.2", false, true],
    ["1.18.2+new", "1.18.2+old", true, false],
    ["unknown", "1.18.2", false, false],
    ["1.18.3", "invalid", false, false],
    ["", "1.18.2", false, false],
    ["1.18.2-01", "1.18.2", false, false],
  ] as const;
  for (const [hostVersion, appTarget, satisfied, interrupted] of cases) {
    const saved = { ...intent, appTarget };
    assert.equal(appIntentSatisfied(saved, hostVersion), satisfied, `${hostVersion} satisfies ${appTarget}`);
    assert.equal(restoredAppIntentInterrupted(saved, hostVersion, idle), interrupted, `${hostVersion} interrupted ${appTarget}`);
    assert.deepEqual(nextUpdateStep(saved, hostVersion, []), { kind: satisfied ? "complete" : "waitForApp" });
  }
  assert.deepEqual(nextUpdateStep(intent, "3.0.0", items), { kind: "plugin", id: "plugin" });
}
console.log("Semantic app target reconciliation: 43 assertions passed");

import { appendUpdateFailure } from "./updatePanelModel";
{
  const failure = { message: "opaque", details: "cause", channelUnavailable: false };
  assert.deepEqual(appendUpdateFailure([failure, failure], failure), [failure]);
  let history = [failure];
  for (let i = 0; i < 20; i++) history = appendUpdateFailure(history, { ...failure, details: String(i) });
  assert.equal(history.length, 10);
  assert.equal(history[0].details, "10");
  assert.equal(history[9].details, "19");
  const rows = updateRunRows(offered, [], external, null, null, { plugin: "failed" });
  assert.deepEqual(rows.map(row => row.state), ["completed", "completed"]);
  assert.equal(summarizeUpdatePanel(rows, { busy: false, failed: false }).tone, "green");
  assert.equal(summarizeUpdatePanel(updateRunRows(offered, offered, [plugin], null, null, { plugin: "failed" }),
    { busy: false, failed: true }).tone, "red");
}
console.log("Superseded failure headers and deduplicated bounded history: 7 assertions passed");
