import type { ChatBlockTone } from "./ChatBlockFrame";

export type AgentRunState = "working" | "idle" | "waiting" | "starting" | "offline";

/** Facts the host reports about one agent tab; the card derives everything it shows from these. */
export interface AgentBlockInfo {
  tabId: string;
  name: string;
  role: string | null;
  provider: string | null;
  model: string | null;
  state: AgentRunState;
  statusLine: string | null;
  task: { key: string; name: string; plan: string } | null;
  /** Peer name when the tab lives on another machine. */
  peer: string | null;
  canStop: boolean;
}

export type AgentBlockState =
  | { kind: "loading" }
  | { kind: "missing" }
  | { kind: "ready"; agent: AgentBlockInfo };

const TAB_REF = /^[A-Za-z0-9][A-Za-z0-9_-]{3,63}$/;

/** A full tab id or the 8-character prefix agents see in routing markers. */
export function parseTabRef(argument: string): string | null {
  const ref = argument.trim();
  return TAB_REF.test(ref) ? ref : null;
}

export const AGENT_STATE_LABEL: Record<AgentRunState, string> = {
  working: "Working",
  idle: "Idle",
  waiting: "Waiting for you",
  starting: "Starting",
  offline: "Not running",
};

export const AGENT_STATE_TONE: Record<AgentRunState, ChatBlockTone> = {
  working: "accent",
  idle: "ok",
  waiting: "warn",
  starting: "muted",
  offline: "muted",
};

/** Identity line under the name: role, then the model when known, else the provider. */
export function agentIdentity(agent: AgentBlockInfo): string {
  return [agent.role, agent.model ?? agent.provider, agent.peer && `on ${agent.peer}`].filter(Boolean).join(" · ");
}

/** The current task when bound to one, else the provider's live status line. */
export function agentActivity(agent: AgentBlockInfo): string | null {
  if (agent.task) return `${agent.task.key} ${agent.task.name}`.trim();
  return agent.statusLine?.trim() || null;
}
