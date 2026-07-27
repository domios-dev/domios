// Run: npx tsx src/components/Canvas/workspacePlanEntry.test.ts

import type { Pane, Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import type { PlanIndexEntry } from "./plansApi";
import { canvasFocusStore } from "./canvasFocusStore";
import {
  deriveWorkspacePlanEntries,
  openTeamPlanCanvas,
  openPanePlanFocus,
  openWorkspacePlanCanvas,
  resolvePanePlanFocusTarget,
  resolveWorkspaceFocusNodeIds,
  canvasFitContextKey,
  findPlanOwnerWorkspace,
  openPlanDeepLink,
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
    file_path: `/repo/hub/.codeterm/plans/active/${FIXTURE_PLAN.id}/plan.json`,
    canonical_project_path: "",
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

test("workspacePaneIds collects visible tabs and child Tabs", () => {
  const ids = workspacePaneIds(workspace, panes);
  assert(ids.has("orch-1"), "orchestrator tab");
  assert(ids.has("worker-1"), "worker child Tab");
});

test("deriveWorkspacePlanEntries matches project-local plans", () => {
  const entries = deriveWorkspacePlanEntries([indexEntry()], workspace, panes);
  assert(entries.length === 1, "one relevant plan");
  assert(entries[0].plan_id === FIXTURE_PLAN.id, "fixture plan id");
});

test("deriveWorkspacePlanEntries reaches file-backed fallback when canonical ownership is omitted", () => {
  const { canonical_project_path: _canonicalProjectPath, ...legacyEntry } = indexEntry();
  const entries = deriveWorkspacePlanEntries([legacyEntry], workspace, panes);
  assert(entries.length === 1, "omitted canonical ownership reaches legacy file-backed matching");
});

test("deriveWorkspacePlanEntries reaches file-backed fallback when canonical ownership is blank", () => {
  const entries = deriveWorkspacePlanEntries(
    [indexEntry({ canonical_project_path: "  " })],
    workspace,
    panes,
  );
  assert(entries.length === 1, "blank canonical ownership reaches legacy file-backed matching");
});

test("deriveWorkspacePlanEntries matches Windows verbatim canonical ownership", () => {
  const windowsWorkspace = { ...workspace, cwd: "D:\\Developer\\codeterm" };
  const entries = deriveWorkspacePlanEntries(
    [indexEntry({
      file_path: "C:\\Users\\User\\.codeterm\\plans.db",
      canonical_project_path: "\\\\?\\D:\\Developer\\codeterm",
    })],
    windowsWorkspace,
    panes,
  );
  assert(entries.length === 1, "verbatim Windows ownership belongs to workspace");
});

test("deriveWorkspacePlanEntries matches Unix canonical ownership", () => {
  const entries = deriveWorkspacePlanEntries(
    [indexEntry({
      file_path: "/home/user/.codeterm/plans.db",
      canonical_project_path: "/repo/hub",
    })],
    workspace,
    panes,
  );
  assert(entries.length === 1, "Unix canonical ownership belongs to workspace");
});

test("deriveWorkspacePlanEntries never treats a shared database path as ownership", () => {
  const centralDatabase = indexEntry({
    file_path: "/repo/hub/.codeterm/plans.db",
    canonical_project_path: "",
  });
  const entries = deriveWorkspacePlanEntries([centralDatabase], workspace, panes);
  assert(entries.length === 0, "shared plans.db location is storage, not ownership");
});

test("projected legacy centralized ownership resolves only in its current workspace", () => {
  const currentWorkspace = { ...workspace, id: "ws-current", cwd: "/repo/hub" };
  const foreignWorkspace = { ...workspace, id: "ws-foreign", cwd: "/repo/foreign" };
  const currentEntry = indexEntry({
    plan_id: "pl-legacy-current",
    file_path: "/home/user/.codeterm/plans.db",
    canonical_project_path: "/repo/hub",
  });
  const foreignEntry = indexEntry({
    plan_id: "pl-legacy-foreign",
    file_path: "/home/user/.codeterm/plans.db",
    canonical_project_path: "/repo/foreign",
  });

  const currentOwner = findPlanOwnerWorkspace(
    currentEntry.plan_id,
    [currentEntry],
    [currentWorkspace, foreignWorkspace],
  );
  const foreignOwner = findPlanOwnerWorkspace(
    foreignEntry.plan_id,
    [foreignEntry],
    [currentWorkspace],
  );

  assert(currentOwner?.workspace.id === currentWorkspace.id, "legacy current-workspace plan resolves");
  assert(foreignOwner === null, "legacy foreign-workspace plan does not resolve in the current workspace");
});

test("canonical ownership overrides a misleading legacy file path", () => {
  const foreign = indexEntry({
    file_path: `/repo/hub/.codeterm/plans/active/${FIXTURE_PLAN.id}/plan.json`,
    canonical_project_path: "/repo/other",
  });
  const entries = deriveWorkspacePlanEntries([foreign], workspace, panes);
  assert(entries.length === 0, "foreign canonical owner cannot fall back to a local file path");
});

test("deriveWorkspacePlanEntries ignores archived plans", () => {
  const archived = indexEntry({
    status: "archived",
    file_path: "/home/user/.codeterm/plans.db",
    canonical_project_path: "/repo/hub",
  });
  const entries = deriveWorkspacePlanEntries([archived], workspace, panes);
  assert(entries.length === 0, "archived hidden");
});

test("deriveWorkspacePlanEntries ignores assigned plans from another project", () => {
  const assigned = indexEntry({
    plan_id: "plan-worker",
    file_path: "/repo/other/.codeterm/plans/active/plan-worker/plan.json",
    owner: { pane_id: "other-orch", session_id: null, title: null },
    assigned_pane_ids: ["worker-1"],
  });
  const entries = deriveWorkspacePlanEntries([assigned], workspace, panes);
  assert(entries.length === 0, "foreign worker assignment stays off workspace action");
});

test("deriveWorkspacePlanEntries does not leak foreign project plans through owner Tabs", () => {
  const foreign = indexEntry({
    plan_id: "plan-foreign",
    file_path: "/repo/other/.codeterm/plans/active/plan-foreign/plan.json",
    owner: { pane_id: "orch-1", session_id: null, title: null },
    assigned_pane_ids: [],
  });
  const entries = deriveWorkspacePlanEntries([foreign], workspace, panes);
  assert(entries.length === 0, "foreign project owner does not surface plan");
});

test("deriveWorkspacePlanEntries matches project plans even when workspace has no Tabs", () => {
  const emptyWorkspace = { ...workspace, tabs: [] };
  const entries = deriveWorkspacePlanEntries(
    [indexEntry({ file_path: `${workspace.cwd}/.codeterm/plans/active/${FIXTURE_PLAN.id}/plan.json` })],
    emptyWorkspace,
    {},
  );
  assert(entries.length === 1, "project-local plan surfaces without live Tabs");
});

test("deriveWorkspacePlanEntries hides empty project plans without Tab assignment", () => {
  const emptyWorkspace = { ...workspace, tabs: [] };
  const entries = deriveWorkspacePlanEntries(
    [
      indexEntry({
        file_path: `${workspace.cwd}/.codeterm/plans/active/empty/plan.json`,
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
    file_path: "/other/repo/.codeterm/plans/active/plan/plan.json",
    owner: { pane_id: "foreign-orch", session_id: null, title: null },
    assigned_pane_ids: [],
  });
  const entries = deriveWorkspacePlanEntries([foreign], { ...workspace, tabs: [] }, {});
  assert(entries.length === 0, "foreign project path hidden");
});

test("deriveWorkspacePlanEntries hides plans when workspace has no cwd and no Tab match", () => {
  const noCwd = { ...workspace, cwd: null, tabs: [] };
  const orphan = indexEntry({ assigned_pane_ids: [] });
  const entries = deriveWorkspacePlanEntries([orphan], noCwd, {});
  assert(entries.length === 0, "no cwd and no Tab ownership");
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
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 12, open_task_count: 7 })]) === "7 tasks",
    "open_task_count takes priority — shows remaining not total",
  );
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 12, open_task_count: 1 })]) === "1 task",
    "open_task_count singular",
  );
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 12, open_task_count: 0, section_count: 3 })]) === "3 sections",
    "zero open tasks falls through to sections",
  );
});

