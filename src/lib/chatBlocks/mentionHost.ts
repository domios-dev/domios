import type { PlanView as Plan } from "../../components/Canvas/planProjection";
import type { ChatMentionHost, ChatMentionValue } from "../../../packages/chat-ui/src/blocks/ChatBlockHost";
import type { CanonicalMentionKind } from "../../../packages/chat-ui/src/codeterm/canonicalMentions";
import type { AutomationGraph } from "../../components/Automation/types";
import type { PlanTarget } from "../../components/Canvas/teamViewNavigation";
import type { MuxStateStore } from "../../store/multiplexer/selectorStore";
import { mentionScopeForTab, planInMentionScope } from "../mentionSources/scope";
import type { PeerMentionTab } from "../mentionSources/peerTabs";
import { blueprintInMentionScope } from "../mentionSources/blueprints";
import type { MentionScope } from "../composerMentions";
import type { ScopedPlanBlockRead } from "./scopedPlanBlockRead";
import type { ValueFeed } from "./valueFeed";
import { watchMentionState } from "../mentionSources/live";
import { tabActivity } from "../tabActivity";
import { surfaceBacking } from "../surfaceTab";
import { isBrowserSurface } from "../../components/Surface/surfaceIcon";
import { parseCompositeTabId } from "../remoteTabTransport";
import { cleanMachineName } from "../machineIdentity";
import type { Tab } from "../types";

function resolveLocalTabRef(tabs: Readonly<Record<string, Tab>>, ref: string): Tab | null {
  if (tabs[ref]) return tabs[ref];
  const matches = Object.values(tabs).filter(tab => !tab.id.startsWith("mesh:") && !tab.remote_peer_id && tab.id.startsWith(ref));
  return matches.length === 1 ? matches[0] : null;
}

function resolveAttachedRemoteTabRef(tabs: Readonly<Record<string, Tab>>, targetId: string): Tab | null {
  if (tabs[targetId]) return tabs[targetId];
  const route = parseCompositeTabId(targetId);
  if (!route) return null;
  const loweredPeer = route.peerId.toLowerCase();
  const peerMatches = Object.values(tabs).filter(tab => {
    if (!tab.remote_peer_id || !tab.remote_tab_id) return false;
    if (tab.remote_peer_id === route.peerId) return true;
    const display = cleanMachineName(tab.remote_peer_name, tab.remote_peer_id) ?? tab.remote_peer_name ?? "";
    return (tab.remote_peer_name?.toLowerCase() === loweredPeer || display.toLowerCase() === loweredPeer);
  });
  const exact = peerMatches.filter(tab => tab.remote_tab_id === route.remoteTabId);
  if (exact.length === 1) return exact[0];
  const prefixed = peerMatches.filter(tab => tab.remote_tab_id?.startsWith(route.remoteTabId));
  return prefixed.length === 1 ? prefixed[0] : null;
}

