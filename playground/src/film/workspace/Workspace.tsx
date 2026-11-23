import "../fixtures/supporter";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { canvasFocusStore } from "../../../../src/components/Canvas/canvasFocusStore";
import { ExtensionsView } from "../../../../src/components/Extensions/ExtensionsView";
import { Titlebar } from "../../../../src/components/Titlebar/Titlebar";
import { UpdateProvider } from "../../../../src/hooks/useUpdate";
import { UpdatePanelProvider } from "../../../../src/hooks/useUpdatePanel";
import { DEFAULT_THEME } from "../../../../src/lib/themeCss";
import { clearRelayTarget, relayTarget, setRelayTarget } from "../../../../src/lib/webApi";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { Tab, TabStatus, Team, Workspace as WorkspaceData } from "../../../../src/lib/types";
import { API, DEMO_PROVIDERS, ProductProviders } from "../../gallery/fixtures";
import { PortalStage } from "../fixtures/stage";
import type { Notification } from "../../../../src/lib/notificationTypes";
import { NeighborhoodPane } from "./NeighborhoodPane";
import { JobsPane, MemoryPane, MeshPane } from "./AppViews";
import { CursorTour } from "./CursorTour";
import { guideRoute, tourRoute, type GuideId } from "./tour";
import { emitTabUiMode } from "../../../../src/lib/chatUiMode";
import { NewSessionPicker } from "../../../../src/components/Terminal/NewSessionPicker";
import type { MultiplexerActionsContextType } from "../../../../src/store/multiplexer/context";
import { TeamPackageInput, type SampleTeamRole } from "./TeamPackageInput";
import { ProviderSwitchDialog, ProviderSwitchPreviewContext } from "../../../../src/components/Sidebar/ProviderSwitchDialog";
import { RemoteFolderPicker } from "../../../../src/components/shared/RemoteFolderPicker";
import { beatFrame } from "./beat";
import { activityFrame } from "./activity";
import { ChatPane } from "./ChatPane";
import { localTime, pauseClocks, resumeClocks, startClock, IDLE } from "./clocks";
import { activeTab, activeWorkspace, focusTab, goPlace, initialNav, openTask, openView, placeOf, selectMachine, selectWorkspace, type Nav, type PlaceId, type View, type World } from "./nav";
import { PlanPane } from "./PlanPane";
import { UsagePane } from "./UsagePane";
import { registerWorkspaceRoutes } from "./routes";
import { ACTIVITY, ACTIVITY_PERIOD, activePlan, BEATS, MACHINES, MEDIA_HOME, PEER_MACHINE, workAt, WORKING_LINE, type MachineId } from "./story";
import { WorkspaceSidebar } from "./WorkspaceSidebar";

export type WorkspaceLayout = "landscape" | "portrait";

export interface WorkspaceController {
  /** Whether the host paused before the workspace took the controller over. */
  paused: boolean;
  setPlace: (place: PlaceId) => void;
  setPaused: (paused: boolean) => void;
  setTour: (on: boolean) => void;
  guide: (action: GuideId) => void;
  pendingGuide: GuideId | null;
  guideFinished: (action: GuideId) => void;
  composer: () => HTMLElement | null;
}

export interface WorkspaceProps {
  place: PlaceId;
  layout: WorkspaceLayout;
  reducedMotion: boolean;
  /** The cursor tour; never on touch layouts or with reduced motion. */
  tour: boolean;
  controller: WorkspaceController;
  onPlace: (place: PlaceId) => void;
}

const WORLD: World<MachineId> = {
  machines: Object.values(MACHINES).map((m) => ({ id: m.id, home: m.home, workspaces: m.workspaces.map((ws) => ({ id: ws.id, tabs: ws.tabs })) })),
  local: "local",
  remote: "gpu-rig",
  teams: { machine: "local", workspace: MACHINES.local.workspaces[0].id, tab: MEDIA_HOME },
};

const ALL_TABS: Tab[] = Object.values(MACHINES).flatMap((m) => m.tabs);
const noop = () => undefined;
const ACTIVITY_CLOCK = "activity";
const ROUTE = tourRoute({ agent: MACHINES.local.tabs.find((t) => t.title === "Mira")!.id, home: MACHINES.local.home, card: "FN-203" });
const ACTIVITIES = new Map();
const DISPLAY = new Map();

