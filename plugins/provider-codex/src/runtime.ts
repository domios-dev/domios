import { goalPlan, goalResponse, goalIntent } from "./runtimeGoal";
import { initializedResult, runtimeBinaryVersion } from "./runtimeVersion";
import { threadStatus } from "./runtimeTurnStatus";
import type {
  RuntimeContext, Operation, RuntimePlan, RuntimeStep, RpcReply, RuntimeSettings,
  RuntimeFailure, JsonValue, ChatMsg, OpenItem, RuntimeHistoryQuestion,
} from "@codeterm/plugin-sdk";
import { object, text, list, userThread, steerRefused, archivedResume, missingRollout } from "./runtimeProtocol";
import { HISTORY_CHUNK_BYTES, serializedBytes, sanitizeHistory, historyItem } from "./runtimeHistoryBounds";
import { json } from "./runtimeMessages";
import { pagedHistoryPlan, pagedHistoryResponse } from "./runtimeHistoryPager";
import { projectHistoryItem } from "./runtimeStoredHistory";
import { loginPlan, loginResponse } from "./runtimeLogin";
import { rateLimitEvents } from "./runtimeUsage";
import {resetCredits, resetOutcome} from "./resetCredits";
import { accountAuth } from "./runtimeAuth";
import {backgroundOwns, backgroundPlan, backgroundResponse} from "./runtimeBackground";
import { interruptTargets } from "./runtimeInterrupt";
import { descendantThread, historyThread } from "./runtimeBinding";
import { questionAnswerInput, reconcileHistoryQuestions } from "./runtimeQuestions";
import { runtimeAnswer } from "./runtimePrompts";
import { restoreContextItems } from "./runtimeContext";
import { codexServiceTier, codexRuntimePolicy, codexThreadConfig, codexEffectiveSettings, codexWorkspaceWriteConfirmed, codexWritableRootsConfirmed, codexNeedsWindowsSandboxSelection, codexWindowsSandboxOverride } from "./runtimeSettings";
import { modelServiceTiers, serviceTierSelection, tierAvailable } from "./runtimeServiceTiers";
export { runtimeReduce } from "./runtimeReduce";

