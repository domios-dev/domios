import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Tab, Team, Workspace } from "../../lib/types";
import { SidebarGeneralSection } from "./SidebarGeneralSection";
import type { SidebarWorkspaceDetailActions, SidebarWorkspaceDetailData } from "./SidebarWorkspaceDetail";
import { SidebarDnd } from "./sortable";
import { publishNeighborGateways } from "../Neighbors/neighborGateways";

vi.mock("../../lib/bridge", () => ({ invoke: vi.fn().mockResolvedValue([]), listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../hooks/useWorkspaceCrons", () => ({ useWorkspaceCrons: () => ({ jobs: [], refresh: () => {} }) }));
vi.mock("./useGeneralJobs", () => ({ useGeneralJobs: () => ({ jobs: [], failed: false, refresh: () => {} }) }));
vi.mock("../MeshPanel/useMeshData", () => ({ useMeshData: () => ({
  services: [], peers: [], thisMachine: { name: "Local", ip: "" }, loading: false, error: null, loaded: true, refresh: () => {},
}) }));
vi.mock("./SpinalCord", () => ({ SpinalCordButton: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => { localStorage.clear(); publishNeighborGateways({}); });

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
  closeWorkspace: noop, newTerminal: noop, newSession: noop, newAgent: noop, openRemoteTab: noop,
};

async function mount() {
  const tabs = Object.fromEntries([main, manager, agent, ...shells, surface].map((tab) => [tab.id, tab]));
  const data: SidebarWorkspaceDetailData = {
    tabs, tabStatuses: {}, attention: {}, teams: [team], planIndex: [], plans: [], providers: [], providersLoading: false,
    closingTabIds: new Set(), appearingTabIds: new Set(), closingTeamIds: new Set(), activeTabIds: { general: main.id },
    activeWorkspaceId: "general", isWorkspaceMode: true, workspaceCanvasActive: false, touchMode: true, compact: false,
    sidebarActionText: "", orchestrators: [manager], newSessionDisabled: false,
  };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = async (expanded: boolean) => act(async () => root.render(
    <SidebarDnd handlers={{ reorderTabs: noop, reorderDisplay: noop, reorderWorkspaces: noop }}>
      <SidebarGeneralSection workspace={workspace} expanded={expanded} onToggle={noop} data={data} actions={actions} workspaces={[workspace]} />
    </SidebarDnd>,
  ));
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

it("keeps Neighbor concierge agents out of the General session rows", async () => {
  publishNeighborGateways({ friend: agent.id });
  const view = await mount();
  try { expect(view.host.querySelector('button[aria-label="Open tab Solo"]')).toBeNull(); }
  finally { await view.close(); }
});
