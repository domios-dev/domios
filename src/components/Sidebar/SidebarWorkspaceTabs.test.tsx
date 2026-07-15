// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.test.tsx

import { act } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Pane, Tab, Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../Canvas/cards/fixtures.testdata";
import { groupTasksIntoColumns } from "../Canvas/planView/boardModel";
import { TaskStatus, type Plan, type Task } from "../Canvas/types";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import {
  deriveTeamTaskBuckets,
  donutBackground,
  type TeamSnapshot,
} from "./TeamUnit";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
  Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
  (globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (callback) => {
      callback(0);
      return 0;
    };
  (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
}

function pane(overrides: Partial<Pane> & { team_id?: string | null }): Pane {
  return {
    id: "pane",
    pty_id: null,
    pane_type: "Terminal",
    title: "Pane",
    cwd: null,
    role: null,
    role_profile: null,
    parent_pane_id: null,
    ...overrides,
  } as Pane;
}

function tab(id: string, paneId: string, sortOrder: number): Tab {
  return { id, pane_id: paneId, sort_order: sortOrder };
}

function task(id: string, status: Task["status"]): Task {
  return { ...FIXTURE_TASK, id, name: id, status };
}

function plan(tasks: Task[]): Plan {
  return { ...FIXTURE_PLAN, id: "plan-team", title: "Sidebar team effort", team_id: "team-one", tasks } as Plan;
}

const TEAM: TeamSnapshot = {
  id: "team-one",
  name: "Sidebar team",
  lifecycle: "active",
  created_at: 1,
  workspace_id: "workspace",
};

async function renderSidebar({
  panes,
  tabs,
  plans = [],
  statuses = {},
}: {
  panes: Record<string, Pane>;
  tabs: Tab[];
  plans?: Plan[];
  statuses?: Record<string, "Working" | "Waiting" | "Idle" | "Dead">;
}): Promise<{ host: HTMLDivElement; root: Root }> {
  const workspace: Workspace = {
    id: "workspace",
    name: "Workspace",
    tabs,
    active_tab_id: tabs[0]?.id ?? null,
    cwd: null,
  };
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={workspace.active_tab_id}
          sortedTabs={tabs}
          panes={panes}
          paneStatuses={statuses}
          teams={[TEAM]}
          planIndex={[]}
          plans={plans}
          activeWork={null}
          remoteWorkers={{}}
          providers={[]}
          closingTabIds={new Set()}
          appearingTabIds={new Set()}
          isActiveVisual={true}
          workspaceCanvasActive={false}
          touchMode={false}
          compact={false}
          sidebarActionText="text-xs"
          orchestrators={Object.values(panes).filter((member) => member.role === "Orchestrator")}
          onBeforeOpenPlan={() => {}}
          onActivateTab={() => {}}
          onCloseTab={() => {}}
          onRenameTab={() => {}}
          onNewSession={() => {}}
        />
      </SidebarDnd>,
    );
  });
  return { host, root };
}

setupDom();

test("team_renders_flat_sections", async () => {
  const panes: Record<string, Pane> = {
    plannerTwo: pane({ id: "plannerTwo", title: "Planner Two", role: "Manager", role_profile: "planner", team_id: TEAM.id }),
    workerOne: pane({ id: "workerOne", title: "Worker One", role: "Worker", team_id: TEAM.id, parent_pane_id: "orchOne" }),
    orchTwo: pane({ id: "orchTwo", title: "Orchestrator Two", role: "Orchestrator", team_id: TEAM.id }),
    plannerOne: pane({ id: "plannerOne", title: "Planner One", role: "Manager", role_profile: "planner", team_id: TEAM.id }),
    workerTwo: pane({ id: "workerTwo", title: "Worker Two", role: "Worker", team_id: TEAM.id, parent_pane_id: "workerOne" }),
    orchOne: pane({ id: "orchOne", title: "Orchestrator One", role: "Orchestrator", team_id: TEAM.id }),
    workerThree: pane({ id: "workerThree", title: "Worker Three", role: "Worker", team_id: TEAM.id, parent_pane_id: "orchTwo" }),
  };
  const ids = Object.keys(panes);
  const tabs = ids.map((id, index) => tab(`tab-${id}`, id, index));
  const { host, root } = await renderSidebar({ panes, tabs });

  const team = host.querySelector('[data-testid="team-unit"]');
  const managers = team?.querySelector('[data-testid="managers-section"]');
  const workers = team?.querySelector('[data-testid="workers-section"]');
  assert(team && managers && workers, "team unit renders MANAGERS and collapsible WORKERS");
  const managerTitles = Array.from(managers.querySelectorAll('[data-testid="tab-title"]')).map(
    (element) => element.textContent,
  );
  assert(
    managerTitles.join(",") === "Orchestrator Two,Orchestrator One,Planner Two,Planner One",
    `orchestrators sort before planners: ${managerTitles.join(",")}`,
  );
  assert(workers.querySelectorAll('[data-testid="tab-item"]').length === 3, "three workers render once");
  assert(!team.querySelector(".sidebar-hierarchy-level"), "no nested member tree nodes");
  await act(async () => root.unmount());
});

