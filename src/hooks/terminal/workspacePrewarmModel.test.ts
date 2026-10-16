import assert from "node:assert/strict";
import { test } from "node:test";
import type { Workspace } from "../../lib/types";
import { workspacePrewarmInput } from "./workspacePrewarmModel";

const workspace: Workspace = { id: "workspace", name: "Project", cwd: "/project", tabs: ["active", "main", "other", "missing"], active_tab_id: "active", protected: true };
const tabs = { active: {}, main: { protected: true }, other: {} };

test("a missing folder disables all prewarm inputs including the active and protected tabs", () => {
  assert.deepEqual(workspacePrewarmInput({ ...workspace, cwd_missing: true }, "active", tabs), { workspaceId: null, activeTabId: null, tabIds: [] });
  assert.deepEqual(workspacePrewarmInput(null, null, tabs), { workspaceId: null, activeTabId: null, tabIds: [] });
});

test("an available workspace selects its live tabs and excludes its protected main tab", () => {
  assert.deepEqual(workspacePrewarmInput(workspace, "active", tabs), { workspaceId: "workspace", activeTabId: "active", tabIds: ["active", "other"] });
});
