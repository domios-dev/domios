// Run: npx tsx src/components/Sidebar/sidebarSelection.test.ts

import type { MultiplexerState, Tab, Workspace } from "../../lib/types";
import {
  projectBrowsedWorkspaces,
  projectPairedGeneral,
  resolvePeerLastActiveTab,
  type BrowsedWorkspace,
  type PairedGeneral,
} from "../../store/multiplexer/pairedWorkspaceProjection";
import { projectAttachedRemoteTab } from "../../lib/remoteTabPortal";
import { sidebarWorkspaceKey } from "./sidebarMachineFilter";
import {
  isGeneralSectionOwningTab,
  isWorkspacePlanEntryActive,
  isWorkspaceTabActive,
  resolveMachineSwitchSelection,
  resolveObservedWorkspaceSelectedViewerId,
  resolveSidebarActiveTabId,
  workspacePlanTabId,
} from "./sidebarSelection";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const makeTab = (id: string, extra: Partial<Tab> = {}): Tab => ({
  id,
  title: id,
  tab_type: "Terminal",
  pty_id: null,
  cwd: null,
  ...extra,
});

const makeWorkspace = (id: string, tabs: string[], extra: Partial<Workspace> = {}): Workspace => ({
  id,
  name: id,
  tabs,
  active_tab_id: tabs[0] ?? null,
  cwd: null,
  ...extra,
});

test("plan active only when workspace is visual and canvas is open", () => {
  assert(!isWorkspacePlanEntryActive(false, false), "inactive workspace");
  assert(!isWorkspacePlanEntryActive(false, true), "inactive workspace even if canvas flag set");
  assert(!isWorkspacePlanEntryActive(true, false), "workspace mode but canvas closed");
  assert(isWorkspacePlanEntryActive(true, true), "workspace mode with canvas open");
});

test("workspace plan tab id is stable and workspace-scoped", () => {
  assert(workspacePlanTabId("ws-1") === "workspace-plan:ws-1", "uses virtual workspace plan namespace");
  assert(workspacePlanTabId("workspace_abc") === "workspace-plan:workspace_abc", "does not derive from Tab/tab ids");
});

test("tab active only when workspace visual, canvas closed, and tab matches", () => {
  assert(isWorkspaceTabActive(true, false, "tab-a", "tab-a"), "matching tab");
  assert(!isWorkspaceTabActive(true, false, "tab-a", "tab-b"), "different tab");
  assert(!isWorkspaceTabActive(true, true, "tab-a", "tab-a"), "canvas open suppresses tab highlight");
  assert(!isWorkspaceTabActive(false, false, "tab-a", "tab-a"), "inactive workspace");
});

test("plan and tab highlights are mutually exclusive for the same workspace", () => {
  const cases: Array<[boolean, boolean, string, string | undefined]> = [
    [true, true, "tab-1", "tab-1"],
    [true, true, "tab-1", "tab-2"],
    [true, false, "tab-1", "tab-1"],
    [true, false, "tab-1", undefined],
    [false, true, "tab-1", "tab-1"],
  ];
  for (const [visual, canvas, tabId, activeTabId] of cases) {
    const plan = isWorkspacePlanEntryActive(visual, canvas);
    const tab = isWorkspaceTabActive(visual, canvas, tabId, activeTabId);
    assert(!(plan && tab), `plan+tab both active: visual=${visual} canvas=${canvas} tab=${tabId} active=${activeTabId}`);
  }
});

test("sidebar active tab id requires workspace mode without canvas", () => {
  assert(resolveSidebarActiveTabId(true, false, "ws-1", { "ws-1": "tab-1" }) === "tab-1", "workspace tab active");
  assert(resolveSidebarActiveTabId(false, false, "ws-1", { "ws-1": "tab-1" }) === null, "non-workspace view clears active tab");
  assert(resolveSidebarActiveTabId(true, true, "ws-1", { "ws-1": "tab-1" }) === null, "canvas open clears active tab");
  assert(resolveSidebarActiveTabId(true, false, null, { "ws-1": "tab-1" }) === null, "no active workspace clears active tab");
});

test("observed workspace selection never falls back to remote workspace active_tab_id", () => {
  const viewers = new Set(["mesh:eight/keats", "mesh:eight/verdi"]);
  assert(
    resolveObservedWorkspaceSelectedViewerId("mesh:eight/keats", viewers) === "mesh:eight/keats",
    "matches viewer in this remote workspace",
  );
  assert(
    resolveObservedWorkspaceSelectedViewerId("mesh:eight/main", viewers) === null,
    "selecting General Main clears remote workspace selection",
  );
  assert(
    resolveObservedWorkspaceSelectedViewerId(null, viewers) === null,
    "null active tab leaves remote workspace unselected",
  );
  assert(
    resolveObservedWorkspaceSelectedViewerId("mesh:eight/keats", viewers, true) === null,
    "tab owned by General is not double-selected in remote workspace",
  );
});

