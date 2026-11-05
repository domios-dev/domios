import type { ChatMsg, JsonValue, TokenUsage } from "@codeterm/plugin-sdk";
import { object } from "./runtimeReduceTools";

export function planRow(sessionId:string, update:Record<string,JsonValue>, previous:ChatMsg|undefined, seq:number):ChatMsg {
  const entries=Array.isArray(update.entries) ? update.entries.map(object) : [];
  return {id:`${sessionId}:plan`,seq:previous?.seq ?? seq,type:"tool_call",content:"",toolName:"update_plan",
    toolId:`${sessionId}:plan`,toolInput:{plan:entries},toolResult:JSON.stringify(entries.map(entry=>({step:entry.content,status:entry.status}))),collapsed:true};
}
export function usage(value:unknown):TokenUsage|undefined {
  const raw=object(value);
  const count=(value:JsonValue|undefined):value is number=>typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
  if (!count(raw.inputTokens) || !count(raw.outputTokens)) return undefined;
  const mapped:TokenUsage={input_tokens:raw.inputTokens,output_tokens:raw.outputTokens};
  if (count(raw.cachedReadTokens)) mapped.cache_read_tokens=raw.cachedReadTokens;
  if (count(raw.cachedWriteTokens)) mapped.cache_write_tokens=raw.cachedWriteTokens;
  if (count(raw.used)) mapped.used_tokens=raw.used;
  if (count(raw.size)) mapped.window_tokens=raw.size;
  return mapped;
}
