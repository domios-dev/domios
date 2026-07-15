// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.test.tsx

import { act, useState } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Pane, Tab, Team, Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../Canvas/cards/fixtures.testdata";
import { groupTasksIntoColumns } from "../Canvas/planView/boardModel";
import { TaskStatus, type Plan, type Task } from "../Canvas/types";
import { SidebarDnd } from "./sortable";
import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import {
  deriveTeamTaskBuckets,
  donutBackground,
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

async function openTabActionsMenu(tabRow: Element): Promise<void> {
  const trigger = tabRow.querySelector('button[title="Tab actions"]');
  assert(trigger?.parentElement, "tab actions trigger present");
  await act(async () => {
    trigger.parentElement!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  });
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

const TEAM: Team = {
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
  activeTabId,
  onActivateTab = () => {},
  onCloseTab = () => {},
  onRenameTeam,
  onAddTeamMember,
  onCloseTeam = () => {},
}: {
  panes: Record<string, Pane>;
  tabs: Tab[];
  plans?: Plan[];
  statuses?: Record<string, "Working" | "Waiting" | "Idle" | "Dead">;
  activeTabId?: string | null;
  onActivateTab?: (tabId: string) => void;
  onCloseTab?: (tabId: string) => void;
  onRenameTeam?: (teamId: string, name: string) => void;
  onAddTeamMember?: (teamId: string, role: "orchestrator" | "planner" | "worker", providerId: string, parentPaneId: string | null) => void;
  onCloseTeam?: (teamId: string) => void;
}): Promise<{ host: HTMLDivElement; root: Root }> {
  const workspace: Workspace = {
    id: "workspace",
    name: "Workspace",
    tabs,
    active_tab_id: activeTabId ?? tabs[0]?.id ?? null,
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
          onActivateTab={onActivateTab}
          onCloseTab={onCloseTab}
          onRenameTab={() => {}}
          onNewSession={() => {}}
          onRenameTeam={onRenameTeam}
          onAddTeamMember={onAddTeamMember}
          onCloseTeam={onCloseTeam}
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

test("team menu never creates efforts and exposes explicit Team closure", async () => {
  const closed: string[] = [];
  const panes = {
    manager: pane({ id: "manager", title: "Manager", role: "Orchestrator", team_id: TEAM.id }),
  };
  canvasFocusStore.closeWorkspace();
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-manager", "manager", 0)],
    onRenameTeam: () => {},
    onCloseTeam: (teamId) => closed.push(teamId),
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "team menu trigger exists");
  await act(async () => trigger.click());
  const menu = document.querySelector('[data-testid="team-menu"]');
  assert(menu?.textContent?.includes("Rename"), "supported rename remains available");
  assert(!menu?.textContent?.includes("Add orchestrator"), "unsupported staffing is hidden");
  assert(!menu?.textContent?.includes("Create effort"), "humans cannot create efforts from Team UI");
  assert(!menu?.textContent?.includes("Open effort"), "no effort action is shown without a plan");
  const close = Array.from(menu?.querySelectorAll("button") ?? []).find((button) => button.textContent?.includes("Close Team")) as HTMLButtonElement | undefined;
  assert(close, "Close Team action is available");
  await act(async () => close.click());
  assert(closed.join(",") === TEAM.id, "Close Team reports the exact Team id");
  assert(host.querySelector('[data-testid="team-no-effort"]')?.textContent === "No effort yet", "empty effort slot remains visible");
  canvasFocusStore.closeWorkspace();
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

test("completed effort renders a solid progress ring without an inner glyph", async () => {
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
  assert(donut?.dataset.progress === "2/2", "ring exposes completed task progress");
  assert(donutBackground(buckets) === "var(--ct-green)", "completed effort resolves to a solid progress ring");
  assert(!donut?.style.background.includes("conic-gradient"), "solid donut never uses a sparse gradient");
  assert(!donut?.querySelector("svg"), "progress ring has no inner icon");
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

test("team worker rows follow tab sort order", async () => {
  const panes: Record<string, Pane> = {
    orchestrator: pane({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Orchestrator",
      team_id: TEAM.id,
    }),
    workerTwo: pane({ id: "workerTwo", title: "Worker Two", role: "Worker", team_id: TEAM.id }),
    workerOne: pane({ id: "workerOne", title: "Worker One", role: "Worker", team_id: TEAM.id }),
  };
  const tabs = [
    tab("tab-orchestrator", "orchestrator", 0),
    tab("tab-worker-one", "workerOne", 1),
    tab("tab-worker-two", "workerTwo", 2),
  ];
  const { host, root } = await renderSidebar({ panes, tabs });
  const workerSection = host.querySelector('[data-testid="workers-section"]');
  const rendered = Array.from(workerSection?.querySelectorAll("[data-tab-id]") ?? []).map(
    (element) => element.getAttribute("data-tab-id"),
  );
  assert(rendered.join(",") === "tab-worker-one,tab-worker-two", "worker DOM order follows tab order");
  await act(async () => root.unmount());
});

test("clicking a team worker activates it", async () => {
  const panes: Record<string, Pane> = {
    orchestrator: pane({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Orchestrator",
      team_id: TEAM.id,
    }),
    worker: pane({ id: "worker", title: "Worker", role: "Worker", team_id: TEAM.id }),
  };
  const tabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-worker", "worker", 1)];
  const workspace: Workspace = {
    id: "workspace",
    name: "Workspace",
    tabs,
    active_tab_id: "tab-orchestrator",
    cwd: null,
  };

  function Harness() {
    const [activeTabId, setActiveTabId] = useState("tab-orchestrator");
    return (
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={activeTabId}
          sortedTabs={tabs}
          panes={panes}
          paneStatuses={{}}
          teams={[TEAM]}
          planIndex={[]}
          plans={[]}
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
          orchestrators={[panes.orchestrator]}
          onBeforeOpenPlan={() => {}}
          onActivateTab={setActiveTabId}
          onCloseTab={() => {}}
          onRenameTab={() => {}}
          onNewSession={() => {}}
        />
      </SidebarDnd>
    );
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<Harness />));
  const workerRow = host.querySelector('[data-tab-id="tab-worker"]') as HTMLElement | null;
  assert(workerRow?.className.includes("sidebar-tab-dimmed"), "worker starts inactive");
  await act(async () => workerRow?.click());
  assert(
    !host.querySelector('[data-tab-id="tab-worker"]')?.className.includes("sidebar-tab-dimmed"),
    "worker receives active chrome after click",
  );
  await act(async () => root.unmount());
});

test("waiting team worker keeps prompt pill without status rail", async () => {
  const panes: Record<string, Pane> = {
    orchestrator: pane({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Orchestrator",
      team_id: TEAM.id,
    }),
    attention: pane({
      id: "attention",
      title: "Attention",
      role: "Worker",
      team_id: TEAM.id,
      agent_type: "claude",
      pending_prompt: {
        question: "Allow team action?",
        options: [{ label: "Yes", value: "yes" }],
      },
    }),
  };
  const tabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-attention", "attention", 1)];
  const { host, root } = await renderSidebar({
    panes,
    tabs,
    statuses: { attention: "Waiting" },
    activeTabId: "tab-attention",
  });
  const row = host.querySelector('[data-tab-id="tab-attention"]');
  assert(row?.querySelector('span[title="Allow team action?"]'), "prompt pill remains visible");
  assert(!row?.querySelector('[data-testid="tab-status-rail"]'), "waiting worker has no left status rail");
  await act(async () => root.unmount());
});

test("working team worker keeps activity dots without status rail", async () => {
  const panes: Record<string, Pane> = {
    orchestrator: pane({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Orchestrator",
      team_id: TEAM.id,
    }),
    worker: pane({
      id: "worker",
      title: "Worker",
      role: "Worker",
      team_id: TEAM.id,
      agent_type: "claude",
    }),
  };
  const tabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-worker", "worker", 1)];
  const { host, root } = await renderSidebar({
    panes,
    tabs,
    statuses: { worker: "Working" },
    activeTabId: "tab-worker",
  });
  const row = host.querySelector('[data-tab-id="tab-worker"]');
  assert(row?.querySelector('[data-testid="tab-working-activity"]'), "working dots remain visible");
  assert(!row?.querySelector('[data-testid="tab-status-rail"]'), "working worker has no left status rail");
  await act(async () => root.unmount());
});

test("team worker and manager descriptions do not add row lines", async () => {
  const panes: Record<string, Pane> = {
    orchestrator: pane({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Orchestrator",
      team_id: TEAM.id,
    }),
    manager: pane({
      id: "manager",
      title: "Planner",
      description: "manager description must stay tooltip-only",
      role: "Manager",
      role_profile: "planner",
      team_id: TEAM.id,
    }),
    worker: pane({
      id: "worker",
      title: "Worker",
      description: "worker description must stay tooltip-only",
      role: "Worker",
      team_id: TEAM.id,
    }),
  };
  const tabs = [
    tab("tab-orchestrator", "orchestrator", 0),
    tab("tab-manager", "manager", 1),
    tab("tab-worker", "worker", 2),
  ];
  const { host, root } = await renderSidebar({ panes, tabs });
  const managerRow = host.querySelector('[data-tab-id="tab-manager"]');
  const workerRow = host.querySelector('[data-tab-id="tab-worker"]');
  assert(!managerRow?.textContent?.includes("manager description"), "manager description is not a row line");
  assert(!workerRow?.textContent?.includes("worker description"), "worker description is not a row line");
  assert(!workerRow?.querySelector('[data-testid="tab-subline"]'), "worker remains a single-line row");
  assert(managerRow?.querySelectorAll('[data-testid="tab-subline"]').length === 1, "manager has only its role subline");
  await act(async () => root.unmount());
});

test("team worker close control calls onCloseTab", async () => {
  const closed: string[] = [];
  const panes: Record<string, Pane> = {
    orchestrator: pane({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Orchestrator",
      team_id: TEAM.id,
    }),
    worker: pane({
      id: "worker",
      title: "Worker",
      role: "Worker",
      team_id: TEAM.id,
      agent_type: "claude",
    }),
  };
  const tabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-worker", "worker", 1)];
  const { host, root } = await renderSidebar({ panes, tabs, onCloseTab: (id) => closed.push(id) });
  const row = host.querySelector('[data-tab-id="tab-worker"]');
  assert(row, "worker row rendered");
  await openTabActionsMenu(row);
  const close = document.querySelector('[data-testid="tab-menu-close"]') as HTMLButtonElement | null;
  assert(close && !close.disabled, "worker close control enabled");
  await act(async () => close.click());
  assert(closed.includes("tab-worker"), "worker close calls onCloseTab");
  await act(async () => root.unmount());
});

test("team manager close control calls onCloseTab", async () => {
  const closed: string[] = [];
  const panes: Record<string, Pane> = {
    orchestrator: pane({
      id: "orchestrator",
      title: "Orchestrator",
      role: "Orchestrator",
      team_id: TEAM.id,
    }),
    manager: pane({
      id: "manager",
      title: "Planner",
      role: "Manager",
      role_profile: "planner",
      team_id: TEAM.id,
      agent_type: "claude",
    }),
  };
  const tabs = [tab("tab-orchestrator", "orchestrator", 0), tab("tab-manager", "manager", 1)];
  const { host, root } = await renderSidebar({ panes, tabs, onCloseTab: (id) => closed.push(id) });
  const row = host.querySelector('[data-tab-id="tab-manager"]');
  assert(row, "manager row rendered");
  await openTabActionsMenu(row);
  const close = document.querySelector('[data-testid="tab-menu-close"]') as HTMLButtonElement | null;
  assert(close && !close.disabled, "manager close control enabled");
  await act(async () => close.click());
  assert(closed.includes("tab-manager"), "manager close calls onCloseTab");
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
