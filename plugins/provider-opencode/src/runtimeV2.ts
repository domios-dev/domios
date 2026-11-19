import type {Operation, RpcReply, RuntimeContext, RuntimePlan, RuntimeSettings, RuntimeStep} from "@codeterm/plugin-sdk";
import {attachmentUrl, done, fail, list, messageIdentity, modelParts, next, object, own, refused, request, text} from "./runtimeProtocol";
import {binding, claimed, loaded} from "./runtimeState";
import {v2AuthEvents, v2LoginPlan, v2LoginResponse, v2Providers} from "./runtimeV2Auth";
import {v2Answer} from "./runtimeV2Requests";
import {v2History} from "./runtimeV2Projection";

export function v2Model(settings:RuntimeSettings):Record<string,string>|undefined {
  const model=modelParts(settings.model);
  const variant=settings.effort && settings.effort !== "default" ? settings.effort : undefined;
  return model ? {providerID:model.providerID,id:model.modelID,...(variant ? {variant} : {})} : undefined;
}
function prompt(op:Extract<Operation,{kind:"submitTurn"}>, id:string):RuntimePlan {
  const plan=request("v2.session.prompt",{path:{sessionID:op.threadId},body:{id,text:op.input.text,files:op.input.attachments.map(attachment=>({uri:attachmentUrl(attachment)}))}},{stage:"prompt",id});
  if (plan.action.kind === "request") plan.action.call.target={threadId:op.threadId,turnId:id};
  return plan;
}
export function v2Plan(ctx:RuntimeContext, op:Operation):RuntimePlan {
  const state=binding(ctx);
  switch(op.kind) {
    case "initialize": return request("health",{});
    case "openThread": {
      if (ctx.threadId) fail("binding_mismatch","New OpenCode session requires an unbound runtime");
      return request("v2.session.create",{body:{location:{directory:ctx.cwd},permissions:[{action:"*",resource:"*",effect:"ask"}],...(v2Model(op.settings) ? {model:v2Model(op.settings)} : {})}});
    }
    case "resumeThread": return request("v2.session.get",{path:{sessionID:own(ctx,op.threadId)}});
    case "readHistory": {
      own(ctx,op.threadId);
      if (op.cursor && op.cursor.length > 4096) fail("invalid_schema","Invalid OpenCode history page");
      return request("v2.session.messages",{path:{sessionID:op.threadId},query:{limit:"40",...(op.cursor ? {cursor:op.cursor} : {order:op.order ?? "desc"})}});
    }
    case "submitTurn": {
      own(ctx,state.sessionId);
      if (op.activeTurnId) fail("provider_refused","OpenCode does not accept concurrent prompt turns");
      if (JSON.stringify(v2Model(op.settings)) !== JSON.stringify(v2Model(state.settings))) fail("unsupported","OpenCode model changes require restarting the same session");
      const id=messageIdentity(op.sendId,Date.now(),++state.counter);
      claimed(ctx,{sendId:op.sendId,messageId:id,turnId:id});
      return prompt(op,id);
    }
    case "answer": return v2Answer(ctx,op);
    case "control":
      if (op.action === "auth") return request("v2.provider.list",{query:{"location[directory]":ctx.cwd}},{stage:"authProviders"});
      if (op.action === "login") return v2LoginPlan(ctx,op);
      fail("unsupported",`OpenCode runtime control ${op.action} is unsupported`);
    default: fail("unsupported",`OpenCode runtime operation ${op.kind} is unsupported`);
  }
}
function directory(value:string):string {return value.replace(/\\/g,"/").replace(/\/$/,"");}
export function v2Response(ctx:RuntimeContext, op:Operation, state:Record<string,unknown>, reply:RpcReply):RuntimeStep {
  if (reply.kind === "error") {
    if (op.kind === "resumeThread" && reply.error.code === 404) return {kind:"refused",reason:{code:"thread_not_found",message:"The exact OpenCode session was not found"},beforeAcceptance:false};
    if (op.kind === "control" && op.action === "login") return done([{kind:"authLogin",login:{status:"failed",loginId:String(object(op.value).loginId),error:`Sign-in request was refused (${reply.error.code})`}}]);
    return refused(`OpenCode request was refused (${reply.error.code})`);
  }
  const envelope=object(reply.value), body=envelope.body, value=object(body), data=object(value.data);
  switch(op.kind) {
    case "initialize":
      if (state.stage === "health") {
        if (!text(value.version) || !Number.isSafeInteger(value.pid) || !Array.isArray(value.urls) || !text(object(value.paths).tmp)) return refused("OpenCode did not confirm its v2 server capabilities");
        return next("v2.provider.list",{query:{"location[directory]":ctx.cwd}},{stage:"initializeProviders",version:value.version});
      }
      if (state.stage === "initializeProviders") {v2Providers(ctx,body);return next("v2.integration.list",{query:{"location[directory]":ctx.cwd}},{stage:"initializeIntegrations",version:state.version});}
      if (state.stage !== "initializeIntegrations") return refused("Unexpected OpenCode initialization response");
      return {kind:"complete",result:{kind:"initialized",binaryVersion:String(state.version),agentName:"OpenCode",midTurnInput:false},events:v2AuthEvents(ctx,body)};
    case "openThread": case "resumeThread": {
      const setup=state.stage === "instructions" || state.stage === "model";
      const id=setup ? text(state.id) : text(data.id);
      if (!id || op.kind === "resumeThread" && id !== op.threadId) fail("binding_mismatch","OpenCode did not confirm the exact session");
      if (!setup) {
        const cwd=text(object(data.location).directory);
        if (!cwd || directory(cwd) !== directory(ctx.cwd)) fail("binding_mismatch","OpenCode session directory differs from the launch directory");
        const model=v2Model(op.settings);
        if (model && op.kind === "resumeThread") return next("v2.session.model",{path:{sessionID:id},body:{model}},{stage:"model",id});
      }
      if (state.stage !== "instructions") {
        if (setup && body !== null) return refused("OpenCode did not confirm the session model update");
        return next("v2.session.instructions",{path:{sessionID:id,key:"domios"},body:{value:op.instructions}},{stage:"instructions",id});
      }
      if (body !== null) return refused("OpenCode did not confirm the instruction entry");
      loaded(ctx,id,op.instructions,op.settings);
      return {kind:"complete",result:{kind:"threadLoaded",threadId:id,settings:op.settings}};
    }
    case "submitTurn": {
      const planned=binding(ctx).sends.find(send=>send.sendId === op.sendId && send.messageId === state.id);
      if (!planned) return refused("Prompt response has no exact runtime handoff");
      if (data.id !== planned.messageId || data.sessionID !== op.threadId || data.type !== "user" || object(data.payload).text !== op.input.text) return refused("OpenCode did not acknowledge the exact prompt inbox entry");
      const files=list(object(data.payload).files).map(object);
      if (files.length !== op.input.attachments.length || files.some((file,index)=>file.uri !== attachmentUrl(op.input.attachments[index]))) return refused("OpenCode did not acknowledge the exact prompt files");
      return {kind:"complete",result:{kind:"turnAccepted",threadId:op.threadId,sendId:op.sendId,turnId:planned.turnId}};
    }
    case "readHistory": {
      own(ctx,op.threadId);
      if (!Array.isArray(value.data) || value.data.length > 40) return refused("Invalid bounded OpenCode history response");
      const cursor=text(object(value.cursor).next);
      if (cursor && (cursor.length > 4096 || cursor === op.cursor)) return refused("OpenCode history cursor did not advance");
      const order=op.order ?? "desc";
      return {kind:"complete",result:{kind:"historyPage",threadId:op.threadId,items:v2History(value.data,order),openItems:[],order,...(cursor ? {nextCursor:cursor} : {})}};
    }
    case "answer": return reply.value === null ? done() : refused("OpenCode did not acknowledge the prompt answer");
    case "control":
      if (op.action === "auth") {
        if (state.stage === "authProviders") {v2Providers(ctx,body);return next("v2.integration.list",{query:{"location[directory]":ctx.cwd}},{stage:"authIntegrations"});}
        return done(v2AuthEvents(ctx,body));
      }
      if (op.action === "login") return v2LoginResponse(ctx,op,state,body);
      return refused("Unsupported OpenCode control response");
    default:return refused("Unsupported OpenCode runtime response");
  }
}
