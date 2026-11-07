import type { AgentEvent, JsonValue, RuntimeReduction } from "@codeterm/plugin-sdk";
import { forgetAccounting } from "./runtimeReduceAccounting";

const omitted=(value:unknown):boolean=>value === undefined || typeof value === "function" || typeof value === "symbol";

const MAX_BYTES=4*1024*1024-1024, MAX_EVENTS=1024, MAX_STATE_BYTES=256*1024;
const marker="\n[ACP row truncated: runtime reduction byte limit]";
const utf8=(code:number):number=>code<=127 ? 1 : code<=2047 ? 2 : code<=65535 ? 3 : 4;
const escaped=(code:number):number=>code<32 ? (code===8 || code===9 || code===10 || code===12 || code===13) ? 2 : 6
  : code===34 || code===92 ? 2 : code>=0xd800 && code<=0xdfff ? 6 : utf8(code);
function stringBytes(value:string):{json:number;raw:number} {
  let json=2, raw=0;
  for (let index=0;index<value.length;) {
    const code=value.codePointAt(index)!;
    json+=escaped(code);raw+=utf8(code);index+=code>65535 ? 2 : 1;
  }
  return {json,raw};
}
function size(value:unknown):number {
  if (omitted(value)) return 0;
  if (typeof value === "string") return stringBytes(value).json;
  if (typeof value === "number") return Number.isFinite(value) ? String(value).length : 4;
  if (value === null) return 4;
  if (typeof value === "boolean") return value ? 4 : 5;
  if (typeof value !== "object") return MAX_BYTES+1;
  if (Array.isArray(value)) return 2+Math.max(0,value.length-1)+value.reduce((sum,entry)=>sum+(omitted(entry) ? 4 : size(entry)),0);
  const entries=Object.entries(value).filter(([,entry])=>!omitted(entry));
  return 2+Math.max(0,entries.length-1)+entries.reduce((sum,[key,entry])=>sum+stringBytes(key).json+1+size(entry),0);
}
function upperBound(value:unknown):number {
  if (omitted(value)) return 0;
  if (typeof value === "string") return value.length*6+2;
  if (typeof value === "number") return 32;
  if (value === null || typeof value === "boolean") return 5;
  if (typeof value !== "object") return MAX_BYTES+1;
  let total=2;
  if (Array.isArray(value)) {
    for (const entry of value) {total+=(omitted(entry) ? 4 : upperBound(entry))+1;if (total>MAX_BYTES) break;}
  } else {
    for (const [key,entry] of Object.entries(value)) {if (omitted(entry)) continue;total+=key.length*6+4+upperBound(entry);if (total>MAX_BYTES) break;}
  }
  return total;
}
function body(event:AgentEvent):{id:string;content:string}|undefined {
  if (event.kind === "chatItemAppend") return {id:event.itemId,content:event.content};
  if (event.kind === "chatItem" && ["user","assistant","thinking"].includes(event.item.type)) return event.item;
}
function replace(event:AgentEvent, content:string):AgentEvent {
  return event.kind === "chatItem" ? {...event,item:{...event.item,content}} : {...event,content} as AgentEvent;
}
function prefix(value:string, budget:number):{value:string;cost:number;raw:number} {
  let length=0, cost=0, raw=0;
  while (length<value.length) {
    const code=value.codePointAt(length)!, next=escaped(code);
    if (next>budget) break;
    budget-=next;cost+=next;raw+=utf8(code);length+=code>65535 ? 2 : 1;
  }
  return {value:value.slice(0,length),cost,raw};
}
function shift(state:JsonValue, events:AgentEvent[], index:number, id:string, removed:number):number {
  // Budget cuts replace cursor entries after per-frame accounting has finished.
  forgetAccounting(state);
  let delta=0;
  for (let next=index+1;next<events.length;next++) {
    const event=events[next];
    if (event.kind === "chatItemAppend" && event.itemId === id) {
      delta+=String(event.offset-removed).length-String(event.offset).length;
      events[next]={...event,offset:event.offset-removed};
    }
  }
  const cursor=state as unknown as {chunk?:{id:string;offset:number;echo?:string};offsetById?:Record<string,{offset:number;chars:number;echo?:string;row?:string}>};
  if (cursor.chunk?.id === id) {
    const before=size(cursor.chunk);
    cursor.chunk={...cursor.chunk,offset:cursor.chunk.offset-removed};delete cursor.chunk.echo;
    delta+=size(cursor.chunk)-before;
  }
  const key=Object.keys(cursor.offsetById ?? {}).find(key=>(cursor.offsetById![key].row ?? key) === id);
  const previous=key === undefined ? undefined : cursor.offsetById![key];
  if (key !== undefined && previous) {
    cursor.offsetById![key]={...previous,offset:previous.offset-removed};delete cursor.offsetById![key].echo;
    delta+=size(cursor.offsetById![key])-size(previous);
  }
  return delta;
}
export function fitTextReduction(state:JsonValue, source:AgentEvent[]):RuntimeReduction {
  if (source.length<=MAX_EVENTS && upperBound(source)+MAX_STATE_BYTES+32<=MAX_BYTES) return {state,events:source};
  const events=[...source];
  const bounded={state,events};
  if (events.length>MAX_EVENTS) {
    let thread:string|undefined;
    for (let index=events.length-1;index>=0 && events.length>=MAX_EVENTS;index--) {
      const text=body(events[index]);
      if (!text) continue;
      const event=events[index];
      if ("threadId" in event) thread=event.threadId;
      shift(state,events,index,text.id,stringBytes(text.content).raw);
      events.splice(index,1);
    }
    if (thread) events.push({kind:"notice",threadId:thread,message:"ACP text omitted: runtime reduction event limit"});
  }
  let measured=size({state,events:[]})+Math.max(0,events.length-1);
  const candidates=events.flatMap((event,index)=>{
    const text=body(event);
    if (!text) {measured+=size(event);return [];}
    const {json:cost,raw}=stringBytes(text.content);
    measured+=size(replace(event,""))-2+cost;
    return [{index,text,cost,raw}];
  });
  let excess=measured-MAX_BYTES;
  if (excess<=0) return bounded;
  const markerBytes=stringBytes(marker);
  candidates.sort((a,b)=>b.cost-a.cost || a.index-b.index);
  for (const {index,text,cost,raw} of candidates) {
    if (excess<=0) break;
    if (cost<=markerBytes.json) continue;
    const kept=prefix(text.content,Math.max(0,cost-2-excess-(markerBytes.json-2)-64));
    events[index]=replace(events[index],kept.value+marker);
    excess-=cost-(kept.cost+markerBytes.json);
    excess+=shift(state,events,index,text.id,raw-kept.raw-markerBytes.raw);
  }
  return bounded;
}
