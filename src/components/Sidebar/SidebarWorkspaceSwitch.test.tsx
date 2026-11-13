import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { afterEach, expect, it, vi } from "vitest";
import type { AppConfig, Tab, Team, Workspace } from "../../lib/types";
import type { PlanView } from "../Canvas/planProjection";
import { Sidebar } from "./Sidebar";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceDetail, type SidebarWorkspaceDetailActions, type SidebarWorkspaceDetailData } from "./SidebarWorkspaceDetail";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { usePtyConnection } from "../../hooks/terminal/usePtyConnection";
import { invoke } from "../../lib/bridge";
import { subscribeTabOutput } from "../../lib/outputStream";
import type { Terminal } from "@xterm/xterm";

const mux = vi.hoisted(() => ({ state: {} as Record<string, unknown>, listeners: new Set<() => void>() }));
vi.mock("../../store/multiplexer/hooks", async () => {
  const { useSyncExternalStore } = await import("react");
  const subscribe = (listener: () => void) => { mux.listeners.add(listener); return () => { mux.listeners.delete(listener); }; };
  return {
    useMultiplexer: () => ({
      setActiveTab: (workspaceId: string, tabId: string) => {
        mux.state = { ...mux.state, active_workspace_id: workspaceId, activeTabIds: { ...mux.state.activeTabIds as Record<string, string>, [workspaceId]: tabId } };
        for (const listener of [...mux.listeners]) listener();
      },
    }),
    useMuxSelector: (select: (state: object) => unknown) => select(useSyncExternalStore(subscribe, () => mux.state)),
  };
});
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false }) }));
vi.mock("../../hooks/useTouchOptimizedUi", () => ({ useTouchOptimizedUi: () => false }));
vi.mock("../../lib/bridge", async (actual) => ({ ...await actual<object>(), invoke: vi.fn().mockResolvedValue([]), listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../lib/outputStream", async actual => ({ ...await actual<object>(), subscribeTabOutput: vi.fn(() => () => {}) }));
vi.mock("../Settings/useAccount", () => ({ useAccount: () => ({ model: { kind: "signed_out" } }) }));
vi.mock("../Automation/hostAdapter", () => ({
  primeWorkspaceGraphs: async () => undefined,
  hostAutomationAdapter: {
    read: () => new Promise(() => undefined),
    subscribe: async () => () => undefined,
    stop: async () => undefined,
    delete: async () => undefined,
  },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const config = { general_agent: { first_run_complete: true }, agent: {}, mesh: { enabled: false } } as AppConfig;

const member = (id: string, workspace_id: string, extra: Partial<Tab> = {}): Tab => ({
  id, title: id, tab_type: "Terminal", cwd: "/w", pty_id: null, agent_type: "fixture-provider", workspace_id, ...extra,
} as Tab);
const manager = (id: string, workspace_id: string) => member(id, workspace_id, { team_id: `team-${workspace_id}`, role: "Manager", role_profile: "planner" });
const worker = (id: string, workspace_id: string) => member(id, workspace_id, { team_id: `team-${workspace_id}`, role: "Worker", role_profile: "worker" });
const workspaceOf = (id: string, tabs: readonly Tab[]): Workspace => ({ id, name: id, tabs: tabs.map((tab) => tab.id), active_tab_id: tabs[0]?.id ?? null, cwd: "/w" });
const team = (workspace_id: string): Team => ({ id: `team-${workspace_id}`, name: "Crew", lifecycle: "active", created_at: 1, workspace_id });
const plan = (workspace_id: string) => ({
  id: `plan-${workspace_id}`, title: "Late plan", status: "active", team_id: `team-${workspace_id}`, sections: [], tasks: [],
  created_at: 1, updated_at: 2, project_path: "/w", milestone_kinds: [], loops: [], elements: [], canvas: { viewport: null }, owner_history: [],
}) as unknown as PlanView;

const hosts: Array<() => Promise<void>> = [];
function mountHost() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  hosts.push(async () => { await act(async () => root.unmount()); host.remove(); });
  return { host, root };
}
afterEach(async () => {
  for (const close of hosts.splice(0)) await close();
  localStorage.clear();
  vi.unstubAllGlobals();
});

function detailData(tabs: readonly Tab[], teams: Team[], plans: PlanView[]): SidebarWorkspaceDetailData {
  return {
    tabs: Object.fromEntries(tabs.map((tab) => [tab.id, tab])), tabStatuses: {}, attention: {}, teams, planIndex: [], plans,
    providers: [], providersLoading: false, closingTabIds: new Set(), closingTeamIds: new Set(), activeTabIds: {},
    activeWorkspaceId: "late", isWorkspaceMode: true, workspaceCanvasActive: false, touchMode: false, compact: false,
    sidebarActionText: "text-xs", orchestrators: [], newSessionDisabled: false,
  };
}
const detailActions = {
  activateWorkspace: () => undefined, activateTab: () => undefined, closeTab: () => undefined, renameTab: () => undefined,
  renameWorkspace: () => undefined, closeWorkspace: () => undefined, newTerminal: () => undefined, newSession: () => undefined,
  newAgent: () => undefined, openRemoteTab: () => undefined, openAutomation: () => undefined, closeTeam: () => undefined,
} satisfies SidebarWorkspaceDetailActions;
const row = (host: Element, tabId: string) => host.querySelector<HTMLElement>(`[data-tab-id="${tabId}"]`);

it("draws a Team's sections and members in the first frame while its Team record and plan are still unread, and keeps those rows when they arrive", async () => {
  const tabs = [manager("lead", "late"), worker("hand", "late")];
  const workspace = workspaceOf("late", tabs);
  const { host, root } = mountHost();
  const view = (teams: Team[], plans: PlanView[]) => <SidebarDnd handlers={{ reorderTabs: () => undefined, reorderDisplay: () => undefined, reorderWorkspaces: () => undefined }}>
    <SidebarWorkspaceDetail key={workspace.id} workspace={workspace} status={null} data={detailData(tabs, teams, plans)} actions={detailActions} />
  </SidebarDnd>;

  flushSync(() => root.render(view([], [])));
  const managers = host.querySelector('[data-testid="managers-section"]');
  const workers = host.querySelector('[data-testid="workers-section"]');
  const leadRow = row(host, "lead");
  const handRow = row(host, "hand");
  expect(managers?.contains(leadRow)).toBe(true);
  expect(workers?.contains(handRow)).toBe(true);

  await act(async () => root.render(view([team("late")], [plan("late")])));
  expect(host.querySelector('[data-testid="team-name"]')?.textContent).toBe("Late plan");
  expect(row(host, "lead")).toBe(leadRow);
  expect(row(host, "hand")).toBe(handRow);
});

function setMux(workspaces: Workspace[], tabs: readonly Tab[], active: string) {
  mux.state = {
    workspaces, tabs: Object.fromEntries(tabs.map((tab) => [tab.id, tab])), teams: workspaces.map((workspace) => team(workspace.id)),
    notifications: [], tabStatuses: {}, activeTabIds: {}, active_workspace_id: active, closedTabs: [], orphanedTabs: [],
  };
}
async function mountSidebar(workspaces: Workspace[], tabs: readonly Tab[], active: string, terminal = false) {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  setMux(workspaces, tabs, active);
  const { host, root } = mountHost();
  await act(async () => root.render(<><Sidebar config={config} />{terminal && <SelectedTerminal />}</>));
  return { host, render: () => { for (const listener of [...mux.listeners]) listener(); } };
}

function SelectedTerminal() {
  const selected = useMuxSelector(state => state.activeTabIds[state.active_workspace_id ?? ""]);
  const tab = useMuxSelector(state => selected ? state.tabs[selected] : undefined);
  const termRef = useRef({ cols: 80, rows: 24,
    buffer: { active: { type: "normal", viewportY: 0, baseY: 0 } },
    scrollToBottom() {}, scrollToLine() {},
    write(_data: string, done?: () => void) { done?.(); },
  } as unknown as Terminal);
  const connectedPtyRef = useRef<string | null>(null);
  usePtyConnection({ termRef, connectedPtyRef, tabId: selected ?? "", ptyId: tab?.pty_id,
    termReady: Boolean(selected), onShellReady() {}, onOutputChunk() {},
  });
  return null;
}
const entering = (host: Element) => [...host.querySelectorAll('[data-tab-id][data-entering="true"]')].map((element) => element.getAttribute("data-tab-id"));

it("plays no entrance for any row on a switch, even one that joined that workspace just before", async () => {
  const seen = [manager("seen", "away")];
  const here = [worker("here", "home")];
  const view = await mountSidebar([workspaceOf("home", here), workspaceOf("away", seen)], [...here, ...seen], "home");

  const joined = worker("joined", "away");
  const grown = [...seen, joined];
  setMux([workspaceOf("home", here), workspaceOf("away", grown)], [...here, ...grown], "home");
  await act(async () => view.render());
  setMux([workspaceOf("home", here), workspaceOf("away", grown)], [...here, ...grown], "away");
  await act(async () => view.render());

  expect(row(view.host, "joined")).not.toBeNull();
  expect(row(view.host, "seen")).not.toBeNull();
  expect(entering(view.host)).toEqual([]);
});

it("plays no entrance for a row that left the shown workspace and came back", async () => {
  const here = [worker("stays", "home"), worker("returns", "home")];
  const view = await mountSidebar([workspaceOf("home", here)], here, "home");

  setMux([workspaceOf("home", [here[0]])], [here[0]], "home");
  await act(async () => view.render());
  expect(row(view.host, "returns")).toBeNull();

  setMux([workspaceOf("home", here)], here, "home");
  flushSync(() => view.render());

  expect(row(view.host, "returns")).not.toBeNull();
  expect(entering(view.host)).toEqual([]);
});

it("plays the entrance for a member that joins the shown workspace, from its first frame, and only for that member", async () => {
  const here = [worker("here", "home")];
  const view = await mountSidebar([workspaceOf("home", here)], here, "home");

  const grown = [...here, worker("arrival", "home")];
  setMux([workspaceOf("home", grown)], grown, "home");
  flushSync(() => view.render());

  expect(entering(view.host)).toEqual(["arrival"]);
});

it("selecting Team tabs retains General, Utilities and both rows of workspace cards without remeasurement", async () => {
  const main = member("main", "general", { protected: true, role: "Manager", role_profile: "general_agent" });
  const utility = member("utility", "general", { agent_type: undefined, purpose: "qa" });
  const here = [manager("Pasteur", "home"), worker("Lattice", "home")];
  const general = { ...workspaceOf("general", [main, utility]), protected: true };
  const workspaces = [general, ...Array.from({ length: 8 }, (_, index) => workspaceOf(index === 7 ? "home" : `other-${index}`, index === 7 ? here : []))];
  const width = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(260);
  const view = await mountSidebar(workspaces, [main, utility, ...here], "home");
  try {
    const pinned = view.host.querySelector<HTMLElement>('[data-testid="general-section"]')!;
    const grid = view.host.querySelector<HTMLElement>('[data-testid="workspace-card-grid"]')!;
    const utilities = pinned.querySelector('button[aria-label="UTILITIES 1"]')!;
    expect(pinned).not.toBeNull();
    expect(utilities?.getAttribute("aria-expanded")).toBe("false");
    expect(grid.children).toHaveLength(9);
    expect(grid.style.gridTemplateColumns).toBe("repeat(5, 42px)");
    const pinnedStructure = pinned.innerHTML;
    const gridStructure = grid.innerHTML;
    const cards = [...grid.children];
    const measured = width.mock.calls.length;
    for (const tabId of ["Pasteur", "Lattice", "Pasteur", "Lattice"]) {
      const button = row(view.host, tabId)?.querySelector<HTMLButtonElement>(`button[aria-label="Open tab ${tabId}"]`);
      expect(button).toBeTruthy();
      await act(async () => button!.click());
      expect(button!.getAttribute("aria-current")).toBe("page");
      expect(view.host.querySelector('[data-testid="general-section"]')).toBe(pinned);
      expect(view.host.querySelector('[data-testid="workspace-card-grid"]')).toBe(grid);
      expect(pinned.innerHTML).toBe(pinnedStructure);
      expect(grid.innerHTML).toBe(gridStructure);
      expect([...grid.children]).toEqual(cards);
      expect(grid.style.gridTemplateColumns).toBe("repeat(5, 42px)");
      expect(width.mock.calls.length).toBe(measured);
    }
  } finally { width.mockRestore(); }
});

it.each([null, "owned-pty"])("selecting a failed tab never restarts it or changes its activity and height-bearing content (pty=%s)", async ptyId => {
  const tabs = [manager("Faraday", "home"), worker("Cxauthor", "home")].map(tab => ({
    ...tab, boot_stage: "spawn_failed" as const, boot_failure_reason: "opaque launch failure", pty_id: ptyId,
  }));
  const view = await mountSidebar([workspaceOf("home", tabs)], tabs, "home", true);
  const spawns: unknown[] = [];
  vi.mocked(subscribeTabOutput).mockClear();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "spawn_pty") {
      spawns.push(args);
      const tabId = args?.tabId as string;
      const current = mux.state.tabs as Record<string, Tab>;
      mux.state = { ...mux.state, tabs: { ...current, [tabId]: { ...current[tabId], boot_stage: "pty_starting" } } };
      view.render();
      return new Promise(() => {});
    }
    return [];
  });
  try {
    const structure = (entry: HTMLElement) => [...entry.querySelectorAll(".sidebar-tab-body *")].map(element => [element.tagName, element.textContent]);
    const contents = tabs.map(tab => structure(row(view.host, tab.id)!));
    for (const tabId of ["Faraday", "Cxauthor", "Faraday", "Cxauthor"]) {
      const button = row(view.host, tabId)!.querySelector<HTMLButtonElement>(`button[aria-label="Open tab ${tabId}, launch failed"]`)!;
      expect(button).toBeTruthy();
      await act(async () => button.click());
      expect(spawns).toEqual([]);
      expect(button.getAttribute("aria-current")).toBe("page");
      if (ptyId) expect(subscribeTabOutput).toHaveBeenLastCalledWith(tabId, expect.any(Object), expect.any(Object), ptyId);
      else expect(subscribeTabOutput).not.toHaveBeenCalled();
      for (const [index, tab] of tabs.entries()) {
        const entry = row(view.host, tab.id)!;
        expect(entry.dataset.tabActivity).toBe("failed");
        expect(entry.querySelector('[data-testid="tab-launch-failure-badge"]')).not.toBeNull();
        expect(structure(entry)).toEqual(contents[index]);
      }
    }
  } finally { vi.mocked(invoke).mockResolvedValue([]); }
});