test("open_task_count reflects counts_as_open_work semantics: excludes done, cancelled, paused", () => {
  // open_task_count from backend = 3 (only counts_as_open_work tasks)
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 10, open_task_count: 3 })]) === "3 tasks",
    "hint shows only the open-work count (done+cancelled+paused excluded)",
  );
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 5, open_task_count: 0, section_count: 2 })]) === "2 sections",
    "zero open tasks (all terminal/paused) falls through to sections",
  );
  assert(
    workspacePlanEntryHint([indexEntry({ task_count: 8, open_task_count: 1 })]) === "1 task",
    "singular open task",
  );
});

test("workspacePlanEntryExtraCount counts plans beyond the lead", () => {
  assert(workspacePlanEntryExtraCount([indexEntry()]) === 0, "single → 0");
  assert(
    workspacePlanEntryExtraCount([indexEntry(), indexEntry({ plan_id: "b" }), indexEntry({ plan_id: "c" })]) === 2,
    "three → +2",
  );
});

test("resolvePanePlanFocusTarget carries Tab and optional plan id", () => {
  const one = resolvePanePlanFocusTarget([indexEntry()], "worker-1");
  assert(one?.focusPaneId === "worker-1", "focus Tab");
  assert(one?.planId === FIXTURE_PLAN.id, "single plan id");
  const many = resolvePanePlanFocusTarget(
    [indexEntry(), indexEntry({ plan_id: "plan-b", title: "Beta" })],
    "worker-1",
  );
  assert(many?.focusPaneId === "worker-1", "focus Tab with many plans");
  assert(many?.planId === undefined, "no single plan id when ambiguous");
});

