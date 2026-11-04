import type {
  AgentEvent, JsonValue, Operation, RpcReply, RuntimeContext, RuntimeFailure,
  RuntimeFrame, RuntimePlan, RuntimeReduction, RuntimeStep,
} from "@codeterm/plugin-sdk";
import { clientCapabilities, methods, protocolVersion } from "./runtimeProtocol";

interface Negotiation {
  epoch:number;
  nonce:string;
  capabilities:Record<string, JsonValue>;
  authMethods:JsonValue[];
  sessionId?:string;
  instructions?:string;
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
function negotiated(ctx:RuntimeContext):Negotiation {
  const state = epochs.get(ctx.tabId);
  if (!state || state.epoch !== ctx.epoch || state.nonce !== ctx.bindingNonce)
    fail("protocol_error", "ACP initialization is required for this runtime epoch");
  touch(ctx.tabId, state);
  return state;
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
  switch (op.kind) {
    case "openThread":
      if (ctx.threadId || state.sessionId) fail("binding_mismatch", "ACP session is already bound");
      return request(methods.newSession, {cwd:ctx.cwd, mcpServers:[]});
    case "resumeThread": {
      const capabilities = state.capabilities;
      const session = object(capabilities.sessionCapabilities);
      const method = record(session.resume) ? methods.resume : capabilities.loadSession === true ? methods.load : null;
      if (!method) fail("provider_refused", "ACP agent did not advertise session/load or session/resume");
      return request(method, {sessionId:op.threadId, cwd:ctx.cwd, mcpServers:[]});
    }
    case "submitTurn": {
      const sessionId = ownSession(ctx, state);
      if (op.input.attachments.length) fail("unsupported", "ACP lifecycle supports text prompts only");
      const prompt:JsonValue[] = [];
      if (state.instructions) prompt.push({type:"text", text:state.instructions});
      prompt.push({type:"text", text:op.input.text});
      return request(methods.prompt, {sessionId, prompt});
    }
    case "interrupt": {
      const sessionId = ownSession(ctx, state);
      const turnId = op.activeTurnId ?? ctx.activeTurnId;
      if (!turnId || !ctx.activeTurns.some(turn=>turn.threadId === sessionId && turn.turnId === turnId))
        fail("binding_mismatch", "ACP cancel requires this tab's exact active turn");
      return {action:{kind:"notification", notification:{method:methods.cancel, params:{sessionId}, target:{threadId:sessionId, turnId}}}, state:{method:methods.cancel}};
    }
    default: fail("unsupported", `ACP operation is unavailable: ${op.kind}`);
  }
}

export function runtimeResponse(ctx:RuntimeContext, op:Operation, raw:JsonValue, reply:RpcReply):RuntimeStep {
  binding(ctx, op);
  if (reply.kind === "error") return refused("provider_refused", reply.error.message);
  const value = object(reply.value);
  const method = object(raw).method;
  if (op.kind === "initialize") {
    if (method !== methods.initialize || value.protocolVersion !== protocolVersion) {
      epochs.delete(ctx.tabId);
      return refused("protocol_error", "ACP agent must negotiate protocolVersion 1");
    }
    if (value.agentCapabilities !== undefined && !record(value.agentCapabilities)
      || value.authMethods !== undefined && !Array.isArray(value.authMethods))
      return refused("protocol_error", "Invalid ACP initialization response");
    remember(ctx, {epoch:ctx.epoch, nonce:ctx.bindingNonce,
      capabilities:object(value.agentCapabilities), authMethods:Array.isArray(value.authMethods) ? value.authMethods : []});
    return {kind:"complete", result:{kind:"initialized"}};
  }
  const state = negotiated(ctx);
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
    return {kind:"complete", result:{kind:"threadLoaded", threadId:sessionId, settings:op.settings}};
  }
  if (op.kind === "interrupt" && method === methods.cancel) return {kind:"complete", result:{kind:"done"}};
  if (op.kind === "submitTurn" && method === methods.prompt) {
    const sessionId = ownSession(ctx, state);
    if (typeof value.stopReason !== "string") return refused("protocol_error", "ACP prompt response is missing stopReason");
    state.instructions = undefined;
    return {kind:"complete", result:{kind:"turnAccepted", threadId:sessionId, sendId:op.sendId},
      events:[{kind:"turnState", threadId:sessionId, state:"idle"}]};
  }
  return refused("unsupported", "ACP response is unavailable");
}

export function runtimeReduce(ctx:RuntimeContext, raw:JsonValue, frames:readonly RuntimeFrame[]):RuntimeReduction {
  binding(ctx);
  const events:AgentEvent[] = [];
  const state = epochs.get(ctx.tabId);
  for (const frame of frames) {
    if (frame.kind !== "notification" || frame.method !== methods.update) continue;
    const params = object(frame.params);
    if (!ctx.threadId || params.sessionId !== ctx.threadId || !state
      || state.epoch !== ctx.epoch || state.nonce !== ctx.bindingNonce || state.sessionId !== ctx.threadId) continue;
    touch(ctx.tabId, state);
    const update = object(params.update);
    if (update.sessionUpdate === "agent_message_chunk") events.push({kind:"turnState", threadId:ctx.threadId, state:"working"});
  }
  return {state:raw, events};
}
