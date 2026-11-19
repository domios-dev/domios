import type {AgentEvent, JsonValue, Operation, RpcReply, RuntimeContext, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";
import {authEvents, loginPlan, loginResponse, methods, providerSummary} from "./runtimeAuth";
import {attachmentUrl, done, fail, messageIdentity, modelParts, next, object, own, refused, request, text} from "./runtimeProtocol";
import {historyRows} from "./runtimeProjection";
import {answerPlan} from "./runtimeRequests";
import {binding, claimed, loaded} from "./runtimeState";
import {v2Plan, v2Response} from "./runtimeV2";
export {runtimeReduce} from "./runtimeReduce";

function directory(value:string):string { return value.replace(/\\/g,"/").replace(/\/$/,""); }
function session(ctx:RuntimeContext, raw:unknown, expected?:string):string {
  const value=object(raw), id=text(value.id), cwd=text(value.directory);
  if (!id || expected && expected !== id || !cwd || directory(cwd) !== directory(ctx.cwd)) fail("binding_mismatch","OpenCode did not confirm the exact session and directory");
  return id;
}
function historyPlan(ctx:RuntimeContext, op:Extract<Operation,{kind:"readHistory"}>):RuntimePlan {
  own(ctx,op.threadId);
  if (op.order !== "desc") fail("unsupported","OpenCode runtime history requires descending pages");
  if (op.cursor && op.cursor.length > 4096) fail("invalid_schema","Invalid OpenCode history cursor");
  return request("session.messages",{path:{sessionID:op.threadId},query:{limit:"40",...(op.cursor ? {before:op.cursor} : {})}});
}
export function runtimePlan(ctx:RuntimeContext, op:Operation):RuntimePlan {
  if (!ctx.tabId || !ctx.bindingNonce || !Number.isSafeInteger(ctx.epoch) || ctx.epoch <= 0) fail("binding_mismatch","Runtime launch identity missing");
  if ("threadId" in op && op.threadId) own(ctx,op.threadId);
  if (op.kind === "interrupt") {
    const threadId=own(ctx,op.threadId);
    const turnId=op.activeTurnId ?? ctx.activeTurns?.find(turn=>turn.threadId === threadId)?.turnId;
    const plan=request("session.abort",{path:{sessionID:threadId}});
    if (plan.action.kind === "request") plan.action.call.target={threadId,...(turnId ? {turnId} : {})};
    return plan;
  }
  const state=binding(ctx);
  if (!state.protocol && op.kind === "resumeThread") return request("health",{},{stage:"recoverProtocol"});
  if (state.protocol === "v2" && op.kind !== "initialize") return v2Plan(ctx,op);
  switch (op.kind) {
    case "initialize": return request("health",{});
    case "openThread":
      if (ctx.threadId) fail("binding_mismatch","New OpenCode session requires an unbound runtime");
      return request("session.create",{body:{permission:[{permission:"*",pattern:"*",action:"ask"}]}});
    case "resumeThread": return request("session.get",{path:{sessionID:op.threadId}});
    case "readHistory": return historyPlan(ctx,op);
    case "submitTurn": {
      own(ctx,state.sessionId);
      if (op.activeTurnId) fail("provider_refused","OpenCode does not accept concurrent prompt turns");
      if (state.sends.some(send=>send.sendId === op.sendId)) fail("provider_refused","Prompt handoff already planned; replay refused");
      const messageId=messageIdentity(op.sendId,Date.now(),++state.counter), model=modelParts(op.settings.model);
      claimed(ctx,{sendId:op.sendId,messageId,turnId:messageId});
      const plan=request("session.prompt",{path:{sessionID:op.threadId},body:{messageID:messageId,
        parts:[{type:"text",text:op.input.text},...op.input.attachments.map(attachment=>({type:"file",mime:attachment.mimeType,url:attachmentUrl(attachment)}))],
        ...(model ? {model} : {}),...(op.settings.effort ? {variant:op.settings.effort} : {}),...(state.instructions ? {system:state.instructions} : {}),
        tools:{...(ctx.hostOwned?.questions ? {question:false} : {}),...(ctx.hostOwned?.todos ? {todoread:false,todowrite:false} : {})}}}, {stage:"session.prompt",messageId});
      if (plan.action.kind === "request") plan.action.call.target={threadId:op.threadId,turnId:messageId};
      return plan;
    }
    case "answer": return answerPlan(ctx,op);
    case "control": {
      if (op.action === "auth") return request("provider.list",{}, {stage:"authProviders"});
      if (op.action === "login") return loginPlan(ctx,op);
      fail("unsupported",`OpenCode runtime control ${op.action} is unsupported`);
    }
    default: fail("unsupported",`OpenCode runtime operation ${op.kind} is unsupported`);
  }
}
export function runtimeResponse(ctx:RuntimeContext, op:Operation, raw:JsonValue, reply:RpcReply):RuntimeStep {
  const state=object(raw);
  if (op.kind === "interrupt" && reply.kind === "success") {
    const body=object(reply.value).body;
    const protocol=binding(ctx).protocol;
    if (protocol !== "v1" && typeof object(body).interrupted === "boolean") return done();
    if (protocol === "v2" || body !== true) return refused("OpenCode did not confirm the interrupt");
    const events:AgentEvent[]=op.activeTurnId ? [{kind:"turnState",threadId:op.threadId,turnId:op.activeTurnId,state:"idle",detail:"interrupted"}] : [];
    return done(events);
  }
  if (op.kind === "resumeThread" && state.stage === "recoverProtocol" && reply.kind === "success") {
    const protocol=healthProtocol(object(object(reply.value).body));
    if (!protocol) return refused("OpenCode did not confirm authenticated server health");
    binding(ctx).protocol=protocol;
    return {kind:"continue",plan:runtimePlan(ctx,op)};
  }
  if (binding(ctx).protocol === "v2") return v2Response(ctx,op,state,reply);
  if (reply.kind === "error") {
    if (op.kind === "control" && op.action === "login" && text(object(op.value).loginId)) return done([{kind:"authLogin",login:{status:"failed",loginId:String(object(op.value).loginId),error:`Sign-in request was refused (${reply.error.code})`}}]);
    if (op.kind === "resumeThread" && reply.error.code === 404) return {kind:"refused",reason:{code:"thread_not_found",message:"The exact OpenCode session was not found"},beforeAcceptance:false};
    return refused(`OpenCode request was refused (${reply.error.code})`);
  }
  const envelope=object(reply.value), body=envelope.body;
  switch (op.kind) {
    case "initialize": {
      if (state.stage === "health") {
        const health=object(body);
        const protocol=healthProtocol(health);
        if (!protocol) return refused("OpenCode did not confirm authenticated server health");
        binding(ctx).protocol=protocol;
        if (protocol === "v2") return v2Response(ctx,op,state,reply);
        return next("provider.list",{}, {stage:"initializeProviders",version:health.version});
      }
      if (state.stage === "initializeProviders") return next("provider.auth",{}, {stage:"initializeMethods",version:state.version,providers:providerSummary(body)});
      if (state.stage !== "initializeMethods") return refused("Unexpected initialization response");
      binding(ctx).methods=methods(ctx,state.providers,body);
      return {kind:"complete",result:{kind:"initialized",binaryVersion:String(state.version),agentName:"OpenCode",midTurnInput:false},events:authEvents(ctx,state.providers,body)};
    }
    case "openThread": case "resumeThread": {
      const id=session(ctx,body,op.kind === "resumeThread" ? op.threadId : undefined);
      loaded(ctx,id,op.instructions,op.settings);
      return {kind:"complete",result:{kind:"threadLoaded",threadId:id,settings:op.settings}};
    }
    case "submitTurn": {
      if (body !== null) return refused("OpenCode async prompt acknowledgement was not empty");
      const planned=binding(ctx).sends.find(send=>send.sendId === op.sendId && send.messageId === state.messageId);
      if (!planned) return refused("Prompt response has no exact runtime handoff");
      return {kind:"complete",result:{kind:"turnAccepted",threadId:op.threadId,sendId:op.sendId,turnId:planned.turnId}};
    }
    case "readHistory": {
      own(ctx,op.threadId);
      const rows=historyRows(op.threadId,body);
      const cursor=text(object(envelope.headers)["x-next-cursor"]);
      if (cursor && (cursor.length > 4096 || cursor === op.cursor)) return refused("OpenCode history cursor did not advance");
      return {kind:"complete",result:{kind:"historyPage",threadId:op.threadId,items:rows,openItems:[],order:"desc",...(cursor ? {nextCursor:cursor} : {})}};
    }
    case "answer": return done();
    case "control": {
      if (op.action === "login") return loginResponse(ctx,op,state,body);
      if (op.action === "auth") {
        if (state.stage === "authProviders") return next("provider.auth",{}, {stage:"authMethods",providers:providerSummary(body)});
        if (state.stage !== "authMethods") return refused("Unexpected auth response");
        binding(ctx).methods=methods(ctx,state.providers,body);
        return done(authEvents(ctx,state.providers,body));
      }
      return refused("Unsupported OpenCode control response");
    }
    default: return refused("Unsupported OpenCode runtime response");
  }
}

function healthProtocol(health:Record<string,unknown>):"v1"|"v2"|undefined {
  if (!text(health.version)) return undefined;
  if (Number.isSafeInteger(health.pid) && Array.isArray(health.urls) && text(object(health.paths).tmp)) return "v2";
  if (health.healthy === true) return "v1";
  return undefined;
}
