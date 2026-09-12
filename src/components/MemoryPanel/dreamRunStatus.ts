import type { DiaryFinalization, GraphWorkerInfo } from "../../lib/types";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";

export interface DiaryCompleteness {
  complete: boolean;
  placeholderInsight: boolean;
  pendingFields: string[];
}

const PLACEHOLDER_INSIGHT_TOKEN = "wip";
const PENDING_FIELD_VALUES = new Set(["pending", "not_run", "not run", "timeout"]);

function taggedBlock(body: string, tag: string): string | null {
  const open = `<${tag}>`;
  const close = `</${tag}>`;
  const start = body.indexOf(open);
  if (start < 0) return null;
  const end = body.indexOf(close, start + open.length);
  if (end < 0) return null;
  return body.slice(start + open.length, end);
}

function insightIsPlaceholder(body: string): boolean {
  const insight = taggedBlock(body, "insight");
  if (insight === null) return body.trim().length === 0;
  const text = insight.trim();
  if (text.length === 0) return true;
  const first = text.split(/\s+/)[0].replace(/[:\-.]+$/, "").toLowerCase();
  return first === PLACEHOLDER_INSIGHT_TOKEN;
}

function pendingLogFields(body: string): string[] {
  const log = taggedBlock(body, "log");
  if (log === null) return [];
  const pending: string[] = [];
  for (const line of log.split("\n")) {
    const colon = line.indexOf(":");
    if (colon < 0) continue;
    const name = line.slice(0, colon).trim();
    if (!name || !/^[A-Z_]+$/.test(name)) continue;
    const value = line.slice(colon + 1).trim();
    const unfilled =
      value.length === 0 || value.startsWith("<") || PENDING_FIELD_VALUES.has(value.toLowerCase());
    if (unfilled && !pending.includes(name)) pending.push(name);
  }
  return pending;
}

// Placeholder and pending markers prove a diary is incomplete. They never
// imply its run is live — liveness comes from the worker records.
export function diaryCompleteness(body: string | null | undefined): DiaryCompleteness {
  const text = body ?? "";
  const placeholderInsight = insightIsPlaceholder(text);
  const pendingFields = pendingLogFields(text);
  return {
    complete: !placeholderInsight && pendingFields.length === 0,
    placeholderInsight,
    pendingFields,
  };
}

export type DiaryRecordState =
  | { state: "complete" }
  | { state: "incomplete_live"; workerId: string }
  | { state: "incomplete_recoverable"; reason: string };

export function describeIncompleteness(
  diaryId: string,
  completeness: DiaryCompleteness,
): string {
  const parts: string[] = [];
  if (completeness.placeholderInsight) parts.push("insight is still the skeleton placeholder");
  if (completeness.pendingFields.length > 0) {
    parts.push(`pending fields: ${completeness.pendingFields.join(", ")}`);
  }
  if (parts.length === 0) return `diary ${diaryId} is complete`;
  return `diary ${diaryId} incomplete — ${parts.join("; ")}`;
}

// Classify one stored diary. The live owner is resolved from worker records
// by diary identity, never from the diary body.
export function diaryRecordState(
  diaryId: string,
  body: string | null | undefined,
  workers: GraphWorkerInfo[],
): DiaryRecordState {
  const completeness = diaryCompleteness(body);
  if (completeness.complete) return { state: "complete" };
  const owner = workers.find(
    (w) => w.diary_id === diaryId && !isTerminalWorkerStatus(w.status),
  );
  if (owner) return { state: "incomplete_live", workerId: owner.worker_id };
  return { state: "incomplete_recoverable", reason: describeIncompleteness(diaryId, completeness) };
}

export interface DreamRunLabel {
  label: string;
  color: string;
  reason: string | null;
}

const RUNNING: DreamRunLabel = { label: "running", color: "#fbbf24", reason: null };

// Ending execution is not finalizing an artifact. A run whose execution ended
// without finalization evidence reads as unverified, never as done.
export function dreamRunLabel(worker: {
  status: string;
  finalization?: DiaryFinalization | null;
}): DreamRunLabel {
  if (!isTerminalWorkerStatus(worker.status)) return RUNNING;
  const finalization = worker.finalization;
  switch (finalization?.state) {
    case "succeeded":
      return { label: "done", color: "#34d399", reason: null };
    case "partial":
      return { label: "partial", color: "#fb923c", reason: finalization.reason };
    case "failed":
      return { label: "failed", color: "#f87171", reason: finalization.reason };
    default:
      // Execution ended with no finalization evidence: honest, but not a fault
      // to spell out on every historical row.
      return { label: "unverified", color: "#9ca3af", reason: null };
  }
}
