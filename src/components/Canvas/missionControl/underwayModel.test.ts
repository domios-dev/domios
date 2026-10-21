import assert from "node:assert/strict";
import { test } from "node:test";
import type { Tab } from "../../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { boundedChecklist } from "../../Sidebar/agentChecklistModel";
import { checklistProgress, underwayRows } from "./underwayModel";
import { boundedUnderwaySize } from "./useUnderwayResize";

function tab(id: string, profile = "worker", extra: Partial<Tab> = {}): Tab {
  return { id, title: id, role: "Worker", role_profile: profile, team_id: "team", has_checklist: true, agent_state: { state: "working" }, ...extra } as Tab;
}
function plan(tasks: Array<{ id: string; key: string; status: "in_progress" | "in_review" | "done" }>) {
  return { ...FIXTURE_PLAN, team_id: "team", tasks: tasks.map(({ id, ...values }) => ({ ...FIXTURE_TASK, id, assigned_tab_id: id, ...values })) };
}

test("working rows rank kinds then numeric task keys with stable identities", () => {
  const tabs = [tab("plain"), tab("review", "reviewer"), tab("qa10", "qa"), tab("release", "release_manager"), tab("qa2", "qa")];
  const plans = [plan(tabs.map(({ id }, index) => ({ id, key: id === "qa2" ? "COD-2" : `COD-${index + 8}`, status: "in_progress" })))];
  assert.deepEqual(underwayRows(tabs, plans, "team").map(({ tab }) => tab.id), ["release", "qa2", "qa10", "review", "plain"]);
});

test("every team agent that keeps a checklist is listed, whatever its task state", () => {
  const tabs = [tab("active", "worker", { agent_state: { state: "idle" } }), tab("review"), tab("done"), tab("free"), tab("idle", "worker", { agent_state: { state: "idle" } }), tab("foreign", "worker", { team_id: "other" }), tab("manager", "worker", { role: "Manager" }), tab("empty", "worker", { has_checklist: false })];
  assert.deepEqual(underwayRows(tabs, [plan([{ id: "active", key: "COD-1", status: "in_progress" }, { id: "review", key: "COD-2", status: "in_review" }, { id: "done", key: "COD-3", status: "done" }])], "team").map(({ tab }) => tab.id), ["free", "idle", "manager", "active", "review", "done"]);
});

test("current in-progress assignment wins over older completed assignments", () => {
  const old = plan([{ id: "agent", key: "COD-1", status: "done" }]);
  const current = plan([{ id: "agent", key: "COD-2", status: "in_progress" }]);
  assert.equal(underwayRows([tab("agent")], [old, current], "team")[0]?.task?.key, "COD-2");
});

test("bounded segments preserve checklist order and all five states, excluding skipped from total", () => {
  const states = ["done", "in_progress", "open", "blocked", "skipped"];
  const checklist = boundedChecklist({ tab_id: "agent", generation: 1, revision: 1, items: states.map((state, position) => ({ id: String(position), title: state, state, position })).reverse() });
  assert.deepEqual(checklist?.items.map((item) => item.state), states);
  assert.deepEqual(checklistProgress(checklist?.items ?? []), { done: 1, total: 4 });
  assert.deepEqual(checklistProgress([]), { done: 0, total: 0 });
});

test("resize stays within small bounded viewer preferences", () => {
  assert.deepEqual(boundedUnderwaySize({ width: 1, height: 10 }), { width: 320, height: 200 });
  assert.deepEqual(boundedUnderwaySize({ width: 9000, height: 9000 }), { width: 1600, height: 800 });
  assert.deepEqual(boundedUnderwaySize({ width: 900, height: 300 }, 700), { width: 700, height: 300 });
});
