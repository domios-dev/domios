import type { ChatMentionHost, ChatMentionValue } from "../../../packages/chat-ui/src/blocks/ChatBlockHost";
import type { CanonicalMentionKind } from "../../../packages/chat-ui/src/codeterm/canonicalMentions";
import type { AutomationGraph } from "../../components/Automation/types";
import type { Plan } from "../../components/Canvas/types";
import type { PlanTarget } from "../../components/Canvas/teamViewNavigation";
import type { MuxStateStore } from "../../store/multiplexer/selectorStore";
import { mentionScopeForTab, planInMentionScope } from "../mentionSources/scope";
import { blueprintInMentionScope } from "../mentionSources/blueprints";
import type { MentionScope } from "../composerMentions";
import type { ValueFeed } from "./valueFeed";
import { watchMentionState } from "../mentionSources/live";

export function createMentionHost(ports: {
  mux: Pick<MuxStateStore, "getState" | "subscribe">;
  plans: ValueFeed<readonly Plan[]>;
  readGraph(scope: MentionScope, signal: AbortSignal): Promise<AutomationGraph>;
  watchGraph?(scope: MentionScope, changed: () => void): Promise<() => void>;
  openPlan(target: PlanTarget): void;
  openTab(workspaceId: string, tabId: string): void;
  openBlueprint(workspaceId: string, teamId: string | null, id: string): void;
}): ChatMentionHost {
  const graphs = new Map<string, AutomationGraph>();
  const pending = new Map<string, Promise<void>>();
  const values = new Map<string, ChatMentionValue | null>();
  const listeners = new Set<() => void>();
  const graphWatches = new Map<string, Promise<() => void>>();
  const graphKey = (scope: MentionScope) => JSON.stringify([scope.workspaceId, scope.teamId]);
  const read = (key: string): ChatMentionValue | null => {
    let args: [string, CanonicalMentionKind, string];
    try { args = JSON.parse(key); } catch { return null; }
    const [tabId, kind, id] = args;
    const state = ports.mux.getState();
    const scope = mentionScopeForTab(state, tabId);
    let value: ChatMentionValue | null = null;
    if (scope.workspaceId && kind === "tab") {
      const tab = state.tabs[id];
      if (tab) value = { title: tab.title, kind, id };
    } else if (kind === "plan" || kind === "task") {
      for (const plan of ports.plans.get().filter((item) => planInMentionScope(item, scope))) {
        if (kind === "plan" && plan.id === id) value = { title: plan.title, kind, id, planId: plan.id };
        const task = kind === "task" ? plan.tasks.find((item) => item.key?.toLowerCase() === id.toLowerCase() || item.id === id) : null;
        if (task) {
          if (value) return null;
          value = { title: `${task.key ?? task.id} ${task.name}`, kind, id, planId: plan.id, targetId: task.id };
        }
      }
    } else if (kind === "blueprint") {
      const node = graphs.get(graphKey(scope))?.nodes.find((item) => item.identity.id === id
        && blueprintInMentionScope(item, { scope }, state.tabs));
      if (node) value = { title: node.name, kind, id, teamId: node.identity.team_id };
    }
    const previous = values.get(key);
    if (previous !== undefined && JSON.stringify(previous) === JSON.stringify(value)) return previous;
    values.set(key, value);
    return value;
  };
  const host: ChatMentionHost = {
    binding: {
      read,
      subscribe(listener) {
        listeners.add(listener);
        const offMux = watchMentionState(ports.mux, listener);
        const offPlans = ports.plans.subscribe(listener);
        return () => {
          listeners.delete(listener); offMux(); offPlans();
          if (!listeners.size) { for (const stop of graphWatches.values()) void stop.then((off) => off()); graphWatches.clear(); }
        };
      },
      refresh(key) {
        const [tabId, kind] = JSON.parse(key) as [string, CanonicalMentionKind, string];
        if (kind !== "blueprint") return;
        const scope = mentionScopeForTab(ports.mux.getState(), tabId);
        const cacheKey = graphKey(scope);
        if (scope.workspaceId && !graphWatches.has(cacheKey) && ports.watchGraph) {
          graphWatches.set(cacheKey, ports.watchGraph(scope, () => { graphs.delete(cacheKey); host.binding.refresh(key); }).catch(() => () => {}));
        }
        if (!scope.workspaceId || pending.has(cacheKey) || graphs.has(cacheKey)) return;
        const reading = ports.readGraph(scope, new AbortController().signal).then((graph) => {
          graphs.set(cacheKey, graph);
          for (const changed of listeners) changed();
        }).catch(() => {}).finally(() => pending.delete(cacheKey));
        pending.set(cacheKey, reading);
      },
    },
    open(key) {
      const value = read(key);
      if (!value) return;
      const [tabId] = JSON.parse(key) as [string];
      const state = ports.mux.getState();
      const scope = mentionScopeForTab(state, tabId);
      if (value.kind === "tab") {
        const tab = state.tabs[value.id];
        const workspace = state.workspaces.find((item) => item.id === tab?.workspace_id || item.tabs.includes(value.id));
        if (workspace) ports.openTab(workspace.id, value.id);
      } else if (value.kind === "blueprint" && scope.workspaceId) ports.openBlueprint(scope.workspaceId, value.teamId ?? null, value.id);
      else if (value.planId) ports.openPlan({ planId: value.planId, taskId: value.targetId, teamId: scope.teamId ?? undefined });
    },
  };
  return host;
}
