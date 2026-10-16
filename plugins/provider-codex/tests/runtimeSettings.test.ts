import assert from "node:assert/strict";
import { codexEffectiveSettings } from "../src/runtimeSettings";

assert.equal(codexEffectiveSettings({ model: "fixture" }, { serviceTier: "priority" }).fastMode, true);
assert.equal(codexEffectiveSettings({}, { thread: { serviceTier: "priority" } }).fastMode, true);
assert.equal(codexEffectiveSettings({ fastMode: true }, { serviceTier: null }).fastMode, false);
assert.equal(codexEffectiveSettings({ fastMode: true }, {}).fastMode, undefined);
console.log("runtime tier confirmation: 4 assertions passed");