test("general section owns local tabs, remote general viewers, and local-team remote tabs, not remote workspace viewers", () => {
  const localMain = makeTab("local-main", { protected: true, workspace_id: "general" });
  const remoteTeamWorker = makeTab("mesh:eight/juniper", {
    workspace_id: "general",
    remote_peer_id: "eight",
    remote_tab_id: "juniper",
    team_id: "local-general-team",
  });
  const remoteWorkspaceViewer = projectAttachedRemoteTab(
    makeTab("keats", { role: "Manager", team_id: "sauron-team" }),
    { peer_id: "eight", peer_name: "eight" },
    "general",
  );
  const tabs: Record<string, Tab> = {
    [localMain.id]: localMain,
    [remoteTeamWorker.id]: remoteTeamWorker,
    [remoteWorkspaceViewer.id]: remoteWorkspaceViewer,
  };
  const observedGeneral = new Set(["mesh:eight/main"]);
  assert(isGeneralSectionOwningTab("general", "general", "local-main", tabs, observedGeneral), "owns local General tab");
  assert(isGeneralSectionOwningTab("general", "general", "mesh:eight/main", tabs, observedGeneral), "owns remote General viewer");
  assert(isGeneralSectionOwningTab("general", "general", "mesh:eight/juniper", tabs, observedGeneral), "owns local-team remote tab in General");
  assert(!isGeneralSectionOwningTab("general", "general", "mesh:eight/keats", tabs, observedGeneral), "does not own remote workspace viewer");
  assert(!isGeneralSectionOwningTab("other-ws", "general", "mesh:eight/main", tabs, observedGeneral), "inactive General owns nothing");
});

test("resolvePeerLastActiveTab uses peer active workspace tab, then focused tab, then Main fallback", () => {
  const remoteMain = makeTab("main", {
    title: "Main",
    protected: true,
    role: "Manager",
    role_profile: "general_agent",
    workspace_id: "remote-general",
  });
  const keats = makeTab("keats", {
    title: "Keats",
    role: "Manager",
    role_profile: "orchestrator",
    team_id: "sauron-team",
    workspace_id: "sauron",
  });
  const verdi = makeTab("verdi", {
    title: "Verdi",
    role: "Worker",
    team_id: "sauron-team",
    workspace_id: "sauron",
  });
  const concierge = makeTab("concierge", {
    title: "Concierge",
    workspace_id: "sauron",
    neighbor_authority: { uid: "u", friendship_id: "f", root_tab_id: null },
  });
  const generalWs = makeWorkspace("remote-general", [remoteMain.id], { protected: true, active_tab_id: remoteMain.id });
  const sauronWs = makeWorkspace("sauron", [keats.id, verdi.id, concierge.id], { active_tab_id: keats.id });
  const baseSnapshot: MultiplexerState = {
    workspaces: [generalWs, sauronWs],
    tabs: { [remoteMain.id]: remoteMain, [keats.id]: keats, [verdi.id]: verdi, [concierge.id]: concierge },
    teams: [{ id: "sauron-team", workspace_id: "sauron", name: "Sauron", created_at: 1, lifecycle: "active" }],
    active_workspace_id: "sauron",
    focused_tab_id: remoteMain.id,
  };
  const generalSessions = projectPairedGeneral("eight", baseSnapshot);
  const browsed = projectBrowsedWorkspaces("eight", baseSnapshot);

  const fromActiveWs = resolvePeerLastActiveTab(baseSnapshot, generalSessions, browsed);
  assert(
    fromActiveWs?.viewerId === "mesh:eight/keats" && fromActiveWs.workspaceId === "sauron",
    "prefers active workspace active_tab_id",
  );

  const fromLiveFocusedSummary = resolvePeerLastActiveTab(
    baseSnapshot,
    generalSessions,
    browsed,
    [{ ...remoteMain, focused: true }, { ...keats, focused: false }],
  );
  assert(
    fromLiveFocusedSummary?.viewerId === "mesh:eight/main" && fromLiveFocusedSummary.workspaceId === null,
    "prefers live focused tab from /api/tabs when cached snapshot lags behind record_client_selection",
  );

  const fromGeneralWs = resolvePeerLastActiveTab(
    { ...baseSnapshot, active_workspace_id: "remote-general", focused_tab_id: keats.id },
    generalSessions,
    browsed,
  );
  assert(
    fromGeneralWs?.viewerId === "mesh:eight/main" && fromGeneralWs.workspaceId === null,
    "selects remote Main when remote active workspace is General",
  );

  const fromFocused = resolvePeerLastActiveTab(
    {
      ...baseSnapshot,
      active_workspace_id: "sauron",
      workspaces: [generalWs, { ...sauronWs, active_tab_id: concierge.id }],
      focused_tab_id: verdi.id,
    },
    generalSessions,
    browsed,
  );
  assert(
    fromFocused?.viewerId === "mesh:eight/verdi" && fromFocused.workspaceId === "sauron",
    "falls back to focused_tab_id when active workspace tab is not selectable",
  );

  const fallbackMain = resolvePeerLastActiveTab(
    { ...baseSnapshot, active_workspace_id: null, focused_tab_id: null },
    generalSessions,
    browsed,
  );
  assert(
    fallbackMain?.viewerId === "mesh:eight/main" && fallbackMain.workspaceId === null,
    "falls back to remote Main when no active or focused tab is known",
  );

  const noMainNoActive = resolvePeerLastActiveTab(
    { ...baseSnapshot, active_workspace_id: null, focused_tab_id: null },
    [],
    browsed,
  );
  assert(noMainNoActive === null, "never guesses a workspace tab when neither state nor Main exists");
});

