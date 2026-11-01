import { parseDsBlocks, serializeDsBlock, type DsTaskItem, type DsTaskStatus } from "@codeterm/chat-core";
export interface DsTasksBlockItem { id: string; key: string; status: string; name: string }
export interface DsTasksBlock { planId: string; planTitle: string; filter: string[]; items: DsTasksBlockItem[] }
export type DsTaskGroupKey = "in_progress" | "in_review" | "blocked" | "todo" | "done" | "cancelled";
export const DS_TASK_GROUP_ALIASES: Readonly<Record<string, DsTaskGroupKey>> = {
  in_progress: "in_progress", in_review: "in_review", blocked: "blocked", todo: "todo", pending: "todo", paused: "todo", done: "done", cancelled: "cancelled",
};
export const DS_PLAN_ID_RE = /^pl_[A-Za-z0-9_-]+$/;
export const DS_TASK_ID_RE = /^task_[A-Za-z0-9_-]+$/;
export const DS_TASK_KEY_RE = /^[A-Za-z][A-Za-z0-9]*-\d+$/;
export { isDsTaskRef } from "@codeterm/chat-core";
export interface ParsedDsTasksMessage { before: string; block: DsTasksBlock; after: string }
export function formatDsTasksBlock(block: DsTasksBlock): string {
  return serializeDsBlock({ kind: "tasks", plan: block.planId, refs: [], statuses: block.filter as DsTaskStatus[], snapshot: { title: block.planTitle, items: block.items as DsTaskItem[] } });
}
export function parseDsTasksMessage(content: string): ParsedDsTasksMessage | null {
  const segment = parseDsBlocks(content).find((part) => part.type === "block" && part.block.kind === "tasks" && part.block.snapshot);
  if (segment?.type !== "block" || segment.block.kind !== "tasks" || !segment.block.snapshot) return null;
  const block = segment.block;
  return { before: content.slice(0, segment.start).trim(), after: content.slice(segment.end).trim(), block: { planId: block.plan, planTitle: block.snapshot!.title, filter: block.statuses, items: block.snapshot!.items } };
}
