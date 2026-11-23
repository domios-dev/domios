import type { PlanView as Plan } from "../../components/Canvas/planProjection";
import { AgentIcon } from "../../components/AgentIcon";
import { SurfaceIcon } from "../../components/Surface/surfaceIcon";
import { surfaceBacking } from "../surfaceTab";
import { useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { ChatBlockHostProvider, decodeTasksBlockKey, type ChatBlockHost, type ChatMentionValue } from "@codeterm/chat-ui";
import { hostAutomationAdapter, AUTOMATION_GRAPH_EVENT } from "../../components/Automation/hostAdapter";
import { ALL_JOBS_SCOPE } from "../../components/Automation/types";
import { useSidebarPlansOptional } from "../../components/Canvas/PlansProvider";
import { usePlanTargetNavigation, type PlanTarget } from "../../components/Canvas/teamViewNavigation";
import { requestOpenJob } from "../../components/Jobs/openJobEvent";
import { fetchStats } from "../../components/MemoryPanel/memoryApi";
import { useMultiplexer, usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { useMuxStateStore } from "../../store/multiplexer/selectorStore";
import { getAgentDescriptor } from "../agentDescriptorCache";
import { invoke, listen } from "../bridge";
import { openNewSessionPicker } from "../newSessionChoice";
import type { PushChannelDescriptor } from "../notifyDestinations";
import { ensurePluginSnapshot, getPluginSnapshotState, subscribePluginSnapshotStore } from "../pluginSnapshotStore";
import type { ApiConfig, CronJob } from "../types";
import { createAgentBlockHost } from "./agentBlockHost";
import { createChecklistBlockHost } from "./checklistBlockHost";
import { createJobBlockHost } from "./jobBlockHost";
import { createJobsBlockStore } from "./jobsBlockStore";
import { createMentionHost } from "./mentionHost";
import { createTasksBlockHost, type PlansSnapshot } from "./tasksBlockHost";
import { fetchPlans, fetchPlanTaskReference } from "../../components/Canvas/plansApi";
import type { PlanRead } from "../../components/Canvas/planProjection";
import { planRevision } from "../../components/Canvas/planProjection";
import { webRequestContext } from "../webApi";
import { mentionScopeForTab } from "../mentionSources/scope";
import { peerMentionTabs, type MentionPeerInventory, type PeerMentionTab } from "../mentionSources/peerTabs";
import { parseCompositeTabId } from "../remoteTabTransport";
import { watchMentionState } from "../mentionSources/live";
import { resolveTabRef } from "./agentBlockState";
import { createScopedPlanBlockRead } from "./scopedPlanBlockRead";
import { watchPlanBlockChanges } from "./watchPlanBlockChanges";
import { readTasksBlockPlan } from "./readTasksBlockPlan";
import { createValueFeed } from "./valueFeed";
import { recordProductEvent } from "../productAnalytics";

const JOB_EVENTS = ["cron-run-finished", "cron-fired", AUTOMATION_GRAPH_EVENT];

function openPanel(panel: string) {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel } }));
}

function watchJobEvents(changed: () => void): () => void {
  let active = true;
  const stops: Array<() => void> = [];
  for (const event of JOB_EVENTS) {
    void listen(event, changed).then((stop) => (active ? stops.push(stop) : stop()), () => {});
  }
  window.addEventListener("focus", changed);
  return () => {
    active = false;
    window.removeEventListener("focus", changed);
    for (const stop of stops.splice(0)) stop();
  };
}

