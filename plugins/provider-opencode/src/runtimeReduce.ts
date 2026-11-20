import type {AgentEvent, JsonValue, RuntimeContext, RuntimeFrame, RuntimeReduction} from "@codeterm/plugin-sdk";
import {attachmentUrl, fail, json, object, text, utf8Length} from "./runtimeProtocol";
import {partRow} from "./runtimeProjection";
import {requestPrompt} from "./runtimeRequests";
import {binding} from "./runtimeState";
import {parseUsageRecord} from "./usageRecord";
import {count} from "../../provider-shared/usageRecord";
import {v2Reduce} from "./runtimeV2Reduce";
import {runtimeErrorMessage, runtimeTurnErrorEvents} from "./turnErrors";

interface Part {seq:number;bytes:number}
interface Echo {text:boolean;files:number[];itemId?:string}
interface State {
  epoch:number;nonce:string;thread:string;seq:number;turn?:string;
  messages:Record<string,Record<string,unknown>>;parts:Record<string,Part>;echoes:Record<string,Echo>;
  evidenced:string[];completed:string[];usage:string[];
}
function stateFor(ctx:RuntimeContext, raw:JsonValue):State {
  const previous=object(raw);
  if (previous.epoch !== ctx.epoch || previous.nonce !== ctx.bindingNonce || previous.thread !== ctx.threadId)
    return {epoch:ctx.epoch,nonce:ctx.bindingNonce,thread:ctx.threadId!,seq:ctx.restoredRowOrdinal ?? 0,messages:Object.create(null),parts:Object.create(null),echoes:Object.create(null),evidenced:[],completed:[],usage:[]};
  const state=previous as unknown as State;
  return {...state,messages:Object.assign(Object.create(null),state.messages),parts:Object.fromEntries(Object.entries(state.parts).map(([id,part])=>[id,{...part}])),echoes:Object.fromEntries(Object.entries(state.echoes).map(([id,echo])=>[id,{...echo,files:[...echo.files]}])),evidenced:[...state.evidenced],completed:[...state.completed],usage:[...state.usage]};
}
function bounded<T>(map:Record<string,T>, limit:number):void {
  const keys=Object.keys(map);
  for (const key of keys.slice(0,Math.max(0,keys.length-limit))) delete map[key];
}
function ownSession(event:Record<string,unknown>):string|undefined {
  const params=object(event.properties);
  return text(params.sessionID) ?? text(object(params.part).sessionID) ?? text(object(params.info).sessionID)
    ?? (String(event.type).startsWith("session.") ? text(object(params.info).id) : undefined);
}
function currentTurn(ctx:RuntimeContext, state:State, index:number):string|undefined {
  const sequence=ctx.streamWindow?.frameSequences?.[index];
  return state.turn ?? ctx.activeTurnId ?? ctx.streamWindow?.turnRequests?.find(request=>request.written && request.afterSequence !== undefined && sequence !== undefined && sequence > request.afterSequence)?.turnId;
}
function completeTurn(state:State, turn:string):void {
  state.completed.push(turn);state.completed=state.completed.slice(-256);
  delete state.turn;
}
function evidence(ctx:RuntimeContext, state:State, part:Record<string,unknown>, sequence:number|undefined, events:AgentEvent[]):void {
  const messageId=text(part.messageID), planned=binding(ctx).sends.find(send=>send.messageId === messageId);
  if (!planned || state.evidenced.includes(planned.sendId)) return;
  const claim=ctx.claimedSends.find(claim=>claim.sendId === planned.sendId && claim.threadId === ctx.threadId);
  const witness=ctx.streamWindow?.turnRequests?.find(request=>request.sendId === planned.sendId);
  if (!claim || !witness?.written || witness.afterSequence === undefined || sequence === undefined || sequence <= witness.afterSequence) return;
  const echo=state.echoes[planned.sendId] ?? {text:false,files:[]};
  if (part.type === "text" && part.text === claim.input.text && part.synthetic !== true) {echo.text=true;echo.itemId=String(part.id);}
  if (part.type === "file") {
    const index=claim.input.attachments.findIndex(attachment=>attachment.mimeType === part.mime && attachmentUrl(attachment) === part.url);
    if (index >= 0 && !echo.files.includes(index)) echo.files.push(index);
  }
  state.echoes[planned.sendId]=echo;
  if (!echo.text || echo.files.length !== claim.input.attachments.length) return;
  state.evidenced.push(planned.sendId);
  state.evidenced=state.evidenced.slice(-256);
  delete state.echoes[planned.sendId];
  events.push({kind:"chatItem",threadId:ctx.threadId!,item:{id:planned.messageId,seq:++state.seq,type:"user",content:claim.input.text,attachments:claim.input.attachments.map(attachment=>({id:attachment.id,mimeType:attachment.mimeType,url:attachmentUrl(attachment)}))}},
    {kind:"sendEvidence",threadId:ctx.threadId!,sendId:planned.sendId,turnId:witness.turnId,itemId:planned.messageId});
}
export function runtimeReduce(ctx:RuntimeContext, raw:JsonValue, frames:readonly RuntimeFrame[]):RuntimeReduction {
  const protocol=binding(ctx).protocol;
  if (protocol === "v2" || !protocol && object(raw).protocol === "v2") return v2Reduce(ctx,raw,frames);
  if (!protocol) return {state:raw,events:[]};
  if (!ctx.threadId) return {state:raw,events:[]};
  const state=stateFor(ctx,raw), events:AgentEvent[]=[];
  for (let index=0;index<frames.length;index++) {
    const frame=frames[index], event=object(frame.params);
    if (ownSession(event) !== ctx.threadId) continue;
    const params=object(event.properties), type=event.type;
    if (frame.kind === "request") {
      const prompt=requestPrompt(ctx,frame);
      const turn=state.turn ?? ctx.activeTurnId;
      if (prompt) events.push(prompt,{kind:"turnState",threadId:ctx.threadId,...(turn ? {turnId:turn} : {}),state:"needs_input",detail:frame.method === "permission.asked" ? "awaitingApproval" : "awaitingInput"});
      continue;
    }
    if (type === "message.updated") {
      const info=object(params.info), id=text(info.id);
      if (!id || (info.role !== "user" && info.role !== "assistant")) continue;
      state.messages[id]={id,role:info.role,...(info.time ? {time:info.time} : {}),...(text(info.parentID) ? {parentID:info.parentID} : {})};
      bounded(state.messages,128);
      if (info.role === "assistant") {
        const tokens=object(info.tokens), cache=object(tokens.cache), completed=count(object(info.time).completed), usage=completed === undefined ? null : parseUsageRecord(info,Math.floor(completed/1000));
        if (completed !== undefined && !state.usage.includes(id) && usage?.kind === "response") {
          state.usage.push(id);state.usage=state.usage.slice(-256);
          events.push({kind:"usage",threadId:ctx.threadId,usage:{response_id:id,input_tokens:usage.tokens.input,output_tokens:usage.tokens.output,...(count(cache.read) !== undefined ? {cache_read_tokens:Number(cache.read)} : {}),...(count(cache.write) !== undefined ? {cache_write_tokens:Number(cache.write)} : {}),...(typeof info.cost === "number" && Number.isFinite(info.cost) && info.cost >= 0 ? {cost_micros:Math.round(info.cost*1e6)} : {})}});
        }
        if (typeof info.modelID === "string" && typeof info.providerID === "string") events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:{model:`${info.providerID}/${info.modelID}`}});
      }
      continue;
    }
    if (type === "message.part.updated") {
      const part=object(params.part), id=text(part.id), info=state.messages[String(part.messageID)];
      if (!id || !info) continue;
      if (info.role === "user") {
        evidence(ctx,state,part,ctx.streamWindow?.frameSequences?.[index],events);
        if (!binding(ctx).sends.some(send=>send.messageId === part.messageID) && part.type === "text" && typeof part.text === "string" && part.synthetic !== true)
          events.push({kind:"chatItemPatch",threadId:ctx.threadId,item:{id:String(part.messageID),seq:++state.seq,type:"user",content:part.text}});
        continue;
      }
      const known=state.parts[id], seq=known?.seq ?? ++state.seq;
      const row=partRow(part,info,seq);
      if (!row) continue;
      state.parts[id]={seq,bytes:utf8Length(row.content)};bounded(state.parts,512);
      events.push({kind:known ? "chatItemPatch" : "chatItem",threadId:ctx.threadId,item:row});
      continue;
    }
    if (type === "message.part.delta") {
      const id=text(params.partID), known=id ? state.parts[id] : undefined;
      if (!id || !known || params.field !== "text" || typeof params.delta !== "string") continue;
      events.push({kind:"chatItemAppend",threadId:ctx.threadId,itemId:id,content:params.delta,offset:known.bytes});
      known.bytes+=utf8Length(params.delta);continue;
    }
    if (type === "permission.replied" || type === "question.replied" || type === "question.rejected") {
      const id=text(params.requestID);
      if (id) events.push({kind:"promptResolved",threadId:ctx.threadId,requestId:id});
      continue;
    }
    if (type === "session.status" || type === "session.idle") {
      const status=type === "session.idle" ? "idle" : object(params.status).type;
      const turn=currentTurn(ctx,state,index);
      if (status === "busy" || status === "retry") {
        if (!turn || state.completed.includes(turn)) continue;
        state.turn=turn;events.push({kind:"turnState",threadId:ctx.threadId,turnId:turn,state:"working"});
      } else if (status === "idle" && turn && !state.completed.includes(turn)) {
        completeTurn(state,turn);
        events.push({kind:"turnState",threadId:ctx.threadId,turnId:turn,state:"idle"});
      }
      continue;
    }
    if (type === "session.error") {
      const error=object(params.error), turn=currentTurn(ctx,state,index);
      if (turn && state.completed.includes(turn)) continue;
      const interrupted=error.name === "MessageAbortedError";
      if (!interrupted) {
        const message=runtimeErrorMessage(error,"OpenCode reported a session error");
        events.push(...runtimeTurnErrorEvents(ctx.threadId,turn,turn ? ++state.seq : state.seq,message));
      }
      if (turn) completeTurn(state,turn);
      events.push({kind:"turnState",threadId:ctx.threadId,...(turn ? {turnId:turn} : {}),state:"idle",detail:interrupted ? "interrupted" : "failed"});
      if (error.name === "ProviderAuthError") events.push({kind:"auth",state:"signedOut"},{kind:"authentication",authentication:{state:"needs_auth",methods:binding(ctx).methods}});
    }
  }
  bounded(state.echoes,32);
  if (utf8Length(JSON.stringify(state)) > 240*1024) fail("size_limit","OpenCode reducer state exceeds its bounded budget");
  return {state:json(state),events};
}
