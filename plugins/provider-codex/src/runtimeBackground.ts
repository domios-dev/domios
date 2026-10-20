import type {Operation, RuntimeContext, RuntimePlan, RuntimeStep, RpcReply, RuntimeBackgroundProcess,JsonValue} from "@codeterm/plugin-sdk";
import {object} from "./runtimeProtocol";

type BackgroundOperation = Extract<Operation,{kind:"backgroundProcesses"}>;
const identifier = (value:unknown):value is string => typeof value === "string" && value.length > 0 && value.length <= 4096 && !value.includes("\0");
function invalid(message:string):never {throw {code:"invalid_schema",message};}

export function backgroundOwns(ctx:RuntimeContext, thread:string):boolean {
  const seen = new Set<string>();
  while (thread !== ctx.threadId) {
    if (seen.has(thread)) return false;
    seen.add(thread);
    const parent = ctx.threadParents.find(parent=>parent.threadId === thread);
    if (!parent) return false;
    thread = parent.parentThreadId;
  }
  return !!ctx.threadId;
}

export function backgroundPlan(ctx:RuntimeContext, op:BackgroundOperation):RuntimePlan {
  if (!backgroundOwns(ctx,op.threadId)) throw {code:"binding_mismatch",message:"Background thread is not owned"};
  const params:Record<string,JsonValue> = {threadId:op.threadId};
  if (op.action.kind === "list") {params.cursor=op.action.cursor;params.limit=256;}
  if (op.action.kind === "terminate") params.processId=op.action.processId;
  if (op.action.kind === "terminate" && !identifier(op.action.processId)) invalid("Invalid background process id");
  if (op.action.kind === "list" && op.action.cursor !== null && !identifier(op.action.cursor)) invalid("Invalid background cursor");
  return {action:{kind:"request",call:{method:`thread/backgroundTerminals/${op.action.kind}`,params,target:{threadId:op.threadId}}},state:null};
}

export function backgroundResponse(op:BackgroundOperation, reply:RpcReply):RuntimeStep {
  if (reply.kind === "error" && reply.error.code === -32600 && reply.error.data == null
    && reply.error.message === `thread not found: ${op.threadId}`) {
    if (op.action.kind === "list") return {kind:"complete",result:{kind:"backgroundProcesses",threadId:op.threadId,processes:[],nextCursor:null}};
    if (op.action.kind === "terminate") return {kind:"complete",result:{kind:"backgroundProcessTermination",threadId:op.threadId,processId:op.action.processId,acknowledged:false}};
    return {kind:"complete",result:{kind:"done"}};
  }
  if (reply.kind === "error") return {kind:"refused",reason:{code:"provider_refused",message:reply.error.message},beforeAcceptance:false};
  const value = object(reply.value);
  if (op.action.kind === "clean") return {kind:"complete",result:{kind:"done"}};
  if (op.action.kind === "terminate") {
    if (typeof value.terminated !== "boolean") invalid("Missing termination acknowledgement");
    return {kind:"complete",result:{kind:"backgroundProcessTermination",threadId:op.threadId,processId:op.action.processId,acknowledged:value.terminated}};
  }
  if (!Array.isArray(value.data) || value.data.length > 256) invalid("Invalid background process page");
  if (value.nextCursor !== null && !identifier(value.nextCursor)) invalid("Missing or invalid background cursor");
  if (value.nextCursor !== null && value.nextCursor === op.action.cursor) invalid("Repeated background cursor");
  const seen = new Set<string>();
  const processes:RuntimeBackgroundProcess[] = value.data.map(raw=>{
    const entry = object(raw);
    if (!identifier(entry.processId) || !identifier(entry.itemId) || !identifier(entry.cwd)
      || typeof entry.command !== "string" || entry.command.length > 65536 || entry.command.includes("\0")
      || (entry.osPid !== null && (!Number.isInteger(entry.osPid) || Number(entry.osPid) < 1 || Number(entry.osPid) > 4294967295))
      || seen.has(entry.processId)) invalid("Invalid background process entry");
    seen.add(entry.processId);
    return {processId:entry.processId,itemId:entry.itemId,command:entry.command,cwd:entry.cwd,osPid:entry.osPid as number|null};
  });
  return {kind:"complete",result:{kind:"backgroundProcesses",threadId:op.threadId,processes,nextCursor:value.nextCursor as string|null}};
}
