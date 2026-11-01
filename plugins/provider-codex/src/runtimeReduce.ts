import { goalIntent } from "./runtimeGoal";
import type { RuntimeContext, JsonValue, RuntimeFrame, RuntimeReduction, AgentEvent, ChatMsg, TokenUsage } from "@codeterm/plugin-sdk";
import { rateLimitEvents } from "./runtimeUsage";
import { object, text, list, serverRequestMethods } from "./runtimeProtocol";
import { json, runtimeItem } from "./runtimeMessages";
import { runtimeError } from "./runtimeErrors";
import { loginCompleted } from "./runtimeLogin";
import { notificationAuth } from "./runtimeAuth";
import { runtimeQuestions } from "./runtimeQuestions";
import { runtimePrompt } from "./runtimePrompts";
import { codexReportedSettings } from "./runtimeSettings";
import { abandonOpenTurn, interruptPendingCalls, runningCallStatus, stampToolLifecycle, type RuntimeItem } from "./runtimeAbandonedTurns";
import { reduceRuntimeAgents, type RuntimeAgents } from "./runtimeAgents";
import { threadStatus, turnOutcome, requestDetail, withDetail } from "./runtimeTurnStatus";
import { compactState, isRetired, markClosed, markOpen } from "./runtimeStateBudget";
import { budgetReduction } from "./runtimeBatchBudget";

