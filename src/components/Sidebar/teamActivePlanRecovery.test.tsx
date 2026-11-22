import { FIXTURE_PLAN_READ } from "../Canvas/cards/fixtures.testdata";
import { muxMockFetch, settleDaemon } from "../../lib/muxMockFetch.testdata";
// Run: npx tsx src/components/Sidebar/teamActivePlanRecovery.test.tsx

import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Tab, MultiplexerState, Team, Workspace } from "../../lib/types";
import { __setListenForTest } from "../../lib/bridge";
import { fireResync } from "../../lib/resync";
import { PlansProvider, usePeerPlanSummaries, usePlanIndex, useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { selectRemoteTeamPlanProjection } from "./teamTaskStatus";
import { TeamUnit } from "./TeamUnit";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { PlanSummaryRead } from "../Canvas/planProjection";
import { FIXTURE_PLAN, FIXTURE_TASK, FIXTURE_TASK_METADATA } from "../Canvas/cards/fixtures.testdata";
import type { Plan, Task } from "../Canvas/types";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";

type ListenHandler = (event: { payload: unknown }) => void;

const API = {mesh_tailnet_host: null, cors_allow_origins: [], preview_secret_paths: [],
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
  Object.assign(window, { __CODETERM_DAEMON__: { origin: window.location.origin, token: "test-token" } });
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
    file_path: `/workspace/.domios/plans/active/${value.id}/plan.json`,
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

function summary(value: Plan): PlanSummaryRead {
  return {
    ...FIXTURE_PLAN_READ,
    id: value.id, title: value.title, status: value.status, owner: value.owner,
    project_path: value.project_path, canonical_project_path: value.project_path,
    created_at: value.created_at, updated_at: value.updated_at, canvas: value.canvas,
    mode: value.mode ?? "planning", planning_state: value.planning_state ?? "drafting",
    team_id: value.team_id ?? null,
    revision: String(value.updated_at),
    tasks: [],
    task_counts: { total: value.tasks.length, pending: value.tasks.length, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0 },
  };
}

function snapshot(teamName: string): MultiplexerState {
  const tab = {
    id: TAB_ID,
    pty_id: "pty-orchestrator",
    tab_type: "Terminal",
    title: "Orchestrator",
    cwd: "/workspace",
    role: "Manager",
    role_profile: "orchestrator",
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

// The fixture daemon; the UI reaches it through the mux carrier, the probe's own mux snapshot reads it directly.
const daemon = (input: RequestInfo | URL) => tracked((async () => {
      if (!restAvailable) throw new TypeError("REST unavailable");
      const url = String(input);
      if (url.endsWith("/api/plans?view=summary")) return response({ plans: backendPlans.map(summary) });
      if (url.endsWith("/api/plans")) {
        return response({ plans: backendPlans.map(indexEntry) });
      }
      if (url.endsWith("/api/mux/state")) return response(backendSnapshot);
      const planId = url.match(/\/api\/plans\/([^/?]+)\/projection\?view=summary$/)?.[1];
      const found = backendPlans.find((value) => value.id === planId);
      return found ? response(summary(found)) : new Response("not found", { status: 404 });
    })());

function installFetch() {
  Object.defineProperty(globalThis, "fetch", { configurable: true, value: muxMockFetch(daemon) });
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
    handler({ payload: planId ? { plan_id: planId, revision: "2", previous_revision: "1", task_ids: [], reset: false, deleted: false } : null });
  }
}

function fireExistingFullResync() {
  window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
  emitPlanChanged(null);
  fireResync();
}

function Projection() {
  const full = useSidebarPlansOptional();
  const index = usePlanIndex();
  const [mux, setMux] = useState(backendSnapshot);

  useEffect(() => {
    const refresh = () => {
      void daemon("http://127.0.0.1:7686/api/mux/state")
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
    <SidebarDnd handlers={{ reorderTabs: () => {}, reorderDisplay: () => {}, reorderWorkspaces: () => {} }}>
      <SidebarWorkspaceTabs
        workspace={workspace}
        activeTabId={workspace.active_tab_id}
        sortedTabs={tabs}
        tabs={mux.tabs}
        tabStatuses={{}}
        teams={teams}
        planIndex={index.index}
        plans={full.plans}
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
  assert(host.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "stale",
    "retained same-authority projection exposes explicit stale freshness instead of inventing zeros");

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
  host.remove();

  // Verify unknown/unavailable counts, index-only positive total, authoritative index title,
  // matching nonzero summary, genuine zero, known absence, and credential change fencing.
  const LARGE_PLAN_ID = "pl_7lsisa";
  const largeIndexEntry: PlanIndexEntry = {
    plan_id: LARGE_PLAN_ID,
    file_path: `/workspace/.domios/plans/active/${LARGE_PLAN_ID}/plan.json`,
    canonical_project_path: "/workspace",
    owner: FIXTURE_PLAN.owner,
    title: "Domios",
    status: "active",
    updated_at: 100,
    team_id: TEAM_ID,
    assigned_tab_ids: [TAB_ID],
    task_count: 1625,
    open_task_count: 46,
  };
  const largeSummary: PlanSummaryRead = {
    ...FIXTURE_PLAN_READ,
    id: LARGE_PLAN_ID,
    title: "Domios",
    status: "active",
    owner: FIXTURE_PLAN.owner,
    project_path: "/workspace",
    canonical_project_path: "/workspace",
    created_at: 100,
    updated_at: 100,
    canvas: FIXTURE_PLAN.canvas,
    mode: "planning",
    planning_state: "drafting",
    team_id: TEAM_ID,
    revision: "100",
    tasks: [],
    task_counts: {
      total: 1625,
      pending: 30,
      in_progress: 6,
      in_review: 10,
      blocked: 0,
      paused: 0,
      done: 1579,
      cancelled: 0,
    },
  };

  let releaseIndex!: () => void;
  let releaseSummary!: () => void;
  let releaseOldCredSummary!: () => void;
  let indexRequested!: () => void;
  const indexReady = new Promise<void>((resolve) => { indexRequested = resolve; });
  let summaryRequested!: () => void;
  const summaryReady = new Promise<void>((resolve) => { summaryRequested = resolve; });
  let oldCredSummaryRequested!: () => void;
  const oldCredSummaryReady = new Promise<void>((resolve) => { oldCredSummaryRequested = resolve; });
  let stage: "hold-both" | "verified-zero" | "absent" | "hold-cred" | "denied" = "hold-both";
  backendSnapshot = snapshot("Domios 1.16.1");

  const prevFetch = globalThis.fetch;
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: muxMockFetch(async (input) => {
      const url = String(input);
      if (url.endsWith("/api/mux/state")) return response(backendSnapshot);
      if (stage === "denied") return new Response("forbidden", { status: 403 });
      if (stage === "absent") {
        return response({ plans: [] });
      }
      if (stage === "verified-zero") {
        return url.endsWith("/api/plans?view=summary")
          ? response({ plans: [summary(currentPlan)] })
          : response({ plans: [indexEntry(currentPlan)] });
      }
      if (stage === "hold-cred") {
        if (url.endsWith("/api/plans?view=summary")) {
          return new Promise<Response>((resolve) => {
            releaseOldCredSummary = () => resolve(response({ plans: [largeSummary] }));
            oldCredSummaryRequested();
          });
        }
        return response({ plans: [largeIndexEntry] });
      }
      if (url.endsWith("/api/plans?view=summary")) {
        return new Promise<Response>((resolve) => {
          releaseSummary = () => resolve(response({ plans: [largeSummary] }));
          summaryRequested();
        });
      }
      if (url.endsWith("/api/plans")) {
        return new Promise<Response>((resolve) => {
          releaseIndex = () => resolve(response({ plans: [largeIndexEntry] }));
          indexRequested();
        });
      }
      return new Response("not found", { status: 404 });
    }),
  });

  let indexObservedInView!: () => void;
  const indexRendered = new Promise<void>((resolve) => { indexObservedInView = resolve; });
  function IndexObserver() {
    const { index } = usePlanIndex();
    useEffect(() => {
      if (index.some((entry) => entry.plan_id === LARGE_PLAN_ID)) indexObservedInView();
    }, [index]);
    return null;
  }

  const stageHost = document.createElement("div");
  document.body.appendChild(stageHost);
  const stageRoot = createRoot(stageHost);
  try {
    await act(async () => {
      stageRoot.render(<PlansProvider api={API}><IndexObserver /><Projection /></PlansProvider>);
      await Promise.all([indexReady, summaryReady]);
    });
    assert(
      stageHost.querySelector('[data-testid="team-status-unavailable"]') !== null,
      "missing projection while loading renders unavailable status instead of false zeros",
    );
    assert(
      stageHost.querySelector('[data-testid="team-status-tally"]') === null,
      "no false-zero bucket tally while projection is missing and loading",
    );

    await act(async () => {
      releaseIndex();
      await indexRendered;
    });
    assert(
      stageHost.querySelector('[data-testid="team-name"]')?.textContent === "Domios",
      "authoritative index title replaces legacy Team display name while summary counts load",
    );
    assert(
      stageHost.querySelector('[data-testid="team-status-unavailable"]')?.textContent?.includes("1625 tasks"),
      "index-only positive task_count reports loading counts instead of fabricating zeros",
    );
    assert(
      stageHost.querySelector('[data-testid="team-status-tally"]') === null,
      "index-only positive total never renders zero buckets",
    );

    await act(async () => {
      releaseSummary();
      await settleDaemon();
    });
    assert(
      stageHost.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "verified",
      "matching summary marks counts verified",
    );
    assert(
      stageHost.querySelector('[data-testid="team-status-todo"] .ct-plan-status-breakdown-count')?.textContent === "36"
      && stageHost.querySelector('[data-testid="team-status-review"] .ct-plan-status-breakdown-count')?.textContent === "10"
      && stageHost.querySelector('[data-testid="team-status-blocked"] .ct-plan-status-breakdown-count')?.textContent === "0"
      && stageHost.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent === "1579",
      "matching nonzero summary renders exact task buckets",
    );

    stage = "verified-zero";
    await act(async () => {
      fireExistingFullResync();
      await settleDaemon();
    });
    assert(
      stageHost.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "verified",
      "genuine empty summary marks zero buckets verified",
    );
    assert(
      ["todo", "review", "blocked", "done"].every(
        (bucket) => stageHost.querySelector(`[data-testid="team-status-${bucket}"] .ct-plan-status-breakdown-count`)?.textContent === "0",
      ),
      "genuine zero plan renders verified zero counts",
    );

    stage = "absent";
    await act(async () => {
      fireExistingFullResync();
      await settleDaemon();
    });
    assert(
      stageHost.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "absent",
      "confirmed absence of an active plan is distinguished from unavailable counts",
    );

    stage = "hold-cred";
    await act(async () => {
      stageRoot.render(<PlansProvider api={{ ...API, token: "cred-1" }}><IndexObserver /><Projection /></PlansProvider>);
      await oldCredSummaryReady;
    });
    stage = "denied";
    await act(async () => {
      stageRoot.render(<PlansProvider api={{ ...API, token: "cred-2" }}><IndexObserver /><Projection /></PlansProvider>);
      await settleDaemon();
    });
    await act(async () => {
      releaseOldCredSummary();
      await settleDaemon();
    });
    assert(
      stageHost.querySelector('[data-testid="team-status-unavailable"]') !== null,
      "access denial after credential change clears old-authority projection and stays unavailable",
    );
    assert(
      stageHost.querySelector('[data-testid="team-name"]')?.textContent !== "Domios",
      "late old-credential reply cannot restore forbidden plan title",
    );
  } finally {
    await act(async () => stageRoot.unmount());
    stageHost.remove();
    Object.defineProperty(globalThis, "fetch", { configurable: true, value: prevFetch });
  }

  // Source counterexample: old zero summary (updated_at 100, total 0) -> newer index (updated_at 200,
  // task_count 94) while replacement summary is held -> convergence to Done 94 (with 25 loaded rows),
  // plus epoch/credential replacement while held.
  const COUNTER_PLAN_ID = "pl_counter";
  const oldZeroSummary: PlanSummaryRead = {
    ...FIXTURE_PLAN_READ,
    id: COUNTER_PLAN_ID,
    title: "Counter effort",
    status: "active",
    owner: FIXTURE_PLAN.owner,
    project_path: "/workspace",
    canonical_project_path: "/workspace",
    created_at: 100,
    updated_at: 100,
    canvas: FIXTURE_PLAN.canvas,
    mode: "planning",
    planning_state: "drafting",
    team_id: TEAM_ID,
    revision: "100",
    tasks: [],
    task_counts: { total: 0, pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0 },
  };
  const oldZeroIndex: PlanIndexEntry = {
    plan_id: COUNTER_PLAN_ID,
    file_path: `/workspace/.domios/plans/active/${COUNTER_PLAN_ID}/plan.json`,
    canonical_project_path: "/workspace",
    owner: FIXTURE_PLAN.owner,
    title: "Counter effort",
    status: "active",
    updated_at: 100,
    team_id: TEAM_ID,
    assigned_tab_ids: [TAB_ID],
    task_count: 0,
    open_task_count: 0,
  };
  const newerIndex94: PlanIndexEntry = {
    ...oldZeroIndex,
    updated_at: 200,
    task_count: 94,
    open_task_count: 0,
  };
  const newerSummary94: PlanSummaryRead = {
    ...oldZeroSummary,
    updated_at: 200,
    revision: "200",
    tasks: Array.from({ length: 25 }, (_, i) => ({
      ...FIXTURE_TASK_METADATA,
      id: `counter-${i}`,
      name: `Counter ${i}`,
      status: "done" as const,
    })),
    task_counts: { total: 94, pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 94, cancelled: 0 },
  };
  let counterPhase: "old-zero" | "hold-replacement" | "epoch-replaced" = "old-zero";
  let releaseReplacementSummary!: () => void;
  let replacementRequested!: () => void;
  const replacementReady = new Promise<void>((resolve) => { replacementRequested = resolve; });
  let counterIndexUpdated!: () => void;
  const counterIndexReady = new Promise<void>((resolve) => { counterIndexUpdated = resolve; });

  const counterPrevFetch = globalThis.fetch;
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: muxMockFetch(async (input) => {
      const url = String(input);
      if (url.endsWith("/api/mux/state")) return response(backendSnapshot);
      if (counterPhase === "old-zero") {
        return url.endsWith("/api/plans?view=summary")
          ? response({ plans: [oldZeroSummary] })
          : response({ plans: [oldZeroIndex] });
      }
      if (counterPhase === "hold-replacement") {
        if (url.endsWith("/api/plans")) return response({ plans: [newerIndex94] });
        if (url.includes(`/api/plans/${COUNTER_PLAN_ID}/projection`) || url.endsWith("/api/plans?view=summary")) {
          return new Promise<Response>((resolve) => {
            releaseReplacementSummary = () => resolve(response(
              url.includes("/projection") ? newerSummary94 : { plans: [newerSummary94] },
            ));
            replacementRequested();
          });
        }
      }
      if (counterPhase === "epoch-replaced") {
        const replacedSummary: PlanSummaryRead = {
          ...oldZeroSummary,
          id: "pl_epoch_new",
          title: "Post-epoch effort",
          updated_at: 300,
          revision: "1",
          task_counts: { total: 5, pending: 2, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 3, cancelled: 0 },
        };
        const replacedIndex: PlanIndexEntry = {
          ...oldZeroIndex,
          plan_id: "pl_epoch_new",
          title: "Post-epoch effort",
          updated_at: 300,
          task_count: 5,
          open_task_count: 2,
        };
        return url.endsWith("/api/plans?view=summary")
          ? response({ plans: [replacedSummary] })
          : response({ plans: [replacedIndex] });
      }
      return new Response("not found", { status: 404 });
    }),
  });

  function CounterIndexObserver() {
    const { index } = usePlanIndex();
    useEffect(() => {
      if (index.some((entry) => entry.plan_id === COUNTER_PLAN_ID && entry.task_count === 94)) counterIndexUpdated();
    }, [index]);
    return null;
  }

  const counterHost = document.createElement("div");
  document.body.appendChild(counterHost);
  const counterRoot = createRoot(counterHost);
  try {
    await act(async () => {
      counterRoot.render(<PlansProvider api={API}><CounterIndexObserver /><Projection /></PlansProvider>);
      await settleDaemon();
    });
    assert(
      counterHost.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "verified"
      && counterHost.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent === "0",
      "precondition: old summary updated_at 100 starts at verified 0",
    );

    counterPhase = "hold-replacement";
    await act(async () => {
      for (const handler of listenHandlers.get("plan-changed") ?? []) {
        handler({ payload: { plan_id: COUNTER_PLAN_ID, revision: "200", previous_revision: "100", task_ids: [], reset: false, deleted: false } });
      }
      await Promise.all([counterIndexReady, replacementReady]);
    });
    assert(
      counterHost.querySelector('[data-testid="team-status-unavailable"]')?.textContent?.includes("94 tasks"),
      "newer index task_count 94 over older zero summary preserves known total 94 while replacement summary is held",
    );
    assert(
      counterHost.querySelector('[data-testid="team-status-tally"]') === null,
      "superseded zero buckets are never shown as verified zero when newer index reports 94 tasks",
    );

    await act(async () => {
      releaseReplacementSummary();
      await settleDaemon();
    });
    assert(
      counterHost.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "verified"
      && counterHost.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent === "94",
      "matching replacement summary converges to verified Done 94 without counting 25 loaded rows",
    );

    counterPhase = "hold-replacement";
    let releaseEpochHeld!: () => void;
    let epochHeldRequested!: () => void;
    const epochHeldReady = new Promise<void>((resolve) => { epochHeldRequested = resolve; });
    Object.defineProperty(globalThis, "fetch", {
      configurable: true,
      value: muxMockFetch(async (input) => {
        const url = String(input);
        if (url.endsWith("/api/mux/state")) return response(backendSnapshot);
        if (counterPhase === "hold-replacement") {
          if (url.includes("/projection")) {
            return new Promise<Response>((resolve) => {
              releaseEpochHeld = () => resolve(response({ ...newerSummary94, revision: "201" }));
              epochHeldRequested();
            });
          }
          return response({ plans: [newerIndex94] });
        }
        const replacedSummary: PlanSummaryRead = {
          ...oldZeroSummary,
          id: "pl_epoch_new",
          title: "Post-epoch effort",
          updated_at: 300,
          revision: "1",
          task_counts: { total: 5, pending: 2, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 3, cancelled: 0 },
        };
        const replacedIndex: PlanIndexEntry = {
          ...oldZeroIndex,
          plan_id: "pl_epoch_new",
          title: "Post-epoch effort",
          updated_at: 300,
          task_count: 5,
          open_task_count: 2,
        };
        return url.endsWith("/api/plans?view=summary")
          ? response({ plans: [replacedSummary] })
          : response({ plans: [replacedIndex] });
      }),
    });
    await act(async () => {
      for (const handler of listenHandlers.get("plan-changed") ?? []) {
        handler({ payload: { plan_id: COUNTER_PLAN_ID, revision: "201", previous_revision: "200", task_ids: [], reset: false, deleted: false } });
      }
      await epochHeldReady;
    });
    counterPhase = "epoch-replaced";
    await act(async () => {
      fireResync("epoch_changed");
      await settleDaemon();
    });
    await act(async () => {
      releaseEpochHeld();
      await settleDaemon();
    });
    assert(
      counterHost.querySelector('[data-testid="team-name"]')?.textContent === "Post-epoch effort"
      && counterHost.querySelector('[data-testid="team-status-todo"] .ct-plan-status-breakdown-count')?.textContent === "2"
      && counterHost.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent === "3",
      "epoch replacement clears held pre-epoch summary and rejects late pre-epoch reply",
    );
  } finally {
    await act(async () => counterRoot.unmount());
    counterHost.remove();
    Object.defineProperty(globalThis, "fetch", { configurable: true, value: counterPrevFetch });
  }

  // Remote browsed peer Team cards (eight / Sauron case): single bounded peer summary read,
  // held -> unavailable, authoritative Done 94 (never loaded 25), genuine empty, foreign Team fenced,
  // peer switch and late-reply fenced.
  const { peerEventRail } = await import("../../lib/peerEventRail");
  const remoteListeners = new Map<string, Set<(event: import("../../lib/peerEventRail").PeerEvent) => void>>();
  const originalObserve = peerEventRail.observe;
  peerEventRail.observe = (peerId, listener) => {
    const set = remoteListeners.get(peerId) ?? new Set();
    set.add(listener);
    remoteListeners.set(peerId, set);
    return () => { set.delete(listener); };
  };
  let releaseEight!: () => void;
  let eightRequested!: () => void;
  const eightReady = new Promise<void>((resolve) => { eightRequested = resolve; });
  let releaseEightSwitch!: () => void;
  let eightSwitchRequested!: () => void;
  const eightSwitchReady = new Promise<void>((resolve) => { eightSwitchRequested = resolve; });
  let eightMode: "hold" | "hold-switch" = "hold";
  let eightSummaryRequests = 0;

  const relayJson = (data: unknown) => response({ ok: true, status: 200, data });
  const eightPlans: PlanSummaryRead[] = [
    {
      ...FIXTURE_PLAN_READ,
      id: "pl_rek",
      title: "Rekap: Navid Playground",
      status: "active",
      owner: FIXTURE_PLAN.owner,
      project_path: "/sauron",
      canonical_project_path: "/sauron",
      created_at: 94,
      updated_at: 94,
      canvas: FIXTURE_PLAN.canvas,
      mode: "planning",
      planning_state: "drafting",
      team_id: "team_rekap",
      revision: "94",
      tasks: Array.from({ length: 25 }, (_, i) => ({
        ...FIXTURE_TASK_METADATA,
        id: `rek-${i}`,
        name: `REK-${i}`,
        status: "done" as const,
      })),
      task_counts: { total: 94, pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 94, cancelled: 0 },
    },
    {
      ...FIXTURE_PLAN_READ,
      id: "pl_empty_remote",
      title: "Project X",
      status: "active",
      owner: FIXTURE_PLAN.owner,
      project_path: "/sauron",
      canonical_project_path: "/sauron",
      created_at: 1,
      updated_at: 1,
      canvas: FIXTURE_PLAN.canvas,
      mode: "planning",
      planning_state: "drafting",
      team_id: "team_project_x",
      revision: "1",
      tasks: [],
      task_counts: { total: 0, pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0 },
    },
  ];
  const ubuntaPlans: PlanSummaryRead[] = [
    {
      ...FIXTURE_PLAN_READ,
      id: "pl_ubunta",
      title: "Ubunta effort",
      status: "active",
      owner: FIXTURE_PLAN.owner,
      project_path: "/ubunta",
      canonical_project_path: "/ubunta",
      created_at: 2,
      updated_at: 2,
      canvas: FIXTURE_PLAN.canvas,
      mode: "planning",
      planning_state: "drafting",
      team_id: "team_rekap",
      revision: "2",
      tasks: [],
      task_counts: { total: 4, pending: 2, in_progress: 0, in_review: 1, blocked: 0, paused: 0, done: 1, cancelled: 0 },
    },
  ];

  const remotePrevFetch = globalThis.fetch;
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: muxMockFetch(async (input, init) => {
      const url = String(input);
      if (url.endsWith("/api/plans")) return response({ plans: [] });
      if (url.endsWith("/api/mesh/relay-to")) {
        const envelope = JSON.parse(String(init?.body)) as { target_instance_id: string; path: string };
        if (envelope.target_instance_id === "eight" && envelope.path === "/api/plans?view=summary") {
          eightSummaryRequests += 1;
          if (eightMode === "hold") {
            return new Promise<Response>((resolve) => {
              releaseEight = () => resolve(relayJson({ plans: eightPlans }));
              eightRequested();
            });
          }
          return new Promise<Response>((resolve) => {
            releaseEightSwitch = () => resolve(relayJson({ plans: eightPlans }));
            eightSwitchRequested();
          });
        }
        if (envelope.target_instance_id === "ubunta" && envelope.path === "/api/plans?view=summary") {
          return relayJson({ plans: ubuntaPlans });
        }
      }
      return new Response("not found", { status: 404 });
    }),
  });

  function RemoteTeamCardsProbe({ peerId }: { peerId: string }) {
    const remote = usePeerPlanSummaries(peerId);
    const entries = [
      { id: "team_rekap", name: "Rekap: Navid Playground", ownerPeer: peerId },
      { id: "team_project_x", name: "Project X", ownerPeer: peerId },
      { id: "team_foreign", name: "Foreign Team", ownerPeer: "other-peer" },
    ];
    return (
      <>
        {entries.map((entry) => {
          const matched = selectRemoteTeamPlanProjection({
            teamId: entry.id,
            owningPeerId: entry.ownerPeer,
            viewedPeerId: peerId,
            plans: remote.plans,
            available: remote.available,
            loading: remote.loading,
            reconnecting: remote.reconnecting,
            error: remote.error,
          });
          return (
            <TeamUnit
              key={`${peerId}:${entry.id}`}
              team={{
                id: entry.id,
                name: entry.name,
                lifecycle: "active",
                created_at: 1,
                workspace_id: "sauron",
                remote_peer_id: entry.ownerPeer,
                remote_peer_name: entry.ownerPeer,
              }}
              plan={matched.plan}
              planCountMeta={matched.meta}
              tabs={{}}
              managers={null}
              managerCount={1}
              workers={null}
              workerCount={0}
              workingWorkerCount={0}
              providers={[]}
            />
          );
        })}
      </>
    );
  }

  const remoteHost = document.createElement("div");
  document.body.appendChild(remoteHost);
  const remoteRoot = createRoot(remoteHost);
  try {
    await act(async () => {
      remoteRoot.render(<PlansProvider api={API}><RemoteTeamCardsProbe peerId="eight" /></PlansProvider>);
      await eightReady;
    });
    assert(eightSummaryRequests === 1, "one bounded remote summary read serves all remote Team cards");
    assert(
      remoteHost.querySelectorAll('[data-testid="team-status-unavailable"]').length === 3
      && remoteHost.querySelectorAll('[data-testid="team-status-tally"]').length === 0,
      "held remote summary read keeps all remote Team cards unavailable instead of zero",
    );

    await act(async () => {
      releaseEight();
      await settleDaemon();
    });
    const rekapCard = remoteHost.querySelector('[data-team-id="team_rekap"]')!;
    const projectXCard = remoteHost.querySelector('[data-team-id="team_project_x"]')!;
    const foreignCard = remoteHost.querySelector('[data-team-id="team_foreign"]')!;
    assert(
      rekapCard.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "verified"
      && rekapCard.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent === "94"
      && rekapCard.querySelector('[data-testid="team-status-todo"] .ct-plan-status-breakdown-count')?.textContent === "0",
      "remote Team card renders authoritative Done 94, never loaded page 25 or fabricated 0",
    );
    assert(
      projectXCard.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state") === "verified"
      && projectXCard.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent === "0",
      "genuine empty remote plan renders verified zero buckets",
    );
    assert(
      foreignCard.querySelector('[data-testid="team-status-unavailable"]') !== null,
      "foreign-peer Team card stays unavailable rather than cross-matching or fabricating zeros",
    );

    eightMode = "hold-switch";
    await act(async () => {
      for (const listener of remoteListeners.get("eight") ?? []) listener({ type: "plans-loaded" });
      await eightSwitchReady;
    });
    await act(async () => {
      remoteRoot.render(<PlansProvider api={API}><RemoteTeamCardsProbe peerId="ubunta" /></PlansProvider>);
      await settleDaemon();
    });
    await act(async () => {
      releaseEightSwitch();
      await settleDaemon();
    });
    const ubuntaCard = remoteHost.querySelector('[data-team-id="team_rekap"]')!;
    assert(
      ubuntaCard.querySelector('[data-testid="team-status-todo"] .ct-plan-status-breakdown-count')?.textContent === "2"
      && ubuntaCard.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent === "1",
      "peer switch isolates remote counts and late eight reply cannot overwrite ubunta",
    );
  } finally {
    peerEventRail.observe = originalObserve;
    await act(async () => remoteRoot.unmount());
    remoteHost.remove();
    Object.defineProperty(globalThis, "fetch", { configurable: true, value: remotePrevFetch });
  }

  __setListenForTest(null);
  console.log("  ✓ Team active-plan recovery converges after one existing full resync");
}

void main().catch((error) => {
  console.error(`  ✗ ${error instanceof Error ? error.message : error}`);
  process.exitCode = 1;
});