test("openWorkspacePlanCanvas opens workspace target on the focus store", () => {
  canvasFocusStore.closeWorkspace();
  openWorkspacePlanCanvas({ focusPaneId: "orch-1", planId: FIXTURE_PLAN.id });
  assert(canvasFocusStore.isWorkspaceOpen(), "workspace canvas open");
  assert(canvasFocusStore.getWorkspaceTarget()?.planId === FIXTURE_PLAN.id, "plan id stored");
  canvasFocusStore.closeWorkspace();
});

test("openTeamPlanCanvas keeps the team and manager target explicit", () => {
  canvasFocusStore.closeWorkspace();
  openTeamPlanCanvas({ teamId: "team-1", managerPaneId: "manager-1" });
  const target = canvasFocusStore.getWorkspaceTarget();
  assert(target?.teamId === "team-1", "team target survives the focus store");
  assert(target?.managerPaneId === "manager-1", "manager target survives the focus store");
  assert(target?.focusPaneId === "manager-1", "manager target stored");
  assert(target?.planId === undefined, "create flow does not fabricate a plan id");
  canvasFocusStore.closeWorkspace();
});

test("openPlanDeepLink switches to the indexed owner workspace and focuses the task", () => {
  const ownerWorkspace = { ...workspace, id: "ws-owner", cwd: "/repo/owner" };
  const foreignWorkspace = { ...workspace, id: "ws-current", cwd: "/repo/current" };
  const entry = indexEntry({
    file_path: "/home/user/.codeterm/plans.db",
    canonical_project_path: "/repo/owner",
  });
  const owner = findPlanOwnerWorkspace(FIXTURE_PLAN.id, [entry], [foreignWorkspace, ownerWorkspace]);
  assert(owner?.workspace.id === ownerWorkspace.id, "owner comes from canonical project ownership");

  let switchedTo: string | null = null;
  canvasFocusStore.closeWorkspace();
  const opened = openPlanDeepLink(FIXTURE_PLAN.id, FIXTURE_TASK.id, {
    index: [entry],
    workspaces: [foreignWorkspace, ownerWorkspace],
    activeWorkspaceId: foreignWorkspace.id,
    setActiveWorkspace: (id) => { switchedTo = id; },
  });
  assert(opened, "valid indexed plan opens");
  assert(switchedTo === ownerWorkspace.id, "owner workspace is revealed");
  const target = canvasFocusStore.getWorkspaceTarget();
  assert(target?.planId === FIXTURE_PLAN.id, "plan view targets indexed plan");
  assert(target?.nodeIds?.[0] === FIXTURE_TASK.id, "task focus is retained");
  canvasFocusStore.closeWorkspace();
});

