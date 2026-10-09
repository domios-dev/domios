import { tabActivity } from "../tabActivity";
import type { AgentBlockInfo, AgentBlockState, AgentRunState } from "@codeterm/chat-ui";
import type { Plan } from "../../components/Canvas/types";
import { resolveTabCurrentTask } from "../../components/Canvas/tabCurrentTask";
import { taskDisplayKey } from "../../components/Canvas/taskDisplayKey";
import { agentModelForBadge } from "../agentLabel";
import { roleKindLabel } from "../roleKind";
import type { Tab, TabStatus } from "../types";

/** An exact tab id, else a prefix that names exactly one tab. */
export function resolveTabRef(tabs: Readonly<Record<string, Tab>>, ref: string): Tab | null {
  if (tabs[ref]) return tabs[ref];
  const matches = Object.keys(tabs).filter((id) => id.startsWith(ref));
  return matches.length === 1 ? tabs[matches[0]] : null;
}

function hasRuntime(tab: Tab): boolean {
  return Boolean(tab.pty_id) || tab.backing?.kind === "chat_backend";
}

export function agentRunState(tab: Tab, status?: TabStatus | null): AgentRunState {
  const activity = tabActivity(tab, status);
  if (activity.kind === "waiting-on-user") return "waiting";
  if (activity.kind === "failed") return "offline";
  if (activity.kind === "idle" && !hasRuntime(tab)) return "offline";
  return activity.kind;
}

export function agentBlockInfo(tab: Tab, plans: readonly Plan[], providerName: (agentType: string) => string | null, status?: TabStatus | null): AgentBlockInfo {
  const activity = tabActivity(tab, status);
  const state = agentRunState(tab, status);
  const current = resolveTabCurrentTask(plans, tab.id);
  const model = agentModelForBadge({
    sessionModel: tab.agent_session_model,
    providerResolvedModel: tab.agent_provider_resolved_model,
    requestedModel: tab.agent_requested_model,
  });
  return {
    tabId: tab.id,
    name: tab.title,
    role: roleKindLabel(tab),
    provider: tab.agent_type ? providerName(tab.agent_type) ?? tab.agent_type : null,
    model,
    state,
    statusLine: activity.label || tab.description?.trim() || null,
    task: current ? { key: taskDisplayKey(current.task), name: current.task.name, plan: current.plan.title } : null,
    peer: tab.remote_peer_name ?? null,
    canStop: activity.canStop && Boolean(tab.pty_id),
  };
}

export function agentBlockState(tabs: Readonly<Record<string, Tab>>, plans: readonly Plan[], ref: string, providerName: (agentType: string) => string | null, statuses?: Record<string, TabStatus>): AgentBlockState {
  const tab = resolveTabRef(tabs, ref);
  return tab ? { kind: "ready", agent: agentBlockInfo(tab, plans, providerName, statuses?.[tab.id]) } : { kind: "missing" };
}
