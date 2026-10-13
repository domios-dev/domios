import type {
  RuntimeContext, Operation, RuntimePlan, RuntimeStep, RpcReply, RuntimeSettings,
  RuntimeFailure, JsonValue, ChatMsg, OpenItem, RuntimeHistoryQuestion,
} from "@codeterm/plugin-sdk";
import { object, text, list, userThread, steerRefused, archivedResume } from "./runtimeProtocol";
import { json, runtimeItems } from "./runtimeMessages";
import { accountAuth } from "./runtimeAuth";
import { interruptTargets } from "./runtimeInterrupt";
import { questionAnswerInput, reconcileHistoryQuestions } from "./runtimeQuestions";
import { runtimeAnswer } from "./runtimePrompts";
import { codexRuntimePolicy, codexThreadConfig, codexEffectiveSettings, codexReportedSettings } from "./runtimeSettings";
export { runtimeReduce } from "./runtimeReduce";

function fail(code: RuntimeFailure["code"], message: string): never {
  throw {code,message} satisfies RuntimeFailure;
}
function request(method: string, params: unknown, state: unknown = {stage:method}): RuntimePlan {
  const target = method === "turn/interrupt" ? {threadId:String(object(params).threadId),turnId:String(object(params).turnId)} : undefined;
  return {action:{kind:"request",call:{method,params:json(params),...(target ? {target} : {})}},state:json(state)};
}
function settingsParams(ctx: RuntimeContext, settings: RuntimeSettings, instructions?: string): Record<string,unknown> {
  const policies = codexRuntimePolicy(settings);
  const sandbox = object(policies.sandboxPolicy).type;
  return {cwd:ctx.cwd,model:settings.model,threadSource:"user",config:codexThreadConfig(settings),
    approvalPolicy:policies.approvalPolicy,approvalsReviewer:policies.approvalsReviewer,
    sandbox:({readOnly:"read-only",workspaceWrite:"workspace-write",dangerFullAccess:"danger-full-access"} as Record<string,string>)[String(sandbox)],
    ...(instructions !== undefined ? {developerInstructions:instructions} : {})};
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
    ...codexRuntimePolicy(op.settings),model:op.settings.model,effort:op.settings.effort};
}
function validateBinding(ctx: RuntimeContext, op: Operation): void {
  if ("threadId" in op && op.threadId && ctx.threadId && op.threadId !== ctx.threadId) fail("binding_mismatch","Thread differs from the bound thread");
  if ("activeTurnId" in op && op.activeTurnId && op.activeTurnId !== ctx.activeTurnId) fail("binding_mismatch","Active turn differs from the bound turn");
}

