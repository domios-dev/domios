import assert from "node:assert/strict";
import { activateReturnToChat, returnToOrigin, type ReturnToChatContext, type ReturnToRoute } from "./returnTo";

const calls: unknown[] = [];
const context: ReturnToChatContext = {
  workspaces: [{ id: "origin-workspace", tabs: ["origin-tab"] }],
  closePanels: () => calls.push("close-panels"),
  setChatMode: (tabId) => calls.push(["chat", tabId]),
  setActiveTab: (workspaceId, tabId) => calls.push(["active", workspaceId, tabId]),
  setFocusedTab: (tabId) => calls.push(["focus", tabId]),
};
const origin = { workspaceId: "origin-workspace", tabId: "origin-tab" };
const route: ReturnToRoute = { returnTo: origin };
const activate = (target: typeof origin) => activateReturnToChat(target, context);
let normalBacks = 0;
const back = () => { if (!returnToOrigin(route, activate)) normalBacks++; };
back();
assert.deepEqual(calls, ["close-panels", ["chat", "origin-tab"], ["active", "origin-workspace", "origin-tab"], ["focus", "origin-tab"]]);
assert.equal(route.returnTo, undefined);
back();
assert.equal(normalBacks, 1);
assert.equal(calls.length, 4);
assert.equal(returnToOrigin({}, activate), false);
assert.equal(calls.length, 4);

for (const target of [{ ...origin, tabId: "closed-tab" }, { ...origin, workspaceId: "closed-workspace" }]) {
  const missing: ReturnToRoute = { returnTo: target };
  assert.equal(returnToOrigin(missing, activate), false);
  assert.equal(missing.returnTo, undefined);
  assert.equal(calls.length, 4);
}

const reentrant: ReturnToRoute = { returnTo: origin };
assert.equal(returnToOrigin(reentrant, () => {
  assert.equal(returnToOrigin(reentrant, activate), false);
  return true;
}), true);
console.log("Return navigation consumes the origin once and falls back without trapping Back");
