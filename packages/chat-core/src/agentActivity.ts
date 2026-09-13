import type { AgentActivity, AgentActivityAction, AgentActivityState, NormalizedChatMessage, ToolEventKind } from "./types";

export interface AgentNode {
  key: string;
  id?: string;
  path?: string;
  callId?: string;
  /** Last path segment, else the source description, else the exact id. */
  name: string;
  state: AgentActivityState;
  depth: number;
  description?: string;
  model?: string;
  durationMs?: number;
  toolUses?: number;
  tokens?: number;
  result?: string;
  actions?: AgentActivityAction[];
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
const ACTION_KINDS: readonly ToolEventKind[] = ["command", "edit", "read", "search", "task", "fetch", "codeterm", "web", "image", "generic"];
const DETAIL_KEYS = ["description", "model", "durationMs", "toolUses", "tokens", "result", "actions"] as const;

function clean(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function count(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}

function segments(path: string | undefined): string[] {
  return path ? path.split("/").filter(Boolean) : [];
}

function actionsOf(value: unknown): AgentActivityAction[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const actions = value.flatMap((entry): AgentActivityAction[] => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) return [];
    const item = entry as Record<string, unknown>;
    const summary = clean(item.summary);
    if (!summary) return [];
    const id = clean(item.id);
    const detail = clean(item.detail);
    const description = clean(item.description);
    return [{
      ...(id ? { id } : {}),
      kind: ACTION_KINDS.find((kind) => kind === item.kind) ?? "generic",
      summary,
      ...(detail ? { detail } : {}),
      ...(description ? { description } : {}),
      status: item.status === "success" || item.status === "error" ? item.status : "pending",
    }];
  });
  return actions.length > 0 ? actions : undefined;
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
  const callId = clean(source.agent_call_id);
  const from = clean(source.message_from);
  const to = clean(source.message_to);
  const edge = Boolean(from && to && from !== to);
  if (!agentId && !agentPath && !callId && !edge) return null;
  const state = KNOWN_STATES.find((known) => known === source.agent_state) ?? "unknown";
  const activity: AgentActivity = {
    ...(agentId ? { agentId } : {}),
    ...(agentPath ? { agentPath } : {}),
    ...(callId ? { callId } : {}),
    state,
    ...(edge ? { messageFrom: from, messageTo: to } : {}),
  };
  const details: Partial<AgentActivity> = {
    description: clean(source.agent_description),
    model: clean(source.agent_model),
    durationMs: count(source.agent_duration_ms),
    toolUses: count(source.agent_tool_uses),
    tokens: count(source.agent_tokens),
    result: clean(source.agent_result),
    actions: actionsOf(source.agent_actions),
  };
  for (const key of DETAIL_KEYS) {
    if (details[key] !== undefined) Object.assign(activity, { [key]: details[key] });
  }
  return activity;
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

/** A later record's reported fact replaces the earlier one; a fact it omits stays as last reported. */
function mergeDetails(node: AgentNode, record: AgentActivity): void {
  for (const key of DETAIL_KEYS) {
    if (record[key] !== undefined) (node as unknown as Record<string, unknown>)[key] = record[key];
  }
}

export function buildAgentTree(records: readonly AgentActivity[]): AgentActivityTree {
  const nodes: AgentNode[] = [];
  const edges = new Map<string, AgentMessageEdge>();
  for (const record of records) {
    if (record.agentId || record.agentPath || record.callId) {
      const node = nodes.find((candidate) => (record.callId && candidate.callId === record.callId)
        || (record.agentId && candidate.id === record.agentId)
        || (record.agentPath && candidate.path === record.agentPath && (!record.agentId || !candidate.id)));
      const state = (record.state ?? "unknown") as AgentActivityState;
      if (node) {
        node.id ??= record.agentId;
        node.path ??= record.agentPath;
        node.callId ??= record.callId;
        if (state !== "unknown") node.state = state;
        mergeDetails(node, record);
      } else {
        const created: AgentNode = {
          key: record.agentId ?? record.agentPath ?? record.callId ?? "",
          id: record.agentId, path: record.agentPath, name: "", state, depth: 0,
          ...(record.callId ? { callId: record.callId } : {}),
        };
        mergeDetails(created, record);
        nodes.push(created);
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
    node.name = parts[parts.length - 1] ?? node.description ?? node.id ?? node.callId ?? "";
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
