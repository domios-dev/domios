import type {
  JsonValue, Operation, RpcReply, RuntimeContext, RuntimeFailure,
  RuntimeFrame, RuntimePlan, RuntimeReduction, RuntimeStep, TokenUsage,
} from "@codeterm/plugin-sdk";
import {decodeAuthMethods, agentAuthMethod, type AuthMethod} from "./auth";
import { methods, protocolVersion } from "./runtimeProtocol";
import { clientRequest, reduceStream, settleStream, type StreamCursor } from "./runtimeReduce";
import { usage } from "./runtimeReducePlan";
import { fitTextReduction } from "./runtimeReduceBudget";
import { runtimePrompt, runtimeAnswer } from "./runtimePrompts";
import { authentication, sessionSettings, modelOption, effortOption, label } from "./runtimeControls";
import { launchSelection } from "./launchSelection";

interface Negotiation {
  epoch:number;
  nonce:string;
  capabilities:Record<string, JsonValue>;
  authMethods:AuthMethod[];
  authenticated?:boolean;
  required?:boolean;
  settings?:import("@codeterm/plugin-sdk").RuntimeSettings;
  sessionId?:string;
  sessionActive?:boolean;
  instructions?:string;
  cursor:StreamCursor;
  inFlight:boolean;
  promptOrdinal?:number;
  queued?:{prompt:number;sendId:string;completed?:{cancelled:boolean;usage?:TokenUsage}}[];
  mcpUnavailable?:string;
  projection?:JsonValue;
  promptSend?:{sendId:string};
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

function agentIdentity(raw:unknown):{agentName?:string;binaryVersion?:string} {
  const info=object(raw);
  if (!label(info.name) || !label(info.version) || info.title !== undefined && !label(info.title)) return {};
  return {agentName:typeof info.title === "string" ? info.title : info.name,binaryVersion:info.version};
}
function sessionAuthentication(state:Negotiation):import("@codeterm/plugin-sdk").AgentEvent[] {
  if (!state.authMethods.length || state.authenticated === true && state.required !== true) return [];
  state.authenticated=true;
  state.required=false;
  return [{kind:"authentication",authentication:authentication(state.authMethods,true,false)}];
}
function threadNotices(state:Negotiation, threadId:string, events:import("@codeterm/plugin-sdk").AgentEvent[]) {
  if (state.mcpUnavailable) {
    events.push({kind:"notice",threadId,message:state.mcpUnavailable});
    delete state.mcpUnavailable;
  }
  return events.length ? {events} : {};
}

function loadedThread(op:Extract<Operation,{kind:"openThread"|"resumeThread"}>, state:Negotiation, sessionId:string, attempted:string[]=[], confirmAuthentication=true):RuntimeStep {
  const selection=launchSelection(op.settings,state.settings,attempted);
  if (selection.kind === "refused") return refused("provider_refused",selection.message);
  if (selection.kind === "set") {
    const plan=request("session/set_config_option",{sessionId,configId:selection.id,value:selection.value});
    plan.state={method:"session/set_config_option",attempted:[...attempted,selection.category],confirmAuthentication};
    return {kind:"continue",plan};
  }
  return {kind:"complete",result:{kind:"threadLoaded",threadId:sessionId,settings:state.settings ?? {}},
    ...threadNotices(state,sessionId,confirmAuthentication ? sessionAuthentication(state) : [])};
}

function clientCapabilities(ctx:RuntimeContext):JsonValue {
  return {fs:{readTextFile:ctx.clientBridges?.fsRead === true,writeTextFile:ctx.clientBridges?.fsWrite === true},terminal:ctx.clientBridges?.terminal === true};
}

function sessionRequest(ctx:RuntimeContext, method:string, sessionId?:string):RuntimePlan {
  const mcpServers=hostMcpServers(ctx);
  const plan=request(method, {...(sessionId ? {sessionId} : {}),cwd:ctx.cwd,mcpServers});
  plan.state={method,mcpOffered:mcpServers.length > 0};
  return plan;
}

function hostMcpServers(ctx:RuntimeContext):JsonValue[] {
  const descriptor=ctx.hostMcp;
  return descriptor
    ? [{name:descriptor.name,command:descriptor.command,args:[...descriptor.args],env:Object.entries(descriptor.env ?? {}).sort(([a],[b])=>a < b ? -1 : a > b ? 1 : 0).map(([name,value])=>({name,value}))}] : [];
}

export function runtimePlan(ctx:RuntimeContext, op:Operation):RuntimePlan {
  binding(ctx, op);
  if (op.kind === "initialize") {
    epochs.delete(ctx.tabId);
    return request(methods.initialize, {
      protocolVersion, clientCapabilities:clientCapabilities(ctx),
      clientInfo:{name:"domios", title:"Domios", version:ctx.hostVersion},
    });
  }
  const state = negotiated(ctx);
  if (!state) return refusePlan("ACP initialization is required for this runtime epoch");
  if (op.kind === "control" && op.action === "auth") {
    return request(methods.initialize, {protocolVersion, clientCapabilities:clientCapabilities(ctx), clientInfo:{name:"domios",title:"Domios",version:ctx.hostVersion}});
  }
  if (op.kind === "control" && op.action === "login") {
    try {
      const method = agentAuthMethod(state.authMethods, object(op.value).methodId);
      return request("authenticate", {methodId:method.id});
    } catch {return refusePlan("Authentication method was not declared or is unavailable");}
  }
  if (op.kind === "control" && op.action === "settings") {
    const sessionId=ownSession(ctx,state);
    const value=object(op.value);
    const option=modelOption(state.settings);
    if (value.model === undefined) return refusePlan("Settings control requires a model change");
    if (value.model === option?.currentValue) return refusePlan("Settings already match the reported session model");
    if (!option) return refusePlan("This agent declared no model option");
    if (!option.options.some(entry=>entry.value === value.model)) return refusePlan("Model was not declared by this agent");
    return request("session/set_config_option",{sessionId,configId:option.id,value:value.model});
  }
  if (op.kind === "control" && ["config", "mode"].includes(op.action)) {
    const sessionId=ownSession(ctx,state);
    const value=object(op.value);
    if (op.action === "mode") {
      if (typeof op.value !== "string" || !state.settings?.modes?.some(mode=>mode.id === op.value))
        return refusePlan("Session mode was not advertised");
      return request("session/set_mode",{sessionId,modeId:op.value});
    }
    const option=state.settings?.configOptions?.find(option=>option.id === value.id);
    if (!option?.options.some(option=>option.value === value.value)) return refusePlan("Configuration value was not advertised");
    return request("session/set_config_option",{sessionId,configId:option.id,value:value.value});
  }
  switch (op.kind) {
    case "answer": {
      ownSession(ctx,state);
      const pending=ctx.pendingRequest;
      if (!pending || pending.requestId !== op.requestId) return refusePlan("Unknown permission request");
      return {action:{kind:"reply",requestId:op.requestId,result:{kind:"success",value:runtimeAnswer(ctx,pending.method,pending.params,op.requestId,op.answer)}},state:{method:pending.method}};
    }
    case "openThread":
      if (ctx.threadId || state.sessionId) return refusePlan("ACP session is already bound");
      return sessionRequest(ctx,methods.newSession);
    case "resumeThread": {
      if (state.sessionId && state.sessionId !== op.threadId) return refusePlan("ACP session does not belong to this runtime");
      if (state.sessionActive && state.sessionId === op.threadId)
        return {action:{kind:"local"},state:{method:"boundSession"}};
      const capabilities = state.capabilities;
      const session = object(capabilities.sessionCapabilities);
      const method = record(session.resume) ? methods.resume : capabilities.loadSession === true ? methods.load : null;
      if (!method) return refusePlan("ACP agent did not advertise session/load or session/resume");
      state.sessionActive=false;
      state.sessionId=op.threadId;
      state.cursor={replay:state.cursor.replay+1,prompt:0,loading:true};
      return sessionRequest(ctx,method,op.threadId);
    }
    case "submitTurn": {
      const sessionId = ownSession(ctx, state);
      const slash=op.input.text.match(/^\/([^\s]+)(?:\s|$)/);
      if (slash && !state.settings?.commands?.some(command=>command.name === slash[1]))
        return refusePlan("Command was not advertised by this session");
      if (op.input.attachments.length) return refusePlan("ACP lifecycle supports text prompts only");
      const prompt:JsonValue[] = [];
      if (state.instructions) prompt.push({type:"text", text:state.instructions});
      prompt.push({type:"text", text:op.input.text});
      const ordinal=Math.max(state.promptOrdinal ?? 0,state.cursor.prompt,Number(object(state.projection).turn) || 0,ctx.restoredRowOrdinal ?? 0,...(state.queued ?? []).map(value=>value.prompt))+1;
      state.promptOrdinal=ordinal;
      if (state.inFlight) {
        (state.queued ??= []).push({prompt:ordinal,sendId:op.sendId});
      } else {
        state.cursor.prompt=ordinal;
        state.cursor.loading=false;
        state.inFlight=true;
        state.promptSend={sendId:op.sendId};
      }
      const plan=request(methods.prompt, {sessionId, prompt});
      if (plan.action.kind === "request")
        plan.action.call.target={threadId:sessionId,turnId:`${sessionId}:turn:${ordinal}`};
      plan.state={method:methods.prompt,prompt:ordinal};
      return plan;
    }
    case "interrupt": {
      const sessionId = ownSession(ctx, state);
      const turnId = op.activeTurnId ?? ctx.activeTurnId;
      if (turnId ? !ctx.activeTurns.some(turn=>turn.threadId === sessionId && turn.turnId === turnId) : !state.inFlight)
        return refusePlan("ACP cancel requires this tab's active prompt");
      return {action:{kind:"notification", notification:{method:methods.cancel, params:{sessionId}, target:{threadId:sessionId,...(turnId ? {turnId} : {})}}}, state:{method:methods.cancel}};
    }
    default: return refusePlan(`ACP operation is unavailable: ${op.kind}`);
  }
}


function promoteQueued(ctx:RuntimeContext, state:Negotiation):import("@codeterm/plugin-sdk").AgentEvent[] {
  const sessionId=ownSession(ctx,state);
  const events:import("@codeterm/plugin-sdk").AgentEvent[]=[];
  for (let queued=state.queued?.shift(); queued; queued=state.queued?.shift()) {
    state.cursor.prompt=queued.prompt;
    state.promptSend={sendId:queued.sendId};
    state.inFlight=true;
    const next=reduceStream(ctx,state.projection ?? null,[],state.cursor);
    state.projection={...object(next.state),started:true};
    events.push({kind:"turnState",threadId:sessionId,turnId:`${sessionId}:turn:${queued.prompt}`,state:"working"});
    if (!queued.completed) break;
    const settledQueued=settleStream(ctx,state.projection,queued.completed.cancelled ? "cancelled" : "end_turn");
    state.projection=settledQueued.state;
    state.inFlight=false;
    if (queued.completed.usage) events.push({kind:"usage",threadId:sessionId,usage:queued.completed.usage});
    events.push(...settledQueued.events);
  }
  return events;
}

export function runtimeResponse(ctx:RuntimeContext, op:Operation, raw:JsonValue, reply:RpcReply):RuntimeStep {
  binding(ctx, op);
  const method = object(raw).method;
  if (reply.kind === "error") {
    const sessionOperation=op.kind === "openThread" && method === methods.newSession
      || op.kind === "resumeThread" && (method === methods.load || method === methods.resume);
    const state=negotiated(ctx);
    if (state && op.kind === "submitTurn" && method === methods.prompt) {
      const turn=object(raw).prompt;
      state.queued=state.queued?.filter(queued=>queued.prompt !== turn || queued.sendId !== op.sendId);
      if (state.cursor.prompt === turn && state.promptSend?.sendId === op.sendId) {
        state.inFlight=false;
        delete state.promptSend;
        const projected=reduceStream(ctx,state.projection ?? null,[],state.cursor).state;
        state.projection=settleStream(ctx,projected,"error").state;
        // The host settles refusals and starts deferred activity; keep the successor projection ready.
        promoteQueued(ctx,state);
      }
    }
    if (op.kind === "resumeThread" && sessionOperation && reply.error.code !== -32000)
      return refused("session_load_refused", reply.error.message);
    if (sessionOperation && ![-32000,-32002,-32601].includes(reply.error.code) && state && !state.sessionActive && object(raw).mcpOffered === true && ctx.hostMcp) {
      state.mcpUnavailable=`Domios tools are unavailable for this agent: ${Array.from(reply.error.message).slice(0,1000).join("")}`;
      const plan=request(method as string,{...(op.kind === "resumeThread" ? {sessionId:op.threadId} : {}),cwd:ctx.cwd,mcpServers:[]});
      plan.state={method,mcpOffered:false};
      return {kind:"continue",plan};
    }
    if ((op.kind === "openThread" || op.kind === "resumeThread") && method === "session/set_config_option") {
      const state=negotiated(ctx);
      if (!state?.sessionActive || !state.sessionId || op.kind === "resumeThread" && state.sessionId !== op.threadId)
        return refused("binding_mismatch","ACP session is not active in this runtime");
      return refused("provider_refused",`Requested session options were refused: ${Array.from(reply.error.message).slice(0,1000).join("")}`);
    }
    if (reply.error.code === -32000) {
      const state=negotiated(ctx);
      if (state) {state.authenticated=false;state.required=true;}
      return refused("authentication_required",reply.error.message);
    }
    return refused("provider_refused", reply.error.message);
  }
  const value = object(reply.value);
  if (op.kind === "control" && op.action === "login" && method === "authenticate") {
    const next = request(methods.initialize, {protocolVersion, clientCapabilities:clientCapabilities(ctx), clientInfo:{name:"domios",title:"Domios",version:ctx.hostVersion}});
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
      capabilities:object(value.agentCapabilities), authMethods, authenticated, required:authenticated ? false : prior?.required,
      sessionId:prior?.sessionId, sessionActive:prior?.sessionActive, instructions:prior?.instructions,
      cursor:prior?.cursor ?? {replay:0,prompt:0,loading:false},inFlight:prior?.inFlight ?? false,
      promptOrdinal:prior?.promptOrdinal, projection:prior?.projection, promptSend:prior?.promptSend, queued:prior?.queued, settings:prior?.settings});
    return {kind:"complete",result:op.kind === "initialize" ? {kind:"initialized",...agentIdentity(value.agentInfo)} : {kind:"done"},
      events:[{kind:"authentication",authentication:authentication(authMethods,authenticated,authenticated ? false : prior?.required)}]};
  }
  const state = negotiated(ctx);
  if (!state) return refused("provider_refused", "ACP initialization is required for this runtime epoch");
  if ((op.kind === "openThread" || op.kind === "resumeThread") && method === "session/set_config_option") {
    if (!state.sessionActive || !state.sessionId || op.kind === "resumeThread" && state.sessionId !== op.threadId)
      return refused("binding_mismatch","ACP session is not active in this runtime");
    if (!Array.isArray(value.configOptions)) return refused("protocol_error","Invalid session option response");
    delete state.settings?.model;
    delete state.settings?.effort;
    state.settings={...state.settings,...sessionSettings(value)};
    const attempted=object(raw).attempted;
    if (!Array.isArray(attempted) || attempted.length > 2 || !attempted.every(value=>value === "model" || value === "thought_level"))
      return refused("protocol_error","Invalid session selection continuation");
    return loadedThread(op,state,state.sessionId,attempted as string[],object(raw).confirmAuthentication === true);
  }
  if (op.kind === "resumeThread" && method === "boundSession") {
    if (!state.sessionActive || state.sessionId !== op.threadId) return refused("binding_mismatch", "ACP session is not active in this runtime");
    return loadedThread(op,state,ownSession(ctx,state),[],false);
  }
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
    state.sessionActive = true;
    state.instructions = op.instructions;
    state.cursor.loading=op.kind === "resumeThread";
    state.settings={commands:[],configOptions:[],modes:[],...sessionSettings(value)};
    return loadedThread(op,state,sessionId);
  }
  if (op.kind === "control" && ["config","mode","settings"].includes(op.action)) {
    const sessionId=ownSession(ctx,state);
    const settings:import("@codeterm/plugin-sdk").RuntimeSettings | null=["config","settings"].includes(op.action) && method === "session/set_config_option" && Array.isArray(value.configOptions)
      ? sessionSettings(value) : op.action === "mode" && method === "session/set_mode" && typeof op.value === "string" ? {mode:op.value} : null;
    if (!settings) return refused("protocol_error","Invalid session control response");
    state.settings={...state.settings,...settings};
    if (settings.configOptions) {
      delete state.settings.model;
      delete state.settings.effort;
      const model=modelOption(settings)?.currentValue;
      if (model) state.settings.model=model;
      const effort=effortOption(settings)?.currentValue;
      if (effort) state.settings.effort=effort;
    }
    return {kind:"complete",result:{kind:"done"},events:[{kind:"controlsConfirmed",threadId:sessionId,settings:op.action === "settings" ? state.settings : settings}]};
  }
  if (op.kind === "answer" && method === "session/request_permission")
    return {kind:"complete",result:{kind:"done"},events:[{kind:"promptResolved",threadId:ownSession(ctx,state),requestId:op.requestId}]};
  if (op.kind === "interrupt" && method === methods.cancel) {
    return {kind:"complete",result:{kind:"done"},events:[]};
  }
  if (op.kind === "submitTurn" && method === methods.prompt) {
    const sessionId = ownSession(ctx, state);
    if (typeof value.stopReason !== "string") return refused("protocol_error", "ACP prompt response is missing stopReason");
    const turn=typeof object(raw).prompt === "number" ? object(raw).prompt : object(state.projection).turn;
    if (turn !== state.cursor.prompt || !state.inFlight) {
      const queued=state.queued?.find(queued=>queued.prompt === turn);
      if (queued && !queued.completed) queued.completed={cancelled:value.stopReason === "cancelled",usage:usage(value.usage)};
      return {kind:"complete",result:{kind:"turnAccepted",threadId:sessionId,sendId:op.sendId,turnId:`${sessionId}:turn:${turn}`},events:queued ? sessionAuthentication(state) : []};
    }
    state.inFlight=false;
    state.instructions = undefined;
    const projected=reduceStream(ctx,state.projection ?? null,[],state.cursor,{sendId:op.sendId,input:op.input},true);
    const settled=settleStream(ctx,projected.state,value.stopReason);
    state.projection=settled.state;
    const mapped=usage(value.usage);
    const events:import("@codeterm/plugin-sdk").AgentEvent[]=[...sessionAuthentication(state),...(mapped ? [{kind:"usage" as const,threadId:sessionId,usage:mapped}] : []),...projected.events,...settled.events];
    events.push(...promoteQueued(ctx,state));
    return {kind:"complete", result:{kind:"turnAccepted", threadId:sessionId, sendId:op.sendId,turnId:`${sessionId}:turn:${turn}`},events};

  }
  return refused("unsupported", "ACP response is unavailable");
}

