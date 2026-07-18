import assert from "node:assert/strict";
import { Window } from "happy-dom";
import {
  defaultWorkspaceInitials,
  loadWorkspaceInitials,
  loadSelectedWorkspaceId,
  normalizeWorkspaceInitials,
  persistWorkspaceInitials,
  persistSelectedWorkspaceId,
} from "./workspaceCardMetadata";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });

assert.equal(defaultWorkspaceInitials("CodeTerm"), "CT");
assert.equal(defaultWorkspaceInitials("delivery_codex_lane"), "DCL");
assert.equal(defaultWorkspaceInitials("team-smoke"), "TS");
assert.equal(defaultWorkspaceInitials("backend"), "BAC");
assert.equal(defaultWorkspaceInitials("Проект Два"), "ПД");
assert.equal(normalizeWorkspaceInitials(" a!3 "), "A3");

persistWorkspaceInitials({ ws1: "xy", ws2: "too long" });
assert.deepEqual(loadWorkspaceInitials(), { ws1: "XY", ws2: "TOO" });
persistSelectedWorkspaceId("ws2");
assert.equal(loadSelectedWorkspaceId(), "ws2");

console.log("workspace card metadata tests: passed");
