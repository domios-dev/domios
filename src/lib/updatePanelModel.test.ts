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