export function runtimeReduce(ctx:RuntimeContext, raw:JsonValue, frames:readonly RuntimeFrame[]):RuntimeReduction {
  binding(ctx);
  const state=negotiated(ctx);
  if (!ctx.threadId || !state || state.sessionId !== ctx.threadId) return {state:raw,events:[]};
  const own=frames.flatMap((frame,index)=>object(frame.params).sessionId === ctx.threadId &&
    (frame.kind === "notification" && frame.method === methods.update || frame.kind === "request" && clientRequest(frame.method))
      ? [{frame,index}] : []);
  const prompts=frames.flatMap(frame=>{
    if (frame.kind !== "request") return [];
    const prompt=runtimePrompt(ctx,frame.method,frame.params);
    return prompt ? [{kind:"prompt" as const,threadId:ctx.threadId!,requestId:frame.requestId,prompt}] : [];
  });
  if (!own.length) return {state:raw,events:prompts};
  let projection=state.projection ?? raw;
  const events:import("@codeterm/plugin-sdk").AgentEvent[]=[];
  for (const {frame,index} of own) {
    const window=ctx.streamWindow;
    const witness=window?.turnRequests?.find(request=>request.sendId === state.promptSend?.sendId);
    const sequence=window?.frameSequences?.[index];
    const attributable=!window || witness?.written === true && witness.afterSequence !== undefined
      && sequence !== undefined && sequence > witness.afterSequence;
    const cursor=attributable ? state.cursor : {...state.cursor,prompt:Number(object(projection).prompt) || 0,
      loading:state.cursor.loading || object(projection).loading === true};
    if (!attributable && projection === null)
      projection={...object(reduceStream(ctx,null,[],cursor).state),turn:0};
    const frameContext=!window ? ctx : {...ctx,claimedSends:ctx.claimedSends.filter(claim=>
      window.turnRequests?.some(request=>request.sendId === claim.sendId && request.written === true
        && request.afterSequence !== undefined && sequence !== undefined && sequence > request.afterSequence))};
    const claim=frameContext.claimedSends.find(claim=>claim.threadId === ctx.threadId && claim.sendId === state.promptSend?.sendId);
    const reduced=reduceStream(frameContext,projection,[frame],cursor,state.inFlight && attributable && claim ? {sendId:claim.sendId,input:claim.input} : undefined);
    projection=reduced.state;events.push(...reduced.events);
  }
  const reduction={state:projection,events};
  for (const event of reduction.events) if (event.kind === "controlsConfirmed") {
    state.settings={...state.settings,...event.settings};
    if (event.settings.configOptions) {
      delete state.settings.model;
      delete state.settings.effort;
      const model=modelOption(event.settings)?.currentValue;
      if (model) state.settings.model=model;
      const effort=effortOption(event.settings)?.currentValue;
      if (effort) state.settings.effort=effort;
    }
  }
  const bounded=fitTextReduction(projection,[...reduction.events,...prompts]);
  state.projection=bounded.state;
  return bounded;
}
