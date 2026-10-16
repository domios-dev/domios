import assert from "node:assert/strict";
import { chatGeometryVisible, selectChatGeometrySurface, type ChatGeometrySurface } from "./chatGeometrySurface";

const shown = { hidden: false, inert: false, ariaHidden: false, display: "block", visibility: "visible", opacity: 1 };
const selected: ChatGeometrySurface = {
  tabId: "selected", visible: chatGeometryVisible(1000, 800, [shown]), snapshotReady: true, busy: false,
};
const retained: ChatGeometrySurface = {
  tabId: "retained", visible: chatGeometryVisible(1000, 800, [shown, { ...shown, visibility: "hidden" }]),
  snapshotReady: false, busy: true,
};
assert.equal(selectChatGeometrySurface([retained, selected]), 1);
assert.equal(selectChatGeometrySurface([{ ...retained, snapshotReady: true, busy: false }, selected]), 1);
assert.equal(selectChatGeometrySurface([{ ...selected, snapshotReady: false }, selected]), 1);
assert.equal(selectChatGeometrySurface([{ ...selected, busy: true }, selected]), 1);
assert.equal(selectChatGeometrySurface([retained]), null);
assert.equal(selectChatGeometrySurface([selected, { ...selected, tabId: "other" }]), null);
assert.equal(selectChatGeometrySurface([{ ...selected, tabId: null }]), null);
for (const ancestor of [
  { ...shown, hidden: true }, { ...shown, inert: true }, { ...shown, ariaHidden: true },
  { ...shown, display: "none" }, { ...shown, visibility: "collapse" }, { ...shown, opacity: 0 },
]) assert.equal(chatGeometryVisible(1000, 800, [shown, ancestor]), false);
assert.equal(chatGeometryVisible(0, 800, [shown]), false);
assert.equal(chatGeometryVisible(1000, 0, [shown]), false);
