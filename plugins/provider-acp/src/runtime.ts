import type {
  JsonValue, Operation, RpcReply, RuntimeContext, RuntimeFailure,
  RuntimeFrame, RuntimePlan, RuntimeReduction, RuntimeStep,
} from "@codeterm/plugin-sdk";
import {decodeAuthMethods, agentAuthMethod, type AuthMethod} from "./auth";
import { clientCapabilities, methods, protocolVersion } from "./runtimeProtocol";
import { reduceStream, settleStream, type StreamCursor } from "./runtimeReduce";
import { usage } from "./runtimeReducePlan";

interface Negotiation {
  epoch:number;
  nonce:string;
  capabilities:Record<string, JsonValue>;
  authMethods:AuthMethod[];
  authenticated?:boolean;
  sessionId?:string;
  instructions?:string;
  cursor:StreamCursor;
  projection?:JsonValue;
  settledProjection?:JsonValue;
}
const epochs = new Map<string, Negotiation>();
const object = (value:unknown):Record<string,JsonValue> =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,JsonValue> : {};
const record = (value:unknown):value is Record<string,JsonValue> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
function fail(code:RuntimeFailure["code"], message:string):never { throw {code, message} satisfies RuntimeFailure; }
function refused(code:RuntimeFailure["code"], message:string):RuntimeStep {
  return {kind:"refused", reason:{code, message}, beforeAcceptance:true};
}
function binding(ctx:RuntimeContext, op?:Operation):void {
  if (!ctx.tabId || !ctx.bindingNonce || !Number.isSafeInteger(ctx.epoch)) fail("binding_mismatch", "Missing runtime binding");
  if (op && "threadId" in op && op.threadId && ctx.threadId !== op.threadId)
    fail("binding_mismatch", "Session does not belong to this tab");
}
function negotiated(ctx:RuntimeContext):Negotiation|undefined {
  const state = epochs.get(ctx.tabId);
  if (!state || state.epoch !== ctx.epoch || state.nonce !== ctx.bindingNonce) return undefined;
  touch(ctx.tabId, state);
  return state;
}
function refusePlan(reason:string):RuntimePlan {
  return {action:{kind:"refused", reason},state:null};
}
function touch(tabId:string, state:Negotiation):void {
  epochs.delete(tabId);
  epochs.set(tabId,state);
}
function remember(ctx:RuntimeContext, state:Negotiation):void {
  epochs.delete(ctx.tabId);
  if (epochs.size >= 256) {
    const oldest = epochs.keys().next();
    if (!oldest.done) epochs.delete(oldest.value);
  }
  epochs.set(ctx.tabId,state);
}
function request(method:string, params:JsonValue):RuntimePlan {
  return {action:{kind:"request", call:{method, params}}, state:{method}};
}
function ownSession(ctx:RuntimeContext, state:Negotiation):string {
  if (!ctx.threadId || state.sessionId !== ctx.threadId) fail("binding_mismatch", "ACP session is not bound to this tab");
  return ctx.threadId;
}

export function runtimePlan(ctx:RuntimeContext, op:Operation):RuntimePlan {
  binding(ctx, op);
  if (op.kind === "initialize") {
    epochs.delete(ctx.tabId);
    return request(methods.initialize, {
      protocolVersion, clientCapabilities,
      clientInfo:{name:"domios", title:"Domios", version:ctx.hostVersion},
    });
  }
  const state = negotiated(ctx);
  if (!state) return refusePlan("ACP initialization is required for this runtime epoch");
  if (op.kind === "control" && op.action === "auth") {
    return request(methods.initialize, {protocolVersion, clientCapabilities, clientInfo:{name:"domios",title:"Domios",version:ctx.hostVersion}});
  }
  if (op.kind === "control" && op.action === "login") {
    try {
      const method = agentAuthMethod(state.authMethods, object(op.value).methodId);
      return request("authenticate", {methodId:method.id});
    } catch {return refusePlan("Authentication method was not declared or is unavailable");}
  }
  switch (op.kind) {
    case "openThread":
      if (ctx.threadId || state.sessionId) return refusePlan("ACP session is already bound");
      return request(methods.newSession, {cwd:ctx.cwd, mcpServers:[]});
    case "resumeThread": {
      const capabilities = state.capabilities;
      const session = object(capabilities.sessionCapabilities);
      const method = record(session.resume) ? methods.resume : capabilities.loadSession === true ? methods.load : null;
      if (!method) return refusePlan("ACP agent did not advertise session/load or session/resume");
      state.sessionId=op.threadId;
      state.cursor={replay:state.cursor.replay+1,prompt:0,loading:true};
      return request(method, {sessionId:op.threadId, cwd:ctx.cwd, mcpServers:[]});
    }
    case "submitTurn": {
      const sessionId = ownSession(ctx, state);
      if (op.input.attachments.length) return refusePlan("ACP lifecycle supports text prompts only");
      const prompt:JsonValue[] = [];
      if (state.instructions) prompt.push({type:"text", text:state.instructions});
      prompt.push({type:"text", text:op.input.text});
      state.cursor.prompt=Math.max(state.cursor.prompt,Number(object(state.projection).turn) || 0)+1;
      state.cursor.loading=false;
      return request(methods.prompt, {sessionId, prompt});
    }
    case "interrupt": {
      const sessionId = ownSession(ctx, state);
      const turnId = op.activeTurnId ?? ctx.activeTurnId;
      if (!turnId || !ctx.activeTurns.some(turn=>turn.threadId === sessionId && turn.turnId === turnId))
        return refusePlan("ACP cancel requires this tab's exact active turn");
      return {action:{kind:"notification", notification:{method:methods.cancel, params:{sessionId}, target:{threadId:sessionId, turnId}}}, state:{method:methods.cancel}};
    }
    default: return refusePlan(`ACP operation is unavailable: ${op.kind}`);
  }
}