function fail(code: RuntimeFailure["code"], message: string): never {
  throw {code,message} satisfies RuntimeFailure;
}
function request(method: string, params: unknown, state: unknown = {stage:method}): RuntimePlan {
  const target = method === "turn/interrupt" ? {threadId:String(object(params).threadId),turnId:String(object(params).turnId)} : undefined;
  return {action:{kind:"request",call:{method,params:json(params),...(target ? {target} : {})}},state:json(state)};
}
function settingsParams(ctx: RuntimeContext, settings: RuntimeSettings, instructions?: string, windowsSandbox?: "mxc"): Record<string,unknown> {
  const policies = codexRuntimePolicy(settings);
  const sandbox = object(policies.sandboxPolicy).type;
  return {cwd:ctx.cwd,model:settings.model,...codexServiceTier(settings),threadSource:"user",config:{...codexThreadConfig(settings,ctx.hostOwned),...(windowsSandbox ? {"windows.sandbox":windowsSandbox} : {})},
    ...(policies.runtimeWorkspaceRoots ? {runtimeWorkspaceRoots:policies.runtimeWorkspaceRoots} : {}),
    approvalPolicy:policies.approvalPolicy,approvalsReviewer:policies.approvalsReviewer,
    sandbox:({readOnly:"read-only",workspaceWrite:"workspace-write",dangerFullAccess:"danger-full-access"} as Record<string,string>)[String(sandbox)],
    ...(instructions !== undefined ? {developerInstructions:instructions} : {})};
}
function threadPlan(ctx: RuntimeContext, op: Extract<Operation,{kind:"openThread"|"resumeThread"}>, windowsSandbox?: "mxc"): RuntimePlan {
  const params = settingsParams(ctx,op.settings,op.instructions,windowsSandbox);
  return op.kind === "openThread" ? request("thread/start",params)
    : request("thread/resume",{...params,threadId:op.threadId,excludeTurns:true},{stage:"thread/resume",...(windowsSandbox ? {windowsSandbox} : {})});
}
function turnParams(ctx: RuntimeContext, op: Extract<Operation,{kind:"submitTurn"}>): Record<string,unknown> {
  const input: unknown[] = [{type:"text",text:op.input.text}];
  for (const attachment of op.input.attachments) {
    if (attachment.data) input.push({type:"image",url:`data:${attachment.mimeType};base64,${attachment.data}`});
    else if (attachment.url) input.push({type:"image",url:attachment.url});
    else if (attachment.path) input.push({type:"localImage",path:attachment.path});
    else fail("invalid_schema","Attachment has no image source");
  }
  return {threadId:op.threadId,clientUserMessageId:op.sendId,input,cwd:ctx.cwd,
    ...codexRuntimePolicy(op.settings),...codexServiceTier(op.settings),model:op.settings.model,effort:op.settings.effort};
}
function threadHistoryPlan(ctx: RuntimeContext, thread: string, value: JsonValue): RuntimePlan {
  const action = text(object(value).action);
  const turnId = text(object(value).turnId);
  if ((action !== "fork" && action !== "revert") || !turnId || turnId.length > 4096) fail("invalid_schema","Invalid thread history action");
  if (ctx.activeTurnId) fail("provider_refused","Thread history cannot change during an active turn");
  return request("thread/turns/list",{threadId:thread,itemsView:"notLoaded",sortDirection:"asc",limit:8},
    {stage:"threadHistoryTurns",action,turnId,cursor:null,targetFound:false,...(action === "fork" ? {sourceThreadId:thread} : {})});
}
function forkThreadPlan(ctx: RuntimeContext, op: Extract<Operation,{kind:"forkThread"}>): RuntimePlan {
  if (ctx.threadId || ctx.activeTurnId || ctx.activeTurns.length) fail("binding_mismatch","Fork requires an unbound child runtime");
  return threadHistoryPlan(ctx,op.sourceThreadId,{action:"fork",turnId:op.turnId});
}
function threadHistoryTurns(ctx: RuntimeContext, op: Extract<Operation,{kind:"control"|"forkThread"}>, state: Record<string,unknown>, value: Record<string,unknown>): RuntimeStep {
  const controlThread = op.kind === "control" ? op.threadId : undefined;
  const thread = text(state.sourceThreadId) ?? controlThread ?? ctx.threadId;
  const action = text(state.action);
  const turnId = text(state.turnId);
  if (!thread || !action || !turnId || !Array.isArray(value.data) || value.data.length > 8) return refused("Invalid thread history page");
  let targetFound = state.targetFound === true;
  for (const raw of value.data) {
    const turn = object(raw);
    const id = text(turn.id);
    if (!id || id.length > 4096) return refused("Thread history turn identity missing");
    if (targetFound) {
      if (action === "revert") return continuation(request("thread/revert",{threadId:thread,beforeTurnId:id},{stage:"thread/revert",turnId}));
      return refused("Fork boundary resolution failed");
    }
    if (id !== turnId) continue;
    if (turn.status === "inProgress") return refused("Active turns cannot be forked or reverted");
    if (action === "fork") return continuation(request("thread/fork",{threadId:thread,lastTurnId:turnId,
      ...(ctx.hostOwned?.questions ? {config:codexThreadConfig(ctx.settings,ctx.hostOwned)} : {})},
      {stage:"thread/fork",turnId,sourceThreadId:thread}));
    targetFound = true;
  }
  const next = text(value.nextCursor);
  if (next && (next.length > 4096 || next === state.cursor)) return refused("Thread history cursor did not advance");
  if (next) return continuation(request("thread/turns/list",{threadId:thread,itemsView:"notLoaded",sortDirection:"asc",limit:8,cursor:next},
    {...state,cursor:next,targetFound}));
  return targetFound ? refused("There is no later turn to revert") : refused("Selected turn is not in the bound thread");
}
function validateBinding(ctx: RuntimeContext, op: Operation): void {
  if (op.kind === "backgroundProcesses") {
    if (!backgroundOwns(ctx,op.threadId)) fail("binding_mismatch","Background thread is not owned");
    return;
  }
  if (op.kind === "readHistory" && !ctx.threadId) fail("binding_mismatch","History requires a bound root thread");
  if ("threadId" in op && op.threadId && ctx.threadId && op.threadId !== ctx.threadId
    && !(op.kind === "readHistory" && descendantThread(ctx,op.threadId))) fail("binding_mismatch","Thread differs from the bound thread");
  if ("activeTurnId" in op && op.activeTurnId && op.activeTurnId !== ctx.activeTurnId) fail("binding_mismatch","Active turn differs from the bound turn");
}

