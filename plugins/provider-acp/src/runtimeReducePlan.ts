import type { AgentEvent, ChatMsg, JsonValue, TokenUsage } from "@codeterm/plugin-sdk";
import { object } from "./runtimeReduceTools";

export function planRow(sessionId:string, update:Record<string,JsonValue>, previous:ChatMsg|undefined, seq:number):ChatMsg {
  const entries=Array.isArray(update.entries) ? update.entries.map(object) : [];
  return {id:`${sessionId}:plan`,seq:previous?.seq ?? seq,type:"tool_call",content:"",toolName:"update_plan",
    toolId:`${sessionId}:plan`,toolInput:{plan:entries},toolResult:JSON.stringify(entries.map(entry=>({step:entry.content,status:entry.status}))),collapsed:true};
}
const count=(value:JsonValue|undefined):value is number=>typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

function costMicros(value:JsonValue|undefined):number|undefined {
  const cost=object(value);
  if (cost.currency !== "USD" || typeof cost.amount !== "number" || !Number.isFinite(cost.amount) || cost.amount < 0) return undefined;
  const micros=Math.round(cost.amount*1_000_000);
  return count(micros) ? micros : undefined;
}

export function contextUsage(threadId:string, value:unknown):Extract<AgentEvent,{kind:"context"}>|undefined {
  const raw=object(value);
  if (!count(raw.used) || !count(raw.size)) return undefined;
  const cost=costMicros(raw.cost);
  return {kind:"context",threadId,used_tokens:raw.used,window_tokens:raw.size,...(cost !== undefined ? {cost_micros:cost} : {})};
}

export function usage(value:unknown):TokenUsage|undefined {
  const raw=object(value);
  if (!count(raw.inputTokens) || !count(raw.outputTokens)) return undefined;
  const mapped:TokenUsage={input_tokens:raw.inputTokens,output_tokens:raw.outputTokens};
  if (count(raw.cachedReadTokens)) mapped.cache_read_tokens=raw.cachedReadTokens;
  if (count(raw.cachedWriteTokens)) mapped.cache_write_tokens=raw.cachedWriteTokens;
  if (count(raw.used)) mapped.used_tokens=raw.used;
  if (count(raw.size)) mapped.window_tokens=raw.size;
  const cost=costMicros(raw.cost);
  if (cost !== undefined) mapped.cost_micros=cost;
  return mapped;
}
