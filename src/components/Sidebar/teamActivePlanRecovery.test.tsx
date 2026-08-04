// Run: npx tsx src/components/Sidebar/teamActivePlanRecovery.test.tsx

import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Tab, MultiplexerState, Team, Workspace } from "../../lib/types";
import { __setListenForTest } from "../../lib/bridge";
import { fireResync } from "../../lib/resync";
import { PlansProvider, useFullPlans, usePlanIndex } from "../Canvas/PlansProvider";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../Canvas/cards/fixtures.testdata";
import type { Plan, Task } from "../Canvas/types";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";

type ListenHandler = (event: { payload: unknown }) => void;

const API = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: true,
};
const TEAM_ID = "team-one";
const WORKSPACE_ID = "workspace-one";
const TAB_ID = "orchestrator-one";
const OLD_PLAN_ID = "pl_0ntinb";
const CURRENT_PLAN_ID = "pl_zmsp23";
const listenHandlers = new Map<string, Set<ListenHandler>>();

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/team-recovery" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
  Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
  (globalThis as { requestAnimationFrame: (callback: FrameRequestCallback) => number })
    .requestAnimationFrame = (callback) => {
      callback(0);
      return 0;
    };
  (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
}

function task(index: number): Task {
  return {
    ...FIXTURE_TASK,
    id: `stale-task-${index}`,
    name: `Stale task ${index}`,
    status: "pending",
  };
}

function plan(id: string, title: string, tasks: Task[], updatedAt: number): Plan {
  return {
    ...FIXTURE_PLAN,
    id,
    title,
    team_id: TEAM_ID,
    status: "active",
    tasks,
    created_at: updatedAt,
    updated_at: updatedAt,
  } as Plan;
}

const oldPlan = plan(
  OLD_PLAN_ID,
  "Old active effort",
  Array.from({ length: 35 }, (_, index) => task(index)),
  1,
);
const currentPlan = plan(CURRENT_PLAN_ID, "Current active effort", [], 2);

function indexEntry(value: Plan): PlanIndexEntry {
  return {
    plan_id: value.id,
    file_path: `/workspace/.codeterm/plans/active/${value.id}/plan.json`,
    canonical_project_path: "/workspace",
    owner: value.owner,
    title: value.title,
    status: value.status,
    updated_at: value.updated_at,
    team_id: TEAM_ID,
    assigned_tab_ids: [TAB_ID],
    task_count: value.tasks.length,
    open_task_count: value.tasks.length,
  };
}

function snapshot(teamName: string): MultiplexerState {
  const tab = {
    id: TAB_ID,
    pty_id: "pty-orchestrator",
    tab_type: "Terminal",
    title: "Orchestrator",
    cwd: "/workspace",
    role: "Orchestrator",
    role_profile: null,
    parent_tab_id: null,
    team_id: TEAM_ID,
  } as Tab & { team_id: string };
  return {
    workspaces: [{
      id: WORKSPACE_ID,
      name: "Workspace",
      tabs: [TAB_ID],
      active_tab_id: TAB_ID,
      cwd: "/workspace",
    }],
    tabs: { [TAB_ID]: tab },
    teams: [{
      id: TEAM_ID,
      name: teamName,
      lifecycle: "active",
      created_at: 1,
      workspace_id: WORKSPACE_ID,
    }],
    active_workspace_id: WORKSPACE_ID,
    focused_tab_id: TAB_ID,
  };
}

let restAvailable = true;
let backendPlans = [oldPlan];
let backendSnapshot = snapshot("Old Team label");
let requests: Promise<unknown>[] = [];
let renameAttempts = 0;

function tracked<T>(promise: Promise<T>): Promise<T> {
  requests.push(promise);
  return promise;
}

async function settleRequests() {
  const pending = requests;
  requests = [];
  await Promise.allSettled(pending);
  await Promise.resolve();
}

function response(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

function installFetch() {
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: (input: RequestInfo | URL) => tracked((async () => {
      if (!restAvailable) throw new TypeError("REST unavailable");
      const url = String(input);
      if (url.includes("/api/plans?full=1")) return response({ plans: backendPlans });
      if (url.endsWith("/api/plans")) {
        return response({ plans: backendPlans.map(indexEntry) });
      }
      if (url.endsWith("/api/mux/state")) return response(backendSnapshot);
      const planId = url.match(/\/api\/plans\/([^/?]+)$/)?.[1];
      const found = backendPlans.find((value) => value.id === planId);
      return found ? response(found) : new Response("not found", { status: 404 });
    })()),
  });
}

async function installListen() {
  __setListenForTest(async (event, handler) => {
    const handlers = listenHandlers.get(event) ?? new Set<ListenHandler>();
    handlers.add(handler as ListenHandler);
    listenHandlers.set(event, handlers);
    return () => handlers.delete(handler as ListenHandler);
  });
}

