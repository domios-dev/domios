import { pluginViewBelongsToActiveTab, pluginViewScope } from "./pluginViewScope";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const active = pluginViewScope("workspace-a", { "workspace-a": "tab-a" });
assert(active?.tabId === "tab-a", "active tab scope should be captured");
assert(pluginViewBelongsToActiveTab(active, active), "view remains open on its owner tab");
assert(
  !pluginViewBelongsToActiveTab(active, pluginViewScope("workspace-a", { "workspace-a": "tab-b" })),
  "view closes when another tab becomes active",
);
assert(
  !pluginViewBelongsToActiveTab(active, pluginViewScope("workspace-b", { "workspace-b": "tab-a" })),
  "view closes when another workspace becomes active",
);
assert(pluginViewBelongsToActiveTab(null, active), "unscoped embedded views are unaffected");

console.log("pluginViewScope tests: 4/4 passed");
