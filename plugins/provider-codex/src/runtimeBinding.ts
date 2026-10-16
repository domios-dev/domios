import type { RuntimeContext } from "@codeterm/plugin-sdk";
import { object, text, userThread } from "./runtimeProtocol";

export function descendantThread(ctx: RuntimeContext, threadId: string): boolean {
  if (!ctx.threadId || threadId === ctx.threadId) return false;
  const seen = new Set<string>();
  let current = threadId;
  while (current !== ctx.threadId) {
    if (seen.has(current)) return false;
    seen.add(current);
    const parents = ctx.threadParents.filter(parent => parent.threadId === current);
    if (parents.length !== 1) return false;
    current = parents[0].parentThreadId;
  }
  return true;
}

export function historyThread(ctx: RuntimeContext, value: unknown, expected: string): boolean {
  if (expected === ctx.threadId) return userThread(value, expected) !== undefined;
  if (!descendantThread(ctx, expected)) return false;
  const thread = object(object(value).thread);
  if (text(thread.id) !== expected) return false;
  const parent = ctx.threadParents.find(entry => entry.threadId === expected)?.parentThreadId;
  const declared = text(thread.parentThreadId);
  const source = text(object(object(object(thread.source).subAgent).thread_spawn).parent_thread_id);
  return (declared !== undefined || source !== undefined)
    && (declared === undefined || declared === parent) && (source === undefined || source === parent);
}
