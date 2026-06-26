// Run: npx tsx src/components/Canvas/workspacePlanEntry.test.ts

import type { Pane, Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import type { PlanIndexEntry } from "./plansApi";
import { canvasFocusStore } from "./canvasFocusStore";
import {
  deriveWorkspacePlanEntries,
  openPanePlanFocus,
  openWorkspacePlanCanvas,
  resolvePanePlanFocusTarget,
  resolveWorkspaceFocusNodeIds,
  canvasFitContextKey,
  workspacePlanEntryExtraCount,
  workspacePlanEntryHint,
  workspacePlanEntryKind,
  workspacePlanEntryLabel,
  workspacePlanEntryPrimaryTitle,
  workspacePlanEntryTitle,
  workspacePaneIds,
} from "./workspacePlanEntry";
import type { Plan } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const orchPane: Pane = {
  id: "orch-1",
  pty_id: "pty-1",
  pane_type: "Terminal",
  title: "Orchestrator",
  cwd: null,
  role: "Orchestrator",
  can_spawn: true,
  agent_session_id: "sess-1",
};

const workerPane: Pane = {
  id: "worker-1",
  pty_id: "pty-2",
  pane_type: "Terminal",
  title: "Worker",
  cwd: null,
  role: "Worker",
  parent_pane_id: "orch-1",
};

const panes: Record<string, Pane> = {
  "orch-1": orchPane,
  "worker-1": workerPane,
};

const workspace: Workspace = {
  id: "ws-1",
  name: "Hub",
  cwd: "/repo/hub",
  active_tab_id: "tab-1",
  tabs: [
    { id: "tab-1", pane_id: "orch-1" },
    { id: "tab-2", pane_id: "worker-1" },
  ],
};

function indexEntry(overrides: Partial<PlanIndexEntry> = {}): PlanIndexEntry {
  return {
    plan_id: FIXTURE_PLAN.id,
    file_path: "/repo/hub/.codeterm/plans/plan.json",
    owner: FIXTURE_PLAN.owner,
    title: FIXTURE_PLAN.title,
    status: "active",
    updated_at: FIXTURE_PLAN.updated_at,
    assigned_pane_ids: [],
    task_count: FIXTURE_PLAN.tasks.length,
    section_count: FIXTURE_PLAN.sections.length,
    element_count: FIXTURE_PLAN.elements.length,
    loop_count: FIXTURE_PLAN.loops.length,
    has_goal: !!FIXTURE_PLAN.goal,
    ...overrides,
  };
}

test("workspacePaneIds collects visible tab panes and child panes", () => {
  const ids = workspacePaneIds(workspace, panes);
  assert(ids.has("orch-1"), "orchestrator tab pane");
  assert(ids.has("worker-1"), "worker child pane");
});

test("deriveWorkspacePlanEntries matches project-local plans", () => {
  const entries = deriveWorkspacePlanEntries([indexEntry()], workspace, panes);
  assert(entries.length === 1, "one relevant plan");
  assert(entries[0].plan_id === FIXTURE_PLAN.id, "fixture plan id");
});

test("deriveWorkspacePlanEntries ignores archived plans", () => {
  const archived = indexEntry({ status: "archived" });
  const entries = deriveWorkspacePlanEntries([archived], workspace, panes);
  assert(entries.length === 0, "archived hidden");
});

test("deriveWorkspacePlanEntries ignores assigned plans from another project", () => {
  const assigned = indexEntry({
    plan_id: "plan-worker",
    file_path: "/repo/other/.codeterm/plans/plan-worker.json",
    owner: { pane_id: "other-orch", session_id: null, title: null },
    assigned_pane_ids: ["worker-1"],
  });
  const entries = deriveWorkspacePlanEntries([assigned], workspace, panes);
  assert(entries.length === 0, "foreign worker assignment stays off workspace action");
});

test("deriveWorkspacePlanEntries does not leak foreign project plans through owner panes", () => {
  const foreign = indexEntry({
    plan_id: "plan-foreign",
    file_path: "/repo/other/.codeterm/plans/plan-foreign.json",
    owner: { pane_id: "orch-1", session_id: null, title: null },
    assigned_pane_ids: [],
  });
  const entries = deriveWorkspacePlanEntries([foreign], workspace, panes);
  assert(entries.length === 0, "foreign project owner does not surface plan");
});

test("deriveWorkspacePlanEntries matches project plans even when workspace has no panes", () => {
  const emptyWorkspace = { ...workspace, tabs: [] };
  const entries = deriveWorkspacePlanEntries(
    [indexEntry({ file_path: `${workspace.cwd}/.codeterm/plans/${FIXTURE_PLAN.id}.json` })],
    emptyWorkspace,
    {},
  );
  assert(entries.length === 1, "project-local plan surfaces without live panes");
});

test("deriveWorkspacePlanEntries hides empty project plans without pane assignment", () => {
  const emptyWorkspace = { ...workspace, tabs: [] };
  const entries = deriveWorkspacePlanEntries(
    [
      indexEntry({
        file_path: `${workspace.cwd}/.codeterm/plans/empty.json`,
        assigned_pane_ids: [],
        task_count: 0,
        section_count: 0,
        element_count: 0,
        loop_count: 0,
        has_goal: false,
      }),
    ],
    emptyWorkspace,
    {},
  );
  assert(entries.length === 0, "empty project-local plan hidden");
});

test("deriveWorkspacePlanEntries hides plans outside workspace project path", () => {
  const foreign = indexEntry({
    file_path: "/other/repo/.codeterm/plans/plan.json",
    owner: { pane_id: "foreign-orch", session_id: null, title: null },
    assigned_pane_ids: [],
  });
  const entries = deriveWorkspacePlanEntries([foreign], { ...workspace, tabs: [] }, {});
  assert(entries.length === 0, "foreign project path hidden");
});

test("deriveWorkspacePlanEntries hides plans when workspace has no cwd and no pane match", () => {
  const noCwd = { ...workspace, cwd: null, tabs: [] };
  const orphan = indexEntry({ assigned_pane_ids: [] });
  const entries = deriveWorkspacePlanEntries([orphan], noCwd, {});
  assert(entries.length === 0, "no cwd and no pane ownership");
});

test("workspacePlanEntryKind and label singular and plural", () => {
  assert(workspacePlanEntryKind([indexEntry()]) === "Plan", "single kind");
  assert(workspacePlanEntryLabel([indexEntry()]) === "Plan", "single visible label");
  assert(workspacePlanEntryKind([indexEntry(), indexEntry({ plan_id: "plan-b", title: "Beta" })]) === "Plan", "plural kind still pinned action");
  assert(workspacePlanEntryLabel([indexEntry(), indexEntry({ plan_id: "plan-b", title: "Beta" })]) === "Plan", "multiple visible label");
  assert(workspacePlanEntryTitle([indexEntry()]).includes(FIXTURE_PLAN.title), "single title keeps plan name");
  const manyTitle = workspacePlanEntryTitle([indexEntry(), indexEntry({ plan_id: "plan-b", title: "Beta" })]);
  assert(manyTitle.includes(FIXTURE_PLAN.title) && manyTitle.includes("+1 more plan"), `multiple title detail (got ${manyTitle})`);
});

test("workspacePlanEntryPrimaryTitle leads with the freshest plan name", () => {
  assert(workspacePlanEntryPrimaryTitle([indexEntry()]) === FIXTURE_PLAN.title, "single primary title");
  assert(workspacePlanEntryPrimaryTitle([]) === "Untitled plan", "empty falls back");
  assert(
    workspacePlanEntryPrimaryTitle([indexEntry({ title: "  " })]) === "Untitled plan",
    "blank title falls back",
  );
});

test("workspacePlanEntryHint prefers tasks then sections then items", () => {
  assert(workspacePlanEntryHint([indexEntry({ task_count: 12 })]) === "12 tasks", "task plural");
  assert(workspacePlanEntryHint([indexEntry({ task_count: 1 })]) === "1 task", "task singular");
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 0, section_count: 3 })]) === "3 sections",
    "falls back to sections",
  );
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 0, section_count: 0, element_count: 5 })]) === "5 items",
    "falls back to items",
  );
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 0, section_count: 0, element_count: 0 })]) === null,
    "no count → no hint",
  );
});