function historyTarget(ctx: RuntimeContext, op: Operation, plan: RuntimePlan): RuntimePlan {
  if (op.kind !== "readHistory" || op.threadId === ctx.threadId || plan.action.kind !== "request") return plan;
  return {...plan,action:{...plan.action,call:{...plan.action.call,target:{threadId:op.threadId}}}};
}

export function runtimePlan(ctx: RuntimeContext, op: Operation): RuntimePlan {
  return historyTarget(ctx,op,planOperation(ctx,op));
}

function planOperation(ctx: RuntimeContext, op: Operation): RuntimePlan {
  validateBinding(ctx,op);
  switch (op.kind) {
    case "backgroundProcesses": return backgroundPlan(ctx,op);
    case "initialize":
      return request("initialize",{clientInfo:{name:"domios",version:ctx.hostVersion},capabilities:{experimentalApi:true,optOutNotificationMethods:[]}});
    case "openThread":
    case "resumeThread": return op.settings.mode === "workspace-write"
      ? request("config/read",{includeLayers:false,cwd:ctx.cwd},{stage:"windowsSandboxConfig"}) : threadPlan(ctx,op);
    case "forkThread": return forkThreadPlan(ctx,op);
    case "readHistory":
      if (op.order === "desc") return pagedHistoryPlan(op);
      return request("thread/turns/list",{threadId:op.threadId,itemsView:"notLoaded",sortDirection:op.order ?? "asc",limit:8,cursor:op.cursor},
        {stage:"turns",turns:[],cursor:null,seq:0});
    case "submitTurn": {
      const params = turnParams(ctx,op);
      if (!op.activeTurnId) return request("turn/start",params);
      return request("turn/steer",{threadId:op.threadId,clientUserMessageId:op.sendId,input:params.input,expectedTurnId:op.activeTurnId});
    }
    case "interrupt": {
      const turns = interruptTargets(ctx,op.threadId);
      if (!turns.length) fail("provider_refused","No active turn to interrupt");
      return request("turn/interrupt",{threadId:turns[0].threadId,turnId:turns[0].turnId},{stage:"interrupt",remaining:turns.slice(1)});
    }
    case "answerQuestion": return {action:{kind:"questionAnswer",input:questionAnswerInput(op.itemId,op.answers)},state:null};
    case "answer": {
      const pending = ctx.pendingRequest;
      if (!pending || pending.requestId !== op.requestId) fail("binding_mismatch","Answer has no exact pending request");
      return {action:{kind:"reply",requestId:op.requestId,result:{kind:"success",value:runtimeAnswer(pending.method,pending.params,op.requestId,op.answer)}},state:null};
    }
    case "control": {
      if (op.action === "readResetCredits") return request("account/rateLimits/read",{excludeResetCreditDetails:object(op.value).details !== true});
      if (op.action === "redeemResetCredit") {
        const key = text(object(op.value).idempotencyKey);
        if (!key || !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(key)) fail("invalid_schema","A confirmation UUID is required");
        const creditId = object(op.value).creditId;
        if (creditId != null && (typeof creditId !== "string" || !creditId.trim())) fail("invalid_schema","Reset credit identifier is invalid");
        return request("account/rateLimitResetCredit/consume",{idempotencyKey:key,...(creditId ? {creditId} : {})});
      }
      if (op.action === "login") return loginPlan(op.value);
      if (op.action === "auth") return request("account/read",{refreshToken:false},{stage:"auth"});
      const thread = op.threadId ?? ctx.threadId;
      if (!thread) fail("binding_mismatch","Control requires the bound thread");
      if (op.action === "goal") return goalPlan(thread,op.value);
      if (op.action === "threadHistory") return threadHistoryPlan(ctx,thread,op.value);
      if (op.action === "compact") return request("thread/compact/start",{threadId:thread});
      if (op.action === "serviceTiers") return request("model/list",{includeHidden:true,limit:100},{stage:"serviceTiers",models:[],cursors:[]});
      if (op.action === "restoreContext") return request("thread/inject_items",{threadId:thread,items:restoreContextItems(op.value)});
      if (!["model","effort","mode","settings"].includes(op.action)) fail("unsupported",`Unsupported control: ${op.action}`);
      const settings = op.action === "settings" ? {...ctx.settings,...object(op.value)} : {...ctx.settings,[op.action]:op.value};
      for (const key of ["model","effort","mode"] as const) if (settings[key] !== undefined && typeof settings[key] !== "string") fail("invalid_schema",`Invalid ${key}`);
      if (settings.serviceTier !== undefined && !serviceTierSelection(settings.serviceTier)) fail("invalid_schema","Invalid service tier");
      const parsed = settings as RuntimeSettings;
      if (parsed.serviceTier && !tierAvailable({...parsed,serviceTierAvailability:ctx.settings.serviceTierAvailability},parsed.serviceTier)) fail("provider_refused","Service tier is not available for this account and model");
      if (ctx.settings.mode === "read-only-with-writable-roots") {
        if (parsed.mode !== ctx.settings.mode || JSON.stringify(parsed.additionalDirectories) !== JSON.stringify(ctx.settings.additionalDirectories)
          || parsed.networkAccess !== ctx.settings.networkAccess) fail("provider_refused","Read-only writable roots require a new launch to change");
        return request("thread/settings/update",{threadId:thread,...codexServiceTier(parsed),model:parsed.model,effort:parsed.effort},{stage:"thread/settings/update",settings:parsed});
      }
      if (parsed.mode === "read-only-with-writable-roots") fail("provider_refused","Read-only writable roots require a new launch");
      return request("thread/settings/update",{threadId:thread,...codexRuntimePolicy(parsed),...codexServiceTier(parsed),model:parsed.model,effort:parsed.effort},{stage:"thread/settings/update",settings:parsed});
    }
  }
}

