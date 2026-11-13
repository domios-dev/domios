import {object, count, timestamp, type UsageRecord} from "../../provider-shared/usageRecord";
export function parseUsageRecord(raw:unknown, now:number):UsageRecord|null {
  const value=object(raw), at=timestamp(value,now);
  if (value.compactMetadata !== null && typeof value.compactMetadata === "object" && !Array.isArray(value.compactMetadata)) return {kind:"compaction",timestamp:at};
  const message=object(value.message), usage=object(message.usage), input=count(usage.input_tokens);
  if (input === undefined) return null;
  const uncached=input+(count(usage.cache_creation_input_tokens) ?? 0);
  return {kind:"response",timestamp:at,response_id:typeof message.id === "string" ? `${message.id}:${typeof value.requestId === "string" ? value.requestId : ""}` : null,
    tokens:{input:uncached+(count(usage.cache_read_input_tokens) ?? 0),uncached_input:uncached,output:count(usage.output_tokens) ?? 0}};
}
