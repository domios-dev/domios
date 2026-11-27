import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { afterEach, expect, it, vi } from "vitest";
import type { AppConfig, Tab, Team, Workspace } from "../../lib/types";
import type { PlanView } from "../Canvas/planProjection";
import { Sidebar } from "./Sidebar";
import { setSidebarMachineFilter } from "./sidebarMachineFilter";
import { PairedWorkspaceInventoryContext } from "../../store/multiplexer/context";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceDetail, type SidebarWorkspaceDetailActions, type SidebarWorkspaceDetailData } from "./SidebarWorkspaceDetail";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { usePtyConnection } from "../../hooks/terminal/usePtyConnection";
import { invoke } from "../../lib/bridge";
import { subscribeTabOutput } from "../../lib/outputStream";
import type { Terminal } from "@xterm/xterm";
import type { RemoteWorkspaceCloseConfirmation, RemoteWorkspaceCloseTarget } from "./useRemoteWorkspaceClose";
import type { RemoteTeamClosePreview, RemoteTeamCloseTarget } from "./useRemoteTeamClose";

const mux = vi.hoisted(() => ({ state: {} as Record<string, unknown>, listeners: new Set<() => void>(), closeTab: vi.fn(), upsertToast: vi.fn() }));
const remoteCloseCalls = vi.hoisted(() => ({
  requested: [] as Array<{ peerId: string; workspaceId: string; availability: string; protected: boolean }>,
  confirmed: [] as Array<{ peerId: string; workspaceId: string }>,
}));
const remoteCreateCalls = vi.hoisted(() => [] as Array<{ peerId: string; cwd: string; name?: string }>);
const remoteTeamCloseCalls = vi.hoisted(() => ({
  requested: [] as Array<{ peerId: string; workspaceId: string; teamId: string; availability: string }>,
  confirmed: [] as Array<{ peerId: string; workspaceId: string; teamId: string }>,
}));
const remotePlanCalls = vi.hoisted(() => ({
  peerIds: [] as Array<string | null>,
  result: {
    plans: [] as PlanView[],
    available: false,
    loading: false,
    reconnecting: false,
    error: null as string | null,
    refetch: async () => {},
  },
}));
vi.mock("../../store/multiplexer/hooks", async () => {
  const { useSyncExternalStore } = await import("react");
  const subscribe = (listener: () => void) => { mux.listeners.add(listener); return () => { mux.listeners.delete(listener); }; };
  return {
    useMultiplexer: () => ({
      setActiveWorkspace: (workspaceId: string) => {
        mux.state = { ...mux.state, active_workspace_id: workspaceId };
        for (const listener of [...mux.listeners]) listener();
      },
      setActiveTab: (workspaceId: string, tabId: string) => {
        mux.state = { ...mux.state, active_workspace_id: workspaceId, activeTabIds: { ...mux.state.activeTabIds as Record<string, string>, [workspaceId]: tabId } };
        for (const listener of [...mux.listeners]) listener();
      },
      closeTab: mux.closeTab,
      upsertToast: mux.upsertToast,
    }),
    useMuxSelector: (select: (state: object) => unknown) => select(useSyncExternalStore(subscribe, () => mux.state)),
  };
});
vi.mock("../Canvas/PlansProvider", async (importOriginal) => {
  const original = await importOriginal<typeof import("../Canvas/PlansProvider")>();
  return {
    ...original,
    usePeerPlanSummaries: vi.fn((peerId: string | null | undefined) => {
      remotePlanCalls.peerIds.push(peerId ?? null);
      return remotePlanCalls.result;
    }),
  };
});
vi.mock("../../hooks/useAgentProviders", () => {
  const catalogue = { providers: [], loading: false };
  return { useAgentProviders: () => catalogue };
});
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
vi.mock("./useRemoteWorkspaceClose", async (importOriginal) => {
  const original = await importOriginal<typeof import("./useRemoteWorkspaceClose")>();
  const { useState } = await import("react");
  return {
    ...original,
    useRemoteWorkspaceClose: (availabilityForPeer: Parameters<typeof original.useRemoteWorkspaceClose>[0], onClosed: Parameters<typeof original.useRemoteWorkspaceClose>[1]) => {
      const [pending, setPending] = useState<RemoteWorkspaceCloseConfirmation | null>(null);
      return {
        pending,
        request: async (target: RemoteWorkspaceCloseTarget, isStillSelected: () => boolean) => {
          if (target.availability !== "online" || availabilityForPeer(target.peerId) !== "online") return { kind: "offline" as const };
          if (target.protected) return { kind: "protected" as const };
          if (!isStillSelected()) return { kind: "stale" as const };
          remoteCloseCalls.requested.push({ peerId: target.peerId, workspaceId: target.workspaceId, availability: target.availability, protected: target.protected });
          setPending({ ...target, agentTitles: ["peer-lead", "other-worker"], crons: [], blueprints: [], teams: [] });
          return { kind: "ready" as const };
        },
        cancel: () => setPending(null),
        confirm: async () => {
          if (!pending) return { kind: "none" as const };
          const target = pending;
          setPending(null);
          remoteCloseCalls.confirmed.push({ peerId: target.peerId, workspaceId: target.workspaceId });
          onClosed(target.peerId, target.workspaceId);
          return { kind: "closed" as const, target };
        },
      };
    },
  };
});
vi.mock("./useRemoteWorkspaceCreate", async (importOriginal) => {
  const original = await importOriginal<typeof import("./useRemoteWorkspaceCreate")>();
  return {
    ...original,
    useRemoteWorkspaceCreate: (
      availabilityForPeer: Parameters<typeof original.useRemoteWorkspaceCreate>[0],
      onCreated: Parameters<typeof original.useRemoteWorkspaceCreate>[1],
    ) => ({
      create: async (peerId: string, cwd: string, explicitName?: string) => {
        if (availabilityForPeer(peerId) !== "online") return { kind: "unavailable" as const, peerId, reason: "offline" };
        remoteCreateCalls.push({ peerId, cwd, name: explicitName });
        const workspace = { id: "ws-created", name: explicitName ?? "created" };
        onCreated(peerId, workspace);
        return { kind: "created" as const, peerId, workspace };
      },
    }),
  };
});
vi.mock("./useRemoteTeamClose", async (importOriginal) => {
  const original = await importOriginal<typeof import("./useRemoteTeamClose")>();
  const { useState } = await import("react");
  return {
    ...original,
    useRemoteTeamClose: (
      availabilityForPeer: Parameters<typeof original.useRemoteTeamClose>[0],
      onClosed: Parameters<typeof original.useRemoteTeamClose>[1],
    ) => {
      const [pending, setPending] = useState<RemoteTeamClosePreview | null>(null);
      return {
        pending,
        closingTeamKeys: new Set<string>(),
        request: async (target: RemoteTeamCloseTarget, isStillSelected: () => boolean) => {
          if (target.availability !== "online" || availabilityForPeer(target.peerId) !== "online" || !isStillSelected()) return;
          remoteTeamCloseCalls.requested.push({
            peerId: target.peerId, workspaceId: target.workspaceId, teamId: target.teamId, availability: target.availability,
          });
          setPending({
            ...target,
            phase: "ready",
            id: target.teamId,
            name: target.teamName,
            crons: [],
            blueprints: [],
            lanes: [],
          });
        },
        cancel: () => setPending(null),
        confirm: async () => {
          if (!pending || pending.phase !== "ready") return;
          const target = pending;
          setPending(null);
          remoteTeamCloseCalls.confirmed.push({ peerId: target.peerId, workspaceId: target.workspaceId, teamId: target.teamId });
          onClosed(target.peerId, target.teamId);
        },
      };
    },
  };
});
vi.mock("../Automation/blueprintOwner", () => ({
  readTabCloseBlueprints: vi.fn(async () => []),
  readWorkspaceBlueprints: vi.fn(async () => []),
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
  setSidebarMachineFilter("local");
  remoteCloseCalls.requested.length = 0;
  remoteCloseCalls.confirmed.length = 0;
  remoteCreateCalls.length = 0;
  remoteTeamCloseCalls.requested.length = 0;
  remoteTeamCloseCalls.confirmed.length = 0;
  mux.closeTab.mockReset();
  mux.upsertToast.mockReset();
  vi.mocked(invoke).mockReset().mockResolvedValue([]);
  remotePlanCalls.peerIds.length = 0;
  remotePlanCalls.result = {
    plans: [], available: false, loading: false, reconnecting: false, error: null, refetch: async () => {},
  };
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
  newAgent: () => undefined, openAutomation: () => undefined, closeTeam: () => undefined,
} satisfies SidebarWorkspaceDetailActions;
const row = (host: Element, tabId: string) => host.querySelector<HTMLElement>(`[data-tab-id="${tabId}"]`);

function mockSelectionIntersections() {
  const observers: SelectionObserver[] = [];
  class SelectionObserver {
    readonly targets = new Set<Element>();
    constructor(readonly callback: IntersectionObserverCallback, readonly options: IntersectionObserverInit) {
      observers.push(this);
    }
    observe = vi.fn((target: Element) => this.targets.add(target));
    unobserve = (target: Element) => this.targets.delete(target);
    takeRecords = () => [];
    disconnect = () => this.targets.clear();
    emit(target: Element, top: number, height = 28) {
      const bottom = top + height;
      const insetTop = -Number.parseFloat(this.options.rootMargin ?? "0");
      const isIntersecting = height > 0 && top <= 99 && bottom >= insetTop;
      return act(async () => this.callback([{
        target, isIntersecting, rootBounds: new DOMRect(0, insetTop, 200, 99 - insetTop),
        boundingClientRect: new DOMRect(0, top, 150, height),
      } as IntersectionObserverEntry], this as unknown as IntersectionObserver));
    }
  }
  vi.stubGlobal("IntersectionObserver", SelectionObserver);
  return {
    observed: () => observers.flatMap(observer => [...observer.targets]),
    observerFor: (element: Element) => {
      const observer = observers.find(candidate => candidate.targets.has(element));
      expect(observer).toBeDefined();
      return observer!;
    },
  };
}

function selectionCue(host: HTMLElement) {
  return host.querySelector<HTMLButtonElement>('button[aria-label^="Show selected tab,"]');
}

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

it("reveals the off-screen selected Team row at either edge without activating or focusing it", async () => {
  const intersections = mockSelectionIntersections();
  const tabs = [manager("Ada", "home"), worker("Morse", "home")];
  const { host } = await mountSidebar([workspaceOf("home", tabs)], tabs, "home");
  const morse = row(host, "Morse")!;
  const activate = morse.querySelector<HTMLButtonElement>('button[aria-label="Open tab Morse"]')!;
  await act(async () => activate.click());
  expect(intersections.observed()).toHaveLength(1);
  expect(intersections.observed()[0]).toBe(morse);
  const observer = intersections.observerFor(morse);
  const viewport = host.querySelector<HTMLElement>(".ct-sidebar-content")!;
  expect(observer.options.root).toBe(viewport);
  expect(observer.options.threshold).toEqual([0, 1]);
  const scrollIntoView = vi.fn();
  morse.scrollIntoView = scrollIntoView;
  const selectedState = mux.state;

  await observer.emit(morse, 100);
  const below = selectionCue(host)!;
  expect(below.getAttribute("aria-label")).toBe("Show selected tab, Morse, below");
  expect(below.textContent).toContain("Morse↓");
  expect(scrollIntoView).not.toHaveBeenCalled();
  await act(async () => below.click());
  expect(scrollIntoView).toHaveBeenCalledExactlyOnceWith({ block: "nearest", behavior: "smooth" });
  expect(mux.state).toBe(selectedState);
  expect(activate.getAttribute("aria-current")).toBe("page");
  expect(document.activeElement).not.toBe(activate);
  await observer.emit(morse, 99);
  expect(selectionCue(host)).toBeNull();

  await observer.emit(morse, -28);
  const above = selectionCue(host)!;
  expect(above.getAttribute("aria-label")).toBe("Show selected tab, Morse, above");
  expect(above.textContent).toContain("↑Morse");
  await act(async () => above.click());
  expect(scrollIntoView).toHaveBeenCalledTimes(2);
  await observer.emit(morse, -27);
  expect(selectionCue(host)).toBeNull();

  viewport.scrollTop = 200;
  await observer.emit(morse, -28);
  expect(selectionCue(host)?.getAttribute("aria-label")).toBe("Show selected tab, Morse, above");
  await act(async () => {
    viewport.scrollTop = 0;
    viewport.dispatchEvent(new Event("scroll"));
  });
  expect(observer.observe).toHaveBeenCalledTimes(2);
  await observer.emit(morse, 172);
  expect(selectionCue(host)?.getAttribute("aria-label")).toBe("Show selected tab, Morse, below");

  const ada = row(host, "Ada")!;
  await act(async () => ada.querySelector<HTMLButtonElement>('button[aria-label="Open tab Ada"]')!.click());
  expect(intersections.observed()).toHaveLength(1);
  expect(intersections.observed()[0]).toBe(ada);
  await observer.emit(morse, 100);
  expect(selectionCue(host)).toBeNull();
});

it("clears the cue when the selected Team row is collapsed and reveals instantly with reduced motion", async () => {
  const intersections = mockSelectionIntersections();
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query === "(prefers-reduced-motion: reduce)",
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  const tabs = [worker("Morse", "home")];
  const { host } = await mountSidebar([workspaceOf("home", tabs)], tabs, "home");
  const morse = row(host, "Morse")!;
  await act(async () => morse.querySelector<HTMLButtonElement>('button[aria-label="Open tab Morse"]')!.click());
  await intersections.observerFor(morse).emit(morse, -28);
  const scrollIntoView = vi.fn();
  morse.scrollIntoView = scrollIntoView;
  await act(async () => selectionCue(host)!.click());
  expect(scrollIntoView).toHaveBeenCalledExactlyOnceWith({ block: "nearest", behavior: "auto" });

  const collapse = host.querySelector<HTMLButtonElement>('[aria-label="WORKERS 1"]')!;
  await act(async () => collapse.click());
  expect(collapse.getAttribute("aria-expanded")).toBe("false");
  expect(row(host, "Morse")).toBeNull();
  expect(intersections.observed()).toEqual([]);
  expect(selectionCue(host)).toBeNull();
  expect(mux.state.activeTabIds).toEqual({ home: "Morse" });
});

it("replaces a keyboard cue with focus on the row once it is visible below the sticky header", async () => {
  const intersections = mockSelectionIntersections();
  const tabs = [worker("Morse", "home")];
  const { host } = await mountSidebar([workspaceOf("home", tabs)], tabs, "home");
  const header = host.querySelector(".sidebar-workspace-detail-header")!;
  Object.defineProperty(header, "offsetHeight", { value: 24 });
  const morse = row(host, "Morse")!;
  const activation = morse.querySelector<HTMLButtonElement>('button[aria-label="Open tab Morse"]')!;
  await act(async () => activation.click());
  const observer = intersections.observerFor(morse);
  const scrollIntoView = vi.fn();
  morse.scrollIntoView = scrollIntoView;
  const selectedState = mux.state;

  await observer.emit(morse, -4);
  const cue = selectionCue(host)!;
  expect(cue.getAttribute("aria-label")).toBe("Show selected tab, Morse, above");
  cue.focus();
  await act(async () => cue.click());
  expect(scrollIntoView).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(cue);

  await observer.emit(morse, -3);
  expect(selectionCue(host)).toBeNull();
  expect(document.activeElement).toBe(activation);
  expect(mux.state).toBe(selectedState);
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

it("keeps one workspace switcher and falls back to one visible workspace when filtering", async () => {
  const intersections = mockSelectionIntersections();
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const here = [worker("local-worker", "Local home")];
  setMux([workspaceOf("Local home", here)], here, "Local home");
  const remoteId = "Peer home";
  const lead = manager("peer-lead", remoteId);
  const hand = worker("other-worker", remoteId);
  const { host, root } = mountHost();
  await act(async () => root.render(<PairedWorkspaceInventoryContext.Provider value={{
    generals: [
      { peerId: "blackstar", name: "BLACKSTAR", availability: "online", sessions: [] },
      { peerId: "air", name: "AIR", availability: "online", sessions: [] },
    ],
    browse: null, peerWorkspaces: [{
      peerId: "blackstar", workspace: workspaceOf(remoteId, [lead, hand]), teams: [{ ...team(remoteId), name: null }],
      planTitles: new Map([[team(remoteId).id, "Peer plan title"]]),
      tabs: [{ viewerId: "mesh:blackstar/peer-lead", tab: lead }, { viewerId: "mesh:air/other-worker", tab: hand, peerId: "air" }],
    }],
  }}><Sidebar config={config} /></PairedWorkspaceInventoryContext.Provider>));
  const grid = host.querySelector('[data-testid="workspace-card-grid"]');
  const detail = () => [...host.querySelectorAll('[data-testid="workspace-detail"]')];
  const cards = () => [...host.querySelectorAll('[data-testid="workspace-card"]')];
  expect(cards()).toHaveLength(1);
  expect(detail().map(node => node.getAttribute("aria-label"))).toEqual(["Local home workspace contents"]);
  const selected = row(host, "local-worker")!;
  await act(async () => selected.querySelector<HTMLButtonElement>('button[aria-label="Open tab local-worker"]')!.click());
  await intersections.observerFor(selected).emit(selected, 100);
  expect(selectionCue(host)).not.toBeNull();

  await act(async () => setSidebarMachineFilter("all"));
  expect(cards()).toHaveLength(1);
  const blackstarToggle = host.querySelector<HTMLButtonElement>('[data-testid="workspace-machine-section"][data-peer-id="blackstar"] [data-testid="workspace-machine-toggle"]')!;
  expect(blackstarToggle.getAttribute("aria-expanded")).toBe("false");
  await act(async () => blackstarToggle.click());
  expect(blackstarToggle.getAttribute("aria-expanded")).toBe("true");
  expect(cards()).toHaveLength(2);
  expect(detail()).toHaveLength(1);
  const peerWorkspace = host.querySelector<HTMLButtonElement>('button[aria-label="Select workspace Peer home"]')!;
  await act(async () => peerWorkspace.click());
  expect(detail().map(node => node.getAttribute("aria-label"))).toEqual(["Peer home workspace contents"]);
  expect(intersections.observed()).toEqual([]);
  expect(selectionCue(host)).toBeNull();
  expect(mux.state.activeTabIds).toEqual({ "Local home": "local-worker" });

  await act(async () => setSidebarMachineFilter({ id: "blackstar", name: "BLACKSTAR" }));
  expect(host.querySelector('[data-testid="workspace-card-grid"]')).toBe(grid);
  expect(detail().map(node => node.getAttribute("aria-label"))).toEqual(["Peer home workspace contents"]);
  expect(cards()).toHaveLength(1);
  expect(detail()[0].querySelector('[data-testid="team-name"]')?.textContent).toBe("Peer plan title");
  expect(detail()[0].querySelector('[data-testid="team-status-unavailable"]')).not.toBeNull();
  expect(detail()[0].querySelector('[data-testid="team-status-tally"]')).toBeNull();
  expect(row(host, "mesh:blackstar/peer-lead")?.closest("[hidden]")).toBeNull();
  expect(row(host, "mesh:air/other-worker")?.closest("[hidden]")).toBeNull();
  const actions = row(host, "mesh:air/other-worker")!.querySelector<HTMLButtonElement>('button[aria-label="Tab actions"]')!;
  await act(async () => actions.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  const menu = document.getElementById(actions.getAttribute("aria-controls")!)!;
  const rename = menu.querySelector<HTMLButtonElement>('[data-testid="tab-menu-rename"]')!;
  const restart = menu.querySelector<HTMLButtonElement>('[data-testid="tab-menu-restart-session"]')!;
  const reconnect = menu.querySelector<HTMLButtonElement>('[data-testid="chat-reconnect"]')!;
  const close = menu.querySelector<HTMLButtonElement>('[data-testid="tab-menu-close"]')!;
  expect(rename.getAttribute("aria-disabled")).toBeNull();
  expect(restart.getAttribute("aria-disabled")).toBeNull();
  expect(close.getAttribute("aria-disabled")).toBeNull();
  expect(reconnect.getAttribute("aria-disabled")).toBe("true");
  expect(document.getElementById(reconnect.getAttribute("aria-describedby")!)?.textContent).toBeTruthy();
  await act(async () => reconnect.click());
  expect(actions.getAttribute("aria-expanded")).toBe("true");

  expect(row(host, "local-worker")).toBeNull();
  expect(mux.state.activeTabIds).toEqual({ "Local home": "local-worker" });
  expect(intersections.observed()).toEqual([]);
  expect(selectionCue(host)).toBeNull();
});

it("clears an off-screen General cue when filtering hides the selected row in place", async () => {
  const intersections = mockSelectionIntersections();
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const main = member("main", "general", { protected: true, role: "Manager", role_profile: "general_agent" });
  setMux([{ ...workspaceOf("general", [main]), protected: true }], [main], "general");
  mux.state = { ...mux.state, activeTabIds: { general: main.id } };
  const { host, root } = mountHost();
  await act(async () => root.render(<PairedWorkspaceInventoryContext.Provider value={{
    browse: null,
    generals: [{ peerId: "peer", name: "Desk", availability: "offline", cached: true, sessions: [{ viewerId: "mesh:peer/main", tab: main }] }],
  }}><Sidebar config={config} /></PairedWorkspaceInventoryContext.Provider>));
  const selected = row(host, main.id)!;
  await intersections.observerFor(selected).emit(selected, -28);
  expect(selectionCue(host)).not.toBeNull();

  await act(async () => setSidebarMachineFilter({ id: "peer", name: "Desk" }));
  expect(row(host, main.id)).toBe(selected);
  expect(selected.hidden).toBe(true);
  expect(intersections.observed()).toEqual([]);
  expect(selectionCue(host)).toBeNull();

  await act(async () => setSidebarMachineFilter("local"));
  expect(intersections.observed()).toHaveLength(1);
  expect(intersections.observed()[0]).toBe(selected);
  await intersections.observerFor(selected).emit(selected, 100);
  expect(selectionCue(host)?.getAttribute("aria-label")).toBe("Show selected tab, main, below");
  expect(mux.state.activeTabIds).toEqual({ general: main.id });
});

it("keeps exactly one selected row across General and remote Team cards and auto-selects per machine on switch", async () => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const localMain = member("local-main", "general", { protected: true, role: "Manager", role_profile: "general_agent" });
  const localWorker = worker("local-worker", "Local home");
  const general = { ...workspaceOf("general", [localMain]), protected: true };
  const localHome = workspaceOf("Local home", [localWorker]);
  setMux([general, localHome], [localMain, localWorker], "Local home");
  mux.state = { ...mux.state, activeTabIds: { general: localMain.id, "Local home": localWorker.id } };

  const remoteMain = member("main", "remote-general", { title: "Main", protected: true, role: "Manager", role_profile: "general_agent" });
  const keats = manager("keats", "sauron");
  const verdi = worker("verdi", "sauron");
  const sauronWorkspace = workspaceOf("sauron", [keats, verdi]);
  const otherRemoteTab = worker("other-tab", "other-ws");
  const otherRemoteWorkspace = workspaceOf("other-ws", [otherRemoteTab]);

  const inventory = {
    browse: null,
    generals: [
      {
        peerId: "eight",
        name: "eight",
        availability: "online" as const,
        sessions: [{ viewerId: "mesh:eight/main", tab: remoteMain }],
        lastActiveTab: { viewerId: "mesh:eight/keats", workspaceId: "sauron" },
      },
      {
        peerId: "nine",
        name: "nine",
        availability: "online" as const,
        sessions: [{ viewerId: "mesh:nine/main", tab: remoteMain }],
        lastActiveTab: null,
      },
    ],
    peerWorkspaces: [
      {
        peerId: "eight",
        workspace: otherRemoteWorkspace,
        teams: [team("other-ws")],
        tabs: [{ viewerId: "mesh:eight/other-tab", tab: otherRemoteTab }],
      },
      {
        peerId: "eight",
        workspace: sauronWorkspace,
        teams: [team("sauron")],
        tabs: [{ viewerId: "mesh:eight/keats", tab: keats }, { viewerId: "mesh:eight/verdi", tab: verdi }],
        selected: true,
      },
    ],
  };

  const activeRows = (host: HTMLElement) => [...host.querySelectorAll<HTMLElement>('[data-testid="tab-item"]')]
    .filter(el => !el.closest("[hidden]") && el.querySelector('button[aria-current="page"]') !== null)
    .map(el => el.dataset.tabId);

  const { host, root } = mountHost();
  await act(async () => root.render(
    <PairedWorkspaceInventoryContext.Provider value={inventory}>
      <Sidebar config={config} />
    </PairedWorkspaceInventoryContext.Provider>,
  ));
  expect(activeRows(host)).toEqual(["local-worker"]);

  await act(async () => setSidebarMachineFilter({ id: "eight", name: "eight" }));
  expect(activeRows(host)).toEqual(["mesh:eight/keats"]);
  expect(host.querySelector('[data-testid="workspace-detail"]')?.getAttribute("data-workspace-id")).toBe("sauron");

  const eightMainButton = row(host, "mesh:eight/main")?.querySelector<HTMLButtonElement>("button");
  expect(eightMainButton).toBeTruthy();
  await act(async () => eightMainButton!.click());
  expect(activeRows(host)).toEqual(["mesh:eight/main"]);
  expect(row(host, "mesh:eight/keats")?.querySelector('button[aria-current="page"]')).toBeNull();

  const keatsButton = row(host, "mesh:eight/keats")?.querySelector<HTMLButtonElement>("button");
  expect(keatsButton).toBeTruthy();
  await act(async () => keatsButton!.click());
  expect(activeRows(host)).toEqual(["mesh:eight/keats"]);
  expect(row(host, "mesh:eight/main")?.querySelector('button[aria-current="page"]')).toBeNull();

  await act(async () => setSidebarMachineFilter({ id: "nine", name: "nine" }));
  expect(activeRows(host)).toEqual(["mesh:nine/main"]);

  await act(async () => setSidebarMachineFilter("local"));
  expect(activeRows(host)).toEqual(["local-worker"]);
  expect(host.querySelector('[data-testid="workspace-detail"]')?.getAttribute("data-workspace-id")).toBe("Local home");

  await act(async () => row(host, "local-main")!.querySelector<HTMLButtonElement>("button")!.click());
  expect(activeRows(host)).toEqual(["local-main"]);
  await act(async () => setSidebarMachineFilter({ id: "eight", name: "eight" }));
  expect(activeRows(host)).toEqual(["mesh:eight/keats"]);
  await act(async () => setSidebarMachineFilter("local"));
  expect(activeRows(host)).toEqual(["local-main"]);
});

it("waits for fresh peer sync before auto-selecting and respects a manual click during sync", async () => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const localMain = member("local-main", "general", { protected: true, role: "Manager", role_profile: "general_agent" });
  const general = { ...workspaceOf("general", [localMain]), protected: true };
  setMux([general], [localMain], "general");
  mux.state = { ...mux.state, activeTabIds: { general: localMain.id } };

  const remoteMain = member("main", "remote-general", { title: "Main", protected: true, role: "Manager", role_profile: "general_agent" });
  const staleTab = manager("stale", "sauron");
  const freshTab = worker("fresh", "sauron");
  const sauron = {
    peerId: "eight",
    workspace: workspaceOf("sauron", [staleTab, freshTab]),
    teams: [team("sauron")],
    tabs: [{ viewerId: "mesh:eight/stale", tab: staleTab }, { viewerId: "mesh:eight/fresh", tab: freshTab }],
    selected: true,
  };

  const activeRows = (host: HTMLElement) => [...host.querySelectorAll<HTMLElement>('[data-testid="tab-item"]')]
    .filter(el => !el.closest("[hidden]") && el.querySelector('button[aria-current="page"]') !== null)
    .map(el => el.dataset.tabId);

  const { host, root } = mountHost();
  const renderWith = (synced: boolean, lastActiveTab: { viewerId: string; workspaceId: string | null }) =>
    act(async () => root.render(
      <PairedWorkspaceInventoryContext.Provider value={{
        browse: { target: { id: "eight", name: "eight" }, availability: "online", workspaces: [sauron], synced },
        generals: [{
          peerId: "eight",
          name: "eight",
          availability: "online",
          sessions: [{ viewerId: "mesh:eight/main", tab: remoteMain }],
          lastActiveTab,
        }],
        peerWorkspaces: [sauron],
      }}><Sidebar config={config} /></PairedWorkspaceInventoryContext.Provider>,
    ));

  await renderWith(false, { viewerId: "mesh:eight/stale", workspaceId: "sauron" });
  await act(async () => setSidebarMachineFilter({ id: "eight", name: "eight" }));
  expect(activeRows(host)).toEqual([]);

  await renderWith(true, { viewerId: "mesh:eight/fresh", workspaceId: "sauron" });
  expect(activeRows(host)).toEqual(["mesh:eight/fresh"]);

  await act(async () => setSidebarMachineFilter("local"));
  expect(activeRows(host)).toEqual(["local-main"]);

  await renderWith(false, { viewerId: "mesh:eight/stale", workspaceId: "sauron" });
  await act(async () => setSidebarMachineFilter({ id: "eight", name: "eight" }));
  await act(async () => row(host, "mesh:eight/main")!.querySelector<HTMLButtonElement>("button")!.click());
  expect(activeRows(host)).toEqual(["mesh:eight/main"]);

  await renderWith(true, { viewerId: "mesh:eight/fresh", workspaceId: "sauron" });
  expect(activeRows(host)).toEqual(["mesh:eight/main"]);
});

it("wires the selected peer's authoritative Team summary into the real sidebar card", async () => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const localTabs = [worker("local-worker", "Local home")];
  setMux([workspaceOf("Local home", localTabs)], localTabs, "Local home");
  const peerId = "blackstar";
  const remoteId = "Peer home";
  const lead = manager("peer-lead", remoteId);
  const hand = worker("other-worker", remoteId);
  const remotePlan = {
    ...plan(remoteId),
    tasks: Array.from({ length: 25 }, (_, index) => ({ id: `loaded-${index}`, status: "done" })) as unknown as PlanView["tasks"],
    task_counts: { total: 94, pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 94, cancelled: 0 },
  } as PlanView;
  remotePlanCalls.result = {
    plans: [remotePlan], available: true, loading: false, reconnecting: false, error: null, refetch: async () => {},
  };
  const { host, root } = mountHost();
  await act(async () => root.render(<PairedWorkspaceInventoryContext.Provider value={{
    generals: [{ peerId, name: "BLACKSTAR", availability: "online", sessions: [] }],
    browse: null,
    peerWorkspaces: [{
      peerId, workspace: workspaceOf(remoteId, [lead, hand]), teams: [{ ...team(remoteId), name: null }], planTitles: new Map(),
      tabs: [{ viewerId: `mesh:${peerId}/${lead.id}`, tab: lead }, { viewerId: `mesh:${peerId}/${hand.id}`, tab: hand }],
    }],
  }}><Sidebar config={config} /></PairedWorkspaceInventoryContext.Provider>));

  await act(async () => setSidebarMachineFilter({ id: peerId, name: "BLACKSTAR" }));
  const detail = host.querySelector('[data-testid="workspace-detail"]');
  expect(remotePlanCalls.peerIds).toContain(peerId);
  expect(detail?.querySelector('[data-testid="team-name"]')?.textContent).toBe("Late plan");
  expect(detail?.querySelector('[data-testid="team-status-done"] .ct-plan-status-breakdown-count')?.textContent).toBe("94");
  expect(detail?.querySelector('[data-testid="team-status-tally"]')?.getAttribute("data-status-state")).toBe("verified");
});

it("closes the selected online remote workspace from its accessible read-only header", async () => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const localTabs = [worker("local-worker", "Local home")];
  setMux([workspaceOf("Local home", localTabs)], localTabs, "Local home");
  const peerId = "blackstar";
  const remoteId = "Peer home";
  const lead = manager("peer-lead", remoteId);
  const hand = worker("other-worker", remoteId);
  const { host, root } = mountHost();
  await act(async () => root.render(<PairedWorkspaceInventoryContext.Provider value={{
    generals: [{ peerId, name: "BLACKSTAR", availability: "online", sessions: [] }],
    browse: null,
    peerWorkspaces: [{
      peerId, workspace: workspaceOf(remoteId, [lead, hand]), teams: [{ ...team(remoteId), name: null }],
      planTitles: new Map([[team(remoteId).id, "Peer plan title"]]),
      tabs: [{ viewerId: `mesh:${peerId}/${lead.id}`, tab: lead }, { viewerId: `mesh:${peerId}/${hand.id}`, tab: hand }],
    }],
  }}><Sidebar config={config} /></PairedWorkspaceInventoryContext.Provider>));

  await act(async () => setSidebarMachineFilter({ id: peerId, name: "BLACKSTAR" }));
  const closeFromHeader = async () => {
    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Actions for Peer home"]')!;
    expect(trigger).toBeTruthy();
    await act(async () => trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    const menu = document.getElementById(trigger.getAttribute("aria-controls")!)!;
    const items = [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    expect(items.map(item => item.textContent?.trim())).toEqual(["New session", "New Terminal", "Rename", "Close Workspace"]);
    await act(async () => items.find(item => item.textContent?.trim() === "Close Workspace")!.click());
  };

  await closeFromHeader();
  let dialog = document.querySelector<HTMLElement>('[role="dialog"][aria-label="Close Peer home on BLACKSTAR"]');
  expect(dialog).not.toBeNull();
  expect(dialog?.textContent).toContain("Other agent tabs (2)");
  expect(remoteCloseCalls.requested).toEqual([{ peerId, workspaceId: remoteId, availability: "online", protected: false }]);
  await act(async () => dialog?.querySelector<HTMLButtonElement>("button")?.click());
  expect(remoteCloseCalls.confirmed).toEqual([]);

  await closeFromHeader();
  dialog = document.querySelector<HTMLElement>('[role="dialog"][aria-label="Close Peer home on BLACKSTAR"]');
  const confirm = [...(dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
    .find(button => button.textContent?.trim() === "Close workspace");
  expect(confirm).toBeTruthy();
  await act(async () => confirm?.click());
  expect(remoteCloseCalls.confirmed).toEqual([{ peerId, workspaceId: remoteId }]);
  expect(host.querySelector('[data-testid="workspace-detail"]')).toBeNull();
});

it("creates a remote workspace, closes a remote Team without Save as template, and closes a remote tab on an online peer", async () => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "fs_list_dir") return { path: "/srv/remote-app", parent: "/srv", home: "/srv", roots: [], entries: [] };
    return [];
  });
  const localTabs = [worker("local-worker", "Local home")];
  setMux([workspaceOf("Local home", localTabs)], localTabs, "Local home");
  const peerId = "blackstar";
  const remoteId = "Peer home";
  const lead = manager("peer-lead", remoteId);
  const hand = worker("other-worker", remoteId);
  const protectedTab = member("peer-protected", remoteId, { protected: true });
  const { host, root } = mountHost();
  await act(async () => root.render(<PairedWorkspaceInventoryContext.Provider value={{
    generals: [{ peerId, name: "BLACKSTAR", availability: "online", sessions: [] }],
    browse: null,
    peerWorkspaces: [{
      peerId,
      workspace: workspaceOf(remoteId, [lead, hand, protectedTab]),
      teams: [{ ...team(remoteId), name: "Remote Crew" }],
      planTitles: new Map(),
      tabs: [
        { viewerId: `mesh:${peerId}/${lead.id}`, tab: lead },
        { viewerId: `mesh:${peerId}/${hand.id}`, tab: hand },
        { viewerId: `mesh:${peerId}/${protectedTab.id}`, tab: protectedTab },
      ],
    }],
  }}><Sidebar config={config} /></PairedWorkspaceInventoryContext.Provider>));

  await act(async () => setSidebarMachineFilter({ id: peerId, name: "BLACKSTAR" }));

  const createButton = host.querySelector<HTMLButtonElement>('[data-testid="workspace-card-create"]')!;
  expect(createButton).toBeTruthy();
  expect(createButton.disabled).toBe(false);
  await act(async () => createButton.click());
  expect(vi.mocked(invoke)).toHaveBeenCalledWith("fs_list_dir", { path: "", showHidden: false, peerId });
  const selectFolderButton = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')]
    .find(button => button.textContent?.trim() === "Select Folder")!;
  await act(async () => selectFolderButton.click());
  expect(remoteCreateCalls).toEqual([{ peerId, cwd: "/srv/remote-app", name: "remote-app" }]);

  const teamTrigger = host.querySelector<HTMLButtonElement>('[data-testid="team-menu-trigger"]')!;
  await act(async () => teamTrigger.click());
  const teamMenuButtons = [...document.querySelectorAll<HTMLButtonElement>('[data-testid="team-menu"] [role="menuitem"]')];
  expect(teamMenuButtons.some(button => button.textContent?.includes("Save as template"))).toBe(false);
  const renameTeamButton = document.querySelector<HTMLButtonElement>('[data-testid="team-menu-rename"]')!;
  expect(renameTeamButton).toBeTruthy();
  expect(renameTeamButton.getAttribute("aria-disabled")).toBeNull();
  const closeTeamButton = teamMenuButtons.find(button => button.textContent?.includes("Close Team"))!;
  expect(closeTeamButton).toBeTruthy();
  expect(closeTeamButton.getAttribute("aria-disabled")).toBeNull();
  await act(async () => closeTeamButton.click());
  const teamDialog = document.querySelector<HTMLElement>('[role="dialog"][aria-label="Close Remote Crew on BLACKSTAR"]');
  expect(teamDialog).not.toBeNull();
  await act(async () => teamDialog?.querySelector<HTMLButtonElement>('[data-testid="group-close-confirm-button"]')?.click());
  expect(remoteTeamCloseCalls.confirmed).toEqual([{ peerId, workspaceId: remoteId, teamId: `team-${remoteId}` }]);

  const workerRow = row(host, `mesh:${peerId}/${hand.id}`)!;
  const workerMenuTrigger = workerRow.querySelector<HTMLButtonElement>('button[aria-label="Tab actions"]')!;
  await act(async () => workerMenuTrigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  const workerMenu = document.getElementById(workerMenuTrigger.getAttribute("aria-controls")!)!;
  const closeWorker = workerMenu.querySelector<HTMLButtonElement>('[data-testid="tab-menu-close"]')!;
  expect(closeWorker).toBeTruthy();
  expect(closeWorker.getAttribute("aria-disabled")).toBeNull();
  await act(async () => closeWorker.click());
  expect(mux.closeTab).toHaveBeenCalledWith(remoteId, `mesh:${peerId}/${hand.id}`);

  const protectedRow = row(host, `mesh:${peerId}/${protectedTab.id}`)!;
  const protectedMenuTrigger = protectedRow.querySelector<HTMLButtonElement>('button[aria-label="Tab actions"]')!;
  await act(async () => protectedMenuTrigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  const protectedMenu = document.getElementById(protectedMenuTrigger.getAttribute("aria-controls")!)!;
  expect(protectedMenu.querySelector('[data-testid="tab-menu-close"]')).toBeNull();
});

it.each([
  ["offline", "offline"],
  ["signed-out", "signed out"],
  ["update-required", "update required"],
] as const)("disables remote workspace, Team and tab actions with the %s status label and never sends", async (availability, label) => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const localTabs = [worker("local-worker", "Local home")];
  setMux([workspaceOf("Local home", localTabs)], localTabs, "Local home");
  const peerId = "blackstar";
  const remoteId = "Peer home";
  const lead = manager("peer-lead", remoteId);
  const hand = worker("other-worker", remoteId);
  const { host, root } = mountHost();
  await act(async () => root.render(<PairedWorkspaceInventoryContext.Provider value={{
    generals: [{ peerId, name: "BLACKSTAR", availability, sessions: [] }],
    browse: null,
    peerWorkspaces: [{
      peerId,
      workspace: workspaceOf(remoteId, [lead, hand]),
      teams: [{ ...team(remoteId), name: "Remote Crew" }],
      planTitles: new Map(),
      tabs: [
        { viewerId: `mesh:${peerId}/${lead.id}`, tab: lead },
        { viewerId: `mesh:${peerId}/${hand.id}`, tab: hand },
      ],
    }],
  }}><Sidebar config={config} /></PairedWorkspaceInventoryContext.Provider>));

  await act(async () => setSidebarMachineFilter({ id: peerId, name: "BLACKSTAR" }));

  const createButton = host.querySelector<HTMLButtonElement>('[data-testid="workspace-card-create"]')!;
  expect(createButton.disabled).toBe(true);
  expect(createButton.getAttribute("aria-label")).toContain(label);
  await act(async () => createButton.click());
  expect(remoteCreateCalls).toEqual([]);

  const wsTrigger = host.querySelector<HTMLButtonElement>('button[aria-label="Actions for Peer home"]')!;
  await act(async () => wsTrigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  const wsMenu = document.getElementById(wsTrigger.getAttribute("aria-controls")!)!;
  const wsItems = [...wsMenu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
  const closeWs = wsItems.find(item => item.textContent?.includes("Close Workspace"))!;
  expect(closeWs).toBeTruthy();
  expect(closeWs.textContent).toContain("Close Workspace");
  expect(closeWs.textContent).toContain(label);
  expect(closeWs.getAttribute("aria-disabled")).toBe("true");
  await act(async () => closeWs.click());
  expect(remoteCloseCalls.requested).toEqual([]);
  const newSessionItem = wsItems.find(item => item.textContent?.includes("New session"))!;
  expect(newSessionItem.getAttribute("aria-disabled")).toBe("true");
  expect(newSessionItem.textContent).toContain(label);
  const workspaceNewSessionButton = host.querySelector<HTMLButtonElement>('[data-testid="workspace-detail"] [data-testid="sidebar-new-session"]');
  expect(workspaceNewSessionButton?.disabled).toBe(true);

  const teamTrigger = host.querySelector<HTMLButtonElement>('[data-testid="team-menu-trigger"]')!;
  await act(async () => teamTrigger.click());
  const renameTeam = document.querySelector<HTMLButtonElement>('[data-testid="team-menu-rename"]')!;
  expect(renameTeam.textContent).toContain("Rename");
  expect(renameTeam.textContent).toContain(label);
  expect(renameTeam.getAttribute("aria-disabled")).toBe("true");
  const closeTeam = document.querySelector<HTMLButtonElement>('[data-testid="team-menu-close"]')!;
  expect(closeTeam.textContent).toContain("Close Team");
  expect(closeTeam.textContent).toContain(label);
  expect(closeTeam.getAttribute("aria-disabled")).toBe("true");
  await act(async () => closeTeam.click());
  expect(remoteTeamCloseCalls.requested).toEqual([]);

  const workerRow = row(host, `mesh:${peerId}/${hand.id}`)!;
  const workerMenuTrigger = workerRow.querySelector<HTMLButtonElement>('button[aria-label="Tab actions"]')!;
  await act(async () => workerMenuTrigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  const workerMenu = document.getElementById(workerMenuTrigger.getAttribute("aria-controls")!)!;
  const closeWorker = workerMenu.querySelector<HTMLButtonElement>('[data-testid="tab-menu-close"]')!;
  expect(closeWorker.getAttribute("aria-disabled")).toBe("true");
  expect(closeWorker.textContent).toContain(label);
  await act(async () => closeWorker.click());
  expect(mux.closeTab).not.toHaveBeenCalled();
});

it("collapses non-local machine groups by default with 6 machines, persists chevron toggles, and never scrolls to top on clicks", async () => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  const localOne = workspaceOf("local-one", [worker("l1", "local-one")]);
  const localTwo = workspaceOf("local-two", [worker("l2", "local-two")]);
  setMux([localOne, localTwo], [worker("l1", "local-one"), worker("l2", "local-two")], "local-one");

  const peers = [
    { peerId: "peer-rog", name: "ROG", ws: "rog-ws", title: "Rogue App" },
    { peerId: "peer-ubunta", name: "ubunta", ws: "ubunta-ws", title: "Ubuntu Box" },
    { peerId: "peer-eight", name: "eight", ws: "eight-ws", title: "Eight Core" },
    { peerId: "peer-rollacodebot", name: "rollacodebot", ws: "bot-ws", title: "Bot Ops" },
    { peerId: "peer-air", name: "air", ws: "air-ws", title: "Air Notes" },
  ];
  const inventory = {
    browse: null,
    generals: peers.map(({ peerId, name }) => ({ peerId, name, availability: "online" as const, sessions: [] })),
    peerWorkspaces: peers.map(({ peerId, ws, title }) => {
      const tab = worker(`${peerId}-w`, ws);
      return {
        peerId,
        workspace: { ...workspaceOf(ws, [tab]), name: title },
        teams: [team(ws)],
        planTitles: new Map<string, string>(),
        tabs: [{ viewerId: `mesh:${peerId}/${tab.id}`, tab }],
      };
    }),
  };

  const renderSidebar = () => (
    <PairedWorkspaceInventoryContext.Provider value={inventory}>
      <Sidebar config={config} />
    </PairedWorkspaceInventoryContext.Provider>
  );

  const { host, root } = mountHost();
  await act(async () => root.render(renderSidebar()));
  await act(async () => setSidebarMachineFilter("all"));

  const sections = () => [...host.querySelectorAll<HTMLElement>('[data-testid="workspace-machine-section"]')];
  expect(sections()).toHaveLength(6);

  const localToggle = sections()[0].querySelector<HTMLButtonElement>('[data-testid="workspace-machine-toggle"]')!;
  expect(localToggle.getAttribute("aria-expanded")).toBe("true");
  expect(localToggle.getAttribute("aria-label")).toContain("this machine");

  for (const peer of peers) {
    const toggle = host.querySelector<HTMLButtonElement>(
      `[data-testid="workspace-machine-section"][data-peer-id="${peer.peerId}"] [data-testid="workspace-machine-toggle"]`,
    )!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(toggle.getAttribute("aria-label")).toContain(peer.name);
    expect(toggle.getAttribute("aria-label")).toContain("online");
  }
  expect(host.querySelectorAll('[data-testid="workspace-card"]')).toHaveLength(2);

  const viewport = host.querySelector<HTMLElement>(".ct-sidebar-content")!;
  viewport.getBoundingClientRect = () => new DOMRect(0, 0, 260, 420);
  viewport.scrollTop = 280;

  const rogToggle = host.querySelector<HTMLButtonElement>(
    '[data-testid="workspace-machine-section"][data-peer-id="peer-rog"] [data-testid="workspace-machine-toggle"]',
  )!;
  await act(async () => rogToggle.click());
  expect(rogToggle.getAttribute("aria-expanded")).toBe("true");
  expect(viewport.scrollTop).toBe(280);

  const rogTile = host.querySelector<HTMLButtonElement>('button[aria-label="Select workspace Rogue App"]')!;
  expect(rogTile).toBeTruthy();
  const rogCard = rogTile.closest<HTMLElement>('[data-testid="workspace-card"]')!;
  rogCard.getBoundingClientRect = () => new DOMRect(0, 180, 42, 40);
  await act(async () => rogTile.click());
  expect(rogTile.getAttribute("aria-pressed")).toBe("true");
  expect(viewport.scrollTop).toBe(280);

  const localTwoTile = host.querySelector<HTMLButtonElement>('button[aria-label="Select workspace local-two"]')!;
  const localTwoCard = localTwoTile.closest<HTMLElement>('[data-testid="workspace-card"]')!;
  localTwoCard.getBoundingClientRect = () => new DOMRect(0, 120, 42, 40);
  await act(async () => localTwoTile.click());
  expect(viewport.scrollTop).toBe(280);

  await act(async () => localToggle.click());
  expect(localToggle.getAttribute("aria-expanded")).toBe("false");
  expect(viewport.scrollTop).toBe(280);

  const workspacesSectionToggle = host.querySelector<HTMLButtonElement>('[data-testid="workspaces-section-toggle"]')!;
  await act(async () => workspacesSectionToggle.click());
  expect(workspacesSectionToggle.getAttribute("aria-expanded")).toBe("false");
  expect(sections()).toHaveLength(0);
  await act(async () => workspacesSectionToggle.click());
  expect(workspacesSectionToggle.getAttribute("aria-expanded")).toBe("true");
  expect(sections()).toHaveLength(6);
  expect(host.querySelector<HTMLButtonElement>(
    '[data-testid="workspace-machine-section"][data-peer-id="peer-rog"] [data-testid="workspace-machine-toggle"]',
  )?.getAttribute("aria-expanded")).toBe("true");

  await act(async () => setSidebarMachineFilter({ id: "peer-ubunta", name: "ubunta" }));
  expect(host.querySelector('button[aria-label="Select workspace Ubuntu Box"]')).not.toBeNull();

  const second = mountHost();
  await act(async () => setSidebarMachineFilter("all"));
  await act(async () => second.root.render(renderSidebar()));
  expect(second.host.querySelector<HTMLButtonElement>(
    '[data-testid="workspace-machine-section"][data-peer-id="peer-rog"] [data-testid="workspace-machine-toggle"]',
  )?.getAttribute("aria-expanded")).toBe("true");
  expect(second.host.querySelectorAll<HTMLButtonElement>('[data-testid="workspace-machine-toggle"]')[0]
    .getAttribute("aria-expanded")).toBe("false");
});
