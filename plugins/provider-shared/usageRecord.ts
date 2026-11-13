export type UsageTokens = {input:number; uncached_input:number; output:number};
export type UsageRecord =
  | {kind:"compaction"; timestamp:number}
  | {kind:"response"; timestamp:number; response_id:string|null; tokens:UsageTokens}
  | {kind:"cumulative"; timestamp:number; total:{input:number; cached_input:number; output:number; total:number}; last:UsageTokens|null};
export const object = (value:unknown):Record<string,unknown> => value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,unknown> : {};
export const count = (value:unknown):number|undefined => typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
export function timestamp(value:Record<string,unknown>, now:number):number {
  const seconds=count(value.timestamp);
  if (seconds !== undefined) return seconds;
  if (typeof value.timestamp === "string" && value.timestamp.endsWith("Z")) {
    const ms=Date.parse(value.timestamp);
    if (Number.isFinite(ms) && ms >= 0) return Math.floor(ms/1000);
  }
  const created=count(object(value.time).created);
  return created === undefined ? now : Math.floor(created/1000);
}