function emitPlanChanged(planId: string | null) {
  for (const handler of listenHandlers.get("plan-changed") ?? []) {
    handler({ payload: planId ? { plan_id: planId } : null });
  }
}

function fireExistingFullResync() {
  window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
  emitPlanChanged(null);
  fireResync();
}

function Projection() {
  const full = useFullPlans();
  const index = usePlanIndex();
  const [mux, setMux] = useState(backendSnapshot);

  useEffect(() => {
    const refresh = () => {
      void fetch("http://127.0.0.1:7686/api/mux/state")
        .then((value) => value.json())
        .then((value) => setMux(value as MultiplexerState));
    };
    window.addEventListener("codeterm-refresh-mux-state", refresh);
    return () => window.removeEventListener("codeterm-refresh-mux-state", refresh);
  }, []);

  const workspace = mux.workspaces[0] as Workspace;
  const tabs = workspace.tabs.map((tabId) => mux.tabs[tabId]).filter(Boolean);
  const teams = (mux.teams ?? []) as Team[];
  return (
    <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
      <SidebarWorkspaceTabs
        workspace={workspace}
        activeTabId={workspace.active_tab_id}
        sortedTabs={tabs}
        tabs={mux.tabs}
        tabStatuses={{}}
        teams={teams}
        planIndex={index.index}
        plans={full.plans}
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
        orchestrators={Object.values(mux.tabs)}
        onBeforeOpenPlan={() => {}}
        onActivateTab={() => {}}
        onCloseTab={() => {}}
        onRenameTab={() => {}}
        onRenameTeam={() => {
          renameAttempts += 1;
        }}
        onNewSession={() => {}}
      />
    </SidebarDnd>
  );
}

function assertCurrentProjection(host: HTMLElement) {
  assert(host.querySelector('[data-testid="team-name"]')?.textContent === "Current active effort",
    "the visible active-effort title comes from the same current plan as the accessible title");
  assert(host.querySelector('[data-testid="team-card-activation"]')
    ?.getAttribute("aria-label") === "Open Current active effort",
  "the sole active Team plan is pl_zmsp23");
  assert(!host.textContent?.includes("Old Team label"),
    "the old persistent Team name is not rendered as the active effort title");
  assert(!host.textContent?.includes("Current Team label"),
    "persistent Team identity does not override an active effort title");
  assert(!host.textContent?.includes("Old active effort"), "the archived effort is absent");
  const taskCount = ["todo", "review", "blocked", "done"]
    .map((bucket) => host.querySelector(`[data-testid="team-status-${bucket}"] .ct-plan-status-breakdown-count`))
    .reduce((sum, element) => sum + Number(element?.textContent ?? Number.NaN), 0);
  assert(taskCount === 0, "all stale task buckets are zero");
}

async function main() {
  setupDom();
  installFetch();
  await installListen();
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(
      <PlansProvider api={API}>
        <Projection />
      </PlansProvider>,
    );
    await Promise.resolve();
  });
  await act(settleRequests);
  assert(host.querySelector('[data-testid="team-card-activation"]')
    ?.getAttribute("aria-label") === "Open Old active effort",
  "precondition: old active plan is visible");
  assert(host.querySelector('[data-testid="team-status-todo"] .ct-plan-status-breakdown-count')
    ?.textContent === "35", "precondition: 35 stale tasks are visible");
  await act(async () => {
    (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click();
  });
  assert(!document.querySelector('[data-testid="team-menu"]')?.textContent?.includes("Rename"),
    "an active effort does not expose a Team rename that cannot change its visible title");
  assert(renameAttempts === 0, "the hidden active-effort rename cannot mutate Team identity");
  await act(async () => {
    (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click();
  });

  restAvailable = false;
  await act(async () => {
    emitPlanChanged(OLD_PLAN_ID);
    emitPlanChanged(CURRENT_PLAN_ID);
    await Promise.resolve();
  });
  await act(settleRequests);
  assert(host.querySelector('[data-testid="team-card-activation"]')
    ?.getAttribute("aria-label") === "Open Old active effort",
  "REST failure intentionally retains the old projection");

  backendPlans = [currentPlan];
  backendSnapshot = snapshot("Current Team label");
  restAvailable = true;
  await act(async () => {
    fireExistingFullResync();
    await Promise.resolve();
  });
  await act(settleRequests);
  assertCurrentProjection(host);

  await act(async () => {
    emitPlanChanged(CURRENT_PLAN_ID);
    await Promise.resolve();
  });
  await act(settleRequests);
  assertCurrentProjection(host);

  await act(async () => root.unmount());
  __setListenForTest(null);
  console.log("  ✓ Team active-plan recovery converges after one existing full resync");
}

void main().catch((error) => {
  console.error(`  ✗ ${error instanceof Error ? error.message : error}`);
  process.exitCode = 1;
});
