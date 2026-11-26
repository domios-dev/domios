import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Tab, Team, Workspace } from "../../lib/types";
import type { PairedGeneral } from "../../store/multiplexer/pairedWorkspaceProjection";
import { PairedWorkspaceInventoryContext } from "../../store/multiplexer/context";
import { SidebarGeneralSection } from "./SidebarGeneralSection";
import type { SidebarWorkspaceDetailActions, SidebarWorkspaceDetailData } from "./SidebarWorkspaceDetail";
import { SidebarDnd } from "./sortable";
import { publishNeighborGateways } from "../Neighbors/neighborGateways";
import { tabAttention, type TabAttention } from "./tabAttention";
import { setSidebarMachineFilter } from "./sidebarMachineFilter";
import { projectAttachedRemoteTab } from "../../lib/remoteTabPortal";
import { rememberAccountMachineNames } from "../../lib/useMeshMachineIdentities";
import { machine, pair, snapshot } from "../MeshPanel/accountMachines.testdata";

vi.mock("../../lib/bridge", async actual => ({ ...await actual<object>(), invoke: vi.fn().mockResolvedValue([]), listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../hooks/useWorkspaceCrons", () => ({ useWorkspaceCrons: () => ({ jobs: [], refresh: () => {} }) }));
vi.mock("./useGeneralJobs", () => ({ useGeneralJobs: () => ({ jobs: [], failed: false, refresh: () => {} }) }));
vi.mock("../MeshPanel/useMeshData", () => ({ useMeshData: () => ({
  services: [], peers: [], thisMachine: { name: "Local", ip: "" }, loading: false, error: null, loaded: true, refresh: () => {},
}) }));
vi.mock("./SpinalCord", () => ({ SpinalCordButton: () => null }));
vi.mock("../../store/multiplexer/hooks", async actual => ({
  ...await actual<object>(),
  useMultiplexer: () => ({ spawnAgentTab: vi.fn(), selectTabByTab: vi.fn(), upsertToast: vi.fn() }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => { localStorage.clear(); publishNeighborGateways({}); setSidebarMachineFilter("all"); });

const main: Tab = { id: "main", title: "Main", protected: true, role: "Manager", role_profile: "general_agent", agent_type: "fixture", pty_id: "main-pty", tab_type: "Terminal", cwd: "/general", sort_order: 10 };
const manager: Tab = { id: "quartz", title: "Quartz", role: "Manager", role_profile: "orchestrator", agent_type: "fixture", team_id: "team", pty_id: "quartz-pty", tab_type: "Terminal", cwd: "/general", sort_order: 2 };
const agent: Tab = { id: "solo", title: "Solo", agent_type: "fixture", pty_id: "solo-pty", tab_type: "Terminal", cwd: "/general", sort_order: 4 };
const shells: Tab[] = [
  { id: "sieve", title: "Sieve", sort_order: 1 },
  { id: "probe", title: "Probe", sort_order: 3, parent_tab_id: main.id },
  { id: "team-shell", title: "Team shell", sort_order: 5, team_id: "team", parent_tab_id: manager.id },
].map((tab): Tab => ({ ...tab, pty_id: `${tab.id}-pty`, tab_type: "Terminal", cwd: "/general", purpose: "qa" }));
const surface: Tab = {
  id: "surface", title: "Utility browser", pty_id: null, tab_type: "Terminal", cwd: null, parent_tab_id: shells[0].id,
  backing: { kind: "surface", plugin_id: "browser", surface_id: "page", instance_id: "utility" },
};
const team: Team = { id: "team", name: "Quartz team", lifecycle: "active", created_at: 1, workspace_id: "general" };
const workspace: Workspace = { id: "general", name: "General", protected: true, cwd: "/general", tabs: [...shells.map((tab) => tab.id), manager.id, agent.id, main.id, surface.id], active_tab_id: main.id };
const noop = () => {};
const actions: SidebarWorkspaceDetailActions = {
  activateWorkspace: noop, activateTab: noop, closeTab: noop, renameTab: noop, renameWorkspace: noop,
  closeWorkspace: noop, newTerminal: noop, newSession: noop, newAgent: noop,
};

async function mount(
  attention: Record<string, TabAttention> = {},
  pairedGenerals: PairedGeneral[] = [],
  onActivate: (workspaceId: string, tabId: string) => void = noop,
  independentGeneralTabs: Tab[] = [],
) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = async (expanded: boolean, attachedTabs: Tab[] = [], activeTabId: string = main.id, peers = pairedGenerals) => {
    const renderWorkspace = {
      ...workspace,
      tabs: [...workspace.tabs, ...independentGeneralTabs.map((tab) => tab.id), ...attachedTabs.map((tab) => tab.id)],
      active_tab_id: activeTabId,
    };
    const tabs = Object.fromEntries([main, manager, agent, ...shells, surface, ...independentGeneralTabs, ...attachedTabs].map((tab) => [tab.id, tab]));
    const data: SidebarWorkspaceDetailData = {
      tabs, tabStatuses: {}, attention, teams: [team], planIndex: [], plans: [], providers: [], providersLoading: false,
      closingTabIds: new Set(), closingTeamIds: new Set(), activeTabIds: { general: activeTabId },
      activeWorkspaceId: "general", isWorkspaceMode: true, workspaceCanvasActive: false, touchMode: true, compact: false,
      sidebarActionText: "", orchestrators: [manager], newSessionDisabled: false,
    };
    await act(async () => root.render(
      <SidebarDnd handlers={{ reorderTabs: noop, reorderDisplay: noop, reorderWorkspaces: noop }}>
        <PairedWorkspaceInventoryContext.Provider value={{ generals: peers, browse: null }}>
          <SidebarGeneralSection workspace={renderWorkspace} expanded={expanded} onToggle={noop} data={data}
            actions={{ ...actions, activateTab: onActivate }} workspaces={[renderWorkspace]} />
        </PairedWorkspaceInventoryContext.Provider>
      </SidebarDnd>,
    ));
  };
  await render(true);
  return { host, render, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}

function utilityToggle(host: HTMLElement) {
  const button = host.querySelector<HTMLButtonElement>('button[aria-label="UTILITIES 3"]');
  expect(button).not.toBeNull();
  return button!;
}

function shellButtons(host: ParentNode) {
  return Array.from(host.querySelectorAll<HTMLButtonElement>('button[aria-label^="Open tab qa shell,"]'));
}

it("collapsed Utilities hides every shell and exposes no shell outside its list", async () => {
  const view = await mount();
  try {
    expect(utilityToggle(view.host).getAttribute("aria-expanded")).toBe("false");
    const list = view.host.querySelector<HTMLUListElement>('ul[aria-label="Utility shells"]')!;
    expect(list.hidden).toBe(true);
    expect(shellButtons(view.host)).toHaveLength(3);
    const attachment = view.host.querySelector('button[aria-label="Open tab Utility browser"]')!;
    expect(attachment).not.toBeNull();
    expect(attachment.closest("[hidden]")).toBe(list);
    for (const button of shellButtons(view.host)) {
      expect(list.contains(button)).toBe(true);
      expect(button.closest("[hidden]")).toBe(list);
    }
  } finally { await view.close(); }
});

it("expanded Utilities shows each shell exactly once inside its controlled list and collapses again", async () => {
  const view = await mount();
  try {
    const toggle = utilityToggle(view.host);
    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const list = view.host.querySelector<HTMLUListElement>('ul[aria-label="Utility shells"]')!;
    expect(toggle.getAttribute("aria-controls")).toBe(list.id);
    expect(list.hidden).toBe(false);
    expect(view.host.querySelectorAll('button[aria-label="Open tab Utility browser"]')).toHaveLength(1);
    expect(list.querySelector('button[aria-label="Open tab Utility browser"]')).not.toBeNull();
    expect(shellButtons(view.host).map((button) => button.getAttribute("aria-label"))).toEqual([
      "Open tab qa shell, Sieve", "Open tab qa shell, Probe", "Open tab qa shell, Team shell",
    ]);
    for (const button of shellButtons(view.host)) {
      expect(list.contains(button)).toBe(true);
      expect(button.closest("[hidden]")).toBeNull();
    }
    await act(async () => toggle.click());
    expect(list.hidden).toBe(true);
    expect(shellButtons(view.host).every((button) => list.contains(button))).toBe(true);
  } finally { await view.close(); }
});

it("places Main, General teams and standalone agents inside General before New session and Utilities", async () => {
  const view = await mount();
  try {
    const general = view.host.querySelector('[role="region"][aria-label="General workspace contents"]')!;
    const mainButton = general.querySelector('button[aria-label="General Agent"]')!;
    expect(mainButton.closest('[data-testid="tab-item"]')?.querySelector('.sidebar-tab-body__sub--machine')).toBeNull();
    const teamRegion = general.querySelector('section[aria-label="Quartz team"]')!;
    const solo = general.querySelector('button[aria-label="Open tab Solo"]')!;
    const newSession = Array.from(general.querySelectorAll("button")).find((button) => button.textContent?.trim() === "New session")!;
    for (const element of [general, mainButton, teamRegion, solo, newSession]) expect(element).toBeTruthy();
    expect(mainButton.compareDocumentPosition(teamRegion) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(teamRegion.compareDocumentPosition(newSession) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(solo.compareDocumentPosition(newSession) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(newSession.compareDocumentPosition(utilityToggle(view.host)) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(general.contains(view.host.querySelector('button[aria-label="Open tab Quartz"]'))).toBe(true);
    await view.render(false);
    expect(view.host.querySelector('section[aria-label="Quartz team"]')).toBeNull();
    expect(shellButtons(view.host)).toHaveLength(0);
  } finally { await view.close(); }
});

it("lists paired General rows alphabetically and selects their composite viewer ids", async () => {
  const activate = vi.fn();
  const pairedGenerals: PairedGeneral[] = [
    { peerId: "peer-z", name: "Zulu", availability: "offline", sessions: [{
      viewerId: "mesh:peer-z/main", tab: { ...main, id: "main", title: "Zulu Main" }, status: "Working",
    }] },
    { peerId: "peer-a", name: "Alpha", availability: "online", sessions: [{
      viewerId: "mesh:peer-a/main", tab: { ...main, id: "main", title: "Alpha Main" }, status: "Idle",
    }] },
    { peerId: "peer-old", name: "Offline card", availability: "offline", sessions: [] },
  ];
  const view = await mount({}, pairedGenerals, activate);
  try {
    const remoteIds = [...view.host.querySelectorAll<HTMLElement>('[data-tab-id^="mesh:"]')].map(row => row.dataset.tabId);
    expect(remoteIds).toEqual(["mesh:peer-a/main", "mesh:peer-z/main"]);
    const localMain = view.host.querySelector('[data-testid="tab-item"][data-tab-id="main"]');
    expect(localMain?.getAttribute("aria-label")).toBe("General Agent");
    const unavailable = view.host.querySelector('[data-testid="tab-item"][data-tab-id="mesh:peer-z/main"]');
    expect(unavailable?.getAttribute("aria-label")).toContain("offline");
    expect(unavailable?.textContent).not.toContain("working");
    expect(unavailable?.querySelector('[data-testid="tab-working-activity"]')).toBeNull();
    expect(view.host.textContent).not.toContain("Offline card");
    const remoteRow = view.host.querySelector<HTMLElement>('[data-testid="tab-item"][data-tab-id="mesh:peer-a/main"]');
    expect(remoteRow).not.toBeNull();
    await act(async () => remoteRow?.querySelector("button")?.click());
    expect(activate).toHaveBeenCalledWith("general", "mesh:peer-a/main");
  } finally { await view.close(); }
});

it("adds the shared machine label to local and peer General subtitles only", async () => {
  const peerId = "peer-general";
  rememberAccountMachineNames(snapshot({
    local_installation_id: "local-installation",
    registry_names: [
      machine("local-installation", "Desk [dev-main]", { admitted_at: 10 }),
      machine("peer-installation", "Desk [dev-peer]", { admitted_at: 20 }),
    ],
    pairs: [pair(peerId, "Desk", { installation_id: "peer-installation" })],
  }));
  const viewerId = `mesh:${peerId}/main`;
  const view = await mount({}, [{
    peerId, name: "Desk [dev-peer]", availability: "online",
    sessions: [{ viewerId, tab: main }],
  }]);
  try {
    const subtitle = (id: string) => view.host.querySelector(`[data-tab-id="${id}"] [data-testid="tab-subline"]`)?.textContent;
    expect(subtitle(main.id)).toBe("General Agent · Desk #1");
    expect(subtitle(viewerId)).toBe("General Agent · Desk #2");
    expect(subtitle(manager.id)).not.toContain("Desk");
    expect(view.host.querySelector(`[data-tab-id="${agent.id}"]`)?.textContent).not.toContain("Desk");
  } finally {
    await view.close();
    rememberAccountMachineNames(null);
  }
});

it("retains one observed General viewer and the same row after attachment", async () => {
  const viewerId = "mesh:peer-a/main";
  const attachedViewer = {
    ...projectAttachedRemoteTab(main, { peer_id: "peer-a", peer_name: "Alpha" }, workspace.id),
    focus_requested: true,
  };
  const peer: PairedGeneral = {
    peerId: "peer-a", name: "Alpha", availability: "online", sessions: [{
      viewerId, tab: { ...main, title: "Peer Main" }, status: "Idle",
    }],
  };
  const independentGeneral: Tab = {
    ...agent, id: "independent-general", title: "Independent General", role_profile: "general_agent", sort_order: 6,
  };
  const activate = vi.fn();
  const view = await mount({ [viewerId]: tabAttention(attachedViewer, "Idle", []) }, [peer], activate, [independentGeneral]);
  try {
    const rowBeforeAttachment = view.host.querySelector<HTMLElement>(`[data-testid="tab-item"][data-tab-id="${viewerId}"]`);
    expect(view.host.querySelectorAll(`[data-testid="tab-item"][data-tab-id="${viewerId}"]`)).toHaveLength(1);
    expect(rowBeforeAttachment).not.toBeNull();

    await view.render(true, [attachedViewer]);
    const rowsAfterAttachment = view.host.querySelectorAll<HTMLElement>(`[data-testid="tab-item"][data-tab-id="${viewerId}"]`);
    expect(rowsAfterAttachment).toHaveLength(1);
    const rowAfterAttachment = rowsAfterAttachment[0];
    expect(rowAfterAttachment).toBe(rowBeforeAttachment);
    expect(rowAfterAttachment.querySelector('[data-kind="requested"]')).not.toBeNull();
    expect(view.host.querySelector('[data-testid="tab-item"][data-tab-id="independent-general"]')).not.toBeNull();

    await view.render(true, [attachedViewer], viewerId);
    const selectedRow = view.host.querySelector<HTMLElement>(`[data-testid="tab-item"][data-tab-id="${viewerId}"]`)!;
    expect(selectedRow.querySelector('button[aria-current="page"]')).not.toBeNull();
    await act(async () => selectedRow.querySelector("button")?.click());
    expect(activate).toHaveBeenCalledWith(workspace.id, viewerId);
  } finally { await view.close(); }
});

it("keeps project viewers opened by machine browsing out of the General list", async () => {
  const attached = projectAttachedRemoteTab(agent, { peer_id: "peer-a", peer_name: "Alpha" }, workspace.id);
  const view = await mount();
  try {
    await view.render(true, [attached]);
    expect(view.host.querySelector(`[data-tab-id="${attached.id}"]`)).toBeNull();
    expect(view.host.querySelector(`[data-tab-id="${main.id}"]`)).not.toBeNull();
  } finally { await view.close(); }
});

it("keeps equal raw tab ids distinct across different peers", async () => {
  const viewerA = "mesh:peer-a/shared";
  const viewerB = "mesh:peer-b/shared";
  const pairedGenerals: PairedGeneral[] = [
    { peerId: "peer-a", name: "Alpha", availability: "online", sessions: [{
      viewerId: viewerA, tab: { ...main, id: "shared", title: "Shared Main" },
    }] },
    { peerId: "peer-b", name: "Beta", availability: "online", sessions: [{
      viewerId: viewerB, tab: { ...main, id: "shared", title: "Shared Main" },
    }] },
  ];
  const activate = vi.fn();
  const view = await mount({}, pairedGenerals, activate);
  try {
    const rowA = view.host.querySelector<HTMLElement>(`[data-testid="tab-item"][data-tab-id="${viewerA}"]`)!;
    const rowB = view.host.querySelector<HTMLElement>(`[data-testid="tab-item"][data-tab-id="${viewerB}"]`)!;
    expect(view.host.querySelectorAll('[data-testid="tab-item"][data-tab-id="mesh:peer-a/shared"], [data-testid="tab-item"][data-tab-id="mesh:peer-b/shared"]')).toHaveLength(2);
    expect(rowA.getAttribute("aria-label")).toContain("Alpha");
    expect(rowB.getAttribute("aria-label")).toContain("Beta");
    await act(async () => rowA.querySelector("button")?.click());
    await act(async () => rowB.querySelector("button")?.click());
    expect(activate.mock.calls).toEqual([[workspace.id, viewerA], [workspace.id, viewerB]]);
  } finally { await view.close(); }
});

it("shows the same machine and state on the harness tooltip and the row menu", async () => {
  const peer: PairedGeneral = { peerId: "peer", name: "Desk [dev-a]", availability: "online",
    sessions: [{ viewerId: "mesh:peer/main", tab: { ...main, frozen: true }, status: "Idle" }] };
  const view = await mount({}, [peer]);
  try {
    const row = view.host.querySelector<HTMLElement>('[data-tab-id="mesh:peer/main"]')!;
    const harness = row.querySelector('[data-testid="tab-tab-icon"]')!.firstElementChild!;
    await act(async () => harness.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" })));
    const tip = document.querySelector('[role="tooltip"]')!;
    expect(tip.textContent).toContain("Desk");
    expect(tip.textContent).toContain("online");
    expect(tip.textContent).toContain("asleep");
    await act(async () => harness.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, pointerType: "mouse" })));
    const actions = row.querySelector<HTMLButtonElement>('button[aria-label="Tab actions"]')!;
    await act(async () => actions.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    expect(actions.getAttribute("aria-expanded")).toBe("true");
    const menu = document.getElementById(actions.getAttribute("aria-controls")!);
    expect(menu?.textContent).toContain("asleep");
    expect(menu?.textContent).toContain("Desk");
  } finally { await view.close(); }
});

it("filters General in place and preserves its selected row", async () => {
  const peer: PairedGeneral = { peerId: "peer", name: "Desk", availability: "online",
    sessions: [{ viewerId: "mesh:peer/main", tab: main, status: "Idle" }] };
  const view = await mount({}, [peer]);
  try {
    const local = view.host.querySelector<HTMLElement>('[data-tab-id="main"]')!;
    const remote = view.host.querySelector<HTMLElement>('[data-tab-id="mesh:peer/main"]')!;
    const selected = local.querySelector('button[aria-current="page"]');
    expect(selected).not.toBeNull();
    await act(async () => setSidebarMachineFilter({ id: "peer", name: "Desk" }));
    expect(local.hidden).toBe(true);
    expect(remote.hidden).toBe(false);
    expect(local.querySelector('button[aria-current="page"]')).toBe(selected);
    expect(view.host.querySelector('[aria-label="New session on another machine"]')).toBeNull();
    const peerNewSession = view.host.querySelector<HTMLButtonElement>('[data-testid="sidebar-new-session"]');
    expect(peerNewSession).not.toBeNull();
    expect(peerNewSession?.disabled).toBe(false);
    expect(peerNewSession?.closest("[hidden]")).toBeNull();
    await act(async () => setSidebarMachineFilter("local"));
    expect(local.hidden).toBe(false);
    expect(remote.hidden).toBe(true);
    expect(view.host.querySelector('[aria-label="New session on another machine"]')).toBeNull();
    await act(async () => setSidebarMachineFilter("all"));
    expect(remote.hidden).toBe(false);
    expect(view.host.querySelector('[data-tab-id="main"]')).toBe(local);
    expect(view.host.querySelector('[aria-label="New session on another machine"]')).toBeNull();
  } finally { await view.close(); }
});

it("hides General when the selected peer has no sessions and shows it when the filter clears", async () => {
  const peer: PairedGeneral = { peerId: "empty-peer", name: "Desk", availability: "online", sessions: [] };
  const view = await mount({}, [peer]);
  try {
    await act(async () => setSidebarMachineFilter({ id: peer.peerId, name: peer.name }));
    expect(view.host.querySelector('[data-testid="general-section"]')).toBeNull();
    await act(async () => setSidebarMachineFilter("all"));
    expect(view.host.querySelector('[data-testid="general-section"]')).not.toBeNull();
  } finally { await view.close(); }
});

it("keeps a restored concierge out of General before the Neighbor roster loads", async () => {
  const concierge: Tab = { ...agent, id: "restored-concierge", title: "Bound agent",
    neighbor_authority: { uid: "owner", friendship_id: "friend", root_tab_id: null } };
  const view = await mount({}, [], noop, [concierge]);
  try {
    expect(view.host.querySelector(`[data-tab-id="${concierge.id}"]`)).toBeNull();
    expect(view.host.querySelector(`[data-tab-id="${agent.id}"]`)).not.toBeNull();
    await act(async () => publishNeighborGateways({ friend: concierge.id }));
    expect(view.host.querySelector(`[data-tab-id="${concierge.id}"]`)).toBeNull();
  } finally { await view.close(); }
});

it("reconciles a cached General session in the same sidebar row", async () => {
  const peer: PairedGeneral = { peerId: "peer", name: "Desk", availability: "reconnecting", cached: true,
    sessions: [{ viewerId: "mesh:peer/main", tab: main, status: "Idle" }] };
  const view = await mount({}, [peer]);
  try {
    const first = view.host.querySelector('[data-tab-id="mesh:peer/main"]')!;
    expect(first.getAttribute("aria-label")).toContain("reconnecting");
    await view.render(true, [], main.id, [{
      ...peer, cached: false, availability: "online",
      sessions: [{ ...peer.sessions[0], tab: { ...main, boot_stage: "agent_ready" } }],
    }]);
    const ready = view.host.querySelector('[data-tab-id="mesh:peer/main"]')!;
    expect(ready).toBe(first);
    expect(ready.getAttribute("aria-label")).toContain("idle");
  } finally { await view.close(); }
});

it("keeps Neighbor concierge agents out of the General session rows", async () => {
  publishNeighborGateways({ friend: agent.id });
  const view = await mount();
  try { expect(view.host.querySelector('button[aria-label="Open tab Solo"]')).toBeNull(); }
  finally { await view.close(); }
});

it("collapsed Utilities carries a highlighted shell's mark on its header and hands it back to the row when expanded", async () => {
  const requested = { ...shells[1], focus_requested: true };
  const view = await mount({ [requested.id]: tabAttention(requested, null, []) });
  try {
    const toggle = view.host.querySelector<HTMLButtonElement>('button[aria-label="UTILITIES 3, 1 needs attention"]');
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(toggle?.querySelector('[data-kind="requested"]')).not.toBeNull();
    await act(async () => toggle!.click());
    expect(toggle!.getAttribute("aria-label")).toBe("UTILITIES 3");
    expect(toggle!.querySelector("[data-kind]")).toBeNull();
    const row = view.host.querySelector('button[aria-label="Open tab qa shell, Probe"]')!;
    expect(row.closest("li")?.querySelector('[data-kind="requested"]')).not.toBeNull();
  } finally { await view.close(); }
});
