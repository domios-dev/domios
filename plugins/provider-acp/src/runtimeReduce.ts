import type { AgentEvent, ChatMsg, JsonValue, RuntimeContext, RuntimeFrame, RuntimeReduction, TurnInput } from "@codeterm/plugin-sdk";
import { object, text, reduceTool } from "./runtimeReduceTools";
import { planRow, usage } from "./runtimeReducePlan";
import { commands, configOptions } from "./runtimeControls";

export interface StreamCursor { replay:number; prompt:number; loading:boolean }
interface StreamState {
  epoch:number; nonce:string; sessionId:string; replay:number; prompt:number;
  turn:number; ordinal:number; seq:number; chunk?:{kind:string; id:string; content:string};
  items:Record<string,ChatMsg>; textById:Record<string,string>; warnings:Record<string,number>; controls:Record<string,JsonValue>;
  evidenced:string[]; compacted:string[]; seenItems:string[]; loading:boolean; settled:boolean; started:boolean;
}
const json=(value:unknown):JsonValue=>value as JsonValue;
function initial(ctx:RuntimeContext, raw:JsonValue, cursor:StreamCursor):StreamState {
  const previous=object(raw);
  if (previous.epoch !== ctx.epoch || previous.nonce !== ctx.bindingNonce || previous.sessionId !== ctx.threadId)
    return {epoch:ctx.epoch,nonce:ctx.bindingNonce,sessionId:ctx.threadId!,replay:cursor.replay,prompt:cursor.prompt,
      turn:cursor.prompt || 1,ordinal:0,seq:0,items:Object.create(null),textById:Object.create(null),warnings:Object.create(null),controls:Object.create(null),evidenced:[],compacted:[],seenItems:[],loading:cursor.loading,settled:false,started:false};
  const state=previous as unknown as StreamState;
  const copy:StreamState={...state,items:Object.assign(Object.create(null),state.items),textById:Object.assign(Object.create(null),state.textById),warnings:Object.assign(Object.create(null),state.warnings),controls:{...state.controls},
    evidenced:[...state.evidenced],compacted:[...state.compacted],seenItems:[...state.seenItems],...(state.chunk ? {chunk:{...state.chunk}} : {})};
  if (copy.replay !== cursor.replay) {
    copy.replay=cursor.replay;copy.turn=1;copy.ordinal=0;copy.seenItems=[];copy.textById=Object.create(null);delete copy.chunk;copy.settled=false;copy.started=false;
  } else if (copy.prompt !== cursor.prompt) {
    copy.turn=Math.max(copy.turn+1,cursor.prompt);copy.ordinal=0;copy.seenItems=[];copy.textById=Object.create(null);delete copy.chunk;copy.settled=false;copy.started=false;
  }
  copy.prompt=cursor.prompt;copy.loading=cursor.loading;
  return copy;
}
function emit(state:StreamState, events:AgentEvent[], row:ChatMsg):void {
  const previous=state.items[row.id];
  row.seq=previous?.seq ?? ++state.seq;
  state.items[row.id]=row;
  if (JSON.stringify(previous) !== JSON.stringify(row)) events.push({kind:"chatItem",threadId:state.sessionId,item:row});
}
function rememberControl(state:StreamState, kind:string, update:Record<string,JsonValue>):void {
  const encoded=JSON.stringify(update);
  if (encoded.length <= 16384) state.controls[kind]=JSON.parse(encoded) as JsonValue;
}
function warn(state:StreamState, events:AgentEvent[], rawKind:string):void {
  const kind=rawKind.slice(0,128);
  if (!Object.prototype.hasOwnProperty.call(state.warnings,kind) && Object.keys(state.warnings).length >= 32) return;
  const count=state.warnings[kind] ?? 0;
  state.warnings[kind]=Math.min(count+1,Number.MAX_SAFE_INTEGER);
  if (!count) events.push({kind:"notice",threadId:state.sessionId,message:`Ignored unknown ACP session update: ${kind}`});

}
function closeChunk(ctx:RuntimeContext, state:StreamState, events:AgentEvent[]):void {
  const chunk=state.chunk;
  if (chunk?.kind === "user_message_chunk" && !state.loading) {
    const claim=ctx.claimedSends.find(claim=>claim.threadId === ctx.threadId && !state.evidenced.includes(claim.sendId)
      && claim.input.attachments.length === 0 && claim.input.text === chunk.content);
    if (claim) {
      if (!events.some(event=>event.kind === "chatItem" && event.item.id === chunk.id))
        events.push({kind:"chatItem",threadId:state.sessionId,item:state.items[chunk.id]});
      state.evidenced.push(claim.sendId);
      events.push({kind:"sendEvidence",threadId:state.sessionId,sendId:claim.sendId,turnId:ctx.activeTurnId ?? `${state.sessionId}:turn:${state.turn}`,itemId:chunk.id});
    }
  }
  delete state.chunk;
}
export function settleStream(ctx:RuntimeContext, raw:JsonValue, reason:string):RuntimeReduction {
  const source=object(raw);
  if (!ctx.threadId) return {state:raw,events:[]};
  if (source.sessionId !== ctx.threadId || source.epoch !== ctx.epoch || source.nonce !== ctx.bindingNonce)
    return {state:raw,events:[{kind:"turnState",threadId:ctx.threadId,state:"idle",...(reason === "cancelled" ? {detail:"interrupted" as const} : {})}]};
  const state=source as unknown as StreamState;
  if (state.settled) return {state:raw,events:[]};
  const next:StreamState={...state,items:Object.assign(Object.create(null),state.items),evidenced:[...state.evidenced],settled:true};
  const events:AgentEvent[]=[];
  closeChunk(ctx,next,events);
  for (const row of Object.values(next.items)) if (row.toolStatus === "running")
    emit(next,events,{...row,toolStatus:"interrupted",toolInterrupted:true});
  events.push({kind:"turnState",threadId:ctx.threadId,turnId:`${state.sessionId}:turn:${state.turn}`,state:"idle",...(reason === "cancelled" ? {detail:"interrupted" as const} : {})});
  return {state:json(next),events};
}
export function clientRequest(method:string):boolean {
  return ["session/request_permission","fs/read_text_file","fs/write_text_file","terminal/create","terminal/output","terminal/wait_for_exit","terminal/kill","terminal/release"].includes(method);
}
function startTurn(ctx:RuntimeContext, state:StreamState, events:AgentEvent[]):void {
  if (!state.loading && !state.settled && state.prompt > 0 && !state.started && !ctx.activeTurnId) {
    state.started=true;
    events.push({kind:"turnState",threadId:state.sessionId,turnId:`${state.sessionId}:turn:${state.turn}`,state:"working"});
  }
}
export function reduceStream(ctx:RuntimeContext, raw:JsonValue, frames:readonly RuntimeFrame[], cursor:StreamCursor, submitted?:{sendId:string;input:TurnInput}):RuntimeReduction {
  if (!ctx.threadId) return {state:raw,events:[]};
  const state=initial(ctx,raw,cursor);
  const events:AgentEvent[]=[];
  const ownSubmitted=submitted && ctx.claimedSends.some(claim=>claim.sendId === submitted.sendId && claim.threadId === state.sessionId) ? submitted : undefined;
  for (const frame of frames) {
    const params=object(frame.params);
    if (params.sessionId !== ctx.threadId) continue;
    const submittedId=`${state.sessionId}:turn:${state.turn}:item:1`;
    if (ownSubmitted && !state.loading && !state.settled && !state.items[submittedId]) {
      state.ordinal=1;
      emit(state,events,{id:submittedId,seq:0,type:"user",content:ownSubmitted.input.text});
    }
    if (frame.kind === "request" && clientRequest(frame.method)) { startTurn(ctx,state,events);continue; }
    if (frame.kind !== "notification" || frame.method !== "session/update") continue;
    const update=object(params.update);
    const kind=text(update.sessionUpdate);
    if (!kind) continue;
    startTurn(ctx,state,events);
    if (ownSubmitted && !state.loading && !state.settled && !state.evidenced.includes(ownSubmitted.sendId)) {
      if (!events.some(event=>event.kind === "chatItem" && event.item.id === submittedId))
        events.push({kind:"chatItem",threadId:state.sessionId,item:state.items[submittedId]});
      state.evidenced.push(ownSubmitted.sendId);
      events.push({kind:"sendEvidence",threadId:state.sessionId,sendId:ownSubmitted.sendId,turnId:`${state.sessionId}:turn:${state.turn}`,itemId:submittedId});
    }
    if (kind === "user_message_chunk" || kind === "agent_message_chunk" || kind === "agent_thought_chunk") {
      const content=object(update.content);
      if (content.type !== "text" || typeof content.text !== "string" || state.settled && !state.loading) continue;
      if (kind === "user_message_chunk" && submitted && state.evidenced.includes(submitted.sendId)) continue;
      const explicit=text(update.messageId);
      if (!state.chunk || state.chunk.kind !== kind || explicit && state.chunk.id !== `${state.sessionId}:turn:${state.turn}:message:${explicit}`) {
        closeChunk(ctx,state,events);
        if (kind === "user_message_chunk" && state.loading && state.ordinal > 0) { state.turn++;state.ordinal=0;state.seenItems=[]; }
        state.ordinal++;
        const id=explicit ? `${state.sessionId}:turn:${state.turn}:message:${explicit}` : `${ctx.threadId}:turn:${state.turn}:item:${state.ordinal}`;
        state.chunk={kind,id,content:explicit ? state.textById[id] ?? "" : ""};
      }
      state.chunk.content+=content.text;
      if (explicit) state.textById[state.chunk.id]=state.chunk.content;
      const type=kind === "user_message_chunk" ? "user" : kind === "agent_thought_chunk" ? "thinking" : "assistant";
      emit(state,events,{id:state.chunk.id,seq:0,type,content:state.chunk.content,...(type === "thinking" ? {collapsed:true} : {})});
      if (!state.loading && !state.started && kind === "agent_message_chunk") events.push({kind:"turnState",threadId:ctx.threadId,turnId:ctx.activeTurnId ?? `${ctx.threadId}:turn:${state.turn}`,state:"working"});
      continue;
    }
    if (kind === "tool_call" || kind === "tool_call_update") {
      if (state.settled && !state.loading) continue;
      closeChunk(ctx,state,events);
      const id=text(update.toolCallId);
      const previous=id ? state.items[id] : undefined;
      if (id && !state.seenItems.includes(id)) {state.ordinal++;state.seenItems.push(id);}
      const row=reduceTool(update,previous,state.seq+1);
      if (row) emit(state,events,row);
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
        events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:{configOptions:configOptions(update.configOptions)}});
      if (kind === "current_mode_update" && typeof update.currentModeId === "string")
        events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:{mode:update.currentModeId}});
      const compaction=object(object(update._meta).compaction);
      if (kind !== "session_info_update" || !Object.keys(compaction).length) continue;
      const id=text(compaction.id) ?? `${ctx.threadId}:compaction:${state.turn}`;
      if (!state.compacted.includes(id)) {state.compacted.push(id);events.push({kind:"contextCompacted",threadId:ctx.threadId,itemId:id});}
    } else if (kind === "usage_update") {
      const mapped=usage(update);
      if (mapped) events.push({kind:"usage",threadId:ctx.threadId,usage:mapped});
    } else if (kind === "compaction_update" || kind === "compaction_summary_chunk") {
      const id=text(update.id) ?? `${ctx.threadId}:compaction:${state.turn}`;
      if (!state.compacted.includes(id)) { state.compacted.push(id);events.push({kind:"contextCompacted",threadId:ctx.threadId,itemId:id}); }
    } else if (kind === "agent_notice") {
      if (typeof update.message === "string" && update.message.length)
        events.push({kind:"notice",threadId:ctx.threadId,message:update.message.slice(0,4096)});
    }
    else warn(state,events,kind);
  }
  return {state:json(state),events};
}
