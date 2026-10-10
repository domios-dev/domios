import assert from "node:assert/strict";
import { selectAiHelperTab } from "./aiHelperFocus";

const calls: string[] = [];
selectAiHelperTab("workspace-general", "tab-helper", (workspaceId, tabId) => calls.push(`select:${workspaceId}:${tabId}`), (tabId) => calls.push(`focus:${tabId}`));
assert.deepEqual(calls, ["select:workspace-general:tab-helper", "focus:tab-helper"]);
console.log("AI helper selection uses the returned workspace and tab before provider readiness");
