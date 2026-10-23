import assert from "node:assert/strict";
import { test } from "node:test";
import { tabCloseRejectionNotice, tabCloseRejectionReason } from "./tabCloseRejection";

test("refused closes map to typed reasons", () => {
  assert.equal(tabCloseRejectionReason(new Error("Cannot close protected Tab")), "protected");
  assert.equal(tabCloseRejectionReason("Tab close reservation lost"), "reservation_lost");
  assert.equal(tabCloseRejectionReason("Tab is closing: t1"), "reservation_lost");
  assert.equal(tabCloseRejectionReason("Workspace is closing: w1"), "membership_locked");
  assert.equal(tabCloseRejectionReason("opaque-fragment"), "unknown");
});

test("a refused close produces one error toast bound to the tab", () => {
  const protectedNotice = tabCloseRejectionNotice(new Error("Cannot close protected Tab"), "w1", "t1");
  assert.equal(protectedNotice.kind, "toast");
  assert.equal(protectedNotice.severity, "Error");
  assert.equal(protectedNotice.tabId, "t1");
  assert.equal(protectedNotice.workspaceId, "w1");
  assert.equal(protectedNotice.id, tabCloseRejectionNotice("again", "w1", "t1").id);
  assert.notEqual(protectedNotice.detail, tabCloseRejectionNotice("Workspace is closing: w1", "w1", "t1").detail);
  assert.ok(tabCloseRejectionNotice("opaque-fragment", "w1", "t1").detail?.includes("opaque-fragment"));
});
