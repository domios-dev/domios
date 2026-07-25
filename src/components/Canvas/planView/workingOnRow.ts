import type { Pane } from "../../../lib/types";
import type { Task, TaskStatus } from "../types";
import { CHIP_FIELD_LABELS, pickHighestLaneWarning, type LaneWarningId } from "./laneWatchdogLabels";
import { middleTruncateSlug } from "./textTruncate";

export type WorkingOnTone = "info" | "amber" | "red" | "grey";

export type WorkingOnChipKind = "model" | "pane" | "peer" | "branch" | "age" | "warn";

export interface WorkingOnChip {
  kind: WorkingOnChipKind;
  tone: WorkingOnTone;
  fieldLabel: string;
  label: string;
}

export interface WorkingOnRowModel {
  chips: WorkingOnChip[];
}

const WORKING_STATUSES = new Set<TaskStatus>(["in_progress", "in_review"]);
const LOCAL_PEERS = new Set(["", "local", "localhost", "self"]);
const CHIP_ORDER: WorkingOnChipKind[] = ["model", "pane", "peer", "branch", "age", "warn"];

const EMPTY_LABELS = new Set(["", "unknown"]);

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text && !EMPTY_LABELS.has(text.toLowerCase()) ? text : null;
}

function shortId(id: string): string {
  return id.length > 8 ? id.slice(0, 8) : id;
}

function remotePeer(task: Task): string | null {
  const peer = clean(task.peer);
  return peer && !LOCAL_PEERS.has(peer.toLowerCase()) ? peer : null;
}

function activityTs(task: Task): number {
  let ts = task.status_changed_at ?? task.updated_at ?? task.created_at ?? 0;
  for (const m of task.milestones) if (m.ts > ts) ts = m.ts;
  return ts;
}

function ageLabel(ts: number, now: number): string | null {
  if (!ts || ts <= 0) return null;
  const secs = Math.max(0, Math.floor((now - ts) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

function chip(
  kind: WorkingOnChipKind,
  tone: WorkingOnTone,
  label: string,
): WorkingOnChip {
  return { kind, tone, fieldLabel: CHIP_FIELD_LABELS[kind], label };
}

function sortChips(chips: WorkingOnChip[]): WorkingOnChip[] {
  return [...chips].sort(
    (a, b) => CHIP_ORDER.indexOf(a.kind) - CHIP_ORDER.indexOf(b.kind),
  );
}

// Lane hygiene warnings (branch missing, tab gone) are active-task-only via working-on chips.
// Done/paused/pending cards surface branch metadata when present, not duplicate warn chips.
export function deriveWorkingOnRow(
  task: Task,
  panes: Record<string, Pane>,
  now: number = Date.now(),
): WorkingOnRowModel | null {
  if (!WORKING_STATUSES.has(task.status)) return null;

  const chips: WorkingOnChip[] = [];
  const warnings: LaneWarningId[] = [];

  const model = clean(task.model);
  if (model) chips.push(chip("model", "info", model));

  const peer = remotePeer(task);
  const paneId = clean(task.assigned_pane_id) ?? clean(task.lane?.pane_id);
  if (paneId) {
    const pane = panes[paneId];
    if (pane) {
      chips.push(chip("pane", "info", clean(pane.title) ?? shortId(paneId)));
    } else if (peer) {
      chips.push(chip("pane", "grey", shortId(paneId)));
    } else {
      warnings.push("pane_gone");
    }
  } else {
    warnings.push("no_pane_recorded");
  }

  if (peer) chips.push(chip("peer", "grey", peer));

  const branch = clean(task.lane?.branch);
  if (branch) chips.push(chip("branch", "info", middleTruncateSlug(branch)));
  else warnings.push("branch_missing");

  const age = ageLabel(activityTs(task), now);
  if (age) chips.push(chip("age", "info", age));

  const warn = pickHighestLaneWarning(warnings);
  if (warn) chips.push(chip("warn", warn.tone, warn.label));

  return { chips: sortChips(chips) };
}