test("workspacePlanEntryExtraCount counts plans beyond the lead", () => {
  assert(workspacePlanEntryExtraCount([indexEntry()]) === 0, "single → 0");
  assert(
    workspacePlanEntryExtraCount([indexEntry(), indexEntry({ plan_id: "b" }), indexEntry({ plan_id: "c" })]) === 2,
    "three → +2",
  );
});

test("resolvePanePlanFocusTarget carries pane and optional plan id", () => {
  const one = resolvePanePlanFocusTarget([indexEntry()], "worker-1");
  assert(one?.focusPaneId === "worker-1", "focus pane");
  assert(one?.planId === FIXTURE_PLAN.id, "single plan id");
  const many = resolvePanePlanFocusTarget(
    [indexEntry(), indexEntry({ plan_id: "plan-b", title: "Beta" })],
    "worker-1",
  );
  assert(many?.focusPaneId === "worker-1", "focus pane with many plans");
  assert(many?.planId === undefined, "no single plan id when ambiguous");
});

test("openWorkspacePlanCanvas opens workspace target on the focus store", () => {
  canvasFocusStore.closeWorkspace();
  openWorkspacePlanCanvas({ focusPaneId: "orch-1", planId: FIXTURE_PLAN.id });
  assert(canvasFocusStore.isWorkspaceOpen(), "workspace canvas open");
  assert(canvasFocusStore.getWorkspaceTarget()?.planId === FIXTURE_PLAN.id, "plan id stored");
  canvasFocusStore.closeWorkspace();
});