export function runtimeResponse(ctx:RuntimeContext, op:Operation, raw:JsonValue, reply:RpcReply):RuntimeStep {
  binding(ctx, op);
  if (reply.kind === "error") return refused("provider_refused", reply.error.message);
  const value = object(reply.value);
  const method = object(raw).method;
  if (op.kind === "control" && op.action === "login" && method === "authenticate") {
    const next = request(methods.initialize, {protocolVersion, clientCapabilities, clientInfo:{name:"domios",title:"Domios",version:ctx.hostVersion}});
    next.state = {method:methods.initialize, authenticated:true};
    return {kind:"continue", plan:next};
  }
  if (op.kind === "initialize" || op.kind === "control" && ["auth", "login"].includes(op.action)) {
    if (method !== methods.initialize || value.protocolVersion !== protocolVersion) {
      epochs.delete(ctx.tabId);
      return refused("protocol_error", "ACP agent must negotiate protocolVersion 1");
    }
    if (value.agentCapabilities !== undefined && !record(value.agentCapabilities)
      || value.authMethods !== undefined && !Array.isArray(value.authMethods))
      return refused("protocol_error", "Invalid ACP initialization response");
    let authMethods:AuthMethod[];
    try {authMethods = decodeAuthMethods(value.authMethods);} catch {return refused("protocol_error", "Invalid ACP authentication declaration");}
    const prior = negotiated(ctx);
    const authenticated = object(raw).authenticated === true || prior?.authenticated === true;
    remember(ctx, {epoch:ctx.epoch, nonce:ctx.bindingNonce,
      capabilities:object(value.agentCapabilities), authMethods, authenticated,
      sessionId:prior?.sessionId, instructions:prior?.instructions,
      cursor:prior?.cursor ?? {replay:0,prompt:0,loading:false},
      projection:prior?.projection, settledProjection:prior?.settledProjection});
    return op.kind === "initialize" ? {kind:"complete", result:{kind:"initialized"}}
      : {kind:"complete", result:{kind:"done"},events:[{kind:"auth",state:authenticated ? "signedIn" : "unknown"}]};
  }
  const state = negotiated(ctx);
  if (!state) return refused("provider_refused", "ACP initialization is required for this runtime epoch");
  if (op.kind === "openThread" || op.kind === "resumeThread") {
    if (op.kind === "resumeThread") {
      const allowed = method === methods.load ? state.capabilities.loadSession === true
        : method === methods.resume && record(object(state.capabilities.sessionCapabilities).resume);
      if (!allowed) return refused("provider_refused", "ACP session method was not advertised");
    } else if (method !== methods.newSession) return refused("protocol_error", "Invalid ACP session response");
    const sessionId = op.kind === "openThread" ? value.sessionId : op.threadId;
    if (typeof sessionId !== "string" || !sessionId || sessionId.length > 4096
      || op.kind === "resumeThread" && value.sessionId !== undefined && value.sessionId !== sessionId)
      return refused("binding_mismatch", "ACP session identity is invalid");
    state.sessionId = sessionId;
    state.instructions = op.instructions;
    state.cursor.loading=false;
    return {kind:"complete", result:{kind:"threadLoaded", threadId:sessionId, settings:op.settings}};
  }
  if (op.kind === "interrupt" && method === methods.cancel) {
    const settled=settleStream(ctx,state.projection ?? null,"cancelled");
    state.projection=settled.state;state.settledProjection=settled.state;
    return {kind:"complete",result:{kind:"done"},events:settled.events};
  }
  if (op.kind === "submitTurn" && method === methods.prompt) {
    const sessionId = ownSession(ctx, state);
    if (typeof value.stopReason !== "string") return refused("protocol_error", "ACP prompt response is missing stopReason");
    state.instructions = undefined;
    const settled=settleStream(ctx,state.projection ?? null,value.stopReason);
    state.projection=settled.state;state.settledProjection=settled.state;
    const mapped=usage(value.usage);
    return {kind:"complete", result:{kind:"turnAccepted", threadId:sessionId, sendId:op.sendId},
      events:[...(mapped ? [{kind:"usage" as const,threadId:sessionId,usage:mapped}] : []),...settled.events]};
  }
  return refused("unsupported", "ACP response is unavailable");
}

export function runtimeReduce(ctx:RuntimeContext, raw:JsonValue, frames:readonly RuntimeFrame[]):RuntimeReduction {
  binding(ctx);
  const state=negotiated(ctx);
  if (!ctx.threadId || !state || state.sessionId !== ctx.threadId) return {state:raw,events:[]};
  const own=frames.filter(frame=>frame.kind === "notification" && frame.method === methods.update && object(frame.params).sessionId === ctx.threadId);
  if (!own.length) return {state:raw,events:[]};
  const reduction=reduceStream(ctx,state.settledProjection ?? raw,own,state.cursor);
  delete state.settledProjection;
  state.projection=reduction.state;
  return reduction;
}
