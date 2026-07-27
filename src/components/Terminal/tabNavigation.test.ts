import assert from "node:assert/strict";
import test from "node:test";
import type { Workspace } from "../../lib/types";
import { resolveTabNavigationTarget } from "./tabNavigation";

const workspaces: Workspace[] = [
  {
    id: "workspace-a",
    name: "A",
    tabs: [{ id: "tab-a", tab_id: "58ffb596-full" }],
    active_tab_id: null,
    cwd: null,
  },
  {
    id: "workspace-b",
    name: "B",
    tabs: [{ id: "tab-b", tab_id: "other-tab" }],
    active_tab_id: null,
    cwd: null,
  },
];

test("resolves an exact Tab to its owning workspace", () => {
  assert.deepEqual(
    resolveTabNavigationTarget("other-tab", ["58ffb596-full", "other-tab"], workspaces),
    { tabId: "other-tab", workspaceId: "workspace-b" },
  );
});

test("resolves a unique short ref to its owning workspace", () => {
  assert.deepEqual(
    resolveTabNavigationTarget("58ffb596", ["58ffb596-full", "other-tab"], workspaces),
    { tabId: "58ffb596-full", workspaceId: "workspace-a" },
  );
});

test("rejects missing, ambiguous, and detached Tab refs", () => {
  assert.equal(resolveTabNavigationTarget("missing", ["58ffb596-full"], workspaces), null);
  assert.equal(
    resolveTabNavigationTarget("58", ["58ffb596-full", "58-second"], workspaces),
    null,
  );
  assert.equal(resolveTabNavigationTarget("detached", ["detached"], workspaces), null);
});
