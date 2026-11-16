import {object, count, timestamp, type UsageRecord, type UsageTokens} from "../../provider-shared/usageRecord";
function tokens(raw:unknown):UsageTokens|null {
  const usage=object(raw), input=count(usage.input_tokens);
  if (input === undefined) return null;
  return {input,uncached_input:Math.max(0,input-(count(usage.cached_input_tokens) ?? 0)),output:count(usage.output_tokens) ?? 0};
}
export function parseUsageRecord(raw:unknown, now:number):UsageRecord|null {
  const value=object(raw), at=timestamp(value,now);
  if (value.type === "compacted") return {kind:"compaction",timestamp:at};
  const info=object(object(value.payload).info), cumulative=object(info.total_token_usage), input=count(cumulative.input_tokens), last=tokens(info.last_token_usage);
  if (input !== undefined) {
    const output=count(cumulative.output_tokens) ?? 0;
    return {kind:"cumulative",timestamp:at,total:{input,cached_input:count(cumulative.cached_input_tokens) ?? 0,output,total:count(cumulative.total_tokens) ?? input+output},last};
  }
  return last ? {kind:"response",timestamp:at,response_id:null,tokens:last} : null;
}