test("resolveMachineSwitchSelection switches to peer last active tab and restores local tab on return", () => {
  const localMain = makeTab("local-main", { protected: true, workspace_id: "general", role: "Manager", role_profile: "general_agent" });
  const localWorker = makeTab("local-worker", { workspace_id: "domios", role: "Worker" });
  const localGeneral = makeWorkspace("general", [localMain.id], { protected: true, active_tab_id: localMain.id });
  const localDomios = makeWorkspace("domios", [localWorker.id], { active_tab_id: localWorker.id });
  const remoteMain = makeTab("main", { protected: true, role: "Manager", role_profile: "general_agent" });
  const keats = makeTab("keats", { role: "Manager", team_id: "sauron-team" });
  const sauron: BrowsedWorkspace = {
    peerId: "eight",
    workspace: makeWorkspace("sauron", [keats.id], { active_tab_id: keats.id }),
    teams: [{ id: "sauron-team", workspace_id: "sauron", name: "Sauron", created_at: 1, lifecycle: "active" }],
    tabs: [{ viewerId: "mesh:eight/keats", tab: keats }],
    selected: true,
  };
  const generals: PairedGeneral[] = [
    {
      peerId: "eight",
      name: "eight",
      availability: "online",
      sessions: [{ viewerId: "mesh:eight/main", tab: remoteMain }],
      lastActiveTab: { viewerId: "mesh:eight/keats", workspaceId: "sauron" },
    },
    {
      peerId: "nine",
      name: "nine",
      availability: "online",
      sessions: [{ viewerId: "mesh:nine/main", tab: remoteMain }],
      lastActiveTab: null,
    },
  ];
  const localTabs = { [localMain.id]: localMain, [localWorker.id]: localWorker };

  const toEight = resolveMachineSwitchSelection({
    filter: { peerId: "eight" },
    activeWorkspaceId: "domios",
    activeTabId: "local-worker",
    generalWorkspaceId: "general",
    lastLocal: { workspaceId: "domios", tabId: "local-worker" },
    workspaces: [localGeneral, localDomios],
    tabs: localTabs,
    generals,
    peerWorkspaces: [sauron],
  });
  assert(
    toEight?.workspaceId === "general"
      && toEight.tabId === "mesh:eight/keats"
      && toEight.preferredWorkspaceKey === sidebarWorkspaceKey("eight", "sauron"),
    "switches to peer's last active tab and selects its workspace",
  );

  const toNine = resolveMachineSwitchSelection({
    filter: { peerId: "nine" },
    activeWorkspaceId: "general",
    activeTabId: "mesh:eight/keats",
    generalWorkspaceId: "general",
    lastLocal: { workspaceId: "domios", tabId: "local-worker" },
    workspaces: [localGeneral, localDomios],
    tabs: localTabs,
    generals,
    peerWorkspaces: [sauron],
  });
  assert(
    toNine?.workspaceId === "general"
      && toNine.tabId === "mesh:nine/main"
      && toNine.preferredWorkspaceKey === undefined,
    "falls back to peer's Main tab when no last active tab is known",
  );

  const backToLocal = resolveMachineSwitchSelection({
    filter: "local",
    activeWorkspaceId: "general",
    activeTabId: "mesh:nine/main",
    generalWorkspaceId: "general",
    lastLocal: { workspaceId: "domios", tabId: "local-worker" },
    workspaces: [localGeneral, localDomios],
    tabs: localTabs,
    generals,
    peerWorkspaces: [sauron],
  });
  assert(
    backToLocal?.workspaceId === "domios"
      && backToLocal.tabId === "local-worker"
      && backToLocal.preferredWorkspaceKey === sidebarWorkspaceKey(null, "domios"),
    "restores local machine's remembered tab and workspace",
  );

  const closedLocalFallback = resolveMachineSwitchSelection({
    filter: "local",
    activeWorkspaceId: "general",
    activeTabId: "mesh:eight/keats",
    generalWorkspaceId: "general",
    lastLocal: { workspaceId: "deleted-ws", tabId: "deleted-tab" },
    workspaces: [localGeneral, localDomios],
    tabs: localTabs,
    generals,
    peerWorkspaces: [sauron],
  });
  assert(
    closedLocalFallback?.workspaceId === "general" && closedLocalFallback.tabId === "local-main",
    "falls back to local Main when remembered local tab was closed",
  );

  const toAll = resolveMachineSwitchSelection({
    filter: "all",
    activeWorkspaceId: "general",
    activeTabId: "mesh:eight/keats",
    generalWorkspaceId: "general",
    lastLocal: { workspaceId: "domios", tabId: "local-worker" },
    workspaces: [localGeneral, localDomios],
    tabs: localTabs,
    generals,
    peerWorkspaces: [sauron],
  });
  assert(toAll === null, "switching to all keeps current selection");
});

function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

main();
