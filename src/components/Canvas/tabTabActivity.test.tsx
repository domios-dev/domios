import { resolveTabTabActivity } from "./tabTabActivity";
import type { PlanIndexEntry } from "./plansApi";
import type { Tab } from "../../lib/types";

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
    owner: { tab_id: "orch-1", session_id: "sess-orch", title: "Orchestrator" },
    title: "Canvas rendering substrate — Figma-grade migration",
    status: "active",
    updated_at: 100,
    assigned_tab_ids: [],
    task_count: 12,
    ...over,
  };
}

function mkTab(id: string, over: Partial<Tab> = {}): Tab {
  return { id, pty_id: null, tab_type: "Terminal", title: id, cwd: null, ...over } as Tab;
}

test("owner Tab → orchestrator activity (plan title + task count)", () => {
  const index = [mkEntry({ assigned_tab_ids: ["w1"] })];
  const tabs = { "orch-1": mkTab("orch-1", { agent_session_id: "sess-orch", can_spawn: true }) };
  const a = resolveTabTabActivity(index, "orch-1", tabs);
  assert(a?.kind === "orchestrator", `owner classified orchestrator (got ${a?.kind})`);
  assert(a.taskCount === 12, `task count from index (got ${a.taskCount})`);
  assert(a.planTitle.includes("Figma-grade"), "carries plan title for hover");
});

test("directly-assigned Tab → worker activity", () => {
  const index = [mkEntry({ assigned_tab_ids: ["w1"] })];
  const tabs = {
    "orch-1": mkTab("orch-1", { agent_session_id: "sess-orch" }),
    w1: mkTab("w1", { parent_tab_id: "orch-1", agent_session_id: "sess-w1" }),
  };
  const a = resolveTabTabActivity(index, "w1", tabs);
  assert(a?.kind === "worker", `assigned Tab classified worker (got ${a?.kind})`);
  assert(a.planId === "plan-1", "worker carries its plan id");
});

test("idle / unrelated Tab → no activity (honest)", () => {
  const index = [mkEntry({ assigned_tab_ids: ["w1"] })];
  assert(resolveTabTabActivity(index, "stranger", {}) === null, "unrelated Tab shows nothing");
  assert(resolveTabTabActivity([], "w1", {}) === null, "empty index → nothing");
});

test("worker nested under orchestrator but NOT assigned → no activity", () => {
  const index = [mkEntry({ assigned_tab_ids: [] })];
  const tabs = {
    "orch-1": mkTab("orch-1", { agent_session_id: "sess-orch" }),
    w1: mkTab("w1", { parent_tab_id: "orch-1", agent_session_id: "sess-w1" }),
  };
  assert(resolveTabTabActivity(index, "w1", tabs) === null, "nested-only Tab stays bare");
});

test("session-id ownership survives a tab-id change", () => {
  const index = [mkEntry({ owner: { tab_id: "old-tab", session_id: "sess-orch", title: "Orch" } })];
  const tabs = { "orch-2": mkTab("orch-2", { agent_session_id: "sess-orch" }) };
  assert(resolveTabTabActivity(index, "orch-2", tabs)?.kind === "orchestrator", "matches on own session id");
});

test("multi-plan owner counts plans and leads with the freshest", () => {
  const index = [
    mkEntry({ plan_id: "quiet", title: "Quiet", updated_at: 1, task_count: 3 }),
    mkEntry({ plan_id: "busy", title: "Busy", updated_at: 999, task_count: 7 }),
  ];
  const tabs = { "orch-1": mkTab("orch-1", { agent_session_id: "sess-orch" }) };
  const a = resolveTabTabActivity(index, "orch-1", tabs);
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
