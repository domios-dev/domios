import { useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { ChatBlockHostProvider, type ChatBlockHost } from "@codeterm/chat-ui";
import { hostAutomationAdapter, AUTOMATION_GRAPH_EVENT } from "../../components/Automation/hostAdapter";
import { ALL_JOBS_SCOPE } from "../../components/Automation/types";
import { useSidebarPlansOptional } from "../../components/Canvas/PlansProvider";
import { usePlanTargetNavigation, type PlanTarget } from "../../components/Canvas/teamViewNavigation";
import { requestOpenJob } from "../../components/Jobs/openJobEvent";
import { fetchStats } from "../../components/MemoryPanel/memoryApi";
import { useMultiplexer } from "../../store/multiplexer/context";
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
import { createTasksBlockHost, type PlansSnapshot } from "./tasksBlockHost";
import { createValueFeed } from "./valueFeed";

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
  const { selectTabByTab } = useMultiplexer();
  const { plans, loading } = useSidebarPlansOptional();
  const navigate = usePlanTargetNavigation();
  const live = useRef({ navigate, api, selectTabByTab });
  live.current = { navigate, api, selectTabByTab };

  const plansFeed = useMemo(() => createValueFeed<PlansSnapshot>({ plans: [], loaded: false }), []);
  const plainPlans = useMemo(() => createValueFeed<PlansSnapshot["plans"]>([]), []);
  useLayoutEffect(() => {
    plansFeed.set({ plans, loaded: !loading });
    plainPlans.set(plans);
  }, [plansFeed, plainPlans, plans, loading]);

  const host = useMemo<ChatBlockHost>(() => {
    const muxSource = { get: mux.getState, getState: mux.getState, subscribe: mux.subscribe };
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
    return {
      agents: createAgentBlockHost({
        mux: muxSource,
        plans: plainPlans,
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
      tasks: createTasksBlockHost(plansFeed, (target: PlanTarget) => live.current.navigate(target)),
    };
  }, [mux, plainPlans, plansFeed]);

  return <ChatBlockHostProvider value={host}>{children}</ChatBlockHostProvider>;
}
