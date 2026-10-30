import type { AgentEvent, ChatMsg, JsonValue, RuntimeReduction } from "@codeterm/plugin-sdk";
import { capRow, capText, EVENT_TEXT_CAP } from "./runtimeStateBudget";

export const REDUCTION_BYTE_BUDGET = 4 * 1024 * 1024;
const TRUNCATED = "[earlier output truncated]\n";
const CONTINUATION = "[full output available in session history]\n";
const TEXT_FIELDS = ["content","toolResult","toolDetail","toolSummary","toolArgs","toolTitle","toolName","toolDescription"] as const;
const DATA_FIELDS = ["toolInput","toolRichResult","fileDiffs","toolEdits"] as const;

function serializedBytes(value: unknown): number {
  const serialized = JSON.stringify(value);
  let size = 0;
  for (const character of serialized) {
    const point = character.codePointAt(0)!;
    size += point < 0x80 ? 1 : point < 0x800 ? 2 : point < 0x10000 ? 3 : 4;
  }
  return size;
}

function coalesce(events: AgentEvent[]): AgentEvent[] {
  const last = new Map<string,number>();
  events.forEach((event,index) => {
    if (event.kind === "chatItem") last.set(JSON.stringify([event.threadId,event.item.id]),index);
  });
  return events.filter((event,index) => event.kind !== "chatItem" || last.get(JSON.stringify([event.threadId,event.item.id])) === index);
}

function shrinkRow(row: ChatMsg, cap: number): ChatMsg {
  const result = {...row};
  let shortened = false;
  for (const field of TEXT_FIELDS) {
    const value = result[field];
    if (typeof value === "string" && value.length > cap) {
      result[field] = capText(value,cap);
      shortened = true;
    }
  }
  let omitted = false;
  for (const field of DATA_FIELDS) {
    if (result[field] !== undefined && serializedBytes(result[field]) > cap) {
      delete result[field];
      omitted = true;
    }
  }
  if (omitted && !result.content.startsWith(TRUNCATED)) result.content = TRUNCATED + result.content;
  if ((shortened || omitted) && !result.content.includes(CONTINUATION)) result.content = CONTINUATION + result.content;
  return result;
}

export function budgetReduction(state: JsonValue, updates: AgentEvent[]): RuntimeReduction {
  let events = coalesce(updates).map(event => event.kind === "chatItem" ? {...event,item:capRow(event.item,EVENT_TEXT_CAP)} : event);
  let bytes = serializedBytes({state,events});
  for (let cap = EVENT_TEXT_CAP / 2; bytes > REDUCTION_BYTE_BUDGET && cap >= 64; cap = Math.floor(cap / 2)) {
    events = events.map(event => event.kind === "chatItem" ? {...event,item:shrinkRow(event.item,cap)} : event);
    bytes = serializedBytes({state,events});
  }
  if (bytes > REDUCTION_BYTE_BUDGET) throw new Error("runtime batch metadata exceeds output budget");
  return {state,events};
}
