import type { AgentActivity, AgentActivityState, NormalizedChatMessage } from "./types";

export interface AgentNode {
  key: string;
  id?: string;
  path?: string;
  /** Last path segment, else the exact id. */
  name: string;
  state: AgentActivityState;
  depth: number;
}

export interface AgentMessageEdge {
  from: string;
  to: string;
  fromLabel: string;
  toLabel: string;
  count: number;
}

export interface AgentActivityTree {
  agents: AgentNode[];
  messages: AgentMessageEdge[];
}

const KNOWN_STATES: readonly AgentActivityState[] = ["started", "running", "completed", "failed", "interrupted", "closed"];

function clean(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function segments(path: string | undefined): string[] {
  return path ? path.split("/").filter(Boolean) : [];
}

export function isAgentActivityMessage(message: NormalizedChatMessage): boolean {
  return message.type === "tool_call" && message.toolKind === "agent";
}

/** The validated record of an agent-kind call, or null when it names no agent and no distinct edge. */
export function agentActivityOf(message: NormalizedChatMessage): AgentActivity | null {
  const source = isAgentActivityMessage(message) ? message.toolInput : undefined;
  if (!source || typeof source !== "object" || Array.isArray(source)) return null;
  const agentId = clean(source.agent_id);
  const agentPath = clean(source.agent_path);
  const from = clean(source.message_from);
  const to = clean(source.message_to);
  const edge = Boolean(from && to && from !== to);
  if (!agentId && !agentPath && !edge) return null;
  const state = KNOWN_STATES.find((known) => known === source.agent_state) ?? "unknown";
  return {
    ...(agentId ? { agentId } : {}),
    ...(agentPath ? { agentPath } : {}),
    state,
    ...(edge ? { messageFrom: from, messageTo: to } : {}),
  };
}

function compareNodes(a: AgentNode, b: AgentNode): number {
  if (!a.path || !b.path) return a.path ? -1 : b.path ? 1 : 0;
  const left = segments(a.path);
  const right = segments(b.path);
  for (let index = 0; index < Math.min(left.length, right.length); index++) {
    if (left[index] !== right[index]) return left[index] < right[index] ? -1 : 1;
  }
  return left.length - right.length;
}

export function buildAgentTree(records: readonly AgentActivity[]): AgentActivityTree {
  const nodes: AgentNode[] = [];
  const edges = new Map<string, AgentMessageEdge>();
  for (const record of records) {
    if (record.agentId || record.agentPath) {
      const node = nodes.find((candidate) => (record.agentId && candidate.id === record.agentId)
        || (record.agentPath && candidate.path === record.agentPath && (!record.agentId || !candidate.id)));
      const state = (record.state ?? "unknown") as AgentActivityState;
      if (node) {
        node.id ??= record.agentId;
        node.path ??= record.agentPath;
        if (state !== "unknown") node.state = state;
      } else {
        nodes.push({ key: record.agentId ?? record.agentPath ?? "", id: record.agentId, path: record.agentPath, name: "", state, depth: 0 });
      }
    }
    if (record.messageFrom && record.messageTo) {
      const key = JSON.stringify([record.messageFrom, record.messageTo]);
      const edge = edges.get(key);
      if (edge) edge.count += 1;
      else edges.set(key, { from: record.messageFrom, to: record.messageTo, fromLabel: "", toLabel: "", count: 1 });
    }
  }
  const pathDepths = nodes.filter((node) => node.path).map((node) => segments(node.path).length);
  const minDepth = pathDepths.length ? Math.min(...pathDepths) : 0;
  for (const node of nodes) {
    const parts = segments(node.path);
    node.name = parts[parts.length - 1] ?? node.id ?? "";
    node.depth = node.path ? parts.length - minDepth : 0;
  }
  // Labels are exact source references; any shortening is visual only.
  const label = (ref: string) => nodes.find((candidate) => candidate.id === ref)?.path ?? ref;
  const messages = [...edges.values()].map((edge) => ({ ...edge, fromLabel: label(edge.from), toLabel: label(edge.to) }));
  return { agents: [...nodes].sort(compareNodes), messages };
}

export function agentTreesEquivalent(a: AgentActivityTree | undefined, b: AgentActivityTree | undefined): boolean {
  if (a === b) return true;
  return Boolean(a && b) && JSON.stringify(a) === JSON.stringify(b);
}
