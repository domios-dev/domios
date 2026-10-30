import assert from "node:assert/strict";
import test from "node:test";
import { resolveDevNotifyTarget } from "../dev-notify-target.mjs";

test("dev report target prefers an explicit tab over the inspected parent", () => {
  assert.deepEqual(resolveDevNotifyTarget({
    explicitTab: "chosen-tab",
    shellTab: "build-shell",
    parentHint: "stale-parent",
    tabModel: { result: [{ id: "build-shell", parent_tab_id: "live-parent" }] },
  }), { kind: "send", tab: "chosen-tab", mesh: "" });
});

test("dev report target returns to an agent tab itself, not to its parent", () => {
  assert.deepEqual(resolveDevNotifyTarget({
    shellTab: "worker-tab",
    tabModel: { result: [{ id: "worker-tab", agent_type: "claude", parent_tab_id: "orchestrator" }] },
  }), { kind: "send", tab: "worker-tab", mesh: "" });
  assert.deepEqual(resolveDevNotifyTarget({
    explicitTab: "orchestrator",
    shellTab: "worker-tab",
    tabModel: { result: [{ id: "worker-tab", agent_type: "claude", parent_tab_id: "orchestrator" }] },
  }), { kind: "send", tab: "orchestrator", mesh: "" });
});

test("dev report target uses the live parent before falling back to the shell", () => {
  assert.deepEqual(resolveDevNotifyTarget({
    shellTab: "build-shell",
    tabModel: { result: [{ id: "build-shell", parent_tab_id: "orchestrator" }] },
  }), { kind: "send", tab: "orchestrator", mesh: "" });

  assert.deepEqual(resolveDevNotifyTarget({
    shellTab: "standalone-shell",
    tabModel: { result: [{ id: "standalone-shell" }] },
  }), { kind: "notify", tab: "standalone-shell", mesh: "" });

  assert.deepEqual(resolveDevNotifyTarget({
    shellTab: "uninspected-shell",
    parentHint: "launch-parent",
    tabModel: { result: [] },
  }), { kind: "send", tab: "launch-parent", mesh: "" });
});

test("dev report target can find the current Team owner when the shell has no parent", () => {
  assert.deepEqual(resolveDevNotifyTarget({
    shellTab: "team-shell",
    tabModel: { result: [{ id: "team-shell", team_id: "team" }] },
    teamModel: { result: { members: [
      { tab_id: "worker", role_type: "Worker" },
      { tab_id: "current-owner", role_type: "Manager", anchors_team: true },
    ] } },
  }), { kind: "send", tab: "current-owner", mesh: "" });
});

test("dev report target follows a persisted mesh return route", () => {
  const tabModel = [{
    id: "remote-shell",
    parent_tab_id: "remote-parent",
    report_target_tab_id: "origin-owner",
    report_target_mesh_peer: "origin-instance-id",
  }];

  assert.deepEqual(resolveDevNotifyTarget({ shellTab: "remote-shell", tabModel: { result: tabModel } }), {
    kind: "send",
    tab: "origin-owner",
    mesh: "origin-instance-id",
  });
  assert.deepEqual(resolveDevNotifyTarget({
    explicitTab: "origin-owner",
    shellTab: "remote-shell",
    tabModel: { result: tabModel },
  }), { kind: "send", tab: "origin-owner", mesh: "origin-instance-id" });
});
