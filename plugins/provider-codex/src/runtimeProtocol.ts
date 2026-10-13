export type CodexObject = Record<string, unknown>;

export function object(value: unknown): CodexObject {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as CodexObject : {};
}

export function text(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

export function list(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

export function userThread(value: unknown, expected?: string): string | undefined {
  const thread = object(object(value).thread);
  const id = text(thread.id);
  if (!id || (expected && expected !== id) || thread.parentThreadId != null
    || object(thread.source).subAgent != null
    || String(thread.threadSource ?? "").toLowerCase().startsWith("subagent")) return undefined;
  return id;
}

export function steerRefused(error: {code: number; message: string; data?: unknown}, turn: string): boolean {
  return error.code === -32600 && error.data == null && (error.message === "no active turn to steer"
    || (error.message.startsWith(`expected active turn id \`${turn}\` but found \``) && error.message.endsWith("`")));
}

export function archivedResume(message: string): boolean {
  const words = message.toLowerCase().split(/\s+/).map(word => word.replace(/^[`"']+|[`"'.:;,]+$/g,""));
  return words.some((word,index) => (word === "codex" && words[index+1] === "unarchive")
    || (word === "session" && !!words[index+1] && words[index+2] === "is" && words[index+3] === "archived"));
}

export const notificationMethods = [
  "account/updated", "thread/started", "thread/status/changed", "thread/settings/updated", "thread/tokenUsage/updated",
  "thread/compacted", "thread/goal/updated", "thread/goal/cleared", "model/rerouted", "turn/diff/updated", "account/rateLimits/updated", "turn/started", "turn/completed", "turn/plan/updated",
  "item/started", "item/completed", "item/agentMessage/delta", "item/plan/delta",
  "item/reasoning/textDelta", "item/reasoning/summaryTextDelta", "item/commandExecution/outputDelta",
  "item/fileChange/outputDelta", "item/mcpToolCall/progress", "serverRequest/resolved", "error",
] as const;

export const serverRequestMethods = [
  "item/commandExecution/requestApproval", "item/fileChange/requestApproval",
  "item/permissions/requestApproval", "item/tool/requestUserInput", "mcpServer/elicitation/request",
] as const;

export interface CodexTurn {
  id:string;
  status:"inProgress"|"completed"|"interrupted"|"failed";
  startedAt?:number|null;
  completedAt?:number|null;
  error?:{message:string;codexErrorInfo?:unknown}|null;
}
export interface CodexThread {
  id:string;
  parentThreadId?:string|null;
  source?:string|{subAgent:{thread_spawn:{parent_thread_id:string;depth:number}}};
  threadSource?:string|null;
}
export interface CodexPage<T> {data:T[];nextCursor?:string|null}
export interface CodexTokenUsage {
  total:{inputTokens:number;outputTokens:number;cachedInputTokens?:number;cacheWriteInputTokens?:number};
  last:{totalTokens:number};
  modelContextWindow?:number|null;
}
export interface CodexRateLimitWindow {usedPercent:number;resetsAt?:number|null;windowDurationMins?:number|null}
export interface CodexRateLimitSnapshot {limitId?:string|null;primary?:CodexRateLimitWindow|null;secondary?:CodexRateLimitWindow|null}
export interface CodexAccountRead {account?:{type:string;planType?:string|null}|null;requiresOpenaiAuth?:boolean}
export interface CodexQuestion {id?:string;title?:string;question?:string;options?:readonly (string|{id?:string;value?:string;label:string})[]}
