type Block = { kind: string; data: Record<string, unknown> };

export interface AgentActivityRow {
  role: "assistant";
  uuid?: string;
  blocks: Block[];
  ts: number | null;
  timestamp: string | null;
}

const AGENT_STATES: Record<string, string> = {
  started: "started",
  pending_init: "started",
  running: "running",
  completed: "completed",
  errored: "failed",
  failed: "failed",
  interrupted: "interrupted",
  shutdown: "closed",
  closed: "closed",
};

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function row(uuid: string, blocks: Block[], ts: number | null, timestamp: string | null): AgentActivityRow | null {
  if (blocks.length === 0) return null;
  return { role: "assistant", ...(uuid ? { uuid } : {}), blocks, ts, timestamp };
}

/** Neutral agent capability: `tool_kind: "agent"` with agent_id/agent_path/agent_state/message_from/message_to input. */
function agentBlock(fields: Record<string, string>, summary: string, turnId: unknown): Block {
  const input: Record<string, string> = {};
  for (const key in fields) if (fields[key]) input[key] = fields[key];
  const flowId = text(turnId);
  return { kind: "tool_use", data: {
    name: "agent_activity", input, tool_kind: "agent", tool_summary: summary, tool_status: "success",
    ...(flowId ? { tool_flow_id: flowId } : {}),
  } };
}

function thoughtRow(id: unknown, summaries: unknown[], ts: number | null, timestamp: string | null): AgentActivityRow | null {
  const blocks = summaries
    .filter((summary): summary is string => typeof summary === "string" && summary.trim() !== "")
    .map((summary) => ({ kind: "thinking", data: { text: summary } }));
  return row(text(id) ? `reasoning-${text(id)}` : "", blocks, ts, timestamp);
}

/** Completed reasoning item: only its human summary strings, never encrypted or raw content. */
export function reasoningItemRow(item: Record<string, unknown>, ts: number | null, timestamp: string | null): AgentActivityRow | null {
  return thoughtRow(item.id, Array.isArray(item.summary_text) ? item.summary_text : [], ts, timestamp);
}

export function reasoningResponseRow(payload: Record<string, unknown>, ts: number | null, timestamp: string | null): AgentActivityRow | null {
  const parts = Array.isArray(payload.summary) ? payload.summary : [];
  const summaries = parts.map((part) => {
    const entry = record(part);
    return entry && entry.type === "summary_text" ? entry.text : undefined;
  });
  return thoughtRow(payload.id, summaries, ts, timestamp);
}

/** A sub-agent the session reports by thread and path; `interacted` carries no state. */
export function subAgentRow(item: Record<string, unknown>, id: unknown, turnId: unknown, ts: number | null, timestamp: string | null): AgentActivityRow | null {
  const agentId = text(item.agent_thread_id);
  const agentPath = text(item.agent_path);
  if (!agentId && !agentPath) return null;
  const state = AGENT_STATES[text(item.kind)] ?? "unknown";
  const block = agentBlock({ agent_id: agentId, agent_path: agentPath, agent_state: state }, agentPath || agentId, turnId);
  return row(text(id) ? `agent-${text(id)}` : "", [block], ts, timestamp);
}

export function collabCallRow(item: Record<string, unknown>, turnId: unknown, ts: number | null, timestamp: string | null): AgentActivityRow | null {
  const receivers = Array.isArray(item.receiver_thread_ids) ? item.receiver_thread_ids : [];
  const agents = Array.isArray(item.receiver_agents) ? item.receiver_agents.map(record) : [];
  const states = record(item.agents_states);
  const seen: string[] = [];
  const blocks: Block[] = [];
  for (const receiver of receivers) {
    const agentId = text(receiver);
    if (!agentId || seen.indexOf(agentId) !== -1) continue;
    seen.push(agentId);
    const agentPath = text(agents.find((entry) => entry !== null && text(entry.thread_id) === agentId)?.agent_path);
    const state = AGENT_STATES[text(record(states?.[agentId])?.status)] ?? "unknown";
    blocks.push(agentBlock({ agent_id: agentId, agent_path: agentPath, agent_state: state }, agentPath || agentId, turnId));
  }
  return row(text(item.id) ? `collab-${text(item.id)}` : "", blocks, ts, timestamp);
}

/** Inter-agent message: direction exists only when the source names a distinct author and recipient. */
export function agentMessageRow(payload: Record<string, unknown>, ts: number | null, timestamp: string | null): AgentActivityRow | null {
  const from = text(payload.author);
  const to = text(payload.recipient);
  if (!from || !to || from === to) return null;
  const turnId = record(payload.internal_chat_message_metadata_passthrough)?.turn_id;
  const block = agentBlock({ message_from: from, message_to: to }, `${from} → ${to}`, turnId);
  return row(text(payload.id) ? `agent-message-${text(payload.id)}` : "", [block], ts, timestamp);
}