export function runtimePlan(ctx: RuntimeContext, op: Operation): RuntimePlan {
  validateBinding(ctx,op);
  switch (op.kind) {
    case "initialize":
      return request("initialize",{clientInfo:{name:"domios",version:ctx.hostVersion},capabilities:{experimentalApi:true,optOutNotificationMethods:[]}});
    case "openThread": return request("thread/start",settingsParams(ctx,op.settings,op.instructions));
    case "resumeThread": return request("thread/resume",{...settingsParams(ctx,op.settings),threadId:op.threadId,excludeTurns:true});
    case "readHistory":
      return request("thread/turns/list",{threadId:op.threadId,itemsView:"notLoaded",sortDirection:"asc",limit:100,cursor:op.cursor},
        {stage:"turns",turns:[],cursors:[],items:[],openItems:[],seq:0});
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
      const thread = op.threadId ?? ctx.threadId;
      if (!thread) fail("binding_mismatch","Control requires the bound thread");
      if (op.action === "auth") return request("account/read",{refreshToken:false},{stage:"auth"});
      if (op.action === "compact") return request("thread/compact/start",{threadId:thread});
      if (op.action === "restoreContext") return request("thread/inject_items",{threadId:thread,items:op.value});
      if (!["model","effort","mode","settings"].includes(op.action)) fail("unsupported",`Unsupported control: ${op.action}`);
      const settings = op.action === "settings" ? object(op.value) : {...ctx.settings,[op.action]:op.value};
      for (const key of ["model","effort","mode"]) if (settings[key] !== undefined && typeof settings[key] !== "string") fail("invalid_schema",`Invalid ${key}`);
      const parsed = settings as RuntimeSettings;
      return request("thread/settings/update",{threadId:thread,...codexRuntimePolicy(parsed),model:parsed.model,effort:parsed.effort},{stage:"thread/settings/update",settings:parsed});
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
  validateBinding(ctx,op);
  const state = object(raw);
  if (reply.kind === "error") {
    if (op.kind === "readHistory" && state.stage === "goal") return historyComplete(op,state);
    if (op.kind === "readHistory" && ["turns","items"].includes(String(state.stage)) && reply.error.code === -32601)
      return continuation(request("thread/read",{threadId:op.threadId,includeTurns:true},{stage:"snapshot",items:[],openItems:[],questions:[],seq:0}));
    if (op.kind === "submitTurn" && state.stage === "turn/steer" && op.activeTurnId && steerRefused(reply.error,op.activeTurnId))
      return continuation(request("turn/start",turnParams(ctx,op)),true);
    if (op.kind === "resumeThread" && state.stage === "thread/resume" && !state.unarchived
      && archivedResume(reply.error.message))
      return continuation(request("thread/unarchive",{threadId:op.threadId},{stage:"unarchive"}));
    return refused(reply.error.message,op.kind === "submitTurn" && state.stage === "turn/steer" && !!op.activeTurnId && steerRefused(reply.error,op.activeTurnId));
  }
  const value = object(reply.value);
  switch (op.kind) {
    case "initialize":
      if (state.stage === "initialize") return continuation({action:{kind:"notification",notification:{method:"initialized",params:{}}},state:{stage:"initializedNotification"}});
      if (state.stage === "initializedNotification") return continuation(request("account/read",{refreshToken:false},{stage:"initializeAccount"}));
      return {kind:"complete",result:{kind:"initialized"},events:[accountAuth(value)]};
    case "openThread": {
      if (state.stage === "materialize") return {kind:"complete",result:{kind:"threadLoaded",threadId:String(state.threadId),settings:state.settings as RuntimeSettings}};
      const threadId = userThread(value);
      if (!threadId) return refused("New thread identity unverified");
      return continuation(request("thread/inject_items",{threadId,items:[{type:"message",role:"developer",content:[{type:"input_text",text:"Domios thread binding marker."}]}]},
        {stage:"materialize",threadId,settings:codexEffectiveSettings(op.settings,value)}));
    }
    case "resumeThread": {
      if (state.stage === "unarchive") return continuation(request("thread/resume",{...settingsParams(ctx,op.settings),threadId:op.threadId,excludeTurns:true},{stage:"thread/resume",unarchived:true}));
      if (!userThread(value,op.threadId)) return refused("Resumed thread identity unverified");
      if (state.stage !== "verify") return continuation(request("thread/read",{threadId:op.threadId,includeTurns:false},{stage:"verify",settings:codexEffectiveSettings(op.settings,value)}));
      return {kind:"complete",result:{kind:"threadLoaded",threadId:op.threadId,settings:state.settings as RuntimeSettings}};
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
    case "control": {
      if (state.stage === "auth") return {kind:"complete",result:{kind:"done"},events:[accountAuth(value)]};
      const reported = state.stage === "thread/settings/update" ? codexReportedSettings(value) : {};
      return {kind:"complete",result:{kind:"done"},...(Object.keys(reported).length ? {events:[{kind:"controlsConfirmed",threadId:op.threadId ?? ctx.threadId!,settings:codexEffectiveSettings(state.settings as RuntimeSettings,value)}]} : {})};
    }
    case "answer": case "answerQuestion": return {kind:"complete",result:{kind:"done"}};
  }
}

function historyResponse(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>, value: Record<string,unknown>): RuntimeStep {
  if (state.stage === "snapshot") {
    if (!userThread(value,op.threadId)) return refused("History snapshot thread identity unverified");
    const turns = object(value.thread).turns;
    if (!Array.isArray(turns)) return refused("Missing history snapshot turns");
    let collected = state;
    for (const raw of turns) {
      const turn = object(raw);
      if (!text(turn.id) || !Array.isArray(turn.items)) return refused("Missing history snapshot turn items");
      collected = collectHistoryItems(op,collected,turn,turn.items);
    }
    return historyGoal(op,collected);
  }
  if (state.stage === "goal") {
    const objective = text(object(value.goal).objective);
    if (objective) state.items = [...list(state.items),{id:"thread:goal",seq:Number(state.seq)+1,type:"custom",content:"",sessionIntent:{goal:objective}}];
    return historyComplete(op,state);
  }
  if (!Array.isArray(value.data)) return refused("Missing history page data");
  if (state.stage === "turns") {
    const turns = list(value.data).map(object);
    const next = text(value.nextCursor);
    if (next && next === op.cursor) return refused("Repeated history cursor");
    const collected = {...state,turns,nextCursor:next,index:0,stage:"items"};
    if (!turns.length) return historyGoal(op,collected);
    return historyItems(op,collected);
  }
  if (state.stage !== "items") return refused("Invalid history state");
  const turns = list(state.turns).map(object);
  const turn = turns[Number(state.index)];
  if (!turn || !text(turn.id)) return refused("Missing history turn id");
  const mapped = collectHistoryItems(op,state,turn,value.data);
  const next = text(value.nextCursor);
  const cursors = list(state.cursors);
  if (next && cursors.includes(next)) return refused("Repeated history cursor");
  const collected = {...mapped};
  if (next) return historyItems(op,{...collected,cursors:[...cursors,next]},next);
  const index = Number(state.index) + 1;
  if (index < turns.length) return historyItems(op,{...collected,index,cursors:[]});
  return historyGoal(op,collected);
}
function collectHistoryItems(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>, turn: Record<string,unknown>, entries: unknown[]): Record<string,unknown> {
  const items = list(state.items) as ChatMsg[];
  const openItems = list(state.openItems) as OpenItem[];
  let seq = Number(state.seq);
  let questions = list(state.questions) as RuntimeHistoryQuestion[];
  for (const entry of entries) {
    const record = object(entry); const rawItem = record.item ?? record;
    questions = reconcileHistoryQuestions(op.threadId,rawItem,questions);
    const rows = runtimeItems(rawItem,seq+1);
    seq += rows.length;
    for (const row of rows) {
    row.toolFlowId = String(turn.id);
    const timestamp = record.startedAtMs ?? record.completedAtMs;
    if (typeof timestamp === "number" && Number.isFinite(timestamp)) row.timestamp = new Date(timestamp).toISOString();
    items.push(row);
    if (turn.status === "inProgress" && turn.startedAt != null && record.completedAtMs == null
      && !["completed","failed","interrupted","declined"].includes(String(object(rawItem).status))) openItems.push({turnId:String(turn.id),item:row});
    }
  }
  return {...state,items,openItems,seq,questions};
}
function historyItems(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>, cursor?: string): RuntimeStep {
  const turn = object(list(state.turns)[Number(state.index)]);
  if (!text(turn.id)) return refused("Missing history turn id");
  return continuation(request("thread/items/list",{threadId:op.threadId,turnId:turn.id,sortDirection:"asc",limit:100,cursor},state));
}
function historyGoal(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>): RuntimeStep {
  if (text(state.nextCursor)) return historyComplete(op,state);
  return continuation(request("thread/goal/get",{threadId:op.threadId},{...state,stage:"goal"}));
}
function historyComplete(op: Extract<Operation,{kind:"readHistory"}>, state: Record<string,unknown>): RuntimeStep {
  return {kind:"complete",result:{kind:"historyPage",threadId:op.threadId,items:list(state.items) as ChatMsg[],openItems:list(state.openItems) as OpenItem[],
    ...(text(state.nextCursor) ? {nextCursor:String(state.nextCursor)} : {}),
    ...(list(state.questions).length ? {questions:list(state.questions) as RuntimeHistoryQuestion[]} : {})}};
}
