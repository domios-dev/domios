import {object, count, timestamp, type UsageRecord} from "../../provider-shared/usageRecord";
export function parseUsageRecord(raw:unknown, now:number):UsageRecord|null {
  const value=object(raw), at=timestamp(value,now);
  if (value.summary === true) return {kind:"compaction",timestamp:at};
  const tokens=object(value.tokens), cache=object(tokens.cache), input=count(tokens.input);
  if (input === undefined || tokens.cache === null || typeof tokens.cache !== "object") return null;
  const uncached=input+(count(cache.write) ?? 0);
  return {kind:"response",timestamp:at,response_id:typeof value.id === "string" ? value.id : null,
    tokens:{input:uncached+(count(cache.read) ?? 0),uncached_input:uncached,output:(count(tokens.output) ?? 0)+(count(tokens.reasoning) ?? 0)}};
}
