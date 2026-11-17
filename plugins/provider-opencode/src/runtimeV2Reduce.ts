import type {AgentEvent, ChatMsg, JsonValue, RuntimeContext, RuntimeFrame, RuntimeReduction} from "@codeterm/plugin-sdk";
import {attachmentUrl, fail, json, list, object, text, utf8Length} from "./runtimeProtocol";
import {binding} from "./runtimeState";
import {contentId, toolId, v2Content} from "./runtimeV2Projection";
import {v2Prompt} from "./runtimeV2Requests";
import {v2LoginEvents} from "./runtimeV2Auth";

interface Part {seq:number;bytes:number;part:Record<string,unknown>}
interface State {epoch:number;nonce:string;thread:string;seq:number;seen:string[];evidenced:string[];completed:string[];turn?:string;parts:Record<string,Part>}
function current(ctx:RuntimeContext, raw:JsonValue):State {
  const old=object(raw);
  if (old.epoch !== ctx.epoch || old.nonce !== ctx.bindingNonce || old.thread !== ctx.threadId) return {epoch:ctx.epoch,nonce:ctx.bindingNonce,thread:ctx.threadId!,seq:ctx.restoredRowOrdinal ?? 0,seen:[],evidenced:[],completed:[],parts:Object.create(null)};
  const state=old as unknown as State;
  return {...state,seen:[...state.seen],evidenced:[...state.evidenced],completed:[...state.completed],parts:Object.fromEntries(Object.entries(state.parts).map(([id,part])=>[id,{...part,part:{...part.part}}]))};
}
function witness(ctx:RuntimeContext, sequence:number|undefined, sendId?:string) {
  return ctx.streamWindow?.turnRequests?.find(value=>(!sendId || value.sendId === sendId) && value.written && value.afterSequence !== undefined && sequence !== undefined && sequence > value.afterSequence);
}
function echoed(ctx:RuntimeContext, state:State, data:Record<string,unknown>, sequence:number|undefined, events:AgentEvent[]):void {
  const id=text(data.inboxID), item=object(data.item), payload=object(item.payload);
  const planned=binding(ctx).sends.find(send=>send.messageId === id);
  if (!planned || item.type !== "user" || state.evidenced.includes(planned.sendId)) return;
  const claim=ctx.claimedSends.find(claim=>claim.sendId === planned.sendId && claim.threadId === ctx.threadId), proof=witness(ctx,sequence,planned.sendId);
  if (!claim || !proof || payload.text !== claim.input.text) return;
  const files=list(payload.files).map(object);
  if (files.length !== claim.input.attachments.length || files.some((file,index)=>file.uri !== attachmentUrl(claim.input.attachments[index]))) return;
  state.evidenced.push(planned.sendId);
  events.push({kind:"chatItem",threadId:ctx.threadId!,item:{id:planned.messageId,seq:++state.seq,type:"user",content:claim.input.text,attachments:claim.input.attachments.map(attachment=>({id:attachment.id,mimeType:attachment.mimeType,url:attachmentUrl(attachment)}))}},
    {kind:"sendEvidence",threadId:ctx.threadId!,sendId:planned.sendId,turnId:proof.turnId,itemId:planned.messageId});
}
export function v2Reduce(ctx:RuntimeContext, raw:JsonValue, frames:readonly RuntimeFrame[]):RuntimeReduction {
  const loginEvents=v2LoginEvents(ctx,frames);
  if (!ctx.threadId) return {state:raw,events:loginEvents};
  const state=current(ctx,raw), events:AgentEvent[]=[...loginEvents];
  const emit=(row:ChatMsg, known:boolean)=>events.push({kind:known ? "chatItemPatch" : "chatItem",threadId:ctx.threadId!,item:row});
  for (let index=0;index<frames.length;index++) {
    const frame=frames[index], event=object(frame.params), data=object(event.data), type=String(event.type), id=text(event.id);
    if ((text(data.sessionID) ?? text(object(data.form).sessionID)) !== ctx.threadId) continue;
    if (event.location && text(object(event.location).directory)?.replace(/\\/g,"/").replace(/\/$/,"") !== ctx.cwd.replace(/\\/g,"/").replace(/\/$/,"")) continue;
    if (!id || state.seen.includes(id)) continue;
    state.seen.push(id);state.seen=state.seen.slice(-512);
    const sequence=ctx.streamWindow?.frameSequences?.[index];
    const turn=state.turn ?? ctx.activeTurnId ?? witness(ctx,sequence)?.turnId;
    if (frame.kind === "request") {
      const prompt=v2Prompt(ctx,frame);
      if (prompt) events.push(prompt,{kind:"turnState",threadId:ctx.threadId,...(turn ? {turnId:turn} : {}),state:"needs_input",detail:frame.method === "permission.asked" ? "awaitingApproval" : "awaitingInput"});
      continue;
    }
    if (type === "session.inbox.enqueued") {echoed(ctx,state,data,sequence,events);continue;}
    if (type === "session.execution.started") {
      if (turn && !state.completed.includes(turn)) {state.turn=turn;events.push({kind:"turnState",threadId:ctx.threadId,turnId:turn,state:"working"});}
      continue;
    }
    if (["session.execution.succeeded","session.execution.failed","session.execution.interrupted"].includes(type)) {
      if (turn && !state.completed.includes(turn)) {
        state.completed.push(turn);
        const detail=type === "session.execution.failed" ? "failed" : type === "session.execution.interrupted" ? "interrupted" : undefined;
        if (detail === "failed") events.push({kind:"notice",threadId:ctx.threadId,message:String(object(data.error).message ?? "OpenCode execution failed").slice(0,4096)});
        events.push({kind:"turnState",threadId:ctx.threadId,turnId:turn,state:"idle",...(detail ? {detail} : {})});delete state.turn;
      }
      continue;
    }
    if (["permission.replied","form.replied","form.cancelled"].includes(type)) {
      const request=text(data.requestID) ?? text(data.formID) ?? text(data.id);
      if (request) events.push({kind:"promptResolved",threadId:ctx.threadId,requestId:request});
      continue;
    }
    const message=text(data.assistantMessageID);
    if (!message) continue;
    if (type === "session.step.started") {
      const model=object(data.model);
      if (text(model.providerID) && text(model.id)) events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:{model:`${model.providerID}/${model.id}`,...(text(model.variant) ? {effort:String(model.variant)} : {})}});
      continue;
    }
    if (type === "session.step.ended") {
      const tokens=object(data.tokens), cache=object(tokens.cache);
      const counts=[tokens.input,tokens.output,tokens.reasoning,cache.read,cache.write];
      if (counts.every(value=>typeof value === "number" && Number.isSafeInteger(value) && value >= 0)) events.push({kind:"usage",threadId:ctx.threadId,usage:{response_id:message,input_tokens:Number(tokens.input)+Number(cache.read),output_tokens:Number(tokens.output)+Number(tokens.reasoning),cache_read_tokens:Number(cache.read),cache_write_tokens:Number(cache.write),...(typeof data.cost === "number" && Number.isFinite(data.cost) && data.cost >= 0 ? {cost_micros:Math.round(data.cost*1e6)} : {})}});
      continue;
    }
    if (/^session\.(text|reasoning)\./.test(type) && Number.isSafeInteger(data.ordinal) && Number(data.ordinal) >= 0) {
      const rowId=contentId(message,Number(data.ordinal)), known=state.parts[rowId], kind=type.split(".")[1];
      if (type.endsWith(".delta")) {
        if (known && typeof data.delta === "string") {events.push({kind:"chatItemAppend",threadId:ctx.threadId,itemId:rowId,content:data.delta,offset:known.bytes});known.bytes+=utf8Length(data.delta);}
      } else {
        const seq=known?.seq ?? ++state.seq, content=type.endsWith(".ended") ? String(data.text ?? "") : "";
        const row=v2Content(message,{type:kind,text:content},Number(data.ordinal),seq);
        if (row) {emit(row,!!known);state.parts[rowId]={seq,bytes:utf8Length(content),part:{}};}
      }
      continue;
    }
    if (type.startsWith("session.tool.") && text(data.id)) {
      const rowId=toolId(message,String(data.id)), known=state.parts[rowId];
      if (!known && type !== "session.tool.input.started") continue;
      const seq=known?.seq ?? ++state.seq, previous=known?.part ?? {}, previousState=object(previous.state);
      const part:Record<string,unknown>={...previous,type:"tool",id:data.id,name:data.name ?? previous.name,time:{...object(previous.time)}};
      if (type === "session.tool.input.started") part.state={status:"pending",input:{}};
      else if (type === "session.tool.called") {part.state={status:"running",input:object(data.input)};object(part.time).ran=event.created;}
      else if (type === "session.tool.success" || type === "session.tool.failed") {
        part.state={...previousState,status:type.endsWith("success") ? "completed" : "error",...(data.content ? {content:data.content} : {}),...(data.error ? {error:data.error} : {})};object(part.time).completed=event.created;
      } else if (type === "session.tool.progress") part.state={...previousState,metadata:data.metadata};
      else continue;
      const row=v2Content(message,part,0,seq);
      if (row) {emit(row,!!known);state.parts[rowId]={seq,bytes:0,part};}
    }
  }
  state.evidenced=state.evidenced.slice(-256);state.completed=state.completed.slice(-256);
  const keys=Object.keys(state.parts);for (const key of keys.slice(0,Math.max(0,keys.length-256))) delete state.parts[key];
  if (utf8Length(JSON.stringify(state)) > 240*1024) fail("size_limit","OpenCode reducer state exceeds its bounded budget");
  return {state:json(state),events};
}