/** Agent, job, checklist and task card services over the app's own stores; mounted inside the plans provider. */
export function AppChatBlockServices({ api, children }: { api: ApiConfig; children: ReactNode }) {
  const mux = useMuxStateStore();
  const { selectTabByTab, setActiveTab } = useMultiplexer();
  const peerInventory = usePairedWorkspaceInventory();
  const { plans, loading } = useSidebarPlansOptional();
  const navigate = usePlanTargetNavigation();
  const live = useRef({ navigate, api, selectTabByTab, setActiveTab });
  live.current = { navigate, api, selectTabByTab, setActiveTab };

  const plansFeed = useMemo(() => createValueFeed<PlansSnapshot>({ plans: [], loaded: false }), []);
  const plainPlans = useMemo(() => createValueFeed<PlansSnapshot["plans"]>([]), []);
  const peerTabs = useMemo(() => createValueFeed<MentionPeerInventory>({ generals: [] }), []);
  useLayoutEffect(() => {
    plansFeed.set({ plans, loaded: !loading });
    plainPlans.set(plans);
  }, [plansFeed, plainPlans, plans, loading]);
  useLayoutEffect(() => { peerTabs.set(peerInventory); }, [peerTabs, peerInventory]);

  const host = useMemo<ChatBlockHost>(() => {
    const muxSource = { get: mux.getState, getState: mux.getState, subscribe: mux.subscribe };
    let indexedInventory: MentionPeerInventory | undefined;
    const peerIndex = new Map<string, Map<string, PeerMentionTab>>();
    const readPeerTab = (id: string) => {
      const route = parseCompositeTabId(id);
      if (!route) return undefined;
      const inventory = peerTabs.get();
      if (indexedInventory !== inventory) {
        indexedInventory = inventory;
        peerIndex.clear();
      }
      let tabs = peerIndex.get(route.peerId);
      if (!tabs) {
        tabs = peerMentionTabs(inventory, route.peerId);
        peerIndex.set(route.peerId, tabs);
      }
      return tabs.get(id);
    };
    const activeWorkspace = () => {
      const state = mux.getState();
      return state.workspaces.find((workspace) => workspace.id === state.active_workspace_id) ?? state.workspaces[0] ?? null;
    };
    const jobsStore = createJobsBlockStore({
      listCrons: () => invoke<CronJob[]>("cron_list", {}),
      readGraph: (workspaceId) => hostAutomationAdapter.read({ connection_id: "chat-blocks", workspace_id: workspaceId, team_id: ALL_JOBS_SCOPE }, new AbortController().signal),
      workspaceIds: () => mux.getState().workspaces.map((workspace) => workspace.id),
      watch: watchJobEvents,
    });
    const epoch=()=>JSON.stringify([webRequestContext(),live.current.api.enabled,live.current.api.host,live.current.api.port,live.current.api.token]);
    const watch=(changed:Parameters<typeof watchPlanBlockChanges>[0])=>{
      const offPlans=watchPlanBlockChanges(changed);
      const offScope=watchMentionState(mux,()=>changed());
      return {ready:offPlans.ready,stop:()=>{offPlans.stop();offScope();}};
    };
    const tasks=createScopedPlanBlockRead<PlanRead>({
      epoch,watch,
      read:(key,signal)=>{const spec=decodeTasksBlockKey(key);if(!spec)throw new Error("Task card is invalid");return readTasksBlockPlan(live.current.api,spec,signal);},
      targets:(key,_value,change)=>decodeTasksBlockKey(key)?.planId===change.plan_id,
      revision:planRevision,
    });
    const agentPlans=createScopedPlanBlockRead<readonly Plan[]>({
      epoch,watch,
      read:async(ref,signal)=>{
        const tab=resolveTabRef(mux.getState().tabs,ref);
        return tab ? fetchPlans(live.current.api,signal,{chat:true,tabId:tab.id}) : [];
      },
    });
    const references=createScopedPlanBlockRead<ChatMentionValue|null>({
      epoch,watch,
      read:async(key,signal)=>{
        const [tabId,,reference]=JSON.parse(key) as [string,string,string];
        const scope=mentionScopeForTab(mux.getState(),tabId);
        const found=await fetchPlanTaskReference(live.current.api,reference,scope.projectPath,scope.teamId,signal);
        return found ? {title:`${found.key} ${found.name}`,kind:"task",id:reference,planId:found.plan_id,targetId:found.task_id,teamId:found.team_id,scopeKey:JSON.stringify([scope.projectPath,scope.teamId])} : null;
      },
    });
    return {
      interacted: (kind, action) => {
        if (kind !== "appearance") void recordProductEvent({ event_type: "ds_block_interacted", event_properties: { kind, action } });
      },
      mentions: createMentionHost({
        mux, plans: plainPlans, taskReferences:references, readPeerTab, watchPeerTabs: peerTabs.subscribe,
        providerName: (id) => getAgentDescriptor(id)?.display_name,
        renderAvatar: (value) => value.tabKind === "agent" ? <AgentIcon providerId={value.providerId} tab={mux.getState().tabs[value.id]} style={{ width: 12, height: 12 }} />
          : value.tabKind === "browser" && value.state === "closed" ? undefined
          : value.tabKind === "browser" || value.tabKind === "surface" ? <SurfaceIcon backing={surfaceBacking(mux.getState().tabs[value.id])} size={11} /> : undefined,
        readGraph: async (scope, signal) => {
          const graphs = await Promise.all((scope.teamId ? [scope.teamId, null] : [null]).map((teamId) => hostAutomationAdapter.read({ connection_id: "composer", workspace_id: scope.workspaceId!, team_id: teamId }, signal)));
          return { ...graphs[0], nodes: graphs.flatMap((graph) => graph.nodes) };
        },
        watchGraph: async (scope, changed) => {
          const stops = await Promise.all((scope.teamId ? [scope.teamId, null] : [null]).map((teamId) => hostAutomationAdapter.subscribe({ connection_id: "composer", workspace_id: scope.workspaceId!, team_id: teamId }, changed)));
          return () => stops.forEach((stop) => stop());
        },
        openPlan: (target) => live.current.navigate(target),
        openTab: (workspaceId, tabId) => {
          if (parseCompositeTabId(tabId)) live.current.setActiveTab(workspaceId, tabId);
          else void live.current.selectTabByTab(workspaceId, tabId);
        },
        openBlueprint: (workspaceId, teamId, blueprintId) => requestOpenJob({ workspaceId, target: { teamId, blueprintId } }),
      }),
      agents: createAgentBlockHost({
        mux: muxSource,
        plans: plainPlans, scopedPlans:agentPlans,
        providerName: (agentType) => getAgentDescriptor(agentType)?.display_name ?? null,
        selectTab: (workspaceId, tabId) => live.current.selectTabByTab(workspaceId, tabId),
        interrupt: (args) => invoke("interrupt_agent", args),
      }),
      jobs: createJobBlockHost(jobsStore, {
        triggerCron: (id) => invoke("cron_trigger", { id }),
        toggleCron: (id, enabled) => invoke("cron_toggle", { id, enabled }),
        cancelCron: (id) => invoke("cron_cancel", { id }),
        stopBlueprint: (attachment, identity) => hostAutomationAdapter.stop(attachment, identity),
        openJob: requestOpenJob,
      }),
      checklists: createChecklistBlockHost({
        mux: muxSource,
        plugins: { get: getPluginSnapshotState, subscribe: subscribePluginSnapshotStore, ensure: () => ensurePluginSnapshot({ background: true }) },
        jobs: jobsStore,
        readChannels: () => invoke<{ channels?: PushChannelDescriptor[] }>("push_channel_roster").then((roster) => roster?.channels ?? []),
        readMemoryKinds: () => fetchStats(live.current.api).then((stats) => stats.by_kind ?? {}),
        links: {
          "new-agent": () => {
            const workspace = activeWorkspace();
            if (workspace) openNewSessionPicker({ workspaceId: workspace.id, workspaceCwd: workspace.cwd });
          },
          plugins: () => openPanel("extensions"),
          jobs: () => {
            const workspace = activeWorkspace();
            if (workspace) requestOpenJob({ workspaceId: workspace.id, target: { teamId: null } });
          },
          memory: () => openPanel("memory"),
        },
      }),
      tasks: createTasksBlockHost(plansFeed, (target: PlanTarget) => live.current.navigate(target),tasks),
    };
  }, [mux, peerTabs, plainPlans, plansFeed]);

  return <ChatBlockHostProvider value={host}>{children}</ChatBlockHostProvider>;
}
