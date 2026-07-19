// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.test.tsx

import { readFileSync } from "node:fs";
import { act, useState } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Pane, PaneStatus, ProviderDescriptor, Tab, Team, Workspace } from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../Canvas/cards/fixtures.testdata";
import { TASK_STATUS_COLORS } from "../Canvas/settings";
import { TaskStatus, type Plan, type Task } from "../Canvas/types";
import { SidebarDnd } from "./sortable";
import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import { forwardTerminalFallback } from "../../lib/terminalFallbackInput";
import { __setInvokeForTest } from "../../lib/bridge";

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
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
  Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
  (globalThis as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (callback) => {
      callback(0);
      return 0;
    };
  (globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};
}

const sidebarStyles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");

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

function planIndexEntry(planId = "plan-team", teamId: string | null = TEAM.id): PlanIndexEntry {
  return {
    plan_id: planId,
    file_path: "/workspace/.codeterm/plans/active/plan-team/plan.json",
    owner: { pane_id: "orch", session_id: null, title: null },
    title: "Sidebar team effort",
    status: "active",
    updated_at: 1,
    team_id: teamId,
    assigned_pane_ids: ["orch"],
    task_count: 1,
  };
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
  planIndex = [],
  statuses = {},
  providers = [],
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
  planIndex?: PlanIndexEntry[];
  statuses?: Record<string, "Working" | "Waiting" | "Idle" | "Dead">;
  providers?: ProviderDescriptor[];
  activeTabId?: string | null;
  onActivateTab?: (tabId: string) => void;
  onCloseTab?: (tabId: string) => void;
  onRenameTeam?: (teamId: string, name: string, managerPaneId: string) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: "planner" | "worker",
    providerId: string,
    managerPaneId: string,
    model?: string,
  ) => void;
  onCloseTeam?: (teamId: string) => void;
}): Promise<{
  host: HTMLDivElement;
  root: Root;
  rerender: (nextPlans: Plan[], nextPlanIndex?: PlanIndexEntry[]) => Promise<void>;
}> {
  const workspace: Workspace = {
    id: "workspace",
    name: "Workspace",
    tabs,
    active_tab_id: activeTabId ?? tabs[0]?.id ?? null,
    cwd: "/workspace",
  };
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const rerender = async (nextPlans: Plan[], nextPlanIndex = planIndex) => act(async () => {
    root.render(
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={workspace.active_tab_id}
          sortedTabs={tabs}
          panes={panes}
          paneStatuses={statuses}
          teams={[TEAM]}
          planIndex={nextPlanIndex}
          plans={nextPlans}
          activeWork={null}
          remoteWorkers={{}}
          providers={providers}
          closingTabIds={new Set()}
          appearingTabIds={new Set()}
          isActiveVisual={true}
          workspaceCanvasActive={false}
          touchMode={true}
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
  await rerender(plans, planIndex);
  return { host, root, rerender };
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

test("plans have only a Team-owned sidebar home", async () => {
  const panes = {
    orch: pane({ id: "orch", title: "Orchestrator", role: "Orchestrator", team_id: TEAM.id }),
  };
  const tabs = [tab("tab-orch", "orch", 0)];
  const bound = plan([]);
  const detached = { ...bound, team_id: null } as Plan;
  const { host, root, rerender } = await renderSidebar({
    panes,
    tabs,
    plans: [bound],
    planIndex: [planIndexEntry()],
  });

  assert(host.querySelector('[data-testid="team-effort"]')?.textContent === bound.title, "bound plan is in its Team card");
  assert(!host.querySelector('[data-testid="workspace-plan-entry"]'), "bound plan has no standalone row");

  await rerender([detached], [planIndexEntry("plan-team", null)]);
  assert(!host.querySelector('[data-testid="team-effort"]'), "detach removes the plan from the Team card");
  assert(!host.querySelector('[data-testid="workspace-plan-entry"]'), "orphan plan has no workspace home");

  await rerender([bound], [planIndexEntry()]);
  assert(host.querySelector('[data-testid="team-effort"]')?.textContent === bound.title, "adopt restores the Team-owned plan");
  assert(!host.querySelector('[data-testid="workspace-plan-entry"]'), "adopt removes the standalone row without reload");
  await act(async () => root.unmount());
});

test("team menu enables staffing only for a Team with a manager", async () => {
  const closed: string[] = [];
  const panes = {
    manager: pane({ id: "manager", title: "Manager", role: "Orchestrator", team_id: TEAM.id }),
  };
  canvasFocusStore.closeWorkspace();
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-manager", "manager", 0)],
    onRenameTeam: () => {},
    onAddTeamMember: () => {},
    onCloseTeam: (teamId) => closed.push(teamId),
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "team menu trigger exists");
  assert(trigger.classList.contains("sidebar-team-kebab"), "team menu uses the same-slot kebab control");
  assert(trigger.tabIndex === 0, "team menu trigger remains keyboard reachable for the focus swap");
  assert(/\.sidebar-team-unit\s*\{[^}]*margin:\s*4px 0;[^}]*background:\s*color-mix\(in srgb, var\(--ct-fg\) 3%, transparent\);/.test(sidebarStyles), "Team cards use lighter tint and compact vertical cadence");
  assert(/\.sidebar-team-header\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\) auto;[^}]*gap:\s*6px;[^}]*margin:\s*0;[^}]*padding:\s*6px 7px 4px 10px;/.test(sidebarStyles), "Team header reclaims the former chevron column");
  assert(/\.sidebar-team-disclosure-bar\s*\{[^}]*position:\s*relative;[^}]*display:\s*block;[^}]*width:\s*100%;[^}]*height:\s*14px;/.test(sidebarStyles), "Team disclosure is a compact full-width bottom handle");
  assert(/\.sidebar-team-disclosure-glyph\s*\{[^}]*left:\s*50%;[^}]*transform:\s*translate\(-50%, -50%\);/.test(sidebarStyles), "Team disclosure glyph stays geometrically centered");
  assert(/\.sidebar-team-disclosure-glyph::before,\s*\.sidebar-team-disclosure-glyph::after\s*\{[^}]*width:\s*17px;[^}]*height:\s*1px;/.test(sidebarStyles), "Team disclosure uses a thin, restrained line");
  assert(sidebarStyles.includes("right: calc(50% - 0.5px);") && sidebarStyles.includes("left: calc(50% - 0.5px);"), "Team disclosure overlaps its two segments at the joint");
  assert(sidebarStyles.includes("transform: rotate(-8deg);"), "Team disclosure uses a gentle arrow transition");
  assert(/\.sidebar-team-disclosure-label,\s*\.sidebar-team-disclosure-count\s*\{[^}]*left:\s*calc\(50% \+ 24px\);/.test(sidebarStyles), "Team disclosure text cannot shift the centered glyph");
  assert(/@container \(max-width:\s*159px\)/.test(sidebarStyles), "compact tally threshold preserves the 160px Team matrix after inset reclaim");
  const header = host.querySelector('.sidebar-team-header') as HTMLDivElement | null;
  const chevron = host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement | null;
  assert(!header?.hasAttribute("role") && header?.tabIndex === -1, "Team header is a non-interactive control container");
  assert(chevron?.getAttribute("aria-expanded") === "true", "Team bottom handle exposes the expanded state");
  assert(chevron?.querySelector(".sidebar-team-disclosure-glyph"), "Team bottom handle renders the animated line glyph");
  assert(chevron?.querySelector(".sidebar-team-disclosure-label")?.textContent?.trim() === "Hide", "expanded Team exposes the hover Hide label");
  const managersSection = host.querySelector('[data-testid="managers-section"]');
  assert(Boolean(chevron && managersSection && (managersSection.compareDocumentPosition(chevron) & Node.DOCUMENT_POSITION_FOLLOWING)), "Team disclosure remains below the manager section");
  await act(async () => chevron?.click());
  assert(chevron?.getAttribute("aria-expanded") === "false", "bottom handle click collapses Team");
  assert(chevron?.querySelector(".sidebar-team-disclosure-label")?.textContent?.trim() === "Show", "collapsed Team exposes the hover Show label");
  assert(chevron?.querySelector(".sidebar-team-disclosure-count")?.textContent?.trim() === "1 tab", "collapsed Team shows its tab count beside the centered handle");
  await act(async () => chevron?.click());
  assert(chevron?.getAttribute("aria-expanded") === "true", "bottom handle click expands Team");
  assert(/\.sidebar-team-progress-slot\s*\{[^}]*width:\s*24px;[^}]*height:\s*20px;/.test(sidebarStyles), "done/total and kebab share one fixed slot");
  assert(/\.sidebar-team-readout-label\s*\{[^}]*margin-left:\s*3px;/.test(sidebarStyles), "full tally labels have explicit count spacing");
  assert(/\.sidebar-team-menu-root \.sidebar-team-kebab\s*\{[^}]*opacity:\s*0;/.test(sidebarStyles), "kebab hides by opacity without removing its slot");
  assert(/\.sidebar-team-menu-root \.sidebar-team-kebab\s*\{[^}]*opacity:\s*0;[^}]*pointer-events:\s*none;/.test(sidebarStyles), "hidden kebab ignores accidental pointer clicks");
  assert(sidebarStyles.includes(".sidebar-team-header:hover .sidebar-team-menu-root .sidebar-team-kebab"), "CSS hover swaps the kebab into the fixed slot");
  assert(sidebarStyles.includes('.sidebar-team-header:not([data-menu-dismissed="true"]):focus-within .sidebar-team-menu-root .sidebar-team-kebab'), "keyboard focus swaps the kebab into the fixed slot unless an outside pointer dismisses it");
  assert(sidebarStyles.includes("[data-touch-mode=\"true\"] .sidebar-team-menu-root .sidebar-team-kebab"), "touch mode persistently exposes the same-slot kebab");
  assert(!sidebarStyles.includes("data-menu-revealed"), "hover visibility has no React state attribute");
  const outsideFocus = document.createElement("button");
  host.appendChild(outsideFocus);
  await act(async () => {
    outsideFocus.focus();
    trigger.focus();
  });
  assert(!header?.hasAttribute("data-menu-dismissed"), "keyboard focus inside the header keeps the menu reveal eligible");
  await act(async () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
  assert(header?.getAttribute("data-menu-dismissed") === "true", "outside pointerdown dismisses the menu despite retained header focus");
  await act(async () => {
    outsideFocus.focus();
    trigger.focus();
  });
  assert(!header?.hasAttribute("data-menu-dismissed"), "a later keyboard focus restores the focus-within reveal");
  await act(async () => trigger.click());
  const menu = document.querySelector('[data-testid="team-menu"]');
  assert(menu?.textContent?.includes("Rename"), "supported rename remains available");
  assert(menu?.textContent?.includes("Add worker"), "worker staffing is enabled with a handler and manager");
  assert(menu?.textContent?.includes("Add planner"), "planner staffing is enabled with a handler and manager");
  assert(!menu?.textContent?.includes("Add orchestrator"), "orchestrator staffing remains hidden");
  assert(!menu?.textContent?.includes("Create effort"), "humans cannot create efforts from Team UI");
  assert(!menu?.textContent?.includes("Open effort"), "no effort action is shown without a plan");
  const close = Array.from(menu?.querySelectorAll("button") ?? []).find((button) => button.textContent?.includes("Close Team")) as HTMLButtonElement | undefined;
  assert(close, "Close Team action is available");
  await act(async () => close.click());
  assert(closed.join(",") === TEAM.id, "Close Team reports the exact Team id");
  assert(host.querySelector('[data-testid="team-icon"]'), "Team identity remains visible without a plan");
  assert(!host.querySelector('[data-testid="team-plan-button"]'), "no plan button is rendered without a plan");
  assert(!host.querySelector('[data-testid="team-status-tally"]'), "no plan tally is rendered without a plan");
  canvasFocusStore.closeWorkspace();
  await act(async () => root.unmount());
});