test("openPlanDeepLink accepts resolved canonical ownership before the index refreshes", () => {
  const ownerWorkspace = { ...workspace, id: "ws-owner", cwd: "/repo/owner" };
  let switchedTo: string | null = null;
  canvasFocusStore.closeWorkspace();
  const opened = openPlanDeepLink(FIXTURE_PLAN.id, FIXTURE_TASK.id, {
    index: [],
    workspaces: [workspace, ownerWorkspace],
    activeWorkspaceId: workspace.id,
    canonicalProjectPath: "/repo/owner",
    setActiveWorkspace: (id) => { switchedTo = id; },
  });
  assert(opened, "backend-resolved canonical ownership opens without an index entry");
  assert(switchedTo === ownerWorkspace.id, "resolved ownership selects the owner workspace");
  assert(canvasFocusStore.getWorkspaceTarget()?.planId === FIXTURE_PLAN.id, "resolved plan opens");
  canvasFocusStore.closeWorkspace();
});

test("openPlanDeepLink leaves a missing reference inert without a missing callback", () => {
  let switched = false;
  const opened = openPlanDeepLink("pl_missing", "task_missing", {
    index: [],
    workspaces: [workspace],
    activeWorkspaceId: workspace.id,
    setActiveWorkspace: () => { switched = true; },
  });
  assert(!opened, "missing plan does not open navigation");
  assert(!switched, "missing plan does not switch workspace");
});

test("openPanePlanFocus routes Tab focus through workspace canvas", () => {
  canvasFocusStore.closeWorkspace();
  openPanePlanFocus([indexEntry({ assigned_pane_ids: ["worker-1"] })], "worker-1", [
    FIXTURE_TASK.id,
  ]);
  assert(canvasFocusStore.isWorkspaceOpen(), "workspace canvas open from Tab chip");
  const target = canvasFocusStore.getWorkspaceTarget();
  assert(target?.focusPaneId === "worker-1", "Tab focus preserved");
  assert(target?.nodeIds?.[0] === FIXTURE_TASK.id, "explicit node ids forwarded");
  canvasFocusStore.closeWorkspace();
});

test("resolveWorkspaceFocusNodeIds prefers explicit node ids", () => {
  const plans = [{ ...FIXTURE_PLAN, tasks: [{ ...FIXTURE_TASK, assigned_pane_id: "worker-1" }] }] as Plan[];
  const ids = resolveWorkspaceFocusNodeIds("workspace", { nodeIds: ["n1", "n2"] }, plans);
  assert(ids.join() === "n1,n2", "node ids pass through");
});

test("resolveWorkspaceFocusNodeIds resolves Tab assignments in workspace mode", () => {
  const plans = [{ ...FIXTURE_PLAN, tasks: [{ ...FIXTURE_TASK, assigned_pane_id: "worker-1" }] }] as Plan[];
  const ids = resolveWorkspaceFocusNodeIds("workspace", { focusPaneId: "worker-1" }, plans);
  assert(ids[0] === FIXTURE_TASK.id, "Tab maps to assigned task");
});

test("resolveWorkspaceFocusNodeIds ignores terminal and empty Tab assignments", () => {
  const plans = [
    {
      ...FIXTURE_PLAN,
      tasks: [
        { ...FIXTURE_TASK, id: "done", status: "done", assigned_pane_id: "worker-1" },
        { ...FIXTURE_TASK, id: "cancelled", status: "cancelled", assigned_pane_id: "worker-1" },
        { ...FIXTURE_TASK, id: "empty", assigned_pane_id: " " },
      ],
    },
  ] as Plan[];
  const ids = resolveWorkspaceFocusNodeIds("workspace", { focusPaneId: "worker-1" }, plans);
  assert(ids.length === 0, "terminal stale bindings do not focus nodes");
});

test("canvasFitContextKey encodes workspace target fields", () => {
  const key = canvasFitContextKey("workspace", "pane-a", {
    focusPaneId: "pane-b",
    planId: "pl-1",
    nodeIds: ["t1"],
  });
  assert(key.includes("pane-a") && key.includes("pane-b") && key.includes("pl-1") && key.includes("t1"), key);
});

