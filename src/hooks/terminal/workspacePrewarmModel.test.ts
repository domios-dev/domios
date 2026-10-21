import assert from "node:assert/strict";
import { test } from "node:test";
import type { Workspace } from "../../lib/types";
import { needsBackendPrewarm, workspacePrewarmInput } from "./workspacePrewarmModel";

const workspace: Workspace = { id: "workspace", name: "Project", cwd: "/project", tabs: ["active", "main", "other", "missing"], active_tab_id: "active", protected: true };
const tabs = { active: {}, main: { protected: true }, other: {} };

test("a missing folder disables all prewarm inputs including the active and protected tabs", () => {
  assert.deepEqual(workspacePrewarmInput({ ...workspace, cwd_missing: true }, "active", tabs), { workspaceId: null, activeTabId: null, tabIds: [] });
  assert.deepEqual(workspacePrewarmInput(null, null, tabs), { workspaceId: null, activeTabId: null, tabIds: [] });
});

test("an available workspace selects its live tabs and excludes its protected main tab", () => {
  assert.deepEqual(workspacePrewarmInput(workspace, "active", tabs), { workspaceId: "workspace", activeTabId: "active", tabIds: ["active", "other"] });
});

test("prewarm wakes a terminal agent tab without a PTY and skips tabs whose backend is already up", () => {
  assert.equal(needsBackendPrewarm({ agent_type: "claude", pty_id: null }), true);
  assert.equal(needsBackendPrewarm({ agent_type: "claude", pty_id: "pty-1" }), false);
  assert.equal(needsBackendPrewarm({ agent_type: null, pty_id: null }), false);
  assert.equal(needsBackendPrewarm({ agent_type: "plugin", pty_id: null, backing: { kind: "chat_backend", plugin_id: "plugin", session_id: "s" } }), false);
  assert.equal(needsBackendPrewarm(undefined), false);
});
