import assert from "node:assert/strict";
import test from "node:test";
import type { Workspace } from "../../lib/types";
import { resolveTabNavigationTarget } from "./tabNavigation";

const workspaces: Workspace[] = [
  {
    id: "workspace-a",
    name: "A",
    tabs: [{ id: "tab-a", pane_id: "58ffb596-full" }],
    active_tab_id: null,
    cwd: null,
  },
  {
    id: "workspace-b",
    name: "B",
    tabs: [{ id: "tab-b", pane_id: "other-pane" }],
    active_tab_id: null,
    cwd: null,
  },
];

test("resolves an exact pane to its owning workspace", () => {
  assert.deepEqual(
    resolveTabNavigationTarget("other-pane", ["58ffb596-full", "other-pane"], workspaces),
    { paneId: "other-pane", workspaceId: "workspace-b" },
  );
});

test("resolves a unique short ref to its owning workspace", () => {
  assert.deepEqual(
    resolveTabNavigationTarget("58ffb596", ["58ffb596-full", "other-pane"], workspaces),
    { paneId: "58ffb596-full", workspaceId: "workspace-a" },
  );
});

test("rejects missing, ambiguous, and detached pane refs", () => {
  assert.equal(resolveTabNavigationTarget("missing", ["58ffb596-full"], workspaces), null);
  assert.equal(
    resolveTabNavigationTarget("58", ["58ffb596-full", "58-second"], workspaces),
    null,
  );
  assert.equal(resolveTabNavigationTarget("detached", ["detached"], workspaces), null);
});