interface ReduceState {
  epoch: number;
  bindingNonce: string;
  threadId?: string;
  seq: number;
  items: Record<string,RuntimeItem>;
  completed: string[];
  compacted: string[];
  evidenced: string[];
  questions?:string[];
  activeTurnId?: string;
  abandoned?: string[];
  parents?:Record<string,string>;
  childActive?:Record<string,string>;
  childCompleted?:string[];
  rateLimitSnapshot?:Record<string,unknown>;
  reasoning?:Record<string,{summary:string[]; content:string[]}>;
  agents?:RuntimeAgents;
  compacting?:string;
  open?:string[];
  retired?:string[];
}
function copyReducerState(state: ReduceState): ReduceState {
  const items: Record<string,RuntimeItem> = Object.create(null);
  for (const [id,row] of Object.entries(state.items)) items[id] = {...row};
  const copy: ReduceState = {...state,items,completed:[...state.completed],compacted:[...state.compacted],evidenced:[...state.evidenced]};
  for (const key of ["questions","abandoned","childCompleted","open","retired"] as const) {
    const values = state[key];
    if (values) copy[key] = [...values];
  }
  if (state.parents) copy.parents = Object.assign(Object.create(null),state.parents);
  if (state.childActive) copy.childActive = {...state.childActive};
  if (state.agents) copy.agents = {...state.agents,rows:Object.assign(Object.create(null),state.agents.rows)};
  if (state.reasoning) copy.reasoning = Object.fromEntries(Object.entries(state.reasoning)
    .map(([id,parts]) => [id,{summary:[...parts.summary],content:[...parts.content]}]));
  return copy;
}
function initial(ctx: RuntimeContext, raw: JsonValue): ReduceState {
  const state = object(raw);
  if (state.epoch === ctx.epoch && state.bindingNonce === ctx.bindingNonce && state.threadId === ctx.threadId) {
    return copyReducerState(state as unknown as ReduceState);
  }
  return {epoch:ctx.epoch,bindingNonce:ctx.bindingNonce,threadId:ctx.threadId,seq:0,items:Object.create(null),completed:[],compacted:[],evidenced:[],activeTurnId:ctx.activeTurnId};
}
function timing(params: Record<string,unknown>): {startedAt?:number; completedAt?:number} {
  const turn = object(params.turn);
  return {
    ...(typeof turn.startedAt === "number" && turn.startedAt >= 0 ? {startedAt:turn.startedAt * 1000} : {}),
    ...(typeof turn.completedAt === "number" && turn.completedAt >= 0 ? {completedAt:turn.completedAt * 1000} : {}),
  };
}
function usage(params: Record<string,unknown>): TokenUsage | undefined {
  const token = object(params.tokenUsage);
  const total = object(token.total);
  if (typeof total.inputTokens !== "number" || typeof total.outputTokens !== "number") return undefined;
  return {...(typeof total.costUsd === "number" && Number.isFinite(total.costUsd) && total.costUsd >= 0 ? {cost_micros:Math.round(total.costUsd * 1e6)} : {}),input_tokens:total.inputTokens,output_tokens:total.outputTokens,
    ...(typeof total.cacheWriteInputTokens === "number" ? {cache_write_tokens:total.cacheWriteInputTokens} : {}),
    ...(typeof total.cachedInputTokens === "number" ? {cache_read_tokens:total.cachedInputTokens} : {}),
    ...(typeof object(token.last).totalTokens === "number" ? {used_tokens:Number(object(token.last).totalTokens)} : {}),
    ...(typeof token.modelContextWindow === "number" ? {window_tokens:token.modelContextWindow} : {})};
}
function sendEvidence(ctx: RuntimeContext, item: Record<string,unknown>, turnId: string | undefined): AgentEvent | undefined {
  if (item.type !== "userMessage" || !turnId) return undefined;
  const sendId = text(item.clientId);
  const claim = ctx.claimedSends.find(c => c.sendId === sendId && c.threadId === ctx.threadId);
  if (!claim) return undefined;
  const parts = list(item.content).map(object);
  const content = parts.filter(p => p.type === "text").map(p => p.text).join("\n");
  if (content !== claim.input.text) return undefined;
  const images = parts.filter(p => p.type === "image" || p.type === "localImage");
  if (images.length !== claim.input.attachments.length) return undefined;
  if (!images.every((image,index) => {
    const expected = claim.input.attachments[index];
    if (expected.data) return image.type === "image" && image.url === `data:${expected.mimeType};base64,${expected.data}`;
    if (expected.url) return image.type === "image" && image.url === expected.url;
    return !!expected.path && image.type === "localImage" && image.path === expected.path;
  })) return undefined;
  return {kind:"sendEvidence",threadId:claim.threadId,sendId:claim.sendId,turnId,...(text(item.id) ? {itemId:text(item.id)!} : {})};
}
function stamp(state: ReduceState, row: RuntimeItem, params: Record<string,unknown>): RuntimeItem {
  const previous = state.items[row.id];
  row.seq = previous?.seq ?? ++state.seq;
  const ms = params.startedAtMs ?? params.completedAtMs;
  if (previous?.timestamp) row.timestamp = previous.timestamp;
  else if (typeof ms === "number" && Number.isFinite(ms)) row.timestamp = new Date(ms).toISOString();
  const turnId = text(params.turnId) ?? text(object(params.turn).id);
  if (turnId && !row.toolFlowId) row.toolFlowId = turnId;
  if (row.toolKind === "agent" && row.toolName === "agent_activity") {
    row.toolInput = json({...object(row.toolInput), agent_binding_nonce: state.bindingNonce, agent_runtime_epoch: state.epoch});
  }
  state.items[row.id] = row;
  return row;
}
function mapItem(state: ReduceState, method: string, params: Record<string,unknown>): ChatMsg | undefined {
  const raw = object(params.item);
  const id = text(raw.id) ?? text(params.itemId);
  if (!id) return undefined;
  const retired = isRetired(state,id);
  const previous = retired ? undefined : state.items[id];
  if (method === "item/started" || method === "item/completed") {
    if (method === "item/started") markOpen(state,id); else markClosed(state,id);
    const row = runtimeItem(raw,state.seq + 1,previous);
    return row ? stamp(state,stampToolLifecycle(row,previous,method,params),params) : undefined;
  }
  const delta = text(params.delta) ?? text(params.message);
  if (!delta || retired) return undefined;
  markOpen(state,id);
  const row: ChatMsg = previous ? {...previous} : {id,seq:state.seq + 1,type:"assistant",content:""};
  switch (method) {
    case "item/agentMessage/delta": case "item/plan/delta": row.type = "assistant"; row.content += delta; break;
    case "item/reasoning/textDelta": case "item/reasoning/summaryTextDelta": row.type = "thinking"; row.collapsed = true; row.content += delta; break;
    case "item/commandExecution/outputDelta": case "item/fileChange/outputDelta":
      row.type = "tool_call"; row.toolKind = method === "item/commandExecution/outputDelta" ? "command" : "edit";
      row.toolId = id; row.toolResult = (row.toolResult ?? "") + delta; break;
    case "item/mcpToolCall/progress": row.type = "tool_call"; row.toolKind = "generic"; row.toolId = id; row.toolDetail = delta; break;
    default: return undefined;
  }
  return stamp(state,stampToolLifecycle(row,previous,method,params),params);
}

