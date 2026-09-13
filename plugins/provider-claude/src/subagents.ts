import { claudeSessionFilePath, familyFromModelId, type ClaudeEntry } from "./session";
import { canonicalToolFields, toolDescription } from "./toolFields";

type Block = { kind: string; data: Record<string, unknown> };

const SIDECHAIN_TAIL_BYTES = 256 * 1024;
const MAX_ACTIONS = 60;
const PLAIN_ID_RE = /^[A-Za-z0-9_-]+$/;
const AGENT_LOG_RE = /^agent-[A-Za-z0-9_-]+\.jsonl$/;

const LAUNCH_STATES: Record<string, string> = {
  async_launched: "running",
  running: "running",
  completed: "completed",
  failed: "failed",
};

const NOTIFICATION_STATES: Record<string, string> = {
  running: "running",
  completed: "completed",
  failed: "failed",
  error: "failed",
  killed: "interrupted",
  stopped: "interrupted",
  cancelled: "interrupted",
  canceled: "interrupted",
};

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function count(value: string | null): number | undefined {
  if (value === null || !/^\d+$/.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : undefined;
}

function modelFamily(value: unknown): string | undefined {
  const model = text(value);
  return model ? familyFromModelId(model) : undefined;
}

export function xmlTagBody(content: string, tag: string): string | null {
  const open = `<${tag}>`;
  const close = `</${tag}>`;
  const start = content.indexOf(open);
  if (start < 0) return null;
  const bodyStart = start + open.length;
  const end = content.indexOf(close, bodyStart);
  if (end < 0) return null;
  return content.slice(bodyStart, end).trim();
}

/** Neutral agent capability keyed by the launching call, so every record of one sub-agent folds into one card. */
function agentBlock(callId: string, fields: Record<string, unknown>, summary: string): Block {
  const input: Record<string, unknown> = { agent_call_id: callId };
  for (const key in fields) {
    const value = fields[key];
    if (value !== undefined && value !== "") input[key] = value;
  }
  return { kind: "tool_use", data: {
    name: "agent_activity", input, tool_kind: "agent", tool_summary: summary || callId, tool_status: "success", tool_flow_id: callId,
  } };
}

export function isAgentActivityBlock(block: Block): boolean {
  return block.kind === "tool_use" && block.data.tool_kind === "agent";
}

export function isSubAgentCall(name: unknown, input: unknown): boolean {
  const tool = text(name);
  return (tool === "Agent" || tool === "Task") && typeof record(input)?.prompt === "string";
}

export function subAgentCallBlock(id: unknown, input: unknown): Block | null {
  const callId = text(id);
  if (!callId) return null;
  const description = toolDescription(input);
  return agentBlock(callId, {
    agent_state: "started", agent_description: description, agent_model: modelFamily(record(input)?.model),
  }, description);
}

/** The call's own result names the agent; an unrecognized status is not a sub-agent launch. */
export function subAgentLaunchBlock(toolUseId: unknown, toolUseResult: unknown): Block | null {
  const callId = text(toolUseId);
  const result = record(toolUseResult);
  const agentId = text(result?.agentId);
  const state = LAUNCH_STATES[text(result?.status)];
  if (!callId || !result || !PLAIN_ID_RE.test(agentId) || !state) return null;
  const description = toolDescription(result);
  return agentBlock(callId, {
    agent_id: agentId, agent_state: state, agent_description: description, agent_model: modelFamily(result.resolvedModel),
  }, description);
}

export interface SubAgentNotification {
  callId: string;
  agentId: string;
  state: string;
  result?: string;
  tokens?: number;
  toolUses?: number;
  durationMs?: number;
}

/** A sub-agent notification carries its call id plus a result or usage; a background command carries neither. */
export function subAgentNotification(content: string): SubAgentNotification | null {
  const start = content.indexOf("<task-notification>");
  const end = content.indexOf("</task-notification>", start);
  if (start < 0 || end < 0) return null;
  const block = content.slice(start, end);
  const callId = xmlTagBody(block, "tool-use-id") ?? "";
  const agentId = xmlTagBody(block, "task-id") ?? "";
  const result = xmlTagBody(block, "result");
  const usage = xmlTagBody(block, "usage");
  if (!callId || !PLAIN_ID_RE.test(agentId) || (result === null && usage === null)) return null;
  return {
    callId,
    agentId,
    state: NOTIFICATION_STATES[xmlTagBody(block, "status") ?? ""] ?? "unknown",
    ...(result ? { result } : {}),
    ...(usage !== null ? {
      tokens: count(xmlTagBody(usage, "subagent_tokens")),
      toolUses: count(xmlTagBody(usage, "tool_uses")),
      durationMs: count(xmlTagBody(usage, "duration_ms")),
    } : {}),
  };
}

interface Sidechain {
  actions: Array<Record<string, unknown>>;
  model?: string;
}

function readTail(path: string): string | null {
  try {
    const raw = host.fs.readFileTail(path, SIDECHAIN_TAIL_BYTES);
    return typeof raw === "string" ? raw : null;
  } catch (_) {
    return null;
  }
}

/** Bounded tail read of the transcript the row's own session wrote for this agent; absent or unreadable is null. */
export function subAgentSidechain(entry: ClaudeEntry, agentId: string): Sidechain | null {
  const cwd = text(entry.cwd);
  const sessionId = text(entry.sessionId);
  if (!cwd || !PLAIN_ID_RE.test(sessionId) || !PLAIN_ID_RE.test(agentId)) return null;
  const mainPath = claudeSessionFilePath(cwd, sessionId);
  if (!mainPath || !mainPath.endsWith(`${sessionId}.jsonl`)) return null;
  const sep = mainPath.lastIndexOf("\\") > mainPath.lastIndexOf("/") ? "\\" : "/";
  const raw = readTail(`${mainPath.slice(0, -".jsonl".length)}${sep}subagents${sep}agent-${agentId}.jsonl`);
  if (raw === null) return null;
  const actions: Array<Record<string, unknown>> = [];
  const byId: Record<string, Record<string, unknown>> = {};
  let model: string | undefined;
  for (const line of raw.split("\n")) {
    let row: ClaudeEntry;
    try { row = JSON.parse(line); } catch (_) { continue; }
    if (!row || row.isSidechain !== true || row.agentId !== agentId || row.sessionId !== sessionId) continue;
    if (row.type === "assistant" && !model) model = modelFamily(row.message?.model);
    const content = row.message?.content;
    if (!Array.isArray(content)) continue;
    for (const block of content) {
      if (!block) continue;
      if (row.type === "assistant" && block.type === "tool_use") {
        const id = text(block.id);
        const canonical = canonicalToolFields(text(block.name), block.input);
        const description = toolDescription(block.input);
        const action: Record<string, unknown> = { id, kind: canonical.kind, summary: canonical.summary, status: "pending" };
        if (canonical.detail) action.detail = canonical.detail;
        if (description) action.description = description;
        actions.push(action);
        if (id) byId[id] = action;
      } else if (row.type === "user" && block.type === "tool_result") {
        const action = byId[text(block.tool_use_id)];
        if (action) action.status = block.is_error === true ? "error" : "success";
      }
    }
  }
  return { actions: actions.slice(-MAX_ACTIONS), ...(model ? { model } : {}) };
}

export function subAgentNotificationBlock(content: string, entry: ClaudeEntry): Block | null {
  const notice = subAgentNotification(content);
  if (!notice) return null;
  const sidechain = subAgentSidechain(entry, notice.agentId);
  return agentBlock(notice.callId, {
    agent_id: notice.agentId,
    agent_state: notice.state,
    agent_result: notice.result,
    agent_tokens: notice.tokens,
    agent_tool_uses: notice.toolUses,
    agent_duration_ms: notice.durationMs,
    agent_model: sidechain?.model,
    agent_actions: sidechain && sidechain.actions.length > 0 ? sidechain.actions : undefined,
  }, notice.agentId);
}

/** Sub-agent transcripts beside this exact session file; a path not named for the session yields none. */
export function subAgentUsageSources(sessionFilePath: string, sessionId: string): string[] {
  const file = text(sessionFilePath);
  const id = text(sessionId);
  if (!file || !PLAIN_ID_RE.test(id)) return [];
  const cut = Math.max(file.lastIndexOf("/"), file.lastIndexOf("\\"));
  if (file.slice(cut + 1) !== `${id}.jsonl`) return [];
  const sep = cut >= 0 ? file.charAt(cut) : "/";
  let entries: Array<{ name: string; path: string; isFile: boolean }> = [];
  try {
    entries = host.fs.readDir(`${file.slice(0, cut + 1)}${id}${sep}subagents`) || [];
  } catch (_) {
    return [];
  }
  return entries
    .filter((entry) => entry && entry.isFile && AGENT_LOG_RE.test(entry.name))
    .map((entry) => entry.path)
    .sort();
}
