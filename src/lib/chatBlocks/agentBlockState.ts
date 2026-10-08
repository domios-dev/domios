import type { AgentBlockInfo, AgentBlockState, AgentRunState } from "@codeterm/chat-ui";
import type { Plan } from "../../components/Canvas/types";
import { resolveTabCurrentTask } from "../../components/Canvas/tabCurrentTask";
import { taskDisplayKey } from "../../components/Canvas/taskDisplayKey";
import { agentModelForBadge } from "../agentLabel";
import { roleKindLabel } from "../roleKind";
import type { Tab } from "../types";

/** An exact tab id, else a prefix that names exactly one tab. */
export function resolveTabRef(tabs: Readonly<Record<string, Tab>>, ref: string): Tab | null {
  if (tabs[ref]) return tabs[ref];
  const matches = Object.keys(tabs).filter((id) => id.startsWith(ref));
  return matches.length === 1 ? tabs[matches[0]] : null;
}

function hasRuntime(tab: Tab): boolean {
  return Boolean(tab.pty_id) || tab.backing?.kind === "chat_backend";
}

export function agentRunState(tab: Tab): AgentRunState {
  const live = tab.agent_state?.state;
  if (live === "working") return "working";
  if (live === "question") return "waiting";
  if (tab.agent_booting || tab.boot_stage) return "starting";
  if (live === "idle") return "idle";
  return hasRuntime(tab) ? "idle" : "offline";
}

export function agentBlockInfo(tab: Tab, plans: readonly Plan[], providerName: (agentType: string) => string | null): AgentBlockInfo {
  const state = agentRunState(tab);
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
    statusLine: tab.agent_state?.status_line?.trim() || tab.description?.trim() || null,
    task: current ? { key: taskDisplayKey(current.task), name: current.task.name, plan: current.plan.title } : null,
    peer: tab.remote_peer_name ?? null,
    canStop: state === "working" && Boolean(tab.pty_id),
  };
}

export function agentBlockState(tabs: Readonly<Record<string, Tab>>, plans: readonly Plan[], ref: string, providerName: (agentType: string) => string | null): AgentBlockState {
  const tab = resolveTabRef(tabs, ref);
  return tab ? { kind: "ready", agent: agentBlockInfo(tab, plans, providerName) } : { kind: "missing" };
}
