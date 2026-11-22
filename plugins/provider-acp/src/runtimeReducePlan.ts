import type { AgentEvent, ChatMsg, JsonValue, TokenUsage } from "@codeterm/plugin-sdk";
import { object } from "./runtimeReduceTools";

export function planRow(sessionId:string, update:Record<string,JsonValue>, previous:ChatMsg|undefined, seq:number):ChatMsg {
  const entries=Array.isArray(update.entries) ? update.entries.map(object) : [];
  return {id:`${sessionId}:plan`,seq:previous?.seq ?? seq,type:"tool_call",content:"",toolName:"update_plan",
    toolId:`${sessionId}:plan`,toolInput:{plan:entries},toolResult:JSON.stringify(entries.map(entry=>({step:entry.content,status:entry.status}))),collapsed:true};
}
const count=(value:JsonValue|undefined):value is number=>typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

function firstCount(...values:(JsonValue|undefined)[]):number|undefined {
  for (const value of values) if (count(value)) return value;
  return undefined;
}

function costMicros(raw:Record<string,JsonValue>):number|undefined {
  if (raw.cost !== undefined && raw.cost !== null) {
    const cost=object(raw.cost);
    if (cost.currency !== "USD" || typeof cost.amount !== "number" || !Number.isFinite(cost.amount) || cost.amount < 0) return undefined;
    const micros=Math.round(cost.amount*1_000_000);
    return count(micros) ? micros : undefined;
  }
  if (count(raw.costUsdTicks)) {
    const micros=Math.round(raw.costUsdTicks/10_000);
    return count(micros) ? micros : undefined;
  }
  const usd=raw.costUsd ?? raw.cost_usd;
  if (typeof usd === "number" && Number.isFinite(usd) && usd >= 0) {
    const micros=Math.round(usd*1_000_000);
    return count(micros) ? micros : undefined;
  }
  return undefined;
}

export function contextUsage(threadId:string, value:unknown):Extract<AgentEvent,{kind:"context"}>|undefined {
  const raw=object(value);
  if (!count(raw.used) || !count(raw.size)) return undefined;
  const cost=costMicros(raw);
  return {kind:"context",threadId,used_tokens:raw.used,window_tokens:raw.size,...(cost !== undefined ? {cost_micros:cost} : {})};
}

export function usage(value:unknown):TokenUsage|undefined {
  const raw=object(value);
  const input=firstCount(raw.inputTokens,raw.input_tokens);
  const output=firstCount(raw.outputTokens,raw.output_tokens);
  if (input === undefined || output === undefined) return undefined;
  const cacheRead=firstCount(raw.cachedReadTokens,raw.cached_read_tokens,raw.cache_read_input_tokens);
  const cacheWrite=firstCount(raw.cachedWriteTokens,raw.cacheCreationTokens,raw.cached_write_tokens,raw.cache_creation_tokens,raw.cache_creation_input_tokens);
  const thought=firstCount(raw.thoughtTokens,raw.reasoningTokens,raw.thought_tokens,raw.reasoning_tokens);
  const total=firstCount(raw.totalTokens,raw.total_tokens);
  const thoughtAdded=thought !== undefined && thought > 0 && (total === undefined || input+output < total) ? thought : 0;
  const outputTokens=output+thoughtAdded;
  let inputTokens=input;
  if (cacheRead !== undefined && (cacheRead > input || total !== undefined && cacheRead > 0 && input+cacheRead+(cacheWrite ?? 0)+outputTokens === total)) {
    inputTokens=input+cacheRead+(cacheWrite ?? 0);
  } else if (cacheWrite !== undefined && cacheWrite > 0 && total !== undefined && input+cacheWrite+outputTokens === total) {
    inputTokens=input+cacheWrite;
  }
  if (!count(inputTokens) || !count(outputTokens)) return undefined;
  const mapped:TokenUsage={input_tokens:inputTokens,output_tokens:outputTokens};
  if (cacheRead !== undefined) mapped.cache_read_tokens=cacheRead;
  if (cacheWrite !== undefined) mapped.cache_write_tokens=cacheWrite;
  if (count(raw.used)) mapped.used_tokens=raw.used;
  if (count(raw.size)) mapped.window_tokens=raw.size;
  const cost=costMicros(raw);
  if (cost !== undefined) mapped.cost_micros=cost;
  return mapped;
}

export function usageFromPayload(value:unknown):TokenUsage|undefined {
  const raw=object(value);
  return usage(raw) ?? usage(raw.usage) ?? usage(object(raw._meta).usage) ?? usage(object(raw._meta).tokenUsage);
}

export function usageFromReply(value:unknown):TokenUsage|undefined {
  const raw=object(value);
  return usage(raw.usage) ?? usage(object(raw._meta).usage) ?? usage(object(raw._meta).tokenUsage);
}