export function createMentionHost(ports: {
  mux: Pick<MuxStateStore, "getState" | "subscribe">;
  plans: ValueFeed<readonly Plan[]>;
  readPeerTab?(id: string): PeerMentionTab | undefined;
  watchPeerTabs?(changed: () => void): () => void;
  taskReferences?: ScopedPlanBlockRead<ChatMentionValue | null>;
  readGraph(scope: MentionScope, signal: AbortSignal): Promise<AutomationGraph>;
  watchGraph?(scope: MentionScope, changed: () => void): Promise<() => void>;
  openPlan(target: PlanTarget): void;
  openTab(workspaceId: string, tabId: string): void;
  openBlueprint(workspaceId: string, teamId: string | null, id: string): void;
  providerName?(id: string | null | undefined): string | undefined;
  renderAvatar?: ChatMentionHost["renderAvatar"];
}): ChatMentionHost {
  const graphs = new Map<string, AutomationGraph>();
  const pending = new Map<string, Promise<void>>();
  const values = new Map<string, ChatMentionValue | null>();
  const knownTabs = new Map<string, ChatMentionValue>();
  const listeners = new Set<() => void>();
  const graphWatches = new Map<string, Promise<() => void>>();
  let stopSources: (() => void) | null = null;
  let planSnapshot = ports.plans.get();
  const indexes = new Map<string, Map<string, ChatMentionValue | null>>();
  const notify = () => { for (const changed of listeners) changed(); };
  const graphKey = (scope: MentionScope) => JSON.stringify([scope.workspaceId, scope.teamId]);
  const read = (key: string): ChatMentionValue | null => {
    let args: [string, CanonicalMentionKind, string];
    try { args = JSON.parse(key); } catch { return null; }
    const [tabId, kind, id] = args;
    const state = ports.mux.getState();
    const scope = mentionScopeForTab(state, tabId);
    let value: ChatMentionValue | null = null;
    if ((scope.workspaceId || scope.peerId) && kind === "tab") {
      const targetId = scope.peerId && !id.startsWith("mesh:") ? `mesh:${scope.peerId}/${id}` : id;
      const isRemote = targetId.startsWith("mesh:");
      const peerTab = isRemote ? ports.readPeerTab?.(targetId) : undefined;
      const tab = peerTab?.tab ?? (isRemote ? resolveAttachedRemoteTabRef(state.tabs, targetId) : resolveLocalTabRef(state.tabs, targetId));
      if (tab) {
        const resolvedId = tab.id;
        const activity = tabActivity(tab, peerTab?.status ?? state.tabStatuses[resolvedId]);
        const surface = surfaceBacking(tab);
        const peerName = peerTab?.peerName ?? (tab.remote_peer_id ? cleanMachineName(tab.remote_peer_name, tab.remote_peer_id) ?? tab.remote_peer_name ?? undefined : undefined);
        value = { title: tab.title, kind, id: resolvedId, tabKind: surface ? isBrowserSurface(surface) ? "browser" : "surface" : tab.agent_type ? "agent" : "shell",
          providerId: tab.agent_type, providerName: ports.providerName?.(tab.agent_type), role: tab.role ?? undefined,
          peerName,
          workspaceName: peerTab?.workspaceName ?? state.workspaces.find(ws => ws.id === tab.workspace_id || ws.tabs.includes(resolvedId))?.name,
          activity: activity.kind === "working" ? "working" : activity.kind === "waiting-on-user" ? "waiting" : activity.kind === "failed" ? "error" : "idle" };
        knownTabs.set(targetId, value);
        knownTabs.set(resolvedId, value);
      } else {
        const known = knownTabs.get(targetId);
        if (known) value = { ...known, state: "closed", activity: "idle" };
      }
    } else if (kind === "task" && ports.taskReferences) {
      const requested=ports.taskReferences.read(key);
      const found=requested.data;
      const sameScope=!found || found.targetId?.toLowerCase()===id.toLowerCase() || ("scopeKey" in found && found.scopeKey===JSON.stringify([scope.projectPath,scope.teamId]));
      value=requested.loaded && sameScope ? found : {title:id,kind,id,state:"loading"};
    } else if (kind === "plan" || kind === "task") {
      const plans = ports.plans.get();
      if (plans !== planSnapshot) { planSnapshot = plans; indexes.clear(); }
      const scopeKey = JSON.stringify([scope.projectPath, scope.teamId]);
      let index = indexes.get(scopeKey);
      if (!index) {
        index = new Map();
        for (const plan of plans) {
          const inScope = planInMentionScope(plan, scope);
          if (inScope) index.set(`plan:${plan.id}`, { title: plan.title, kind: "plan", id: plan.id, planId: plan.id });
          for (const task of plan.tasks) {
            for (const ref of new Set([task.id.toLowerCase(), inScope ? task.key?.toLowerCase() : undefined].filter((key): key is string => !!key))) {
              const key = `task:${ref}`;
              index.set(key, index.has(key) ? null : { title: `${task.key ?? task.id} ${task.name}`, kind: "task", id: ref, planId: plan.id, targetId: task.id, teamId: plan.team_id ?? null });
            }
          }
        }
        indexes.set(scopeKey, index);
      }
      const found = index.get(`${kind}:${kind === "task" ? id.toLowerCase() : id}`);
      if (found) value = { ...found, id };
    } else if (kind === "blueprint") {
      const node = graphs.get(graphKey(scope))?.nodes.find((item) => item.identity.id === id
        && blueprintInMentionScope(item, { scope }, state.tabs));
      if (node) value = { title: node.name, kind, id, teamId: node.identity.team_id };
      else if (pending.has(graphKey(scope))) value = { title: id, kind, id, state: "loading" };
    }
    const previous = values.get(key);
    if (previous !== undefined && JSON.stringify(previous) === JSON.stringify(value)) return previous;
    values.set(key, value);
    return value;
  };
  const host: ChatMentionHost = {
    renderAvatar: ports.renderAvatar,
    binding: {
      read,
      retain(key){
        try{const [,kind]=JSON.parse(key);if(kind==="task")return ports.taskReferences?.retain(key) ?? (()=>{});}catch{}
        return ()=>{};
      },
      subscribe(listener) {
        listeners.add(listener);
        if (!stopSources) {
          const offReferences=ports.taskReferences?.subscribe(notify);
          const offPeers = ports.watchPeerTabs?.(notify);
          const offMux = watchMentionState(ports.mux, notify, true);
          const offPlans = ports.plans.subscribe(() => { indexes.clear(); planSnapshot = ports.plans.get(); notify(); });
          stopSources = () => { offMux(); offPlans(); offReferences?.(); offPeers?.(); };
        }
        return () => {
          listeners.delete(listener);
          if (!listeners.size) {
            stopSources?.(); stopSources = null;
            for (const stop of graphWatches.values()) void stop.then((off) => off());
            graphWatches.clear(); graphs.clear(); values.clear();
            pending.clear();
          }
        };
      },
      refresh(key) {
        const [tabId, kind] = JSON.parse(key) as [string, CanonicalMentionKind, string];
        if (kind === "task" && ports.taskReferences) { ports.taskReferences.refresh(key); return; }
        if (kind !== "blueprint") return;
        const scope = mentionScopeForTab(ports.mux.getState(), tabId);
        const cacheKey = graphKey(scope);
        if (scope.workspaceId && !graphWatches.has(cacheKey) && ports.watchGraph) {
          graphWatches.set(cacheKey, ports.watchGraph(scope, () => { graphs.delete(cacheKey); host.binding.refresh(key); }).catch(() => () => {}));
        }
        if (!scope.workspaceId || pending.has(cacheKey) || graphs.has(cacheKey)) return;
        const reading = ports.readGraph(scope, new AbortController().signal).then((graph) => {
          if (pending.get(cacheKey) === reading) graphs.set(cacheKey, graph);
        }).catch(() => {}).finally(() => {
          if (pending.get(cacheKey) === reading) { pending.delete(cacheKey); notify(); }
        });
        pending.set(cacheKey, reading);
        notify();
      },
    },
    open(key) {
      const value = read(key);
      if (!value || value.state) return;
      const [tabId] = JSON.parse(key) as [string];
      const state = ports.mux.getState();
      const scope = mentionScopeForTab(state, tabId);
      if (value.kind === "tab") {
        const tab = state.tabs[value.id];
        const workspace = state.workspaces.find((item) => item.id === tab?.workspace_id || item.tabs.includes(value.id));
        const workspaceId = workspace?.id ?? ports.readPeerTab?.(value.id)?.workspaceId;
        if (workspaceId) ports.openTab(workspaceId, value.id);
      } else if (value.kind === "blueprint" && scope.workspaceId) ports.openBlueprint(scope.workspaceId, value.teamId ?? null, value.id);
      else if (value.planId) ports.openPlan({ planId: value.planId, taskId: value.targetId, teamId: (value.teamId === undefined ? scope.teamId : value.teamId) ?? undefined });
    },
  };
  return host;
}