function refused(message: string, beforeAcceptance = false): RuntimeStep {
  return {kind:"refused",reason:{code:"provider_refused",message},beforeAcceptance};
}
function continuation(plan: RuntimePlan, beforeAcceptance?: boolean): RuntimeStep {
  return {kind:"continue",plan,...(beforeAcceptance ? {beforeAcceptance} : {})};
}
export function runtimeResponse(ctx: RuntimeContext, op: Operation, raw: JsonValue, reply: RpcReply): RuntimeStep {
  const step = respondOperation(ctx,op,raw,reply);
  return step.kind === "continue" ? {...step,plan:historyTarget(ctx,op,step.plan)} : step;
}

function respondOperation(ctx: RuntimeContext, op: Operation, raw: JsonValue, reply: RpcReply): RuntimeStep {
  validateBinding(ctx,op);
  if (op.kind === "backgroundProcesses") return backgroundResponse(op,reply);
  const state = object(raw);
  if (op.kind === "control" && op.action === "goal") return goalResponse(op.threadId ?? ctx.threadId!,state,reply);
  if ((op.kind === "forkThread" || op.kind === "control" && op.action === "threadHistory")
    && state.stage === "threadHistoryTurns") {
    if (reply.kind === "error") return refused(reply.error.message);
    return threadHistoryTurns(ctx,op,state,object(reply.value));
  }
  if (op.kind === "readHistory" && reply.kind === "error" && state.stage === "turns"
    && !op.cursor && reply.error.code === -32600
    && reply.error.message === `thread ${op.threadId} is not materialized yet; thread/turns/list is unavailable before first user message`)
    return continuation(request("thread/read",{threadId:op.threadId,includeTurns:false},{stage:"unmaterialized"}));
  if (op.kind === "readHistory" && state.stage === "unmaterialized" && reply.kind === "success") {
    const thread = object(object(reply.value).thread);
    if (!historyThread(ctx,reply.value,op.threadId) || !Array.isArray(thread.turns) || thread.turns.length)
      return refused("Empty thread identity unverified");
    return {kind:"complete",result:{kind:"historyPage",threadId:op.threadId,items:[],openItems:[],...(op.order ? {order:op.order} : {})}};
  }
  if (op.kind === "readHistory" && state.pager === 1) {
    if (reply.kind === "error" && reply.error.code === -32601 && ["turns","items"].includes(String(state.stage)))
      return historyComplete(op,{items:[],openItems:[]});
    return pagedHistoryResponse(op,state,reply);
  }
  if (reply.kind === "error") {
    if (op.kind === "resumeThread" && state.stage === "thread/resume" && missingRollout(reply.error,op.threadId))
      return {kind:"refused",reason:{code:"thread_not_found",message:"The provider's saved conversation rollout is unavailable."},beforeAcceptance:true};
    if (op.kind === "control" && op.action === "serviceTiers") return {kind:"complete",result:{kind:"done"},events:[{kind:"controlsConfirmed",threadId:op.threadId ?? ctx.threadId!,settings:{serviceTierAvailability:{}}}]};
    const smaller = object(reply.error.data).historyPageLimit;
    if (op.kind === "readHistory" && state.stage === "items" && reply.error.code === -32097
      && typeof smaller === "number" && Number.isSafeInteger(smaller) && smaller >= 1 && smaller < Number(state.historyItemLimit ?? 100))
      return historyItems(op,{...state,historyItemLimit:smaller},text(state.cursor));
    if (op.kind === "control" && op.action === "redeemResetCredit" && state.stage === "refreshResetCredits") return {kind:"complete",result:{kind:"resetCreditConsumed",outcome:"reset"}};
    if (op.kind === "initialize" && state.stage === "initializeLimits") return {kind:"complete",result:initializedResult(state),events:[state.auth as unknown as import("@codeterm/plugin-sdk").AgentEvent]};
    if (op.kind === "control" && op.action === "login") return refused("Sign-in failed. Try again.");
    if (op.kind === "readHistory" && state.stage === "goal") return historyComplete(op,state);
    if (op.kind === "readHistory" && ["turns","items"].includes(String(state.stage)) && reply.error.code === -32601)
      return historyComplete(op,{items:[],openItems:[]});
    if (op.kind === "submitTurn" && state.stage === "turn/steer" && op.activeTurnId && steerRefused(reply.error,op.activeTurnId))
      return continuation(request("turn/start",turnParams(ctx,op)),true);
    if (op.kind === "resumeThread" && state.stage === "thread/resume" && !state.unarchived
      && archivedResume(reply.error.message))
      return continuation(request("thread/unarchive",{threadId:op.threadId},{stage:"unarchive",...(state.windowsSandbox === "mxc" ? {windowsSandbox:"mxc"} : {})}));
    return refused(reply.error.message,op.kind === "submitTurn" && state.stage === "turn/steer" && !!op.activeTurnId && steerRefused(reply.error,op.activeTurnId));
  }
  const value = object(reply.value);
  if (op.kind === "openThread" || op.kind === "resumeThread") {
    if (state.stage === "windowsSandboxConfig") return continuation(codexNeedsWindowsSandboxSelection(value)
      ? request("configRequirements/read",null,{stage:"windowsSandboxRequirements"}) : threadPlan(ctx,op));
    if (state.stage === "windowsSandboxRequirements") return continuation(threadPlan(ctx,op,codexWindowsSandboxOverride(value)));
  }
  switch (op.kind) {
    case "initialize":
      if (state.stage === "initialize") return continuation({action:{kind:"notification",notification:{method:"initialized",params:{}}},state:json({stage:"initializedNotification",binaryVersion:runtimeBinaryVersion(value.userAgent)})});
      if (state.stage === "initializedNotification") return continuation(request("account/read",{refreshToken:false},{...state,stage:"initializeAccount"}));
      if (state.stage === "initializeAccount") {
        const auth = accountAuth(value);
        if (auth.billingMode === "subscription") return continuation(request("account/rateLimits/read",{excludeResetCreditDetails:true}, {...state,stage:"initializeLimits",auth}));
        return {kind:"complete",result:initializedResult(state),events:[auth]};
      }
      return {kind:"complete",result:initializedResult(state),events:[state.auth as unknown as import("@codeterm/plugin-sdk").AgentEvent,...rateLimitEvents(value.rateLimits,value.rateLimitResetCredits)]};
    case "openThread": {
      const threadId = userThread(value);
      if (!threadId) return refused("New thread identity unverified");
      if (op.settings.mode === "workspace-write" && !codexWorkspaceWriteConfirmed(value)) return refused("Provider did not confirm the requested workspace-write sandbox");
      if (!codexWritableRootsConfirmed(op.settings,value)) return refused("Provider did not confirm the exact read-only writable roots");
      return {kind:"complete",result:{kind:"threadLoaded",threadId,settings:codexEffectiveSettings(op.settings,value)}};
    }
    case "resumeThread": {
      if (state.stage === "unarchive") return continuation(request("thread/resume",{...settingsParams(ctx,op.settings,op.instructions,state.windowsSandbox === "mxc" ? "mxc" : undefined),threadId:op.threadId,excludeTurns:true},{stage:"thread/resume",unarchived:true,...(state.windowsSandbox === "mxc" ? {windowsSandbox:"mxc"} : {})}));
      if (!userThread(value,op.threadId)) return refused("Resumed thread identity unverified");
      if (state.stage !== "verify" && op.settings.mode === "workspace-write" && !codexWorkspaceWriteConfirmed(value)) return refused("Provider did not confirm the requested workspace-write sandbox");
      if (state.stage !== "verify" && !codexWritableRootsConfirmed(op.settings,value)) return refused("Provider did not confirm the exact read-only writable roots");
      if (state.stage !== "verify") return continuation(request("thread/read",{threadId:op.threadId,includeTurns:false},{stage:"verify",settings:codexEffectiveSettings(op.settings,value)}));
      const idle = threadStatus(object(value.thread).status)?.state === "idle";
      return {kind:"complete",result:{kind:"threadLoaded",threadId:op.threadId,settings:state.settings as RuntimeSettings},
        ...(idle ? {events:[{kind:"turnState",threadId:op.threadId,state:"idle",...(ctx.activeTurnId ? {turnId:ctx.activeTurnId} : {})}]} : {})};
    }
    case "readHistory": return historyResponse(op,state,value);
    case "submitTurn": {
      const turnId = text(object(value.turn).id) ?? text(value.turnId);
      return {kind:"complete",result:{kind:"turnAccepted",threadId:op.threadId,sendId:op.sendId,...(turnId ? {turnId} : {})}};
    }
    case "interrupt": {
      const turns = list(state.remaining).map(object);
      if (turns.length) return continuation(request("turn/interrupt",{threadId:turns[0].threadId,turnId:turns[0].turnId},{stage:"interrupt",remaining:turns.slice(1)}));
      return {kind:"complete",result:{kind:"done"}};
    }
    case "forkThread": {
      if (state.stage !== "thread/fork") return refused("Fork did not reach its provider boundary");
      const threadId = userThread(reply.value);
      if (!threadId || threadId === op.sourceThreadId || state.sourceThreadId !== op.sourceThreadId
        || state.turnId !== op.turnId) return refused("Forked thread identity unverified");
      return {kind:"complete",result:{kind:"threadForked",sourceThreadId:op.sourceThreadId,threadId,turnId:op.turnId}};
    }
    case "control": {
      if (op.action === "serviceTiers") {
        if (state.stage === "serviceTiers") {
          const models = [...list(state.models),...list(value.data)];
          const cursors = list(state.cursors);
          const cursor = text(value.nextCursor);
          if (models.length > 1000 || cursor && (cursors.includes(cursor) || cursors.length >= 10)) return refused("Invalid model catalogue pagination");
          if (cursor) return continuation(request("model/list",{includeHidden:true,limit:100,cursor},{stage:"serviceTiers",models,cursors:[...cursors,cursor]}));
          return continuation(request("configRequirements/read",{}, {stage:"tierRequirements",models}));
        }
        return {kind:"complete",result:{kind:"done"},events:[{kind:"controlsConfirmed",threadId:op.threadId ?? ctx.threadId!,settings:{serviceTierAvailability:modelServiceTiers(state.models,value)}}]};
      }
      if (op.action === "readResetCredits") {
        if (!resetCredits(value.rateLimitResetCredits)) return refused("Reset credit allowance was not reported");
        return {kind:"complete",result:{kind:"done"},events:rateLimitEvents(value.rateLimits,value.rateLimitResetCredits)};
      }
      if (op.action === "redeemResetCredit") {
        if (state.stage === "refreshResetCredits") return {kind:"complete",result:{kind:"resetCreditConsumed",outcome:"reset"},events:rateLimitEvents(value.rateLimits,value.rateLimitResetCredits)};
        const outcome = resetOutcome(value);
        if (!outcome) return refused("Unknown reset credit outcome");
        if (outcome === "reset") return continuation(request("account/rateLimits/read",{excludeResetCreditDetails:false},{stage:"refreshResetCredits"}));
        return {kind:"complete",result:{kind:"resetCreditConsumed",outcome}};
      }
      if (op.action === "login") return loginResponse(state,value);
      if (op.action === "threadHistory" && state.stage === "thread/revert") return {kind:"complete",result:{kind:"done"}};
      if (state.stage === "auth") return {kind:"complete",result:{kind:"done"},events:[accountAuth(value)]};
      const awaitingTier = state.stage === "thread/settings/update" && object(state.settings).serviceTier !== undefined;
      return {kind:"complete",result:{kind:"done"},...(state.stage === "thread/settings/update" && !awaitingTier ? {events:[{kind:"controlsConfirmed",threadId:op.threadId ?? ctx.threadId!,settings:codexEffectiveSettings(state.settings as RuntimeSettings,value)}]} : {})};
    }
    case "answer": case "answerQuestion": return {kind:"complete",result:{kind:"done"}};
  }
}