test("deriveWorkspacePlanEntries cumulatively includes root and nested-root plans, excludes siblings and outside roots", () => {
  const ws = { ...workspace, cwd: "/repo/hub", tabs: [] };
  const mk = (planId: string, filePath: string, updatedAt: number) =>
    indexEntry({ plan_id: planId, file_path: filePath, updated_at: updatedAt });
  const index = [
    mk("A1", "/repo/hub/.codeterm/plans/active/A1/plan.json", 40),
    mk("A2", "/repo/hub/.codeterm/plans/active/A2/plan.json", 30),
    mk("p1", "/repo/hub/packages/p1/.codeterm/plans/active/p1/plan.json", 20),
    mk("p2", "/repo/hub/tools/p2/.codeterm/plans/active/p2/plan.json", 10),
    mk("B1", "/repo/hub-worktree/.codeterm/plans/active/B1/plan.json", 99),
    mk("C1", "/other/root/.codeterm/plans/active/C1/plan.json", 99),
  ];
  const ids = deriveWorkspacePlanEntries(index, ws, {}).map((e) => e.plan_id);
  assert(ids.join(",") === "A1,A2,p1,p2", `root + nested only, sibling/outside excluded (got ${ids.join(",")})`);
});

test("deriveWorkspacePlanEntries deduplicates a plan surfaced through root and nested index aliases", () => {
  const ws = { ...workspace, cwd: "/repo/hub", tabs: [] };
  const rootAlias = indexEntry({
    plan_id: "dup",
    file_path: "/repo/hub/.codeterm/plans/active/dup/plan.json",
    updated_at: 50,
  });
  const nestedAlias = indexEntry({
    plan_id: "dup",
    file_path: "/repo/hub/nested/.codeterm/plans/active/dup/plan.json",
    updated_at: 70,
  });
  const entries = deriveWorkspacePlanEntries([rootAlias, nestedAlias], ws, {});
  assert(entries.length === 1, `one canonical entry per plan id (got ${entries.length})`);
  assert(entries[0].updated_at === 70, "keeps the freshest alias of the duplicated plan");
});

test("deriveWorkspacePlanEntries orders stably and deterministically across reloads", () => {
  const ws = { ...workspace, cwd: "/repo/hub", tabs: [] };
  const mk = (planId: string, updatedAt: number) =>
    indexEntry({
      plan_id: planId,
      file_path: `/repo/hub/.codeterm/plans/active/${planId}/plan.json`,
      updated_at: updatedAt,
    });
  // Equal updated_at must break ties deterministically by plan id, regardless of input order.
  const a = [mk("beta", 10), mk("alpha", 10), mk("gamma", 20)];
  const b = [mk("alpha", 10), mk("gamma", 20), mk("beta", 10)];
  const orderA = deriveWorkspacePlanEntries(a, ws, {}).map((e) => e.plan_id).join(",");
  const orderB = deriveWorkspacePlanEntries(b, ws, {}).map((e) => e.plan_id).join(",");
  assert(orderA === "gamma,alpha,beta", `freshest first, id tie-break (got ${orderA})`);
  assert(orderA === orderB, "identical order across reloads with shuffled input");
});

test("deriveWorkspacePlanEntries updates the same surface in place on a nested plan-changed event", () => {
  const ws = { ...workspace, cwd: "/repo/hub", tabs: [] };
  const root = indexEntry({
    plan_id: "A1",
    file_path: "/repo/hub/.codeterm/plans/active/A1/plan.json",
    updated_at: 10,
  });
  const nested = indexEntry({
    plan_id: "p1",
    file_path: "/repo/hub/packages/p1/.codeterm/plans/active/p1/plan.json",
    updated_at: 5,
  });
  const before = deriveWorkspacePlanEntries([root, nested], ws, {}).map((e) => e.plan_id);
  assert(before.join(",") === "A1,p1", `initial cumulative set (got ${before.join(",")})`);
  // Nested plan bumped fresher — same set, reordered in place, no duplicate row.
  const bumped = { ...nested, updated_at: 20 };
  const after = deriveWorkspacePlanEntries([root, bumped], ws, {});
  assert(after.length === 2, "no duplicate rows after live update");
  assert(after.map((e) => e.plan_id).join(",") === "p1,A1", "live update reorders the same surface");
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
