import type { ChatMsg } from "@codeterm/plugin-sdk";
import type { RuntimeItem } from "./runtimeAbandonedTurns";

// The host closes the app-server, killing the live turn, when reducer state exceeds 256 KiB.
export const STATE_TEXT_CAP = 16 * 1024;
export const STATE_TOTAL_BUDGET = 192 * 1024;
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

function capAgentPayload(input: Record<string,unknown>, cap: number): Record<string,unknown> {
  const bounded = {...input};
  const fields = (["agent_result","agent_description"] as const)
    .filter(key => typeof input[key] === "string" && (input[key] as string).length > 0)
    .sort((a,b) => (input[b] as string).length - (input[a] as string).length);
  for (const key of fields) {
    if (utf8Bytes(JSON.stringify(bounded)) <= cap) break;
    const text = bounded[key] as string;
    const body = text.startsWith(TRUNCATED) ? text.slice(TRUNCATED.length) : text;
    const room = cap - utf8Bytes(JSON.stringify({...bounded,[key]:""})) - utf8Bytes(JSON.stringify(TRUNCATED));
    // One UTF-16 unit needs at most six JSON bytes, including escaped control characters.
    const keep = Math.max(0, Math.floor(room / 6));
    let start = Math.max(0, body.length - keep);
    const unit = body.charCodeAt(start), previous = body.charCodeAt(start - 1);
    if (unit >= 0xdc00 && unit <= 0xdfff && previous >= 0xd800 && previous <= 0xdbff) start++;
    bounded[key] = TRUNCATED + body.slice(start);
  }
  // Required metadata remains counted by the aggregate and host limits, even when it cannot fit a row cap.
  return bounded;
}

export function capRow<T extends ChatMsg>(row: T, cap: number): T {
  let capped: T | undefined;
  for (const key of HEAVY) {
    const value = row[key];
    if (typeof value !== "string" || value.length <= cap) continue;
    capped ??= {...row};
    (capped as ChatMsg)[key] = capText(value, cap);
  }
  if (row.toolInput !== undefined) {
    const serialized = JSON.stringify(row.toolInput);
    const agentInput = row.toolKind === "agent" && row.toolName === "agent_activity"
      && row.toolInput !== null && typeof row.toolInput === "object" && !Array.isArray(row.toolInput);
    if ((agentInput ? utf8Bytes(serialized) : serialized.length) > cap) {
      capped ??= {...row};
      if (agentInput) capped.toolInput = capAgentPayload(row.toolInput as Record<string,unknown>,cap);
      else delete capped.toolInput;
    }
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
  fitTotal(state);
}

function utf8Bytes(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit < 0x80) bytes += 1;
    else if (unit < 0x800) bytes += 2;
    else if (unit >= 0xd800 && unit < 0xdc00) { bytes += 4; index++; }
    else bytes += 3;
  }
  return bytes;
}

const MIN_RUNNING_CAP = 1024;

/** Brings the serialized state under the plugin budget: oldest retired identities, then running tails, then all identities. */
function fitTotal(state: BudgetState): void {
  if (utf8Bytes(JSON.stringify(state)) <= STATE_TOTAL_BUDGET) return;
  const retiredOrder = () => (state.retired ?? []).filter(id => state.items[id]);
  while (retiredOrder().length > 0 && utf8Bytes(JSON.stringify(state)) > STATE_TOTAL_BUDGET) {
    const drop = retiredOrder().slice(0,Math.max(1,Math.ceil(retiredOrder().length / 4)));
    for (const id of drop) delete state.items[id];
    state.retired = retiredOrder();
  }
  for (let cap = STATE_TEXT_CAP / 2; cap >= MIN_RUNNING_CAP; cap = Math.floor(cap / 2)) {
    if (utf8Bytes(JSON.stringify(state)) <= STATE_TOTAL_BUDGET) return;
    for (const [id,row] of Object.entries(state.items)) state.items[id] = capRow(row,cap);
    for (const parts of Object.values(state.reasoning ?? {})) {
      parts.summary = parts.summary.map(part => capText(part,cap));
      parts.content = parts.content.map(part => capText(part,cap));
    }
  }
}
