import { goalIntent } from "./runtimeGoal";
import type { ChatMsg, JsonValue, OpenItem, Operation, RpcReply, RuntimeHistoryQuestion, RuntimePlan, RuntimeStep } from "@codeterm/plugin-sdk";
import { HISTORY_CHUNK_BYTES, serializedBytes } from "./runtimeHistoryBounds";
import { projectHistoryItem } from "./runtimeStoredHistory";
import { reconcileHistoryQuestions } from "./runtimeQuestions";
import { object, text } from "./runtimeProtocol";

type Read = Extract<Operation,{kind:"readHistory"}>;
interface Position {
  pager: 1;
  thread: string;
  stage: "turns"|"items"|"goal";
  cursor: string|null;
  turn?: {id:string;status:string;startedAt:number|null};
  nextTurn?: string|null;
  offset?: number;
  historyItemLimit?: number;
}
const PREFIX = "history:";
function failure(message:string):never { throw {code:"provider_refused",message}; }
function encode(position:Position):string { return PREFIX + JSON.stringify(position); }
function decode(op:Read):Position {
  if (!op.cursor) return {pager:1,thread:op.threadId,stage:"turns",cursor:null,historyItemLimit:16};
  if (!op.cursor.startsWith(PREFIX) || op.cursor.length > 128 * 1024) failure("Invalid history checkpoint");
  const position = JSON.parse(op.cursor.slice(PREFIX.length)) as Position;
  if (position.pager !== 1 || position.thread !== op.threadId || !["turns","items","goal"].includes(position.stage)
    || (position.cursor !== null && typeof position.cursor !== "string")
    || (position.stage === "items" && !text(object(position.turn).id))) failure("History checkpoint differs from bound thread");
  position.historyItemLimit ??= 16;
  if (!Number.isSafeInteger(position.historyItemLimit) || position.historyItemLimit < 1 || position.historyItemLimit > 16) failure("Invalid history item limit");
  return position;
}
export function pagedHistoryPlan(op:Read):RuntimePlan {
  const state = decode(op);
  const params = {threadId:op.threadId,sortDirection:"desc",cursor:state.cursor};
  const call = state.stage === "items" ? {method:"thread/items/list",params:{...params,turnId:state.turn!.id,limit:state.historyItemLimit}}
    : state.stage === "goal" ? {method:"thread/goal/get",params:{threadId:op.threadId}}
    : {method:"thread/turns/list",params:{...params,itemsView:"notLoaded",limit:1}};
  return {action:{kind:"request",call:{...call,params:call.params as JsonValue}},state:state as unknown as JsonValue};
}
function nextCursor(value:Record<string,unknown>, previous:string|null):string|null {
  const next = text(value.nextCursor) ?? null;
  if (next && (next.length > 4096 || next === previous)) failure("History RPC cursor did not advance");
  return next;
}
function finish(op:Read, items:ChatMsg[], openItems:OpenItem[], questions:RuntimeHistoryQuestion[], next?:Position):RuntimeStep {
  return {kind:"complete",result:{kind:"historyPage",threadId:op.threadId,order:"desc",items,openItems,
    ...(questions.length ? {questions} : {}),...(next ? {nextCursor:encode(next)} : {})}};
}
export function pagedHistoryResponse(op:Read, raw:Record<string,unknown>, reply:RpcReply):RuntimeStep {
  const state = raw as unknown as Position;
  if (state.thread !== op.threadId) failure("History checkpoint differs from bound thread");
  if (reply.kind === "error") {
    const smaller = object(reply.error.data).historyPageLimit;
    if (state.stage === "items" && reply.error.code === -32097
      && typeof smaller === "number" && Number.isSafeInteger(smaller) && smaller >= 1 && smaller < (state.historyItemLimit ?? 16)) {
      return {kind:"continue",plan:pagedHistoryPlan({...op,cursor:encode({...state,historyItemLimit:smaller})})};
    }
    if (state.stage === "goal") return finish(op,[],[],[]);
    failure(`History ${state.stage} read failed (${reply.error.code}): ${reply.error.message}`);
  }
  const value = object(reply.value);
  if (state.stage === "goal") {
    const goal = text(object(value.goal).objective);
    return finish(op,goal ? [{id:"thread:goal",seq:0,type:"custom",content:"",sessionIntent:goalIntent(value.goal) ?? {goal}}] : [],[],[]);
  }
  if (!Array.isArray(value.data)) failure("History page has no data array");
  const next = nextCursor(value,state.cursor);
  if (state.stage === "turns") {
    if (value.data.length > 1) failure("History turn page exceeds requested limit");
    const turn = object(value.data[0]);
    if (!value.data.length) return finish(op,[],[],[], next ? {...state,cursor:next} : {...state,stage:"goal",cursor:null});
    if (!text(turn.id) || String(turn.id).length > 4096) failure("History turn identity missing or oversized");
    const position:Position = {pager:1,thread:op.threadId,stage:"items",cursor:null,nextTurn:next,
      turn:{id:String(turn.id),status:turn.status === "inProgress" ? "inProgress" : "completed",startedAt:typeof turn.startedAt === "number" && Number.isFinite(turn.startedAt) ? turn.startedAt : null}};
    return {kind:"continue",plan:pagedHistoryPlan({...op,cursor:encode(position)})};
  }
  if (value.data.length > (state.historyItemLimit ?? 16)) failure("History item page exceeds requested limit");
  const items:ChatMsg[] = [], openItems:OpenItem[] = [];
  let questions:RuntimeHistoryQuestion[] = [], bytes = 0;
  const offset = state.offset ?? 0;
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > value.data.length) failure("History item offset is invalid");
  for (let index=offset; index<value.data.length; index++) {
    const record = object(value.data[index]), item = record.item ?? record;
    const rows = projectHistoryItem(item,0,state.turn!.id,record.startedAtMs ?? record.completedAtMs,op.threadId);
    const nextQuestions = reconcileHistoryQuestions(op.threadId,item,questions);
    const active = state.turn!.status === "inProgress" && state.turn!.startedAt != null && record.completedAtMs == null
      && !["completed","failed","interrupted","declined"].includes(String(object(item).status));
    const opened = active ? rows.map(item=>({turnId:state.turn!.id,item})) : [];
    const used = serializedBytes({rows,opened,questions:nextQuestions});
    if (bytes+used>HISTORY_CHUNK_BYTES && index>offset) return finish(op,items,openItems,questions,{...state,offset:index});
    bytes += used;
    items.unshift(...rows);openItems.unshift(...opened);questions=nextQuestions;
  }
  const position:Position = next ? {...state,cursor:next,offset:0}
    : {pager:1,thread:op.threadId,stage:state.nextTurn ? "turns" : "goal",cursor:state.nextTurn ?? null};
  return finish(op,items,openItems,questions,position);
}
