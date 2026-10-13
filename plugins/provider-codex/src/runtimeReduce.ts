import type { RuntimeContext, JsonValue, RuntimeFrame, RuntimeReduction, AgentEvent, ChatMsg, TokenUsage } from "@codeterm/plugin-sdk";
import { object, text, list, serverRequestMethods } from "./runtimeProtocol";
import { json, runtimeItem, runtimeItems } from "./runtimeMessages";
import { runtimeError } from "./runtimeErrors";
import { notificationAuth } from "./runtimeAuth";
import { runtimeTurnDiff } from "./runtimeDiffs";
import { runtimeQuestions } from "./runtimeQuestions";
import { runtimePrompt } from "./runtimePrompts";

interface ReduceState {
  epoch: number;
  bindingNonce: string;
  threadId?: string;
  seq: number;
  items: Record<string,ChatMsg>;
  completed: string[];
  compacted: string[];
  evidenced: string[];
  questions?:string[];
  activeTurnId?: string;
  parents?:Record<string,string>;
  childActive?:Record<string,string>;
  childCompleted?:string[];
  rateLimitSnapshot?:Record<string,unknown>;
  reasoning?:Record<string,{summary:string[]; content:string[]}>;
}
function initial(ctx: RuntimeContext, raw: JsonValue): ReduceState {
  const state = object(raw);
  if (state.epoch === ctx.epoch && state.bindingNonce === ctx.bindingNonce && state.threadId === ctx.threadId) {
    const restored=JSON.parse(JSON.stringify(state)) as ReduceState;
    restored.items=Object.assign(Object.create(null),restored.items);
    if(restored.parents) restored.parents=Object.assign(Object.create(null),restored.parents);
    return restored;
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
  return {input_tokens:total.inputTokens,output_tokens:total.outputTokens,
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
  return {kind:"sendEvidence",threadId:claim.threadId,sendId:claim.sendId,turnId};
}
function stamp(state: ReduceState, row: ChatMsg, params: Record<string,unknown>): ChatMsg {
  const previous = state.items[row.id];
  row.seq = previous?.seq ?? ++state.seq;
  const ms = params.startedAtMs ?? params.completedAtMs;
  if (previous?.timestamp) row.timestamp = previous.timestamp;
  else if (typeof ms === "number" && Number.isFinite(ms)) row.timestamp = new Date(ms).toISOString();
  const turnId = text(params.turnId) ?? text(object(params.turn).id);
  if (turnId) row.toolFlowId = turnId;
  state.items[row.id] = row;
  return row;
}
function mapItem(state: ReduceState, method: string, params: Record<string,unknown>): ChatMsg | undefined {
  const raw = object(params.item);
  const id = text(raw.id) ?? text(params.itemId);
  if (!id) return undefined;
  const previous = state.items[id];
  if (method === "item/started" || method === "item/completed") {
    const row = runtimeItem(raw,state.seq + 1,previous);
    return row ? stamp(state,row,params) : undefined;
  }
  const delta = text(params.delta) ?? text(params.message);
  if (!delta) return undefined;
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
  return stamp(state,row,params);
}

export function runtimeReduce(ctx: RuntimeContext, raw: JsonValue, frames: readonly RuntimeFrame[]): RuntimeReduction {
  const state = initial(ctx,raw);
  const events: AgentEvent[] = [];
  for (const frame of frames) {
    const params = object(frame.params);
    if (frame.kind === "notification" && frame.method === "account/updated") {
      events.push(notificationAuth(params)); continue;
    }
    state.parents ??= Object.fromEntries(ctx.threadParents.map(parent=>[parent.threadId,parent.parentThreadId]));
    if (frame.kind === "notification" && frame.method === "thread/started") {
      const thread=object(params.thread);const id=text(thread.id);
      const parent=text(object(object(object(thread.source).subAgent).thread_spawn).parent_thread_id);
      if (id && parent && (parent === ctx.threadId || Object.prototype.hasOwnProperty.call(state.parents,parent))) state.parents[id]=parent;
      continue;
    }
    if (frame.kind === "notification" && params.threadId !== ctx.threadId && typeof params.threadId === "string" && Object.prototype.hasOwnProperty.call(state.parents,params.threadId)) {
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
      const windows = ["primary","secondary"].flatMap(window => {
        const value = object(state.rateLimitSnapshot?.[window]);
        if (typeof value.usedPercent !== "number" || !Number.isFinite(value.usedPercent)) return [];
        return [{window,usedPercent:Math.max(0,Math.min(100,value.usedPercent)),
          ...(typeof value.resetsAt === "number" && value.resetsAt >= 0 ? {resetsAt:value.resetsAt * 1000} : {}),
          ...(typeof value.windowDurationMins === "number" && value.windowDurationMins > 0 ? {windowMinutes:value.windowDurationMins} : {})}];
      });
      if (windows.length) events.push({kind:"rateLimits",rateLimits:windows});
      continue;
    }
    if (!ctx.threadId || params.threadId !== ctx.threadId) continue;
    const threadId = ctx.threadId;
    const turnId = text(params.turnId) ?? text(object(params.turn).id);
    if (frame.kind === "request") {
      if (!(serverRequestMethods as readonly string[]).includes(frame.method)) continue;
      const prompt = runtimePrompt(frame.method,params,frame.requestId);
      if (prompt) events.push({kind:"prompt",threadId,requestId:frame.requestId,prompt},{kind:"turnState",threadId,state:"needs_input",...(turnId ? {turnId} : {})});
      continue;
    }
    switch (frame.method) {
      case "turn/started":
        if (!turnId || state.completed.includes(turnId)) break;
        state.activeTurnId = turnId;
        events.push({kind:"turnState",threadId,turnId,state:"working",...timing(params)}); break;
      case "turn/completed": {
        if (!turnId || state.completed.includes(turnId)) break;
        state.completed.push(turnId);
        if (state.activeTurnId && state.activeTurnId !== turnId) break;
        delete state.activeTurnId;
        const error = text(object(object(params.turn).error).message);
        if (error) events.push({kind:"chatItem",threadId,item:stamp(state,{id:`${turnId}:error`,seq:state.seq+1,type:"system",content:error,systemSubtype:"api_error"},params)});
        events.push({kind:"turnState",threadId,turnId,state:"idle",...timing(params)}); break;
      }
      case "thread/status/changed": {
        const status = object(params.status);
        const type = typeof params.status === "string" ? params.status : status.type;
        if (type === "idle") events.push({kind:"turnState",threadId,state:"idle"});
        else if (type === "active") events.push({kind:"turnState",threadId,state:list(status.activeFlags).some(f => f === "waitingOnApproval" || f === "waitingOnUserInput") ? "needs_input" : "working"});
        break;
      }
      case "thread/tokenUsage/updated": {
        const mapped = usage(params); if (mapped) events.push({kind:"usage",threadId,usage:mapped}); break;
      }
      case "thread/settings/updated": {
        const settings = object(params.threadSettings);
        const model = text(settings.model); const effort = text(settings.effort);
        const sandbox = object(settings.sandboxPolicy).type;
        const mode = settings.approvalPolicy === "untrusted" ? "approval-required"
          : settings.approvalPolicy === "never" && sandbox === "dangerFullAccess" ? "full-access"
          : settings.approvalPolicy === "on-request" && settings.approvalsReviewer === "auto_review" ? "auto"
          : settings.approvalPolicy === "on-request" && settings.approvalsReviewer === "user" && sandbox === "workspaceWrite" ? "auto-accept-edits" : undefined;
        if (model || effort || mode) events.push({kind:"controlsConfirmed",threadId,settings:{...(model ? {model} : {}),...(effort ? {effort} : {}),...(mode ? {mode} : {})}});
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
        if (diff && turnId) events.push({kind:"chatItem",threadId,item:stamp(state,{id:`${turnId}:diff`,seq:state.seq+1,type:"tool_call",content:"",toolKind:"edit",toolName:"turn_diff",fileDiffs:runtimeTurnDiff(diff),toolResult:diff,collapsed:true},params)});
        break;
      }
      case "thread/goal/updated": {
        const goal = text(object(params.goal).objective);
        if (goal) events.push({kind:"chatItem",threadId,item:stamp(state,{id:"thread:goal",seq:state.seq+1,type:"custom",content:"",sessionIntent:{goal}},params)});
        break;
      }
      case "turn/plan/updated": {
        const row = runtimeItem({id:`${turnId}:plan`,type:"planUpdate",plan:params.plan,explanation:params.explanation},state.seq+1);
        if (row) events.push({kind:"chatItem",threadId,item:stamp(state,row,params)}); break;
      }
      case "error": {
        const error = runtimeError(params.error,params.willRetry === true);
        if (turnId) events.push({kind:"chatItem",threadId,item:stamp(state,{id:`${turnId}:error`,seq:state.seq+1,type:"system",content:error.message,systemSubtype:params.willRetry === true ? "provider_retry" : "api_error",toolInput:error.metadata},params)});
        break;
      }
      default: {
        let row:ChatMsg | undefined;
        const rawItem = object(params.item);
        if (frame.method === "item/completed" && rawItem.type === "agentMessage" && rawItem.delivery === "async" && list(rawItem.questions).length) {
          const itemId = text(rawItem.id);
          state.questions ??= [];
          if (itemId && !state.questions.includes(itemId)) {
            state.questions.push(itemId);
            events.push({kind:"question",threadId,itemId,questions:runtimeQuestions(rawItem.questions),response:"message"});
          }
          continue;
        }
        const reasoningId = text(rawItem.id) ?? text(params.itemId);
        if (reasoningId && (rawItem.type === "reasoning" || frame.method.startsWith("item/reasoning/"))) {
          state.reasoning ??= {};
          const parts = state.reasoning[reasoningId] ?? {summary:[],content:[]};
          state.reasoning[reasoningId] = parts;
          if (rawItem.type === "reasoning") {
            if (list(rawItem.summary).length) parts.summary = list(rawItem.summary).map(String);
            if (list(rawItem.content).length) parts.content = list(rawItem.content).map(String);
          } else {
            const stream = frame.method === "item/reasoning/summaryTextDelta" ? "summary" : "content";
            const index = Number(params.summaryIndex ?? params.contentIndex ?? 0);
            if (Number.isSafeInteger(index) && index >= 0 && index < 1024) parts[stream][index] = (parts[stream][index] ?? "") + String(params.delta ?? "");
          }
          row = stamp(state,{id:reasoningId,seq:state.seq+1,type:"thinking",content:parts.summary.join("\n") || parts.content.join("\n"),collapsed:true},params);
        } else if ((frame.method === "item/started" || frame.method === "item/completed") && rawItem.type === "collabAgentToolCall" && list(rawItem.receiverThreadIds).length) {
          for (const activity of runtimeItems(rawItem,state.seq+1)) events.push({kind:"chatItem",threadId,item:stamp(state,activity,params)});
        } else row = mapItem(state,frame.method,params);
        if (row) events.push({kind:"chatItem",threadId,item:row});
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
  return {state:json(state),events};
}