function statusesAt(seconds: number): { tabs: Record<string, Tab>; statuses: Record<string, TabStatus> } {
  const tabs: Record<string, Tab> = {};
  const statuses: Record<string, TabStatus> = {};
  for (const tab of ALL_TABS) {
    const work = workAt(tab, seconds);
    const working = !!work || activityFrame(ACTIVITY, tab.id, seconds).working;
    tabs[tab.id] = working ? { ...tab, agent_state: { state: "working", status_line: work ?? WORKING_LINE[tab.id] ?? "Working…" } } : tab;
    statuses[tab.id] = working ? "Working" : "Idle";
  }
  return { tabs, statuses };
}

/** One explorable Domios workspace: the app's own shell, sidebar, chat, plan and plugins views over one scripted story. */
export function Workspace({ place, layout, reducedMotion, tour, controller, onPlace }: WorkspaceProps) {
  const [nav, setNav] = useState<Nav<MachineId>>(() => goPlace(WORLD, initialNav(WORLD), place));
  const [clocks, setClocks] = useState(() => {
    const at = performance.now();
    const running = startClock(IDLE, ACTIVITY_CLOCK, at);
    return controller.paused ? pauseClocks(running, at) : running;
  });
  const [now, setNow] = useState(() => performance.now());
  const [drawer, setDrawer] = useState(false);
  const [target, setTarget] = useState<{ taskId: string; serial: number } | null>(null);
  const [toasts, setToasts] = useState<Notification[]>([]);
  const [tourOn, setTourOn] = useState(tour);
  const [rootEl, setRootEl] = useState<HTMLDivElement | null>(null);
  const composer = useRef<HTMLDivElement | null>(null);
  const phone = layout === "portrait";
  const touch = navigator.maxTouchPoints > 0;
  const [guide, setGuide] = useState<{ action: GuideId; serial: number; tab: string } | null>(null);
  const [folderPicker, setFolderPicker] = useState(false);
  const [teamPicker, setTeamPicker] = useState(false);
  const [newWorkspaces, setNewWorkspaces] = useState<WorkspaceData[]>([]);
  const [newTeams, setNewTeams] = useState<Team[]>([]);
  const [providerOverrides, setProviderOverrides] = useState<Record<string, Partial<Tab>>>({});
  const [meshConnected, setMeshConnected] = useState(false);
  const [neighborInviteOpen, setNeighborInviteOpen] = useState(false);
  const [invitedNeighbor, setInvitedNeighbor] = useState<string | null>(null);
  const [directSwitch, setDirectSwitch] = useState<{ tabId: string; providerId: string } | null>(null);
  const [newTabs, setNewTabs] = useState<Tab[]>([]);
  const voiceSequence = useRef(0);
  const [voiceNotes, setVoiceNotes] = useState<{ id: string; tab: string; at: number }[]>([]);
  const allTabs = useMemo(() => [...ALL_TABS, ...newTabs], [newTabs]);
  const localWorkspaces = useMemo(() => [...MACHINES.local.workspaces, ...newWorkspaces].map((entry) => ({
    ...entry, tabs: [...entry.tabs, ...newTabs.filter((tab) => tab.workspace_id === entry.id).map((tab) => tab.id)],
  })), [newWorkspaces, newTabs]);
  const world = useMemo(() => ({
    ...WORLD,
    machines: WORLD.machines.map((entry) => entry.id === "local" ? { ...entry, workspaces: localWorkspaces } : entry),
  }), [localWorkspaces]);

  // A control the app has that the demo does not run answers plainly, in the app's own toast, instead of doing nothing.
  const notInDemo = useCallback((what: string) => {
    const id = `demo-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    setToasts((current) => [...current.slice(-2), {
      id, tab_id: null, workspace_id: null, title: "Not in the demo", body: `${what} works in the app. Install Domios to try it.`,
      severity: "Info", timestamp: Date.now(), read: false, ephemeral: true, lifetime_ms: 4000,
    }]);
  }, []);
  const placeNow = placeOf(world, nav);

  // A click inside the UI that lands in another place tells the host, so its chips follow; setPlace calls do not.
  const navigate = useCallback((next: (nav: Nav<MachineId>) => Nav<MachineId>) => {
    setNav((current) => {
      const moved = next(current);
      const before = placeOf(world, current);
      const after = placeOf(world, moved);
      if (after !== before) queueMicrotask(() => onPlace(after));
      return moved;
    });
    setDrawer(false);
  }, [onPlace, world]);

  useLayoutEffect(() => {
    registerWorkspaceRoutes(notInDemo, (tabId, providerId, model) => {
      const tab = allTabs.find((candidate) => candidate.id === tabId);
      const provider = DEMO_PROVIDERS.find((candidate) => candidate.id === providerId);
      if (!tab || !provider) throw new Error("This provider is unavailable in the sample workspace.");
      setProviderOverrides((previous) => ({
        ...previous,
        [tabId]: { agent_type: provider.id, agent_session_model: model ?? provider.default_model },
      }));
    }, meshConnected, allTabs);
  }, [notInDemo, meshConnected, allTabs]);

  const seconds = reducedMotion ? ACTIVITY_PERIOD - 0.25 : localTime(clocks, ACTIVITY_CLOCK, now);
  const secondsRef = useRef(seconds);
  secondsRef.current = seconds;
  const recordVoice = useCallback((tabId: string) => {
    const at = secondsRef.current;
    const id = `voice-${++voiceSequence.current}`;
    setVoiceNotes((previous) => {
      const last = previous[previous.length - 1];
      if (last?.tab === tabId && at - last.at < 2.4) return [...previous.slice(0, -1), { ...last, at: at - 2.4 }];
      return [...previous.slice(-3), { id, tab: tabId, at: reducedMotion ? at - 4 : at }];
    });
  }, [reducedMotion]);
  const openGuide = useCallback((action: GuideId) => {
    if (action === "memory" || action === "mesh" || action === "plugins") {
      navigate((current) => openView(current, action));
    } else if (action === "team") {
      setTeamPicker(true);
    } else if (action === "neighbor") {
      navigate((current) => openView(current, "neighbors"));
      setNeighborInviteOpen(true);
    } else if (action === "voice") {
      const tabId = activeTab(nav) || MACHINES.local.home;
      navigate((current) => focusTab(world, current, tabId));
      queueMicrotask(() => emitTabUiMode(tabId, "chat"));
      recordVoice(tabId);
    } else {
      const tabId = activeTab(nav) || MACHINES.local.home;
      const provider = DEMO_PROVIDERS.find((entry) => entry.id !== (providerOverrides[tabId]?.agent_type ?? allTabs.find((tab) => tab.id === tabId)?.agent_type));
      navigate((current) => focusTab(world, current, tabId));
      if (provider) setDirectSwitch({ tabId, providerId: provider.id });
    }
  }, [navigate, nav, providerOverrides, allTabs, world, recordVoice]);

  const runGuide = useCallback((action: GuideId) => {
    if (guide?.action === action) return;
    const scope = rootEl?.getRootNode();
    if (scope instanceof ShadowRoot) {
      scope.querySelector<HTMLElement>('[role="dialog"]')?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, composed: true }),
      );
    }
    setTeamPicker(false);
    setFolderPicker(false);
    setNeighborInviteOpen(false);
    setDirectSwitch(null);
    setTourOn(false);
    if (phone || touch || reducedMotion) {
      openGuide(action);
      controller.guideFinished(action);
      return;
    }
    if (action === "team") {
      navigate((current) => selectMachine(current, "local"));
    }
    const tabId = activeTab(nav) || MACHINES.local.home;
    if (action === "voice" || action === "provider") navigate((current) => focusTab(world, current, tabId));
    setGuide((previous) => ({ action, serial: (previous?.serial ?? 0) + 1, tab: tabId }));
  }, [controller, guide, nav, navigate, openGuide, phone, rootEl, touch, reducedMotion, world]);
  const guideSteps = useMemo(() => guide ? guideRoute(guide.action, guide.tab, MACHINES.local.home) : ROUTE, [guide]);
  const finishGuide = useCallback((completed: boolean) => {
    if (guide) {
      if (!completed) openGuide(guide.action);
      controller.guideFinished(guide.action);
    }
    setGuide(null);
  }, [controller, guide, openGuide]);
  const finishRef = useRef(finishGuide);
  finishRef.current = finishGuide;
  const onTourFinish = useCallback((completed: boolean) => finishRef.current(completed), []);

  useEffect(() => {
    controller.setPlace = (next) => navigate((current) => goPlace(world, current, next));
    controller.guide = runGuide;
    if (controller.pendingGuide) {
      const action = controller.pendingGuide;
      controller.pendingGuide = null;
      runGuide(action);
    }
    controller.setPaused = (value) => {
      controller.paused = value;
      if (value && guide) {
        controller.guideFinished(guide.action);
        setGuide(null);
      }
      const at = performance.now();
      setClocks((current) => (value ? pauseClocks(current, at) : resumeClocks(current, at)));
      setNow(at);
    };
    controller.setTour = setTourOn;
    controller.composer = () => composer.current;
  }, [controller, guide, navigate, runGuide, world]);

  useEffect(() => {
    if (guide && (phone || reducedMotion)) finishGuide(true);
  }, [guide, phone, reducedMotion, finishGuide]);

  useEffect(() => {
    if (reducedMotion || clocks.pausedAt !== null) return;
    const timer = window.setInterval(() => setNow(performance.now()), 85);
    return () => window.clearInterval(timer);
  }, [reducedMotion, clocks.pausedAt]);

  // The picker shows the host it last switched to; a chip or a report that moves machines walks it through the same
  // ready handshake, with the id of its latest switch, so its label and check mark follow.
  const pickerMachine = useRef<MachineId>("local");
  const lastSwitch = useRef(0);
  useEffect(() => {
    if (pickerMachine.current === nav.machine) return;
    pickerMachine.current = nav.machine;
    const peer = Object.entries(PEER_MACHINE).find(([, machine]) => machine === nav.machine);
    if (peer) setRelayTarget(peer[0], MACHINES[nav.machine].name);
    else clearRelayTarget();
    window.dispatchEvent(new CustomEvent("codeterm-peer-switch-ready", { detail: { switchId: lastSwitch.current, success: true } }));
    clearRelayTarget();
  }, [nav.machine]);

  // The real WebPeerPicker switches hosts through the relay target and waits for the app to rebind; the demo
  // rebinds to that machine's fixture tabs, answers ready, and keeps its own requests local.
  useEffect(() => {
    const rebind = (event: Event) => {
      const { switchId } = (event as CustomEvent<{ switchId: number }>).detail;
      const peer = relayTarget();
      const machine: MachineId = peer ? PEER_MACHINE[peer.id] ?? "local" : "local";
      lastSwitch.current = switchId;
      pickerMachine.current = machine;
      navigate((current) => selectMachine(current, machine));
      window.dispatchEvent(new CustomEvent("codeterm-peer-switch-ready", { detail: { switchId, success: true } }));
      clearRelayTarget();
    };
    window.addEventListener("codeterm-peer-switch-rebind", rebind);
    return () => window.removeEventListener("codeterm-peer-switch-rebind", rebind);
  }, [navigate]);

  // A plugin bubble (Git's branch pill) opens that plugin's live panel, which needs a real repository.
  useEffect(() => {
    const open = (event: Event) => {
      const panel = (event as CustomEvent<{ panel?: string }>).detail?.panel ?? "plugin";
      notInDemo(`The ${panel === "git" ? "Git" : panel} panel`);
    };
    window.addEventListener("codeterm:open-panel", open);
    return () => window.removeEventListener("codeterm:open-panel", open);
  }, [notInDemo]);

  // The sidebar's team plan rows open the workspace plan through the app's canvas store; here that is the board.
  useEffect(() => canvasFocusStore.subscribe(() => {
    const opened = canvasFocusStore.getWorkspaceTarget();
    if (!canvasFocusStore.isWorkspaceOpen() || !opened) return;
    canvasFocusStore.closeWorkspace();
    const taskId = opened.jump?.view === "task" ? opened.jump.taskId : null;
    navigate((current) => openTask(current, taskId));
    if (taskId) setTarget((prev) => ({ taskId, serial: (prev?.serial ?? 0) + 1 }));
  }), [navigate]);

  const showTask = useCallback((taskId: string | null) => {
    navigate((current) => openTask(current, taskId));
    if (taskId) setTarget((prev) => ({ taskId, serial: (prev?.serial ?? 0) + 1 }));
  }, [navigate]);

  const showTab = useCallback((ref: string) => {
    const tab = allTabs.find((candidate) => candidate.id === ref || candidate.id.startsWith(ref) || candidate.title === ref);
    if (tab) navigate((current) => focusTab(world, current, tab.id));
  }, [navigate, world, allTabs]);

  const onView = useCallback((view: View) => navigate((current) => openView(current, view)), [navigate]);

  const statusTime = Math.floor(seconds * 2) / 2;
  const { tabs: liveTabs, statuses } = useMemo(() => {
    const activity = statusesAt(statusTime);
    for (const tab of newTabs) {
      activity.tabs[tab.id] = tab;
      activity.statuses[tab.id] = "Idle";
    }
    for (const [id, override] of Object.entries(providerOverrides)) activity.tabs[id] = { ...activity.tabs[id], ...override };
    return activity;
  }, [statusTime, providerOverrides, newTabs]);
  const boardTime = Math.floor(seconds);
  const plan = useMemo(() => activePlan(boardTime, target?.taskId), [boardTime, target?.taskId]);
  const baseMachine = MACHINES[nav.machine];
  const machine = nav.machine === "local" ? {
    ...baseMachine,
    workspaces: localWorkspaces,
    tabs: [...baseMachine.tabs, ...newTabs],
    teams: [...baseMachine.teams, ...newTeams],
  } : baseMachine;
  const machineTabs = machine.tabs.map((tab) => liveTabs[tab.id]);
  const tabId = activeTab(nav);
  const tab = liveTabs[tabId] ?? liveTabs[machine.home];
  const createWorkspace = (path: string) => {
    const name = path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "Workspace";
    const existing = [...MACHINES.local.workspaces, ...newWorkspaces].find((entry) => entry.cwd === path);
    const id = existing?.id ?? `sample-workspace-${newWorkspaces.length + 1}`;
    if (!existing) setNewWorkspaces([...newWorkspaces, { id, name, tabs: [], active_tab_id: null, cwd: path }]);
    setFolderPicker(false);
    navigate((current) => openView(selectWorkspace(selectMachine(current, "local"), id), "chat"));
  };
  const activity = activityFrame(ACTIVITY, tab.id, seconds);
  const tabNotes = voiceNotes.filter((note) => note.tab === tab.id);
  const lastVoice = tabNotes[tabNotes.length - 1];
  const voiceSeconds = lastVoice ? seconds - lastVoice.at : Infinity;
  const voice = voiceSeconds < 2.4 ? "recording" : voiceSeconds < 3.5 ? "transcribing" : null;
  const deliveredVoice = tabNotes.filter((note) => seconds - note.at >= 3.5);
  const voiceKey = deliveredVoice.map((note) => note.id).join(",");
  const voiceMessages: NormalizedChatMessage[] = deliveredVoice.map((note) => ({
    id: note.id, type: "user", content: "Check the sync repro on the laptop and let me know what you find.",
  }));
  const frame = useMemo(() => ({
    ...activity,
    messages: [...(BEATS[tab.id] ? beatFrame(BEATS[tab.id], Infinity).messages : []), ...activity.messages, ...voiceMessages].slice(-40),
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the frame key encodes the rendered content
  }), [tabId, activity.key, voiceKey]);

  const createTeam = (name: string, roles: SampleTeamRole[], providerIds = ["claude", "codex"], models: (string | undefined)[] = []) => {
    const workspaceId = nav.workspace.local;
    const count = newTeams.length + 1;
    const id = `sample-team-${count}`;
    const lead = `${id}-orchestrator`;
    const cwd = localWorkspaces.find((entry) => entry.id === workspaceId)?.cwd ?? null;
    const tabs = roles.filter((role) => role.kind === "manager").map((role, index): Tab => {
      const agent = DEMO_PROVIDERS.find((entry) => entry.id === providerIds[index % providerIds.length]) ?? DEMO_PROVIDERS[0];
      const tabId = `${id}-${role.name}`;
      return {
        ...MACHINES.local.tabs[0], title: role.title, id: tabId, workspace_id: workspaceId, team_id: id, cwd,
        pty_id: `pty-${tabId}`, agent_session_id: `session-${tabId}`, agent_type: agent.id,
        agent_session_model: models[index] ?? agent.default_model, role_profile: role.name,
        parent_tab_id: tabId === lead ? null : lead, can_spawn: tabId === lead,
      };
    });
    setNewTeams((previous) => [...previous, { id, name, lifecycle: "active", created_at: Date.now(), workspace_id: workspaceId }]);
    setNewTabs((previous) => [...previous, ...tabs]);
    setTeamPicker(false);
    navigate((current) => {
      const selected = selectMachine(current, "local");
      return { ...selected, view: "chat", tab: { ...selected.tab, [workspaceId]: lead } };
    });
    return { plan_id: `sample-plan-${count}`, orchestrator_tab_id: lead, planner_tab_id: tabs.find((tab) => tab.role_profile === "planner")?.id ?? "" };
  };
  const sessionActions: Partial<MultiplexerActionsContextType> = {
    spawnAgentTab: async (workspaceId, providerId, cwd, model) => {
      const provider = DEMO_PROVIDERS.find((entry) => entry.id === providerId) ?? DEMO_PROVIDERS[0];
      const id = `sample-session-${newTabs.length + 1}`;
      setNewTabs((previous) => [...previous, {
        ...MACHINES.local.tabs[0], id, title: provider.display_name, workspace_id: workspaceId, team_id: null,
        pty_id: `pty-${id}`, agent_session_id: `session-${id}`, agent_type: provider.id, agent_session_model: model ?? provider.default_model,
        role: null, role_profile: null, parent_tab_id: null, can_spawn: false, cwd: cwd ?? null,
      }]);
      setTeamPicker(false);
      navigate((current) => {
        const selected = selectWorkspace(selectMachine(current, "local"), workspaceId);
        return { ...selected, view: "chat", tab: { ...selected.tab, [workspaceId]: id } };
      });
      return { workspaceId, tabId: id };
    },
    createOrchestrationPair: async (options) => createTeam("Release review", [
      { name: "orchestrator", title: "Piper", kind: "manager" },
      { name: "planner", title: "Alder", kind: "manager" },
    ], [options.orchestratorAgent, options.plannerAgent], [options.orchestratorModel, options.plannerModel]),
  };
  const main = (() => {
    switch (nav.view) {
      case "usage":
        return <UsagePane />;
      case "plan":
        return <PlanPane plan={plan} mode="tasks" target={target}
          tabs={ALL_TABS.map((member) => liveTabs[member.id])} teams={MACHINES.local.teams} workspaces={MACHINES.local.workspaces}
          workspaceId={MACHINES.local.workspaces[0].id} onOpenTask={showTask} onOpenTab={showTab} />;
      case "plugins":
        return <div className="h-full w-full overflow-hidden" style={{ background: "var(--ct-bg)" }}>
          <ExtensionsView visible pluginActivities={ACTIVITIES} pluginActivityDisplayMap={DISPLAY} />
        </div>;
      case "jobs":
        return <JobsPane />;
      case "memory":
        return <MemoryPane notInDemo={notInDemo} paused={clocks.pausedAt !== null} reducedMotion={reducedMotion} />;
      case "neighbors":
        return <NeighborhoodPane adding={neighborInviteOpen} invited={invitedNeighbor}
          onAdd={() => setNeighborInviteOpen(true)} onCloseAdd={() => setNeighborInviteOpen(false)}
          onInvite={(email) => { setInvitedNeighbor(email); setNeighborInviteOpen(false); }} />;
      case "mesh":
        return <MeshPane notInDemo={notInDemo} onClose={() => onView("chat")} connected={meshConnected} onConnect={() => setMeshConnected(true)} />;
      default:
        if (!tabId) return <div className="grid h-full place-items-center text-sm text-[var(--ct-muted-fg)]">Workspace ready. Select a workspace in the sidebar to explore its agents.</div>;
        return <ChatPane tab={tab} status={statuses[tab.id]} frame={frame} workspaceId={activeWorkspace(nav)} composerRef={composer}
          onFocusTab={showTab} onOpenTask={showTask} onOpenJobs={() => onView("jobs")} notInDemo={notInDemo}
          voice={voice} voiceSeconds={voiceSeconds} onVoice={() => recordVoice(tab.id)}
          onCancelVoice={() => setVoiceNotes((previous) => previous.filter((note) => note.id !== lastVoice?.id))} />;
    }
  })();

  return (
    <ProductProviders tabs={ALL_TABS} workspaceId={activeWorkspace(nav)}>
    <ProviderSwitchPreviewContext value>
    <PortalStage>
      <div ref={setRootEl} className="relative flex h-full w-full flex-col overflow-hidden" data-place={placeNow} data-layout={layout}
        style={{ background: "var(--ct-bg)", color: "var(--ct-fg)", "--ct-composer-bottom-padding": "calc(0.5rem + var(--ct-safe-bottom, 0px))" } as CSSProperties}>
        <UpdateProvider><UpdatePanelProvider>
          <Titlebar api={API} theme={DEFAULT_THEME} showBuildIdentity={false} showSidebarToggle={phone} onToggleSidebar={() => setDrawer((open) => !open)}
            onOpenSettings={() => notInDemo("Settings")} onOpenExtensions={() => onView("plugins")}
            toasts={toasts} onDismissToast={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} onMarkNotificationRead={noop} onNavigateToast={noop} />
        </UpdatePanelProvider></UpdateProvider>
        <div className="flex flex-1 min-h-0" data-testid="app-shell-body" data-shell-viewport={phone ? "phone" : "desktop"}>
          <div className="flex flex-1 min-h-0 relative">
            {phone && drawer && <div className="absolute inset-0 z-30 bg-black/40" aria-hidden="true" onClick={() => setDrawer(false)} />}
            <WorkspaceSidebar machine={machine} workspaceId={activeWorkspace(nav)} activeTabId={tabId} tabs={machineTabs} statuses={statuses} plan={plan}
              view={nav.view} phone={phone} shown={!phone || drawer}
              onWorkspace={(id) => navigate((current) => openView(selectWorkspace(current, id), "chat"))}
              onCreateWorkspace={() => setFolderPicker(true)} onCreateTeam={() => setTeamPicker(true)}
              onTab={showTab} onView={onView} notInDemo={notInDemo} />
            <div className="flex-1 min-w-0 min-h-0 flex flex-col">
              <main className="flex-1 min-w-0 min-h-0 relative overflow-hidden">
                <div className="absolute inset-0">{main}</div>
              </main>
            </div>
          </div>
        </div>
        {folderPicker && <RemoteFolderPicker title="Workspace folder" initialPath="C:/Developer/notes-lab"
          onCancel={() => setFolderPicker(false)} onSelect={createWorkspace} />}
        {teamPicker && <ProductProviders tabs={machineTabs} workspaceId={nav.workspace.local} overrides={sessionActions}>
          <NewSessionPicker workspaceId={nav.workspace.local} workspaceCwd={localWorkspaces.find((entry) => entry.id === nav.workspace.local)?.cwd}
            providerCatalogue={DEMO_PROVIDERS} onClose={() => setTeamPicker(false)}
            additionalOptions={<TeamPackageInput onLoad={(team) => createTeam(team.title, team.roles)} />} />
        </ProductProviders>}
        {directSwitch && liveTabs[directSwitch.tabId] && <ProviderSwitchDialog tab={liveTabs[directSwitch.tabId]}
          initialProviderId={directSwitch.providerId} onClose={() => setDirectSwitch(null)} />}
        <CursorTour host={rootEl} active={!phone && !touch && !reducedMotion && clocks.pausedAt === null && (tourOn || !!guide)}
          steps={guideSteps} guided={!!guide} onFinish={onTourFinish} />
      </div>
    </PortalStage>
    </ProviderSwitchPreviewContext>
    </ProductProviders>
  );
}