test("teamless_tabs_render_plain", async () => {
  const panes = {
    looseOne: pane({ id: "looseOne", title: "Loose terminal" }),
    looseTwo: pane({ id: "looseTwo", title: "Quick fix", role: "Worker" }),
  };
  const tabs = [tab("tab-loose-one", "looseOne", 0), tab("tab-loose-two", "looseTwo", 1)];
  const { host, root } = await renderSidebar({ panes, tabs });

  assert(!host.querySelector('[data-testid="team-unit"]'), "teamless tabs have no team wrapper");
  assert(host.querySelectorAll('[data-testid="tab-item"]').length === 2, "both loose tabs render plainly");
  await act(async () => root.unmount());
});

test("donut_counts_match_board", async () => {
  const panes = {
    orchestrator: pane({ id: "orchestrator", title: "Orchestrator", role: "Orchestrator", team_id: TEAM.id }),
  };
  const currentPlan = plan([
    task("pending", TaskStatus.Pending),
    task("progress", TaskStatus.InProgress),
    task("review", TaskStatus.InReview),
    task("blocked", TaskStatus.Blocked),
    task("done-one", TaskStatus.Done),
    task("done-two", TaskStatus.Done),
  ]);
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-orchestrator", "orchestrator", 0)],
    plans: [currentPlan],
  });

  const board = groupTasksIntoColumns(currentPlan, panes);
  const boardCounts = new Map(board.columns.map((column) => [column.key, column.cards.length]));
  const expected = deriveTeamTaskBuckets(currentPlan, panes);
  assert(expected[0].count === (boardCounts.get("pending") ?? 0) + (boardCounts.get("in_progress") ?? 0), "todo folds board pending + in-progress");
  const donut = host.querySelector('[data-testid="team-donut"]');
  assert(donut?.getAttribute("data-buckets") === JSON.stringify(expected), "donut props deep-equal board-derived buckets");
  await act(async () => root.unmount());
});

test("single_status_solid_donut", async () => {
  const panes = {
    orchestrator: pane({ id: "orchestrator", title: "Orchestrator", role: "Orchestrator", team_id: TEAM.id }),
  };
  const currentPlan = plan([task("done-one", TaskStatus.Done), task("done-two", TaskStatus.Done)]);
  const buckets = deriveTeamTaskBuckets(currentPlan, panes);
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-orchestrator", "orchestrator", 0)],
    plans: [currentPlan],
  });

  const donut = host.querySelector('[data-testid="team-donut"]') as HTMLElement | null;
  assert(donut?.dataset.segmentCount === "1", "one full-circle segment rendered");
  assert(donutBackground(buckets) === buckets.find((bucket) => bucket.count > 0)?.color, "single status resolves to a solid color");
  assert(!donut?.style.background.includes("conic-gradient"), "solid donut never uses a sparse gradient");
  await act(async () => root.unmount());
});

test("workers_collapse_summary", async () => {
  const panes = {
    orchestrator: pane({ id: "orchestrator", title: "Orchestrator", role: "Orchestrator", team_id: TEAM.id }),
    workerOne: pane({ id: "workerOne", title: "Worker One", role: "Worker", team_id: TEAM.id }),
    workerTwo: pane({ id: "workerTwo", title: "Worker Two", role: "Worker", team_id: TEAM.id }),
    workerThree: pane({ id: "workerThree", title: "Worker Three", role: "Worker", team_id: TEAM.id }),
  };
  const tabs = Object.keys(panes).map((id, index) => tab(`tab-${id}`, id, index));
  const { host, root } = await renderSidebar({
    panes,
    tabs,
    statuses: { workerOne: "Working", workerTwo: "Working", workerThree: "Idle" },
  });
  const toggle = host.querySelector('[data-testid="workers-collapse-toggle"]') as HTMLElement | null;
  assert(toggle, "workers collapse toggle present");
  await act(async () => toggle.click());
  const summary = host.querySelector('[data-testid="workers-collapse-summary"]');
  assert(summary?.textContent?.includes("3 workers · 2 working"), `summary=${summary?.textContent}`);
  await act(async () => root.unmount());
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  if (failed > 0) process.exit(1);
  console.log(`\n✓ ${tests.length}/${tests.length} SidebarWorkspaceTabs tests passed`);
}

void main();
