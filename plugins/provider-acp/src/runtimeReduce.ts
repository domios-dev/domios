import type { AgentEvent, ChatMsg, JsonValue, RuntimeContext, RuntimeFrame, RuntimeReduction, TokenUsage, TurnInput } from "@codeterm/plugin-sdk";
import { object, text, reduceTool, validToolCallId, fileUriToPath } from "./runtimeReduceTools";
import { planRow, contextUsage, usageFromPayload } from "./runtimeReducePlan";
import { methods } from "./runtimeProtocol";
import { commands, sessionSettings } from "./runtimeControls";
import { copyAccounting, clearEntries, putEntry, removeEntry, mapBytes, mapKeys, utf8Length } from "./runtimeReduceAccounting";
import { evictedMayInclude, recordEvicted } from "./runtimeReduceSeen";

// `unattributed` marks a frame no written prompt owns: it may update rows and controls but never start a turn.
export interface StreamCursor { replay:number; prompt:number; loading:boolean; unattributed?:boolean }
// `row` names the row that continues a message whose earlier offset was evicted.
interface TextCursor { offset:number; chars:number; echo?:string; row?:string }
const OFFSET_BUDGET=16384;
const TOOL_ROW_BUDGET=32768;
interface StreamState {
  epoch:number; nonce:string; sessionId:string; replay:number; prompt:number;
  turn:number; ordinal:number; seq:number; chunk?:TextCursor & {kind:string; id:string}; turnUsage?:TokenUsage;
  items:Record<string,ChatMsg>; offsetById:Record<string,TextCursor>; warnings:Record<string,number>; controls:Record<string,JsonValue>;
  evidenced:string[]; compacted:string[]; seenItems:string[]; evictedMessages?:string; loading:boolean; settled:boolean; started:boolean;
}
const json=(value:unknown):JsonValue=>value as JsonValue;
function boundedText(value:string,maxBytes:number):{value:string;truncated:boolean} {
  if (utf8Length(value)<=maxBytes) return {value,truncated:false};
  const suffix="…";
  const suffixBytes=utf8Length(suffix);
  let output="",bytes=0;
  for (const character of value) {
    const size=utf8Length(character);
    if (bytes+size+suffixBytes>maxBytes) break;
    output+=character;bytes+=size;
  }
  return {value:output+suffix,truncated:true};
}
const jsonBytes=(value:unknown):number=>utf8Length(JSON.stringify(value));
function retainedToolRow(row:ChatMsg):ChatMsg {
  const retained:ChatMsg={id:row.id,seq:row.seq,type:row.type,content:"",...(row.toolId ? {toolId:row.toolId} : {}),collapsed:true};
  for (const key of ["toolTitle","toolName"] as const) if (typeof row[key] === "string") retained[key]=boundedText(row[key]!,128).value;
  if (row.toolKind) retained.toolKind=row.toolKind;
  if (row.toolStatus) retained.toolStatus=row.toolStatus;
  if (row.toolError !== undefined) retained.toolError=row.toolError;
  if (row.toolInterrupted !== undefined) retained.toolInterrupted=row.toolInterrupted;
  if (row.toolExitCode !== undefined) retained.toolExitCode=row.toolExitCode;
  if (typeof row.toolBackgroundTaskId === "string") retained.toolBackgroundTaskId=boundedText(row.toolBackgroundTaskId,128).value;
  if (typeof row.toolSummary === "string") retained.toolSummary=boundedText(row.toolSummary,128).value;
  if (typeof row.toolDetail === "string") retained.toolDetail=boundedText(row.toolDetail,192).value;
  if (typeof row.toolResult === "string") retained.toolResult=boundedText(row.toolResult,192).value;
  if (typeof row.toolArgs === "string") retained.toolArgs=boundedText(row.toolArgs,192).value;
  if (row.toolInput !== undefined && jsonBytes(row.toolInput)<=256) retained.toolInput=row.toolInput;
  if (row.fileDiffs !== undefined && jsonBytes(row.fileDiffs)<=384) retained.fileDiffs=row.fileDiffs;
  const locs=(row as Record<string,unknown>).toolLocations;
  if (locs !== undefined && jsonBytes(locs)<=384) (retained as Record<string,unknown>).toolLocations=locs;
  for (const key of ["toolInput","toolArgs","toolResult","toolDetail","fileDiffs","toolLocations","toolSummary"] as const) {
    if (jsonBytes(retained)<=1024) break;
    delete (retained as Record<string,unknown>)[key];
  }
  return retained;
}
function boundedToolRow(row:ChatMsg):ChatMsg {
  const result:ChatMsg={id:row.id,seq:row.seq,type:row.type,content:"",...(row.toolId ? {toolId:row.toolId} : {}),collapsed:true};
  let truncated=false;
  for (const key of ["toolTitle","toolName"] as const) if (typeof row[key] === "string") {
    const bounded=boundedText(row[key]!,256);result[key]=bounded.value;truncated ||= bounded.truncated;
  }
  if (row.toolKind) result.toolKind=row.toolKind;
  if (row.toolStatus) result.toolStatus=row.toolStatus;
  if (row.toolError !== undefined) result.toolError=row.toolError;
  if (row.toolInterrupted !== undefined) result.toolInterrupted=row.toolInterrupted;
  if (row.toolExitCode !== undefined) result.toolExitCode=row.toolExitCode;
  if (typeof row.toolBackgroundTaskId === "string") result.toolBackgroundTaskId=boundedText(row.toolBackgroundTaskId,128).value;
  if (typeof row.toolSummary === "string") {
    const bounded=boundedText(row.toolSummary,256);result.toolSummary=bounded.value;truncated ||= bounded.truncated;
  }
  if (typeof row.toolDetail === "string") {
    const bounded=boundedText(row.toolDetail,768);result.toolDetail=bounded.value;truncated ||= bounded.truncated;
  }
  if (typeof row.toolResult === "string") {
    const bounded=boundedText(row.toolResult,768);result.toolResult=bounded.value;truncated ||= bounded.truncated;
  }
  if (typeof row.toolArgs === "string") {
    const bounded=boundedText(row.toolArgs,384);result.toolArgs=bounded.value;truncated ||= bounded.truncated;
  }
  if (row.toolInput !== undefined) {
    if (jsonBytes(row.toolInput)<=2048) result.toolInput=row.toolInput;
    else truncated=true;
  }
  if (row.fileDiffs !== undefined) {
    if (jsonBytes(row.fileDiffs)<=12288) result.fileDiffs=row.fileDiffs;
    else truncated=true;
  }
  const locs=(row as Record<string,unknown>).toolLocations;
  if (locs !== undefined) {
    if (jsonBytes(locs)<=2048) (result as Record<string,unknown>).toolLocations=locs;
    else truncated=true;
  }
  const note="ACP tool details were shortened to fit the display budget.";
  if (truncated) {
    result.toolDetail=[result.toolDetail,note].filter(Boolean).join("\n");
  }
  for (const key of ["toolInput","toolArgs","toolResult","toolDetail","toolSummary","toolLocations","fileDiffs"] as const) {
    if (jsonBytes(result)<=TOOL_ROW_BUDGET) break;
    delete (result as Record<string,unknown>)[key];
    if (!result.toolDetail?.includes(note)) result.toolDetail=[result.toolDetail,note].filter(Boolean).join("\n");
  }
  return result;
}
function toolPatch(row:ChatMsg, previous:ChatMsg|undefined):ChatMsg {
  const patch:ChatMsg={id:row.id,seq:row.seq,type:row.type,content:"",...(row.toolId ? {toolId:row.toolId} : {})};
  if (!previous) return {...row};
  for (const key of Object.keys(row) as (keyof ChatMsg)[]) {
    if (["id","seq","type","content","toolId"].includes(key)) continue;
    const value=row[key];
    if (value !== undefined && JSON.stringify(value)!==JSON.stringify(previous[key]))
      (patch as Record<string,unknown>)[key]=value;
  }
  return patch;
}
function initial(ctx:RuntimeContext, raw:JsonValue, cursor:StreamCursor):StreamState {
  const previous=object(raw);
  if (previous.epoch !== ctx.epoch || previous.nonce !== ctx.bindingNonce || previous.sessionId !== ctx.threadId)
    return {epoch:ctx.epoch,nonce:ctx.bindingNonce,sessionId:ctx.threadId!,replay:cursor.replay,prompt:cursor.prompt,
      turn:cursor.prompt || 1,ordinal:0,seq:0,items:Object.create(null),offsetById:Object.create(null),warnings:Object.create(null),controls:Object.create(null),evidenced:[],compacted:[],seenItems:[],loading:cursor.loading,settled:false,started:false};
  const state=previous as unknown as StreamState;
  const copy:StreamState={...state,items:Object.assign(Object.create(null),state.items),offsetById:Object.assign(Object.create(null),state.offsetById),warnings:Object.assign(Object.create(null),state.warnings),controls:{...state.controls},
    evidenced:[...state.evidenced],compacted:[...state.compacted],seenItems:[...state.seenItems],...(state.chunk ? {chunk:{...state.chunk}} : {}),...(state.turnUsage ? {turnUsage:{...state.turnUsage}} : {})};
  copyAccounting(state,copy);
  if (copy.replay !== cursor.replay) {
    copy.replay=cursor.replay;copy.turn=1;copy.ordinal=0;copy.seenItems=[];clearEntries(copy,"offsetById");delete copy.evictedMessages;delete copy.chunk;delete copy.turnUsage;copy.settled=false;copy.started=false;
  } else if (copy.prompt !== cursor.prompt) {
    copy.turn=Math.max(copy.turn+1,cursor.prompt);copy.ordinal=0;copy.seenItems=[];clearEntries(copy,"offsetById");delete copy.evictedMessages;delete copy.chunk;delete copy.turnUsage;copy.settled=false;copy.started=false;
  }
  copy.prompt=cursor.prompt;copy.loading=cursor.loading;
  return copy;
}
function emit(state:StreamState, events:AgentEvent[], row:ChatMsg, patch=false):void {
  const previous=state.items[row.id];
  row.seq=previous?.seq ?? ++state.seq;
  const maxRowBytes=row.type === "image" ? 262144 : TOOL_ROW_BUDGET;
  if (utf8Length(JSON.stringify(row)) > maxRowBytes) {
    row=row.type === "tool_call" ? boundedToolRow(row) : {id:row.id,seq:row.seq,type:row.type,content:"[ACP row truncated: update exceeded the display budget]",
      ...(row.toolStatus ? {toolStatus:row.toolStatus} : {}),...(row.toolId ? {toolId:row.toolId} : {})};
  }
  const retained=row.type === "tool_call" ? retainedToolRow(row) : {...previous,id:row.id,seq:row.seq,type:row.type,content:"",
    ...(row.toolId ? {toolId:row.toolId} : {}),
    ...(row.toolTitle ? {toolTitle:Array.from(row.toolTitle).slice(0,256).join("")} : {}),
    ...(row.toolName ? {toolName:Array.from(row.toolName).slice(0,256).join("")} : {}),
    ...(row.toolKind ? {toolKind:row.toolKind} : {}),...(row.toolStatus ? {toolStatus:row.toolStatus} : {})};
  if (row.toolStatus === "running" && !previous && !admitToolIdentity(state,{id:row.id,seq:row.seq,type:row.type,content:"",
    ...(row.toolId ? {toolId:row.toolId} : {}),...(row.toolTitle ? {toolTitle:row.toolTitle} : {}),
    ...(row.toolName ? {toolName:row.toolName} : {}),...(row.toolKind ? {toolKind:row.toolKind} : {}),toolStatus:row.toolStatus})) {
    let exhausted:ChatMsg={...row,toolStatus:"interrupted",toolInterrupted:true,toolDetail:"[ACP tool tracking capacity exhausted]"};
    if (jsonBytes(exhausted) > TOOL_ROW_BUDGET) exhausted=boundedToolRow(exhausted);
    events.push({kind:"chatItem",threadId:state.sessionId,item:exhausted});
    return;
  }
  putEntry(state,"items",row.id,retained);
  if (patch) {
    const update=toolPatch(row,previous);
    if (Object.keys(update).some(key=>!(["id","seq","type","content","toolId"].includes(key))))
      events.push({kind:"chatItemPatch",threadId:state.sessionId,item:update});
  } else events.push({kind:"chatItem",threadId:state.sessionId,item:row});
}
function admitToolIdentity(state:StreamState, retained:ChatMsg):boolean {
  let bytes=mapBytes(state,"items")+utf8Length(JSON.stringify({[retained.id]:retained}))-1;
  const submittedId=`${state.sessionId}:turn:${state.turn}:item:1`;
  for (const [id,row] of Object.entries(state.items)) {
    if (bytes <= 16384) break;
    if (row.toolStatus === "running" || id === state.chunk?.id || id === submittedId) continue;
    bytes+=removeEntry(state,"items",id);
  }
  return bytes <= 16384;
}
/** Offsets belong to the active turn; older messages give way least recently used first, so a new message always keeps its identity. */
function rememberOffset(state:StreamState, base:string, cursor:TextCursor):void {
  removeEntry(state,"offsetById",base);
  const entry=utf8Length(JSON.stringify({[base]:cursor}));
  if (entry > OFFSET_BUDGET) return;
  let bytes=mapBytes(state,"offsetById")+entry;
  const evicted:string[]=[];
  for (const known of Object.keys(state.offsetById)) {
    if (bytes <= OFFSET_BUDGET) break;
    bytes+=removeEntry(state,"offsetById",known);
    evicted.push(known);
  }
  const filter=recordEvicted(state.evictedMessages,evicted);
  if (filter !== undefined) state.evictedMessages=filter;
  putEntry(state,"offsetById",base,cursor);
}
function rememberControl(state:StreamState, kind:string, update:Record<string,JsonValue>):void {
  const encoded=JSON.stringify(update);
  if (encoded.length <= 16384) putEntry(state,"controls",kind,JSON.parse(encoded) as JsonValue);
}
function warnInvalidToolCallId(state:StreamState, events:AgentEvent[]):void {
  const key="invalid_tool_call_id";
  if (!Object.prototype.hasOwnProperty.call(state.warnings,key) && Object.keys(state.warnings).length >= 32) return;
  const count=state.warnings[key] ?? 0;
  putEntry(state,"warnings",key,Math.min(count+1,Number.MAX_SAFE_INTEGER));
  if (!count) events.push({kind:"notice",threadId:state.sessionId,message:"Refused ACP tool update with invalid or oversized toolCallId"});
}
function warn(state:StreamState, events:AgentEvent[], rawKind:string):void {
  const kind=rawKind.slice(0,128);
  if (!Object.prototype.hasOwnProperty.call(state.warnings,kind) && Object.keys(state.warnings).length >= 32) return;
  const count=state.warnings[kind] ?? 0;
  putEntry(state,"warnings",kind,Math.min(count+1,Number.MAX_SAFE_INTEGER));
  if (!count) events.push({kind:"notice",threadId:state.sessionId,message:`Ignored unknown ACP session update: ${kind}`});

}
function closeChunk(ctx:RuntimeContext, state:StreamState, events:AgentEvent[]):void {
  const chunk=state.chunk;
  if (chunk?.kind === "user_message_chunk" && !state.loading) {
    const claim=ctx.claimedSends.find(claim=>claim.sendId === chunk.echo && claim.threadId === ctx.threadId
      && !state.evidenced.includes(claim.sendId) && claim.input.attachments.length === 0 && claim.input.text.length === chunk.chars);
    if (claim) {
      if (!events.some(event=>event.kind === "chatItem" && event.item.id === chunk.id))
        events.push({kind:"chatItem",threadId:state.sessionId,item:{...state.items[chunk.id],content:claim.input.text}});
      state.evidenced.push(claim.sendId);
      events.push({kind:"sendEvidence",threadId:state.sessionId,sendId:claim.sendId,turnId:ctx.activeTurnId ?? `${state.sessionId}:turn:${state.turn}`,itemId:chunk.id});
    }
  }
  delete state.chunk;
}
export function streamTurnUsage(raw:JsonValue):TokenUsage|undefined {
  const value=object(object(raw).turnUsage);
  return typeof value.input_tokens === "number" && typeof value.output_tokens === "number"
    ? value as unknown as TokenUsage : undefined;
}
export function settleStream(
  ctx:RuntimeContext,
  raw:JsonValue,
  reason:string,
  apiError?:{content:string; toolInput:JsonValue},
):RuntimeReduction {
  const source=object(raw);
  if (!ctx.threadId) return {state:raw,events:[]};
  const detail=reason === "cancelled" ? {detail:"interrupted" as const} : reason === "failed" ? {detail:"failed" as const} : {};
  if (source.sessionId !== ctx.threadId || source.epoch !== ctx.epoch || source.nonce !== ctx.bindingNonce)
    return {state:raw,events:[{kind:"turnState",threadId:ctx.threadId,state:"idle",...detail}]};
  const state=source as unknown as StreamState;
  if (state.settled) return {state:raw,events:[]};
  const next:StreamState={...state,items:Object.assign(Object.create(null),state.items),offsetById:Object.assign(Object.create(null),state.offsetById),controls:{...state.controls},warnings:Object.assign(Object.create(null),state.warnings),evidenced:[...state.evidenced],settled:true};
  delete next.turnUsage;
  copyAccounting(state,next);
  const events:AgentEvent[]=[];
  closeChunk(ctx,next,events);
  for (const row of Object.values(next.items)) if (row.toolStatus === "running")
    emit(next,events,{id:row.id,seq:row.seq,type:row.type,content:"",...(row.toolId ? {toolId:row.toolId} : {}),toolStatus:"interrupted",toolInterrupted:true},true);
  const turnId=`${state.sessionId}:turn:${state.turn}`;
  if (apiError) {
    emit(next,events,{
      id:`${turnId}:error`,
      seq:0,
      type:"system",
      systemSubtype:"api_error",
      toolFlowId:turnId,
      content:boundedText(apiError.content || "Provider request failed",4096).value,
      toolInput:apiError.toolInput,
    });
  }
  events.push({kind:"turnState",threadId:ctx.threadId,turnId,state:"idle",...detail});
  boundState(next);
  return {state:json(next),events};
}
export function clientRequest(method:string):boolean {
  return [methods.requestPermission,methods.elicitation,"fs/read_text_file","fs/write_text_file","terminal/create","terminal/output","terminal/wait_for_exit","terminal/kill","terminal/release"].includes(method);
}
function startTurn(ctx:RuntimeContext, state:StreamState, events:AgentEvent[]):void {
  if (!state.loading && !state.settled && state.prompt > 0 && !state.started && !ctx.activeTurnId) {
    state.started=true;
    events.push({kind:"turnState",threadId:state.sessionId,turnId:`${state.sessionId}:turn:${state.turn}`,state:"working"});
  }
}
function submittedRow(state:StreamState, events:AgentEvent[], submitted:{sendId:string;input:TurnInput}, evidenced:boolean):void {
  const id=`${state.sessionId}:turn:${state.turn}:item:1`;
  if (!state.items[id]) {
    state.ordinal=Math.max(state.ordinal,1);
    emit(state,events,{id,seq:0,type:"user",content:submitted.input.text});
  }
  if (evidenced && !state.evidenced.includes(submitted.sendId)) {
    if (!events.some(event=>event.kind === "chatItem" && event.item.id === id))
      events.push({kind:"chatItem",threadId:state.sessionId,item:{...state.items[id],content:submitted.input.text}});
    state.evidenced.push(submitted.sendId);
    events.push({kind:"sendEvidence",threadId:state.sessionId,sendId:submitted.sendId,turnId:`${state.sessionId}:turn:${state.turn}`,itemId:id});
  }
  if (submitted.input.attachments.length) {
    for (const event of events) if (event.kind === "chatItem" && event.item.id === id)
      event.item.attachments=[...submitted.input.attachments];
  }
}
const MODE_MARKER=/^\[MODE_UPDATE\] ([A-Za-z][\w-]*)$/;
/** Gemini CLI reports an approval-mode change as one whole unidentified message chunk instead of current_mode_update. */
export function sessionUpdate(raw:JsonValue|undefined):Record<string,JsonValue> {
  const update=object(raw);
  const content=object(update.content);
  if (update.sessionUpdate !== "agent_message_chunk" || update.messageId != null || content.type !== "text" || typeof content.text !== "string") return update;
  const marker=MODE_MARKER.exec(content.text);
  return marker ? {sessionUpdate:"current_mode_update",currentModeId:marker[1]} : update;
}
export function reduceStream(ctx:RuntimeContext, raw:JsonValue, frames:readonly RuntimeFrame[], cursor:StreamCursor, submitted?:{sendId:string;input:TurnInput}, retainSubmitted=false):RuntimeReduction {
  if (!ctx.threadId) return {state:raw,events:[]};
  const state=initial(ctx,raw,cursor);
  const events:AgentEvent[]=[];
  const ownSubmitted=submitted && ctx.claimedSends.some(claim=>claim.sendId === submitted.sendId && claim.threadId === state.sessionId) ? submitted : undefined;
  if (retainSubmitted && ownSubmitted && !state.loading && !state.settled) submittedRow(state,events,ownSubmitted,false);
  let bounded=false;
  for (const frame of frames) {
    const params=object(frame.params);
    if (params.sessionId !== ctx.threadId) continue;
    try {
      if (frame.kind === "notification" && frame.method === methods.extensionUpdate) {
        const update=object(params.update);
        if (text(update.sessionUpdate) === "turn_completed") {
          const context=contextUsage(ctx.threadId,update);
          if (context) events.push(context);
          const tokens=usageFromPayload(update);
          if (tokens && !state.loading && !cursor.unattributed) state.turnUsage=tokens;
        }
        continue;
      }
      if (ownSubmitted && !state.loading && !state.settled) submittedRow(state,events,ownSubmitted,false);
      if (frame.kind === "request" && clientRequest(frame.method)) { if (!cursor.unattributed) startTurn(ctx,state,events);continue; }
      if (frame.kind !== "notification" || frame.method !== "session/update") continue;
      const update=sessionUpdate(params.update);
      const kind=text(update.sessionUpdate);
      if (!kind) continue;
      if (!cursor.unattributed) startTurn(ctx,state,events);
      if (ownSubmitted && !state.loading && !state.settled) submittedRow(state,events,ownSubmitted,true);
      if (kind === "user_message_chunk" || kind === "agent_message_chunk" || kind === "agent_thought_chunk") {
        if (state.settled && !state.loading) continue;
        const content=object(update.content);
        let chunkText:string|undefined;
        if (content.type === "text" && typeof content.text === "string") {
          chunkText=content.text;
        } else if (kind === "agent_message_chunk" && content.type === "image") {
          const mime=text(content.mimeType);
          const data=text(content.data);
          if (mime && /^image\/[a-zA-Z0-9.+-]+$/.test(mime) && data && data.length <= 262144) {
            closeChunk(ctx,state,events);
            state.ordinal++;
            const id=`${ctx.threadId}:turn:${state.turn}:item:${state.ordinal}`;
            emit(state,events,{id,seq:0,type:"image",content:"",imageData:data,imageMimeType:mime});
            if (!cursor.unattributed && !state.loading && !state.started)
              events.push({kind:"turnState",threadId:ctx.threadId,turnId:ctx.activeTurnId ?? `${ctx.threadId}:turn:${state.turn}`,state:"working"});
          }
          continue;
        } else if (kind === "agent_message_chunk" && content.type === "resource_link") {
          const uri=text(content.uri);
          if (!uri) continue;
          const filePath=fileUriToPath(uri);
          const target=filePath ?? uri;
          const label=(text(content.title) ?? text(content.name) ?? target).replace(/[\[\]]/g,"");
          chunkText=`[${label}](${target})`;
        } else if (kind === "agent_message_chunk" && content.type === "resource") {
          const resource=object(content.resource);
          const mime=text(resource.mimeType);
          const blob=text(resource.blob);
          if (mime && /^image\/[a-zA-Z0-9.+-]+$/.test(mime) && blob && blob.length <= 262144) {
            closeChunk(ctx,state,events);
            state.ordinal++;
            const id=`${ctx.threadId}:turn:${state.turn}:item:${state.ordinal}`;
            emit(state,events,{id,seq:0,type:"image",content:"",imageData:blob,imageMimeType:mime});
            continue;
          }
          const uri=text(resource.uri);
          const filePath=uri ? fileUriToPath(uri) : undefined;
          const target=filePath ?? uri;
          const body=typeof resource.text === "string" ? resource.text : undefined;
          if (body && target) chunkText=`[${target}](${target})\n\n${body}`;
          else if (body) chunkText=body;
          else if (target) chunkText=`[${target}](${target})`;
          else continue;
        } else if (kind === "agent_message_chunk" && content.type === "audio") {
          chunkText=`[Audio (${text(content.mimeType) ?? "audio"})]`;
        } else {
          continue;
        }
        if (kind === "user_message_chunk" && submitted && state.evidenced.includes(submitted.sendId)) continue;
        const messageId=text(update.messageId);
        const explicit=messageId ? `${kind}:${messageId}` : undefined;
        const base=explicit ? `${state.sessionId}:turn:${state.turn}:message:${explicit}` : undefined;
        if (!state.chunk || state.chunk.kind !== kind || base && state.chunk.id !== (state.offsetById[base]?.row ?? base)) {
          closeChunk(ctx,state,events);
          if (kind === "user_message_chunk" && state.loading && state.ordinal > 0) {
            // Message identities embed the turn, so a replayed turn's offsets can never match again.
            state.turn++;state.ordinal=0;state.seenItems=[];clearEntries(state,"offsetById");delete state.evictedMessages;
          }
          state.ordinal++;
          const key=explicit ? `${state.sessionId}:turn:${state.turn}:message:${explicit}` : undefined;
          const known=key ? state.offsetById[key] : undefined;
          const resumed=key !== undefined && !known && evictedMayInclude(state.evictedMessages,key);
          const id=!key ? `${ctx.threadId}:turn:${state.turn}:item:${state.ordinal}`
            : known?.row ?? (resumed ? `${key}:continued:${state.seq+1}` : key);
          state.chunk={kind,id,offset:known?.offset ?? 0,chars:known?.chars ?? 0,...(known?.echo ? {echo:known.echo} : {})};
        }
        const chunk=state.chunk;
        if (kind === "user_message_chunk" && !state.loading) {
          const claim=ctx.claimedSends.find(claim=>claim.threadId === ctx.threadId && !state.evidenced.includes(claim.sendId)
            && claim.input.attachments.length === 0 && (chunk.chars === 0 || claim.sendId === chunk.echo)
            && claim.input.text.slice(chunk.chars,chunk.chars+chunkText.length) === chunkText);
          if (claim) chunk.echo=claim.sendId;
          else delete chunk.echo;
        }
        chunk.chars+=chunkText.length;
        const type=kind === "user_message_chunk" ? "user" : kind === "agent_thought_chunk" ? "thinking" : "assistant";
        const previous=state.items[chunk.id];
        const row:ChatMsg={id:chunk.id,seq:previous?.seq ?? ++state.seq,type,content:"",...(type === "thinking" ? {collapsed:true} : {})};
        events.push(chunk.offset ? {kind:"chatItemAppend",threadId:state.sessionId,itemId:row.id,content:chunkText,offset:chunk.offset}
          : {kind:"chatItem",threadId:state.sessionId,item:{...row,content:chunkText}});
        chunk.offset+=utf8Length(chunkText);
        putEntry(state,"items",row.id,row);
        const key=explicit ? `${state.sessionId}:turn:${state.turn}:message:${explicit}` : undefined;
        if (key) rememberOffset(state,key,{offset:chunk.offset,chars:chunk.chars,...(chunk.echo ? {echo:chunk.echo} : {}),...(chunk.id !== key ? {row:chunk.id} : {})});
        if (!cursor.unattributed && !state.loading && !state.started && kind === "agent_message_chunk") events.push({kind:"turnState",threadId:ctx.threadId,turnId:ctx.activeTurnId ?? `${ctx.threadId}:turn:${state.turn}`,state:"working"});
        continue;
      }
      if (kind === "tool_call" || kind === "tool_call_update") {
        if (state.settled && !state.loading) continue;
        closeChunk(ctx,state,events);
        const rawId=text(update.toolCallId);
        const id=validToolCallId(rawId);
        if (!id) {
          if (rawId) warnInvalidToolCallId(state,events);
          continue;
        }
        const previous=state.items[id];
        if (!state.seenItems.includes(id)) {state.ordinal++;state.seenItems.push(id);}
        const row=reduceTool(update,previous,state.seq+1);
        if (row && (previous || kind === "tool_call")) emit(state,events,row,kind === "tool_call_update");
      } else if (kind === "plan") {
        closeChunk(ctx,state,events);
        const id=`${ctx.threadId}:plan`;
        if (!state.seenItems.includes(id)) {state.ordinal++;state.seenItems.push(id);}
        emit(state,events,planRow(ctx.threadId,update,state.items[id],state.seq+1));
      } else if (["available_commands_update","config_option_update","session_info_update","current_mode_update"].includes(kind)) {
        rememberControl(state,kind,update);
        if (kind === "available_commands_update" && Array.isArray(update.availableCommands))
          events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:{commands:commands(update.availableCommands)}});
        if (kind === "config_option_update" && Array.isArray(update.configOptions))
          events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:sessionSettings(update)});
        if (kind === "current_mode_update" && typeof update.currentModeId === "string")
          events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:{mode:update.currentModeId}});
        const compaction=object(object(update._meta).compaction);
        if (kind !== "session_info_update" || !Object.keys(compaction).length) continue;
        const id=text(compaction.id) ?? `${ctx.threadId}:compaction:${state.turn}`;
        if (!state.compacted.includes(id)) {state.compacted.push(id);events.push({kind:"contextCompacted",threadId:ctx.threadId,itemId:id});}
      } else if (kind === "usage_update" || kind === "turn_completed") {
        const mapped=contextUsage(ctx.threadId,update);
        if (mapped) events.push(mapped);
        const tokens=usageFromPayload(update);
        if (tokens && !state.loading && !cursor.unattributed) state.turnUsage=tokens;
      } else if (kind === "compaction_update" || kind === "compaction_summary_chunk") {
        const id=text(update.id) ?? `${ctx.threadId}:compaction:${state.turn}`;
        if (!state.compacted.includes(id)) { state.compacted.push(id);events.push({kind:"contextCompacted",threadId:ctx.threadId,itemId:id}); }
      } else if (kind === "agent_notice") {
        if (typeof update.message === "string" && update.message.length)
          events.push({kind:"notice",threadId:ctx.threadId,message:update.message.slice(0,4096)});
      }
      else warn(state,events,kind);
    } finally {
      boundState(state);
      bounded=true;
    }
  }
  if (!bounded) boundState(state);
  return {state:json(state),events};
}

function boundState(state:StreamState):void {
  for (const key of ["evidenced","compacted","seenItems"] as const) state[key]=state[key].slice(-32);
  let bytes=utf8Length(JSON.stringify({...state,items:{},controls:{},warnings:{},offsetById:{}}));
  for (const key of mapKeys) bytes+=mapBytes(state,key)-2;
  for (const key of ["items","controls","warnings"] as const) {
    for (const id of Object.keys(state[key])) {
      if (bytes <= 47000) break;
      const value=state[key][id];
      if (id === state.chunk?.id || key === "items" && object(value).toolStatus === "running") continue;
      bytes+=removeEntry(state,key,id);
    }
  }
  for (const key of ["evidenced","compacted","seenItems"] as const) {
    while (bytes > 47000 && state[key].length) {
      const count=state[key].length;
      const removed=state[key].shift()!;
      bytes-=utf8Length(JSON.stringify(removed))+(count>1 ? 1 : 0);
    }
  }
}