test("openPanePlanFocus routes pane focus through workspace canvas", () => {
  canvasFocusStore.closeWorkspace();
  openPanePlanFocus([indexEntry({ assigned_pane_ids: ["worker-1"] })], "worker-1", [
    FIXTURE_TASK.id,
  ]);
  assert(canvasFocusStore.isWorkspaceOpen(), "workspace canvas open from pane chip");
  const target = canvasFocusStore.getWorkspaceTarget();
  assert(target?.focusPaneId === "worker-1", "pane focus preserved");
  assert(target?.nodeIds?.[0] === FIXTURE_TASK.id, "explicit node ids forwarded");
  canvasFocusStore.closeWorkspace();
});

test("resolveWorkspaceFocusNodeIds prefers explicit node ids", () => {
  const plans = [{ ...FIXTURE_PLAN, tasks: [{ ...FIXTURE_TASK, assigned_pane_id: "worker-1" }] }] as Plan[];
  const ids = resolveWorkspaceFocusNodeIds("workspace", { nodeIds: ["n1", "n2"] }, plans);
  assert(ids.join() === "n1,n2", "node ids pass through");
});

test("resolveWorkspaceFocusNodeIds resolves pane assignments in workspace mode", () => {
  const plans = [{ ...FIXTURE_PLAN, tasks: [{ ...FIXTURE_TASK, assigned_pane_id: "worker-1" }] }] as Plan[];
  const ids = resolveWorkspaceFocusNodeIds("workspace", { focusPaneId: "worker-1" }, plans);
  assert(ids[0] === FIXTURE_TASK.id, "pane maps to assigned task");
});

test("canvasFitContextKey encodes workspace target fields", () => {
  const key = canvasFitContextKey("workspace", "pane-a", {
    focusPaneId: "pane-b",
    planId: "pl-1",
    nodeIds: ["t1"],
  });
  assert(key.includes("pane-a") && key.includes("pane-b") && key.includes("pl-1") && key.includes("t1"), key);
});

async function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} workspacePlanEntry tests passed`);
}

main();
