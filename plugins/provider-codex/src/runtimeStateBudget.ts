import type { AgentEvent, ChatMsg } from "@codeterm/plugin-sdk";
import type { RuntimeItem } from "./runtimeAbandonedTurns";

// The host closes the app-server, killing the live turn, when reducer state exceeds 256 KiB.
export const STATE_TEXT_CAP = 16 * 1024;
export const EVENT_TEXT_CAP = 256 * 1024;
export const RETIRED_ROW_CAP = 256;
export const ID_LIST_CAP = 128;
const TRUNCATED = "[earlier output truncated]\n";
const HEAVY = ["content","toolResult","toolDetail"] as const;

export interface BudgetState {
  seq: number;
  items: Record<string,RuntimeItem>;
  open?: string[];
  retired?: string[];
  agents?: {rows: Record<string,string>};
  reasoning?: Record<string,{summary:string[]; content:string[]}>;
  completed: string[];
  compacted: string[];
  evidenced: string[];
  questions?: string[];
  abandoned?: string[];
  childCompleted?: string[];
}

export function capText(value: string, cap: number): string {
  if (value.length <= cap) return value;
  const body = value.startsWith(TRUNCATED) ? value.slice(TRUNCATED.length) : value;
  return TRUNCATED + body.slice(body.length - (cap - TRUNCATED.length));
}

export function capRow<T extends ChatMsg>(row: T, cap: number): T {
  let capped: T | undefined;
  for (const key of HEAVY) {
    const value = row[key];
    if (typeof value !== "string" || value.length <= cap) continue;
    capped ??= {...row};
    (capped as ChatMsg)[key] = capText(value, cap);
  }
  if (row.toolInput !== undefined && JSON.stringify(row.toolInput).length > cap) {
    capped ??= {...row};
    delete capped.toolInput;
  }
  return capped ?? row;
}

export function isRetired(state: Pick<BudgetState,"retired">, id: string): boolean {
  return !!state.retired?.includes(id);
}

export function markOpen(state: BudgetState, id: string): void {
  if (!(state.open ??= []).includes(id)) state.open.push(id);
}

export function markClosed(state: BudgetState, id: string): void {
  if (state.open) state.open = state.open.filter(open => open !== id);
  if (state.reasoning) delete state.reasoning[id];
}

function retire(row: RuntimeItem): RuntimeItem {
  const slim: RuntimeItem = {id:row.id,seq:row.seq,type:row.type,content:""};
  if (row.timestamp) slim.timestamp = row.timestamp;
  if (row.toolFlowId) slim.toolFlowId = row.toolFlowId;
  if (row.toolStatus) slim.toolStatus = row.toolStatus;
  return slim;
}

function tail(values: string[] | undefined): string[] | undefined {
  return values && values.length > ID_LIST_CAP ? values.slice(values.length - ID_LIST_CAP) : values;
}

/** Keeps only rows a later frame can extend; finished rows shrink to identity and age out. */
export function compactState(state: BudgetState): void {
  const agents = new Set(Object.values(state.agents?.rows ?? {}));
  const done = new Set(state.completed);
  const open = new Set((state.open ?? []).filter(id => {
    const row = state.items[id];
    return row && (!row.toolStatus || row.toolStatus === "running") && !(row.toolFlowId && done.has(row.toolFlowId));
  }));
  state.open = [...open];
  const retired = new Set(state.retired ?? []);
  for (const [id,row] of Object.entries(state.items)) {
    if (agents.has(id)) continue;
    if (row.toolStatus === "running" || open.has(id)) {
      state.items[id] = capRow(row,STATE_TEXT_CAP);
      retired.delete(id);
    } else {
      state.items[id] = retire(row);
      retired.add(id);
    }
  }
  const order = [...retired].filter(id => state.items[id]).sort((a,b) => state.items[a].seq - state.items[b].seq);
  for (const id of order.slice(0,Math.max(0,order.length - RETIRED_ROW_CAP))) delete state.items[id];
  state.retired = order.slice(Math.max(0,order.length - RETIRED_ROW_CAP));
  for (const parts of Object.values(state.reasoning ?? {})) {
    parts.summary = parts.summary.map(part => capText(part,STATE_TEXT_CAP));
    parts.content = parts.content.map(part => capText(part,STATE_TEXT_CAP));
  }
  state.completed = tail(state.completed)!;
  state.compacted = tail(state.compacted)!;
  state.evidenced = tail(state.evidenced)!;
  state.questions = tail(state.questions);
  state.abandoned = tail(state.abandoned);
  state.childCompleted = tail(state.childCompleted);
}

export function capEvents(events: AgentEvent[]): AgentEvent[] {
  return events.map(event => event.kind === "chatItem" ? {...event,item:capRow(event.item,EVENT_TEXT_CAP)} : event);
}
