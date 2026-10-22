import assert from "node:assert/strict";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../../components/Canvas/cards/fixtures.testdata";
import { fixtureGraph, fixtureNode } from "../../components/Automation/fixtures.testdata";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { createMentionHost } from "./mentionHost";
import { createValueFeed } from "./valueFeed";
import type { Plan } from "../../components/Canvas/types";

async function main() {
  let state = { tabs: { self: { id: "self", title: "Real name", tab_type: "Terminal", pty_id: null, cwd: "/repo", workspace_id: "ws", team_id: "own" } },
    workspaces: [{ id: "ws", name: "Repo", cwd: "/repo", tabs: ["self"], active_tab_id: "self" }], teams: [], active_workspace_id: "ws", focused_tab_id: "self", notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [] } as ExtendedState;
  const mux = createMuxStateStore(() => state);
  const plan = (id: string, teamId: string | null, project = "/repo"): Plan => ({ ...FIXTURE_PLAN, id, team_id: teamId, project_path: project, tasks: [{ ...FIXTURE_TASK, id: `task_${id}`, key: "FRG-1447" }] });
  const feed = createValueFeed<readonly Plan[]>([plan("pl_other", "other"), plan("pl_own", "own"), plan("pl_far", null, "/other")]);
  const opened: unknown[] = [];
  let reads = 0;
  let blueprintDeleted = false;
  let muxSubscriptions = 0;
  let planSubscriptions = 0;
  let muxReads = 0;
  const host = createMentionHost({ mux: {
    getState: () => { muxReads++; return mux.getState(); },
    subscribe: (listener) => { muxSubscriptions++; const off = mux.subscribe(listener); return () => { muxSubscriptions--; off(); }; },
  }, plans: { get: feed.get, set: feed.set, subscribe: (listener) => { planSubscriptions++; const off = feed.subscribe(listener); return () => { planSubscriptions--; off(); }; } },
    readGraph: async (scope) => { reads++; assert.equal(scope.teamId, "own"); return { ...fixtureGraph(), nodes: blueprintDeleted ? [] : [fixtureNode({ id: "bp_real", kind: "blueprint", owner_tab_id: null, workspace_id: "ws", team_id: "own" }, "Real Blueprint"), fixtureNode({ id: "bp_workspace", kind: "blueprint", owner_tab_id: null, workspace_id: "ws", team_id: null }, "Workspace Blueprint")] }; },
    openPlan: (target) => opened.push(target), openTab: (ws, tab) => opened.push([ws, tab]), openBlueprint: (ws, team, id) => opened.push([ws, team, id]),
  });
  const key = (kind: string, id: string) => JSON.stringify(["self", kind, id]);
  const task = key("task", "frg-1447");
  assert.equal(host.binding.read(task)?.planId, "pl_own");
  host.open(task);
  assert.deepEqual(opened.pop(), { planId: "pl_own", taskId: "task_pl_own", teamId: "own" });
  assert.equal(host.binding.read(key("plan", "pl_other")), null);
  host.open(key("task", "task_pl_other"));
  assert.deepEqual(opened.pop(), { planId: "pl_other", taskId: "task_pl_other", teamId: "other" });
  const tab = key("tab", "self");
  assert.equal(host.binding.read(tab)?.title, "Real name");
  let notifications = 0;
  const offActivity = host.binding.subscribe(() => notifications++);
  const extraListeners = Array.from({ length: 100 }, () => host.binding.subscribe(() => {}));
  assert.equal(muxSubscriptions, 1);
  assert.equal(planSubscriptions, 1);
  const beforeActivity = muxReads;
  state = { ...state, tabs: { self: { ...state.tabs.self, agent_state: { state: "working" } as never } } }; mux.notify();
  assert.equal(muxReads - beforeActivity, 1, "one mux projection for all chips on activity");
  assert.equal(notifications, 0, "rendered chips also ignore activity-only events");
  state = { ...state, tabs: { self: { ...state.tabs.self, title: "Renamed" } } }; mux.notify();
  assert.equal(notifications, 1);
  for (const off of extraListeners) off();
  offActivity();
  assert.equal(muxSubscriptions, 0);
  assert.equal(planSubscriptions, 0);
  assert.equal(host.binding.read(tab)?.title, "Renamed");
  host.open(tab); assert.deepEqual(opened.pop(), ["ws", "self"]);
  const blueprint = key("blueprint", "bp_real");
  const keepMounted = host.binding.subscribe(() => {});
  const ready = new Promise<void>((resolve) => { const off = host.binding.subscribe(() => { if (host.binding.read(blueprint)?.state !== "loading") { off(); resolve(); } }); });
  host.binding.refresh(blueprint); host.binding.refresh(blueprint);
  assert.equal(host.binding.read(blueprint)?.state, "loading");
  await ready;
  assert.equal(reads, 1);
  assert.equal(host.binding.read(blueprint)?.title, "Real Blueprint");
  host.open(blueprint); assert.deepEqual(opened.pop(), ["ws", "own", "bp_real"]);
  host.open(key("blueprint", "bp_workspace")); assert.deepEqual(opened.pop(), ["ws", null, "bp_workspace"]);
  keepMounted();
  blueprintDeleted = true;
  assert.equal(host.binding.read(blueprint), null, "unmounted graph cache is discarded");
  const offRemount = host.binding.subscribe(() => {});
  const refreshed = new Promise<void>((resolve) => { const off = host.binding.subscribe(() => { if (host.binding.read(blueprint)?.state !== "loading") { off(); resolve(); } }); });
  host.binding.refresh(blueprint);
  await refreshed;
  assert.equal(reads, 2, "remount reads a fresh graph");
  assert.equal(host.binding.read(blueprint), null, "deletion while unmounted removes the old label");
  host.open(blueprint); assert.equal(opened.length, 0, "deleted Blueprint cannot open");
  offRemount();
  let taskReads = 0;
  const indexedPlan = plan("pl_indexed", "own");
  const tasks = indexedPlan.tasks;
  Object.defineProperty(indexedPlan, "tasks", { get: () => { taskReads++; return tasks; } });
  feed.set([indexedPlan]);
  for (let i = 0; i < 100; i++) host.binding.read(key("task", `SHA-${i}`));
  assert.equal(taskReads, 1, "unknown prose keys share one scoped task index");
  feed.set([{ ...indexedPlan, tasks: [{ ...tasks[0], name: "Updated" }] }]);
  assert(host.binding.read(task)?.title.includes("Updated"), "plan changes invalidate the index");
  state = { ...state, tabs: { self: { ...state.tabs.self, team_id: null } } }; mux.notify();
  assert.equal(host.binding.read(task), null); host.open(task); assert.equal(opened.length, 0);
  feed.set([plan("pl_other", "other"), plan("pl_solo", null), plan("pl_far", null, "/other")]);
  assert.equal(host.binding.read(task)?.planId, "pl_solo");
  feed.set([plan("pl_solo", null), plan("pl_collision", null)]);
  assert.equal(host.binding.read(task), null, "ambiguous keys never pick the first plan");
  host.open(task); assert.equal(opened.length, 0);
  console.log("8 scoped live-label, key-collision and exact Blueprint navigation checks passed");
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