function historyResponse(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>, value: Record<string,unknown>): RuntimeStep {
  if (state.stage === "goal") {
    const objective = text(object(sanitizeHistory(object(value.goal))).objective);
    if (objective) state.items = [...list(state.items),{id:"thread:goal",seq:Number(state.seq)+1,type:"custom",content:"",sessionIntent:goalIntent(value.goal) ?? {goal:objective}}];
    return historyComplete(op,state);
  }
  if (!Array.isArray(value.data)) return refused("Missing history page data");
  if (state.stage === "turns") {
    const source = list(value.data).map(object);
    if (source.length > 8 || source.some(turn=>typeof turn.id !== "string" || !turn.id || turn.id.length > 4096)) return refused("Invalid history turn page");
    const turns = (op.order === "desc" ? [...source].reverse() : source).map(turn=>({id:turn.id,status:turn.status === "inProgress" ? "inProgress" : "completed",
      startedAt:typeof turn.startedAt === "number" && Number.isFinite(turn.startedAt) ? turn.startedAt : null}));
    const next = text(value.nextCursor);
    if (next && (next.length > 4096 || next === op.cursor)) return refused("Repeated history cursor");
    const collected = {...state,turns,nextCursor:next,index:0,stage:"items"};
    if (!turns.length) return historyGoal(op,collected);
    return historyItems(op,collected);
  }
  if (state.stage !== "items") return refused("Invalid history state");
  const turns = list(state.turns).map(object);
  const turn = turns[Number(state.index)];
  if (!turn || !text(turn.id)) return refused("Missing history turn id");
  const mapped = collectHistoryItems(op,{...state,items:[],openItems:[],questions:[]},turn,value.data);
  if (mapped.offset != null) return streamHistory(op,historyItems(op,mapped,text(state.cursor)),mapped);
  const next = text(value.nextCursor);
  if (next && (next.length > 4096 || next === state.cursor)) return refused("Repeated history cursor");
  const collected = {...mapped};
  if (next) return streamHistory(op,historyItems(op,{...collected,cursor:next},next),collected);
  const index = Number(state.index) + 1;
  if (index < turns.length) return streamHistory(op,historyItems(op,{...collected,index,cursor:null}),collected);
  return historyGoal(op,collected);
}
function collectHistoryItems(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>, turn: Record<string,unknown>, entries: unknown[]): Record<string,unknown> {
  const items = list(state.items) as ChatMsg[];
  const openItems = list(state.openItems) as OpenItem[];
  let seq = Number(state.seq);
  let questions = list(state.questions) as RuntimeHistoryQuestion[];
  let bytes = serializedBytes({items,openItems,questions});
  for (let index = Number(state.offset ?? 0); index < entries.length; index++) {
    const record = object(entries[index]); const rawItem = historyItem(record.item ?? record);
    const nextQuestions = reconcileHistoryQuestions(op.threadId,rawItem,questions);
    const timestamp = record.startedAtMs ?? record.completedAtMs;
    const rows = projectHistoryItem(rawItem,seq+1,String(turn.id),timestamp,op.threadId);
    const active = turn.status === "inProgress" && turn.startedAt != null && record.completedAtMs == null
      && !["completed","failed","interrupted","declined"].includes(String(object(rawItem).status));
    const opened = active ? rows.map(item=>({turnId:String(turn.id),item})) : [];
    const rowBytes = serializedBytes(rows) + serializedBytes(opened) + serializedBytes(nextQuestions);
    if (bytes + rowBytes > HISTORY_CHUNK_BYTES && items.length) return {...state,items,openItems,seq,questions,offset:index};
    bytes += rowBytes;
    questions = nextQuestions;
    seq += rows.length;
    items.push(...rows);
    openItems.push(...opened);
  }
  return {...state,items,openItems,seq,questions,offset:null};
}
function historyItems(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>, cursor?: string): RuntimeStep {
  const turn = object(list(state.turns)[Number(state.index)]);
  if (!text(turn.id)) return refused("Missing history turn id");
  return continuation(request("thread/items/list",{threadId:op.threadId,turnId:turn.id,sortDirection:"asc",limit:Number(state.historyItemLimit ?? 100),cursor},{...state,historyItemLimit:Number(state.historyItemLimit ?? 100),cursor:cursor ?? null}));
}
function historyGoal(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>): RuntimeStep {
  if (text(state.nextCursor)) return historyComplete(op,state);
  return streamHistory(op,continuation(request("thread/goal/get",{threadId:op.threadId},{...state,stage:"goal"})),state);
}
function historyComplete(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>): RuntimeStep {
  return {kind:"complete",result:{kind:"historyPage",threadId:op.threadId,items:list(state.items) as ChatMsg[],openItems:list(state.openItems) as OpenItem[],
    ...(text(state.nextCursor) ? {nextCursor:String(state.nextCursor)} : {}),
    ...(list(state.questions).length ? {questions:list(state.questions) as RuntimeHistoryQuestion[]} : {}),
    ...(op.order ? {order:op.order} : {})}};
}

function streamHistory(op: Extract<Operation,{kind:"readHistory"}>, step: RuntimeStep, state: Record<string,unknown>): RuntimeStep {
  if (step.kind !== "continue") return step;
  const {items,openItems,questions,...cursor} = object(step.plan.state);
  const complete = historyComplete(op,{...state,nextCursor:undefined});
  if (complete.kind !== "complete" || complete.result.kind !== "historyPage") return step;
  return {...step,plan:{...step.plan,state:json(cursor)},history:complete.result};
}