test("managerless Team hides staffing and makes no request", async () => {
  const requests: unknown[] = [];
  const panes = {
    worker: pane({ id: "worker", title: "Worker", role: "Worker", team_id: TEAM.id }),
  };
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-worker", "worker", 0)],
    onAddTeamMember: (...args) => requests.push(args),
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "team menu trigger exists");
  await act(async () => trigger.click());
  const menu = document.querySelector('[data-testid="team-menu"]');
  assert(!menu?.textContent?.includes("Add worker"), "managerless Team hides worker staffing");
  assert(!menu?.textContent?.includes("Add planner"), "managerless Team hides planner staffing");
  assert(!menu?.textContent?.includes("Rename"), "managerless Team hides rename");
  assert(requests.length === 0, "managerless Team does not issue a staffing request");
  await act(async () => root.unmount());
});

test("staffing selects the first eligible manager in visible Orchestrator-first order", async () => {
  const requests: Array<[string, "planner" | "worker", string, string]> = [];
  const panes = {
    unrelated: pane({ id: "unrelated", title: "Unrelated pane", role: "Worker" }),
    "a-O2": pane({ id: "a-O2", title: "Orchestrator Two", role: "Orchestrator", team_id: TEAM.id }),
    "p-P1": pane({ id: "p-P1", title: "Planner One", role: "Manager", role_profile: "planner", team_id: TEAM.id }),
    "z-O1": pane({ id: "z-O1", title: "Orchestrator One", role: "Orchestrator", team_id: TEAM.id }),
  };
  const providers = [{
    id: "registry-provider",
    display_name: "Registry provider",
    transport: "pty",
  }, {
    id: "not-spawn-capable",
    display_name: "Unsupported provider",
    transport: "unsupported",
  }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-unrelated", "unrelated", 0), tab("tab-O1", "z-O1", 1), tab("tab-O2", "a-O2", 2), tab("tab-P1", "p-P1", 3)],
    activeTabId: "tab-unrelated",
    providers,
    onAddTeamMember: (teamId, role, providerId, managerPaneId) => {
      requests.push([teamId, role, providerId, managerPaneId]);
    },
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement | null;
  assert(trigger, "team menu trigger exists");
  await act(async () => trigger.click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement | undefined;
  assert(worker, "worker staffing action exists");
  await act(async () => worker.click());
  const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement | undefined;
  assert(provider, "spawn-capable registry provider is offered");
  assert(
    !document.querySelector('[data-testid="team-menu"]')?.textContent?.includes("Unsupported provider"),
    "non-spawn-capable registry providers are absent",
  );
  await act(async () => provider.click());
  assert(
    JSON.stringify(requests) === JSON.stringify([[TEAM.id, "worker", "registry-provider", "z-O1"]]),
    "the first visible Orchestrator is passed as caller identity while an unrelated tab is active",
  );
  await act(async () => root.unmount());
});

test("staffing selects a planner when no Orchestrator is eligible", async () => {
  const requests: Array<[string, "planner" | "worker", string, string]> = [];
  const panes = {
    watcher: pane({ id: "A0", title: "Watcher", role: "Manager", role_profile: "watcher", team_id: TEAM.id }),
    planner: pane({ id: "P1", title: "Planner", role: "Manager", role_profile: "planner", team_id: TEAM.id }),
  };
  const providers = [{ id: "registry-provider", display_name: "Registry provider", transport: "pty" }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-watcher", "watcher", 0), tab("tab-planner", "planner", 1)],
    providers,
    onAddTeamMember: (teamId, role, providerId, managerPaneId) => requests.push([teamId, role, providerId, managerPaneId]),
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
  await act(async () => trigger.click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => worker.click());
  const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement;
  await act(async () => provider.click());
  assert(
    JSON.stringify(requests) === JSON.stringify([[TEAM.id, "worker", "registry-provider", "P1"]]),
    "planner is selected while ineligible manager profiles are skipped",
  );
  await act(async () => root.unmount());
});

test("staffing picker uses descriptor icons and requires a descriptor model choice", async () => {
  const requests: Array<[string, "planner" | "worker", string, string, string?]> = [];
  const panes = {
    manager: pane({ id: "manager", title: "Manager", role: "Orchestrator", team_id: TEAM.id }),
  };
  const providers = [{
    id: "provider-a",
    display_name: "Provider A",
    transport: "pty",
    icon_svg: '<svg id="provider-a-icon"/>',
    icon_color: "#d97757",
    available_models: [{ id: "model-one", display_name: "Model one" }, { id: "model-two", display_name: "Model two" }],
    default_model: "model-one",
    behavior_config: { model: "own_prompt_set" },
  }, {
    id: "provider-b",
    display_name: "Provider B",
    transport: "chat_backend",
    icon_hint: "B",
    behavior_config: { model: "none" },
  }, {
    id: "not-spawn-capable",
    display_name: "Unsupported provider",
    transport: "unsupported",
  }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-manager", "manager", 0)],
    providers,
    onAddTeamMember: (teamId, role, providerId, managerPaneId, model) => requests.push([teamId, role, providerId, managerPaneId, model]),
  });
  await act(async () => (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => worker.click());
  const providerA = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider A")) as HTMLButtonElement;
  assert(providerA.querySelector("svg#provider-a-icon"), "provider SVG comes from its descriptor");
  const providerB = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider B")) as HTMLButtonElement;
  assert(providerB.querySelector(".agent-icon")?.textContent === "B", "provider hint is the descriptor icon fallback");
  assert(!document.querySelector('[data-testid="team-menu"]')?.textContent?.includes("Unsupported provider"), "non-spawn-capable providers are absent");
  await act(async () => providerA.click());
  const defaultModel = document.querySelector('[data-testid="team-picker-model-model-one"]') as HTMLButtonElement;
  assert(defaultModel.getAttribute("aria-checked") === "true", "descriptor default model is preselected");
  const modelTwo = document.querySelector('[data-testid="team-picker-model-model-two"]') as HTMLButtonElement;
  await act(async () => modelTwo.click());
  assert(JSON.stringify(requests) === JSON.stringify([[TEAM.id, "worker", "provider-a", "manager", "model-two"]]), "model selection preserves manager caller identity");
  await act(async () => (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  const workerAgain = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => workerAgain.click());
  const providerAgain = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider A")) as HTMLButtonElement;
  await act(async () => providerAgain.click());
  const back = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Back")) as HTMLButtonElement;
  await act(async () => back.click());
  const restoredProvider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider A")) as HTMLButtonElement;
  assert(document.activeElement === restoredProvider, "Back restores focus to the selected provider");
  await act(async () => restoredProvider.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
  const restoredProviderB = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Provider B")) as HTMLButtonElement;
  assert(document.activeElement === restoredProviderB, "arrow navigation reaches every provider row");
  await act(async () => restoredProviderB.click());
  assert(JSON.stringify(requests) === JSON.stringify([
    [TEAM.id, "worker", "provider-a", "manager", "model-two"],
    [TEAM.id, "worker", "provider-b", "manager", undefined],
  ]), "model:none provider skips model selection and omits model from the request");
  await act(async () => (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  const menu = document.querySelector('[data-testid="team-menu"]') as HTMLDivElement;
  await act(async () => menu.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  assert(!document.querySelector('[data-testid="team-menu"]'), "Escape closes the picker without another request");
  await act(async () => root.unmount());
});

test("staffing failure stays visible without local mutation or refetch", async () => {
  const panes = {
    manager: pane({ id: "manager", title: "Manager", role: "Orchestrator", team_id: TEAM.id }),
  };
  const panesBefore = JSON.stringify(panes);
  const providers = [{ id: "registry-provider", display_name: "Registry provider", transport: "pty" }] as ProviderDescriptor[];
  const originalFetch = globalThis.fetch;
  let fetches = 0;
  globalThis.fetch = async () => {
    fetches += 1;
    throw new Error("unexpected refetch");
  };
  try {
    const { host, root } = await renderSidebar({
      panes,
      tabs: [tab("tab-manager", "manager", 0)],
      providers,
      onAddTeamMember: async () => {
        throw new Error("Team manager permission denied (403)");
      },
    });
    const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
    await act(async () => trigger.click());
    const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
      .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
    await act(async () => worker.click());
    const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
      .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement;
    await act(async () => provider.click());
    assert(
      document.querySelector('[data-testid="team-menu-error"]')?.textContent?.includes("403"),
      "403 staffing rejection is shown inline",
    );
    assert(JSON.stringify(panes) === panesBefore, "staffing rejection does not mutate local pane state");
    assert(fetches === 0, "staffing rejection does not refetch or poll");
    await act(async () => root.unmount());
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("staffing provider action ignores an immediate double click", async () => {
  let invocations = 0;
  let finish: (() => void) | undefined;
  const panes = {
    manager: pane({ id: "manager", title: "Manager", role: "Orchestrator", team_id: TEAM.id }),
  };
  const providers = [{ id: "registry-provider", display_name: "Registry provider", transport: "pty" }] as ProviderDescriptor[];
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-manager", "manager", 0)],
    providers,
    onAddTeamMember: () => {
      invocations += 1;
      return new Promise<void>((resolve) => {
        finish = resolve;
      });
    },
  });
  const trigger = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
  await act(async () => trigger.click());
  const worker = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Add worker")) as HTMLButtonElement;
  await act(async () => worker.click());
  const provider = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Registry provider")) as HTMLButtonElement;
  await act(async () => {
    provider.click();
    provider.click();
  });
  assert(invocations === 1, "two immediate clicks issue exactly one staffing request");
  assert(provider.disabled, "provider action is disabled while staffing is pending");
  finish?.();
  await act(async () => {});
  assert(!document.querySelector('[data-testid="team-menu"]'), "menu closes after successful staffing");
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

test("team header renders the live compact status tally", async () => {
  const panes = {
    orchestrator: pane({ id: "orchestrator", title: "Orchestrator", role: "Orchestrator", team_id: TEAM.id }),
  };
  const currentPlan = plan([
    task("pending-one", TaskStatus.Pending),
    task("pending-two", TaskStatus.Pending),
    task("progress", TaskStatus.InProgress),
    task("review", TaskStatus.InReview),
    task("blocked", TaskStatus.Blocked),
    task("done-one", TaskStatus.Done),
    task("done-two", TaskStatus.Done),
    task("done-three", TaskStatus.Done),
  ]);
  const { host, root, rerender } = await renderSidebar({
    panes,
    tabs: [tab("tab-orchestrator", "orchestrator", 0)],
    plans: [currentPlan],
  });

  const tally = host.querySelector('[data-testid="team-status-tally"]');
  assert(host.querySelector('[data-testid="team-donut"]'), "bound team renders its status donut");
  assert(!host.querySelector('[data-testid="team-donut"] svg'), "donut has no inner icon node");
  assert(!host.querySelector('[data-testid="team-open-effort"]'), "superseded effort row is absent");
  assert(
    Array.from(tally?.querySelectorAll('[data-testid^="team-status-"]') ?? []).map((item) => item.textContent?.trim()).join(",") === "3 todo,1 review,1 blocked,3 done",
    `tally=${tally?.textContent}`,
  );
  const items = Array.from(tally?.querySelectorAll('[data-testid^="team-status-"]') ?? []) as HTMLElement[];
  assert(items.map((item) => item.style.color).join(",") === [
    TASK_STATUS_COLORS.in_progress,
    TASK_STATUS_COLORS.in_review,
    TASK_STATUS_COLORS.blocked,
    TASK_STATUS_COLORS.done,
  ].join(","), "tally colors use the status tokens in board order");
  assert(items.length === 4, "tally exposes all four status buckets");
  assert(items.every((item) => item.querySelector(".sidebar-team-readout-compact-dot")), "every bucket emits compact dot markup");
  assert(items.map((item) => item.querySelector(".sidebar-team-readout-label")?.textContent?.trim()).join(",") === "todo,review,blocked,done", "full labels remain available when the title column fits");
  assert(tally?.textContent?.replace(/\s+/g, " ").trim() === "3 todo · 1 review · 1 blocked · 3 done", "tally uses middle-dot separators");
  const managers = host.querySelector('[data-testid="managers-section"]');
  const disclosure = host.querySelector('[data-testid="team-collapse-toggle"]');
  assert(
    Boolean(tally && disclosure && managers
      && (tally.compareDocumentPosition(managers) & Node.DOCUMENT_POSITION_FOLLOWING)
      && (managers.compareDocumentPosition(disclosure) & Node.DOCUMENT_POSITION_FOLLOWING)),
    "plan tally precedes Team members while disclosure stays below them",
  );
  assert(/\.sidebar-team-readout\s*\{[^}]*margin:\s*6px 12px 9px 14px;[^}]*justify-content:\s*flex-start/.test(sidebarStyles), "tally is aligned to the Team content edge");
  const card = host.querySelector('[data-testid="team-card-activation"]') as HTMLButtonElement | null;
  assert(card, "bound Team exposes a dedicated plan activation surface");
  assert(!host.querySelector('[data-testid="team-plan-button"]'), "superseded Plan button is absent");
  assert(/3\s*\/\s*8/.test(host.querySelector(".sidebar-team-header")?.textContent ?? ""), "header shows done/total progress fraction");
  assert(!host.textContent?.includes("Open effort"), "Team unit has no Open effort chrome");
  canvasFocusStore.closeWorkspace();
  await act(async () => card.click());
  assert(
    canvasFocusStore.getWorkspaceTarget()?.planId === currentPlan.id,
    "bound Team card opens the exact bound plan through the Team canvas entry",
  );

  const updatedPlan = plan(currentPlan.tasks.map((item) =>
    item.id === "blocked" ? { ...item, status: TaskStatus.Done } : item
  ));
  await rerender([updatedPlan]);
  assert(
    Array.from(host.querySelector('[data-testid="team-status-tally"]')?.querySelectorAll('[data-testid^="team-status-"]') ?? []).map((item) => item.textContent?.trim()).join(",") === "3 todo,1 review,0 blocked,4 done",
    "live plan updates replace the tally counts",
  );
  await act(async () => root.unmount());
});

test("Team card keeps plan activation separate from collapse and menu controls", async () => {
  const panes = {
    orchestrator: pane({ id: "orchestrator", title: "Orchestrator", role: "Orchestrator", team_id: TEAM.id }),
  };
  const currentPlan = plan([task("done", TaskStatus.Done)]);
  canvasFocusStore.closeWorkspace();
  const { host, root } = await renderSidebar({
    panes,
    tabs: [tab("tab-orchestrator", "orchestrator", 0)],
    plans: [currentPlan],
    onRenameTeam: () => {},
  });
  const card = host.querySelector('[data-testid="team-card-activation"]') as HTMLButtonElement;
  const chevron = host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement;
  const menu = host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement;
  const header = host.querySelector('.sidebar-team-header') as HTMLElement;
  assert(card.tabIndex === 0 && chevron.tabIndex === 0 && menu.tabIndex === 0, "card, chevron, and menu are distinct keyboard stops");
  assert(!header.hasAttribute("role"), "header does not nest controls in a button role");
  for (const key of ["Enter", " "]) {
    canvasFocusStore.closeWorkspace();
    const keydown = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
    await act(async () => {
      card.focus();
      card.dispatchEvent(keydown);
      if (!keydown.defaultPrevented) card.click();
    });
    assert(document.activeElement === card, `${key} keeps the card focused`);
    assert(!keydown.defaultPrevented, `${key} keeps native button activation enabled`);
    assert(canvasFocusStore.getWorkspaceTarget()?.planId === currentPlan.id, `${key} opens the bound plan`);
  }
  canvasFocusStore.closeWorkspace();
  await act(async () => chevron.click());
  assert(chevron.getAttribute("aria-expanded") === "false", "chevron changes only collapse state");
  assert(canvasFocusStore.getWorkspaceTarget() === null, "chevron never opens a plan");
  const ptyWrites: string[] = [];
  const chevronEnter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  await act(async () => {
    chevron.focus();
    const forwarded = forwardTerminalFallback(chevronEnter, document.activeElement, (data) => ptyWrites.push(data));
    assert(!forwarded, "focused Team disclosure handle never forwards Enter to the PTY");
    if (!chevronEnter.defaultPrevented) chevron.click();
  });
  assert(chevron.getAttribute("aria-expanded") === "true", "Enter on the Team disclosure handle keeps native button toggling");
  assert(ptyWrites.length === 0, "Team disclosure handle Enter produces zero PTY writes");
  const terminalFallbackSurface = document.createElement("div");
  terminalFallbackSurface.tabIndex = 0;
  host.append(terminalFallbackSurface);
  const terminalEnter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  terminalFallbackSurface.focus();
  assert(forwardTerminalFallback(terminalEnter, document.activeElement, (data) => ptyWrites.push(data)), "unfocused terminal fallback forwards Enter");
  assert(terminalEnter.defaultPrevented && ptyWrites.join("") === "\r", "terminal fallback preserves its PTY write");
  await act(async () => (host.querySelector('[data-testid="team-done-count"]') as HTMLElement).click());
  assert(canvasFocusStore.getWorkspaceTarget() === null, "same-slot done total never opens a plan");

  await act(async () => {
    header.dispatchEvent(new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: window.innerWidth - 2,
      clientY: window.innerHeight - 2,
    }));
  });
  const contextMenu = document.querySelector('[data-testid="team-menu"]') as HTMLElement | null;
  assert(contextMenu?.getAttribute("role") === "menu", "right-click opens the shared accessible Team menu");
  const contextActions = Array.from(contextMenu.querySelectorAll("button"));
  const openPlan = contextActions.find((button) => button.textContent?.includes("Open Plan")) as HTMLButtonElement | undefined;
  assert(openPlan, "Team context menu exposes Open Plan when a plan is bound");
  assert(contextActions.some((button) => button.textContent?.includes("Rename")), "Team context menu exposes Rename");
  const menuRectStyle = contextMenu.style;
  assert(Number.parseFloat(menuRectStyle.left) >= 4 && Number.parseFloat(menuRectStyle.top) >= 4, "context menu stays inside viewport margins");
  await act(async () => openPlan.click());
  assert(canvasFocusStore.getWorkspaceTarget()?.planId === currentPlan.id, "context-menu Open Plan targets the bound Team plan");
  assert(!document.querySelector('[data-testid="team-menu"]'), "Open Plan closes the shared menu");

  canvasFocusStore.closeWorkspace();
  await act(async () => menu.click());
  assert(document.querySelector('[data-testid="team-menu"]'), "kebab opens the Team menu");
  assert(document.querySelector('[data-testid="team-menu"]')?.textContent?.includes("Open Plan"), "kebab exposes the same Team actions as right-click");
  assert(canvasFocusStore.getWorkspaceTarget() === null, "kebab never opens a plan");
  await act(async () => root.unmount());

  const unbound = await renderSidebar({
    panes,
    tabs: [tab("tab-orchestrator", "orchestrator", 0)],
    onRenameTeam: () => {},
  });
  const unboundHeader = unbound.host.querySelector('.sidebar-team-header') as HTMLElement;
  assert(!unbound.host.querySelector('[data-testid="team-card-activation"]'), "unbound Team exposes no deceptive activation surface");
  assert(!unboundHeader.classList.contains("sidebar-team-header-active"), "unbound Team has no pointer or press affordance class");
  await act(async () => unboundHeader.click());
  assert(canvasFocusStore.getWorkspaceTarget() === null, "unbound card click has no plan-open side effect");
  await act(async () => (unbound.host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement).click());
  assert(unbound.host.querySelector('[data-testid="team-collapse-toggle"]')?.getAttribute("aria-expanded") === "false", "unbound chevron remains functional");
  await act(async () => unbound.root.unmount());
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

  let updateManagerStatus = (_status: PaneStatus) => {};

  function Harness() {
    const [activeTabId, setActiveTabId] = useState("tab-orchestrator");
    const [managerStatus, setManagerStatus] = useState<PaneStatus>("Working");
    updateManagerStatus = setManagerStatus;
    return (
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={activeTabId}
          sortedTabs={tabs}
          panes={panes}
          paneStatuses={{ orchestrator: managerStatus, worker: "Idle" }}
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
  const managerRow = host.querySelector('[data-tab-id="tab-orchestrator"]');
  const managerButton = managerRow?.querySelector('button[aria-label="Open tab Orchestrator"]');
  assert(managerButton, "active manager remains an accessible named button");
  const managerStatus = managerRow?.querySelector('[role="status"]');
  assert(
    managerStatus?.textContent?.trim() === "Orchestrator tab status Working",
    "working manager status is exposed to assistive technology",
  );
  await act(async () => updateManagerStatus("Idle"));
  assert(
    managerStatus?.textContent?.trim() === "Orchestrator tab status Idle",
    "idle transition updates the assistive status",
  );
  assert(
    managerRow?.querySelector(":scope > .active-tab-pill"),
    "active fill is owned by the row behind the manager button",
  );
  assert(!managerButton.querySelector(".active-tab-pill"), "active fill cannot cover manager content");
  assert(managerButton.textContent?.includes("Orchestrator"), "active manager title remains readable");
  const workerRow = host.querySelector('[data-tab-id="tab-worker"]') as HTMLElement | null;
  assert(workerRow, "worker row rendered");
  assert(workerRow.className.includes("sidebar-tab-dimmed"), "worker starts inactive");
  const workerButton = workerRow.querySelector('button[aria-label="Open tab Worker"]') as HTMLButtonElement | null;
  assert(workerButton, "worker exposes its open action");
  await act(async () => workerButton.click());
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

test("Team disclosure stays user-owned across rerenders and remounts", async () => {
  const panes = {
    orchestrator: pane({ id: "orchestrator", title: "Orchestrator", role: "Orchestrator", team_id: TEAM.id }),
  };
  const tabs = [tab("tab-orchestrator", "orchestrator", 0)];
  const first = await renderSidebar({ panes, tabs });
  const firstHandle = first.host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement;
  await act(async () => firstHandle.click());
  assert(firstHandle.getAttribute("aria-expanded") === "false", "user collapse takes effect");
  await first.rerender([]);
  assert(firstHandle.getAttribute("aria-expanded") === "false", "data refresh cannot expand a collapsed Team");
  await act(async () => first.root.unmount());

  const refreshed = await renderSidebar({ panes, tabs });
  const refreshedHandle = refreshed.host.querySelector('[data-testid="team-collapse-toggle"]') as HTMLButtonElement;
  assert(refreshedHandle.getAttribute("aria-expanded") === "false", "frontend remount restores the collapsed Team state");
  assert(!refreshed.host.querySelector('[data-testid="managers-section"]'), "collapsed Team body stays hidden after remount");
  await act(async () => refreshedHandle.click());
  await act(async () => refreshed.root.unmount());

  const expandedAgain = await renderSidebar({ panes, tabs });
  assert(
    expandedAgain.host.querySelector('[data-testid="team-collapse-toggle"]')?.getAttribute("aria-expanded") === "true",
    "only a user expansion changes the persisted Team state",
  );
  await act(async () => expandedAgain.root.unmount());
});

test("tab rich menu opens from right click and exposes session actions", async () => {
  const commands: string[] = [];
  let copied = "";
  __setInvokeForTest((async (command: string) => {
    commands.push(command);
    return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  }) as any);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: async (value: string) => { copied = value; } },
  });
  const panes: Record<string, Pane> = {
    worker: pane({
      id: "worker",
      title: "Worker",
      role: "Worker",
      team_id: TEAM.id,
      agent_type: "claude",
      agent_session_id: "session-42",
    }),
  };
  const { host, root } = await renderSidebar({ panes, tabs: [tab("tab-worker", "worker", 0)] });
  const row = host.querySelector('[data-tab-id="tab-worker"]');
  assert(row, "worker row rendered");
  await act(async () => {
    row.dispatchEvent(new MouseEvent("contextmenu", {
      bubbles: true,
      clientX: window.innerWidth - 1,
      clientY: window.innerHeight - 1,
    }));
  });

  const menu = document.querySelector('[role="menu"][aria-label="Tab actions"]') as HTMLDivElement | null;
  assert(menu, "right click opens the same accessible tab actions menu");
  assert(Number.parseFloat(menu.style.left) < window.innerWidth - 4, "right-edge menu is clamped inside the viewport");
  assert(Number.parseFloat(menu.style.top) < window.innerHeight - 4, "bottom-edge menu is clamped inside the viewport");
  assert(Number.parseFloat(menu.style.left) > window.innerWidth / 2, "context menu stays anchored near the right-click edge");
  assert(Number.parseFloat(menu.style.top) > window.innerHeight / 2, "context menu stays anchored near the bottom-click edge");
  const labels = Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent?.trim());
  assert(labels.includes("Rename"), "rename remains available");
  assert(labels.includes("Reconnect"), "reconnect is available");
  assert(labels.includes("Restart session"), "restart is available");
  assert(labels.includes("Close Session"), "close remains available");
  assert(!menu.textContent?.includes("Remove Role"), "legacy role removal is gone");
  assert(!menu.textContent?.includes("Open Terminal"), "legacy terminal toggle is gone");

  const copy = menu.querySelector('[data-testid="tab-menu-copy-session-id"]') as HTMLButtonElement;
  assert(copy.getAttribute("aria-label") === "Copy session ID", "copy icon is named beside the session ID");
  await act(async () => copy.click());
  assert(copied === "session-42", "copy icon writes the session ID");

  const reconnect = Array.from(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
    .find((item) => item.textContent?.trim() === "Reconnect");
  assert(reconnect, "reconnect menu item found");
  await act(async () => reconnect.click());
  assert(commands.includes("chat_stop"), "reconnect uses the shared chat reattach path");

  await openTabActionsMenu(row);
  const restart = document.querySelector('[data-testid="tab-menu-restart-session"]') as HTMLButtonElement;
  assert(restart, "three-dot trigger opens the same restart action");
  await act(async () => restart.click());
  assert(commands.includes("restart_agent_pane"), "restart uses the session-preserving backend action");
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
      window.localStorage.clear();
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
