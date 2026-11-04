import type { AgentEvent, JsonValue, RuntimeReduction } from "@codeterm/plugin-sdk";

const MAX_BYTES=4*1024*1024, MAX_EVENTS=1024, MAX_STATE_BYTES=256*1024;
const marker="\n[ACP row truncated: runtime reduction byte limit]";
const bytes=(value:string):number=>{let size=0;for (const char of value) {const code=char.codePointAt(0)!;size+=code<=127 ? 1 : code<=2047 ? 2 : code<=65535 ? 3 : 4;}return size;};
const size=(value:unknown):number=>bytes(JSON.stringify(value));
function body(event:AgentEvent):{id:string;content:string}|undefined {
  if (event.kind === "chatItemAppend") return {id:event.itemId,content:event.content};
  if (event.kind === "chatItem" && ["user","assistant","thinking"].includes(event.item.type)) return event.item;
}
function replace(event:AgentEvent, content:string):AgentEvent {
  return event.kind === "chatItem" ? {...event,item:{...event.item,content}} : {...event,content} as AgentEvent;
}
function prefix(value:string, budget:number):string {
  let length=0;
  for (const char of value) {
    const code=char.codePointAt(0)!;
    const cost=code<32 || char === '"' || char === "\\" ? size(char)-2 : bytes(char);
    if (cost>budget) break;
    budget-=cost;length+=char.length;
  }
  return value.slice(0,length);
}
function shift(state:JsonValue, events:AgentEvent[], index:number, id:string, removed:number):void {
  for (let next=index+1;next<events.length;next++) {
    const event=events[next];
    if (event.kind === "chatItemAppend" && event.itemId === id) events[next]={...event,offset:event.offset-removed};
  }
  const cursor=state as unknown as {chunk?:{id:string;offset:number;echo?:string};offsetById?:Record<string,{offset:number;chars:number;echo?:string}>};
  if (cursor.chunk?.id === id) {cursor.chunk={...cursor.chunk,offset:cursor.chunk.offset-removed};delete cursor.chunk.echo;}
  const previous=cursor.offsetById?.[id];
  if (previous) {cursor.offsetById![id]={...previous,offset:previous.offset-removed};delete cursor.offsetById![id].echo;}
}
export function fitTextReduction(state:JsonValue, source:AgentEvent[]):RuntimeReduction {
  const reduction={state,events:source};
  const eventBytes=size(source);
  if (source.length<=MAX_EVENTS && (eventBytes+MAX_STATE_BYTES+32<=MAX_BYTES || size(reduction)<=MAX_BYTES)) return reduction;
  const events=[...source];
  const bounded={state,events};
  if (events.length>MAX_EVENTS) {
    let thread:string|undefined;
    for (let index=events.length-1;index>=0 && events.length>=MAX_EVENTS;index--) {
      const text=body(events[index]);
      if (!text) continue;
      const event=events[index];
      if ("threadId" in event) thread=event.threadId;
      shift(state,events,index,text.id,bytes(text.content));
      events.splice(index,1);
    }
    if (thread) events.push({kind:"notice",threadId:thread,message:"ACP text omitted: runtime reduction event limit"});
  }
  let excess=size(bounded)-MAX_BYTES;
  while (excess>0) {
    let index=-1, largest=0;
    for (let next=0;next<events.length;next++) {
      const text=body(events[next]);
      if (text && size(text.content)>largest) {index=next;largest=size(text.content);}
    }
    if (index<0 || largest<=size(marker)) break;
    const text=body(events[index])!;
    const content=prefix(text.content,Math.max(0,largest-2-excess-(size(marker)-2)-64))+marker;
    events[index]=replace(events[index],content);
    shift(state,events,index,text.id,bytes(text.content)-bytes(content));
    excess=size(bounded)-MAX_BYTES;
  }
  return bounded;
}