export function runtimeReduce(ctx: RuntimeContext, raw: JsonValue, frames: readonly RuntimeFrame[]): RuntimeReduction {
  const state = initial(ctx,raw);
  const events: AgentEvent[] = [];
  for (const frame of frames) {
    const params = object(frame.params);
    if (frame.kind === "notification" && frame.method === "account/login/completed") {
      delete state.rateLimitSnapshot;
      events.push(loginCompleted(params)); continue;
    }
    if (frame.kind === "notification" && frame.method === "account/updated") {
      delete state.rateLimitSnapshot;
      if (ctx.threadId) events.push({kind:"controlsConfirmed",threadId:ctx.threadId,settings:{serviceTierAvailability:{}}});
      events.push(notificationAuth(params)); continue;
    }
    if (frame.kind === "notification" && frame.method === "thread/reverted") {
      if (ctx.threadId && params.threadId === ctx.threadId) events.push({kind:"threadReverted",threadId:ctx.threadId});
      continue;
    }
    state.parents ??= Object.fromEntries(ctx.threadParents.map(parent=>[parent.threadId,parent.parentThreadId]));
    if (frame.kind === "notification" && frame.method === "thread/started") {
      const thread=object(params.thread);const id=text(thread.id);
      const parent=text(object(object(object(thread.source).subAgent).thread_spawn).parent_thread_id);
      if (id && parent && (parent === ctx.threadId || Object.prototype.hasOwnProperty.call(state.parents,parent))) state.parents[id]=parent;
      continue;
    }
    if (frame.kind === "notification" && params.threadId !== ctx.threadId && typeof params.threadId === "string" && Object.prototype.hasOwnProperty.call(state.parents,params.threadId) && frame.method !== "serverRequest/resolved") {
      if ((frame.method === "item/started" || frame.method === "item/completed") && ctx.threadId) {
        state.agents ??= {rows:Object.create(null),ordinal:0};
        const agents = reduceRuntimeAgents(state.agents,state.items,state.parents,params.threadId,params.item,state.seq+1);
        if (agents) for (const activity of agents) events.push({kind:"chatItem",threadId:ctx.threadId,item:stamp(state,activity,params)});
      }
      const threadId=params.threadId;const turnId=text(object(params.turn).id) ?? text(params.turnId);
      if (!turnId) continue;
      state.childActive ??= {};state.childCompleted ??= [];
      if (frame.method === "turn/started" && !state.childCompleted.includes(`${threadId}:${turnId}`)) {
        state.childActive[threadId]=turnId;
        events.push({kind:"turnState",threadId,turnId,parentThreadId:state.parents[threadId],state:"working",...timing(params)});
      } else if (frame.method === "turn/completed" && !state.childCompleted.includes(`${threadId}:${turnId}`)) {
        state.childCompleted.push(`${threadId}:${turnId}`);
        if (state.childActive[threadId] && state.childActive[threadId] !== turnId) continue;
        delete state.childActive[threadId];
        events.push({kind:"turnState",threadId,turnId,parentThreadId:state.parents[threadId],state:"idle",...timing(params)});
      }
      continue;
    }
    if (frame.kind === "notification" && frame.method === "account/rateLimits/updated") {
      const limits = object(params.rateLimits);
      if (limits.limitId && limits.limitId !== "codex") continue;
      state.rateLimitSnapshot = {...state.rateLimitSnapshot,...limits};
      events.push(...rateLimitEvents(state.rateLimitSnapshot,params.rateLimitResetCredits));
      continue;
    }
    const descendant = typeof params.threadId === "string" && ctx.threadParents.some(parent => parent.threadId === params.threadId);
    if (ctx.threadId && (params.threadId === ctx.threadId || descendant) && frame.kind === "request") {
      if (!(serverRequestMethods as readonly string[]).includes(frame.method)) continue;
      const prompt = runtimePrompt(frame.method,params,frame.requestId);
      const turnId = descendant ? ctx.activeTurnId : text(params.turnId);
      if (prompt) events.push({kind:"prompt",threadId:ctx.threadId,requestId:frame.requestId,prompt},{kind:"turnState",threadId:ctx.threadId,state:"needs_input",detail:requestDetail(frame.method),...(turnId ? {turnId} : {})});
      continue;
    }
    if (ctx.threadId && descendant && frame.kind === "notification" && frame.method === "serverRequest/resolved") {
      if (typeof params.requestId === "string" || typeof params.requestId === "number") events.push({kind:"promptResolved",threadId:ctx.threadId,requestId:params.requestId});
      continue;
    }
    if (!ctx.threadId || params.threadId !== ctx.threadId) continue;
    const threadId = ctx.threadId;
    const turnId = text(params.turnId) ?? text(object(params.turn).id);
    if (turnId && state.abandoned?.includes(turnId)) continue;
    switch (frame.method) {
      case "turn/started": {
        if (!turnId || state.completed.includes(turnId)) break;
        events.push(...abandonOpenTurn(state,threadId,turnId));
        state.activeTurnId = turnId;
        delete state.compacting;
        const settings = codexReportedSettings({...params,...object(params.turn)},ctx.settings);
        if (Object.keys(settings).length) events.push({kind:"controlsConfirmed",threadId,settings});
        events.push({kind:"turnState",threadId,turnId,state:"working",...timing(params)}); break;
      }
      case "turn/completed": {
        if (!turnId || state.completed.includes(turnId)) break;
        state.completed.push(turnId);
        if (state.activeTurnId && state.activeTurnId !== turnId) break;
        events.push(...interruptPendingCalls(state,threadId,turnId));
        delete state.activeTurnId;
        delete state.compacting;
        const failure = object(object(params.turn).error);
        if (Object.keys(failure).length) {
          const error = runtimeError(failure,false,state.rateLimitSnapshot);
          events.push({kind:"chatItem",threadId,item:stamp(state,{id:`${turnId}:error`,seq:state.seq+1,type:"system",content:error.message,systemSubtype:"api_error",toolInput:error.metadata},params)});
        }
        const outcome = turnOutcome(params.turn);
        events.push({kind:"turnState",threadId,turnId,state:"idle",...(outcome ? {detail:outcome} : {}),...timing(params)}); break;
      }
      case "thread/status/changed": {
        const status = threadStatus(params.status);
        if (!status) break;
        if (status.state === "working" && !state.activeTurnId) break;
        if (status.state === "idle" && state.activeTurnId && !pendingStatus(state,threadId)) break;
        if (status.state === "idle") delete state.compacting;
        events.push((status.state !== "needs_input" && pendingStatus(state,threadId))
          || {kind:"turnState",threadId,...withDetail(status,!!state.compacting),...(state.activeTurnId && status.state !== "idle" ? {turnId:state.activeTurnId} : {})});
        break;
      }
      case "item/fileChange/patchUpdated": {
        const id = text(params.itemId);
        if (!id || !Array.isArray(params.changes) || isRetired(state,id)) break;
        const previous = state.items[id];
        const row = runtimeItem({id,type:"fileChange",changes:params.changes,status:"inProgress"},state.seq+1,previous);
        if (!row) break;
        markOpen(state,id);
        if (previous?.toolResult !== undefined) row.toolResult = previous.toolResult;
        events.push({kind:"chatItem",threadId,item:stamp(state,row,params)}); break;
      }
      case "thread/tokenUsage/updated": {
        const mapped = usage(params); if (mapped) events.push({kind:"usage",threadId,usage:mapped}); break;
      }
      case "thread/settings/updated": {
        const settings = codexReportedSettings(params,ctx.settings);
        if (Object.keys(settings).length) events.push({kind:"controlsConfirmed",threadId,settings});
        break;
      }
      case "serverRequest/resolved":
        if (typeof params.requestId === "string" || typeof params.requestId === "number") events.push({kind:"promptResolved",threadId,requestId:params.requestId}); break;
      case "model/rerouted": {
        const model = text(params.toModel);
        if (model) {
          events.push({kind:"controlsConfirmed",threadId,settings:{model}});
          events.push({kind:"chatItem",threadId,item:stamp(state,{id:`${turnId ?? "thread"}:model-rerouted`,seq:state.seq+1,type:"system",content:`Model changed to ${model}`,systemSubtype:"model_rerouted",toolInput:params},params)});
        }
        break;
      }
      case "thread/goal/cleared":
        events.push({kind:"chatItem",threadId,item:stamp(state,{id:"thread:goal",seq:state.seq+1,type:"custom",content:"",sessionIntent:{}},params)}); break;
      case "turn/diff/updated": {
        const diff = text(params.diff);
        if (diff && turnId) events.push({kind:"chatItem",threadId,item:stamp(state,{id:`${turnId}:diff`,seq:state.seq+1,type:"tool_call",content:"",toolKind:"edit",toolName:"turn_diff",toolResult:diff,collapsed:true},params)});
        break;
      }
      case "thread/goal/updated": {
        const goal = text(object(params.goal).objective);
        if (goal) events.push({kind:"chatItem",threadId,item:stamp(state,{id:"thread:goal",seq:state.seq+1,type:"custom",content:"",sessionIntent:goalIntent(params.goal) ?? {goal}},params)});
        break;
      }
      case "turn/plan/updated": {
        const row = runtimeItem({id:`${turnId}:plan`,type:"planUpdate",plan:params.plan,explanation:params.explanation},state.seq+1);
        if (row) events.push({kind:"chatItem",threadId,item:stamp(state,row,params)}); break;
      }
      case "error": {
        const error = runtimeError(params.error,params.willRetry === true,state.rateLimitSnapshot);
        if (turnId) events.push({kind:"chatItem",threadId,item:stamp(state,{id:`${turnId}:error`,seq:state.seq+1,type:"system",content:error.message,systemSubtype:params.willRetry === true ? "provider_retry" : "api_error",toolInput:error.metadata},params)});
        break;
      }
      default: {
        let row:ChatMsg | undefined;
        const rawItem = object(params.item);
        if (frame.method === "item/started" || frame.method === "item/completed") {
          state.agents ??= {rows:Object.create(null),ordinal:0};
          const agents = reduceRuntimeAgents(state.agents,state.items,state.parents!,threadId,rawItem,state.seq+1);
          if (agents) {
            for (const activity of agents) events.push({kind:"chatItem",threadId,item:stamp(state,activity,params)});
            continue;
          }
        }
        if (frame.method === "item/completed" && rawItem.type === "agentMessage" && rawItem.delivery === "async" && list(rawItem.questions).length) {
          const itemId = text(rawItem.id);
          state.questions ??= [];
          if (itemId && !state.questions.includes(itemId)) {
            state.questions.push(itemId);
            events.push({kind:"question",threadId,itemId,questions:runtimeQuestions(rawItem.questions),response:"message"});
          }
          continue;
        }
        const compactionId = rawItem.type === "contextCompaction" ? text(rawItem.id) : undefined;
        if (compactionId && frame.method === "item/started" && state.activeTurnId && (!turnId || turnId === state.activeTurnId)) {
          if (state.compacting === compactionId || state.compacted.includes(compactionId)) continue;
          state.compacting = compactionId;
          events.push({kind:"turnState",threadId,state:"working",detail:"compacting",...(turnId ? {turnId} : {})});
          continue;
        }
        if (compactionId && frame.method === "item/completed" && state.compacting === compactionId) {
          delete state.compacting;
          if (turnId && state.activeTurnId === turnId) events.push({kind:"turnState",threadId,turnId,state:"working"});
        }
        const reasoningId = text(rawItem.id) ?? text(params.itemId);
        if (reasoningId && (rawItem.type === "reasoning" || frame.method.startsWith("item/reasoning/"))) {
          if (frame.method !== "item/completed" && isRetired(state,reasoningId)) continue;
          state.reasoning ??= {};
          const parts = state.reasoning[reasoningId] ?? {summary:[],content:[]};
          state.reasoning[reasoningId] = parts;
          if (rawItem.type === "reasoning") {
            if (list(rawItem.summary).length) parts.summary = list(rawItem.summary).map(String);
            if (list(rawItem.content).length) parts.content = list(rawItem.content).map(String);
          } else if (frame.method === "item/reasoning/summaryPartAdded") {
            const index = Number(params.summaryIndex);
            if (Number.isSafeInteger(index) && index >= 0 && index < 1024) parts.summary[index] ??= "";
            continue;
          } else {
            const stream = frame.method === "item/reasoning/summaryTextDelta" ? "summary" : "content";
            const index = Number(params.summaryIndex ?? params.contentIndex ?? 0);
            if (Number.isSafeInteger(index) && index >= 0 && index < 1024) parts[stream][index] = (parts[stream][index] ?? "") + String(params.delta ?? "");
          }
          row = stamp(state,{id:reasoningId,seq:state.seq+1,type:"thinking",content:parts.summary.filter(Boolean).join("\n") || parts.content.join("\n"),collapsed:true},params);
          if (frame.method === "item/completed") markClosed(state,reasoningId); else markOpen(state,reasoningId);
        } else row = mapItem(state,frame.method,params);
        if (row) {
          events.push({kind:"chatItem",threadId,item:row});
          if (row.type === "tool_call" && state.activeTurnId && (!turnId || turnId === state.activeTurnId)) events.push({kind:"turnState",threadId,...(state.activeTurnId ? {turnId:state.activeTurnId} : {}),state:"working",...{statusLine:runningCallStatus(state) ?? ""}});
        }
        if (frame.method === "item/started" || frame.method === "item/completed") {
          const evidence = sendEvidence(ctx,object(params.item),turnId);
          if (evidence?.kind === "sendEvidence" && !state.evidenced.includes(evidence.sendId)) {
            state.evidenced.push(evidence.sendId); events.push(evidence);
          }
          const item = object(params.item); const id = text(item.id);
          if (frame.method === "item/completed" && item.type === "contextCompaction" && id && !state.compacted.includes(id)) {
            state.compacted.push(id); events.push({kind:"contextCompacted",threadId,itemId:id});
          }
        }
      }
    }
  }
  compactState(state);
  return budgetReduction(json(state),events);
}

function pendingStatus(state: ReduceState, threadId: string): AgentEvent | undefined {
  const statusLine = runningCallStatus(state);
  return statusLine && state.activeTurnId ? {kind:"turnState",threadId,turnId:state.activeTurnId,state:"working",...{statusLine}} : undefined;
}
