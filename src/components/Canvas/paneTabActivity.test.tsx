import { resolvePaneTabActivity } from "./paneTabActivity";
import type { PlanIndexEntry } from "./plansApi";
import type { Pane } from "../../lib/types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkEntry(over: Partial<PlanIndexEntry> = {}): PlanIndexEntry {
  return {
    plan_id: "plan-1",
    file_path: "/tmp/plan-1.json",
    canonical_project_path: "/tmp",
    owner: { pane_id: "orch-1", session_id: "sess-orch", title: "Orchestrator" },
    title: "Canvas rendering substrate — Figma-grade migration",
    status: "active",
    updated_at: 100,
    assigned_pane_ids: [],
    task_count: 12,
    ...over,
  };
}

function mkPane(id: string, over: Partial<Pane> = {}): Pane {
  return { id, pty_id: null, pane_type: "Terminal", title: id, cwd: null, ...over } as Pane;
}

test("owner pane → orchestrator activity (plan title + task count)", () => {
  const index = [mkEntry({ assigned_pane_ids: ["w1"] })];
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch", can_spawn: true }) };
  const a = resolvePaneTabActivity(index, "orch-1", panes);
  assert(a?.kind === "orchestrator", `owner classified orchestrator (got ${a?.kind})`);
  assert(a.taskCount === 12, `task count from index (got ${a.taskCount})`);
  assert(a.planTitle.includes("Figma-grade"), "carries plan title for hover");
});

test("directly-assigned pane → worker activity", () => {
  const index = [mkEntry({ assigned_pane_ids: ["w1"] })];
  const panes = {
    "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }),
    w1: mkPane("w1", { parent_pane_id: "orch-1", agent_session_id: "sess-w1" }),
  };
  const a = resolvePaneTabActivity(index, "w1", panes);
  assert(a?.kind === "worker", `assigned pane classified worker (got ${a?.kind})`);
  assert(a.planId === "plan-1", "worker carries its plan id");
});

test("idle / unrelated pane → no activity (honest)", () => {
  const index = [mkEntry({ assigned_pane_ids: ["w1"] })];
  assert(resolvePaneTabActivity(index, "stranger", {}) === null, "unrelated pane shows nothing");
  assert(resolvePaneTabActivity([], "w1", {}) === null, "empty index → nothing");
});

test("worker nested under orchestrator but NOT assigned → no activity", () => {
  const index = [mkEntry({ assigned_pane_ids: [] })];
  const panes = {
    "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }),
    w1: mkPane("w1", { parent_pane_id: "orch-1", agent_session_id: "sess-w1" }),
  };
  assert(resolvePaneTabActivity(index, "w1", panes) === null, "nested-only pane stays bare");
});

test("session-id ownership survives a pane-id change", () => {
  const index = [mkEntry({ owner: { pane_id: "old-pane", session_id: "sess-orch", title: "Orch" } })];
  const panes = { "orch-2": mkPane("orch-2", { agent_session_id: "sess-orch" }) };
  assert(resolvePaneTabActivity(index, "orch-2", panes)?.kind === "orchestrator", "matches on own session id");
});

test("multi-plan owner counts plans and leads with the freshest", () => {
  const index = [
    mkEntry({ plan_id: "quiet", title: "Quiet", updated_at: 1, task_count: 3 }),
    mkEntry({ plan_id: "busy", title: "Busy", updated_at: 999, task_count: 7 }),
  ];
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }) };
  const a = resolvePaneTabActivity(index, "orch-1", panes);
  assert(a?.planId === "busy", `freshest plan leads (got ${a?.planId})`);
  assert(a.planCount === 2, `counts both owned plans (got ${a.planCount})`);
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
