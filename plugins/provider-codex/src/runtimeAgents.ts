import type { ChatMsg } from "@codeterm/plugin-sdk";
import { object, text, list } from "./runtimeProtocol";
import { runtimeItems } from "./runtimeMessages";

export interface RuntimeAgents {
  rows: Record<string, string>;
  ordinal: number;
}

const TERMINAL_STATES = ["completed", "failed", "interrupted", "closed"];

function update(previous: ChatMsg, activity: ChatMsg): ChatMsg {
  const input = object(previous.toolInput);
  const patch = Object.fromEntries(Object.entries(object(activity.toolInput)).filter(([, value]) => value != null));
  delete patch.agent_call_id;
  if (!text(patch.agent_state)) delete patch.agent_state;
  if (TERMINAL_STATES.includes(String(input.agent_state)) && !TERMINAL_STATES.includes(String(patch.agent_state))) delete patch.agent_state;
  return {...previous, toolInput: {...input, ...patch}};
}

export function reduceRuntimeAgents(
  state: RuntimeAgents,
  items: Record<string, ChatMsg>,
  parents: Record<string, string>,
  parent: string,
  raw: unknown,
  seq: number,
): ChatMsg[] | undefined {
  const item = object(raw);
  if (item.type !== "collabAgentToolCall" && item.type !== "subAgentActivity") return undefined;
  const spawn = item.type === "collabAgentToolCall" && item.tool === "spawnAgent";
  if (spawn && !list(item.receiverThreadIds).length) return [];
  const rows = runtimeItems(item, seq);
  return rows.flatMap(row => {
    const agent = text(object(row.toolInput).agent_id);
    if (!agent) return [row];
    const existing = state.rows[agent];
    const previous = existing ? items[existing] : undefined;
    if (previous) return [update(previous, row)];
    const started = item.type === "subAgentActivity" && item.kind === "started";
    if (!spawn && !started) return [row];
    state.rows[agent] = row.id;
    parents[agent] = parent;
    state.ordinal += 1;
    row.toolInput = {...object(row.toolInput), agent_ordinal: spawn ? object(row.toolInput).agent_ordinal : state.ordinal};
    return [row];
  });
}
