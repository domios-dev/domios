import type { Pane } from "../../../lib/types";
import type { Task, TaskStatus } from "../types";

export type WorkingOnTone = "info" | "amber" | "red" | "grey";

export type WorkingOnChipKind = "model" | "pane" | "peer" | "branch" | "age" | "warn";

export interface WorkingOnChip {
  kind: WorkingOnChipKind;
  tone: WorkingOnTone;
  label: string;
}

export interface WorkingOnRowModel {
  chips: WorkingOnChip[];
}

const WORKING_STATUSES = new Set<TaskStatus>(["in_progress", "in_review"]);
const LOCAL_PEERS = new Set(["", "local", "localhost", "self"]);

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text ? text : null;
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

// Working-on row for in_progress/in_review tasks: labeled fields from existing
// lane metadata, with honest amber/red fallbacks instead of blank noise.
export function deriveWorkingOnRow(
  task: Task,
  panes: Record<string, Pane>,
  now: number = Date.now(),
): WorkingOnRowModel | null {
  if (!WORKING_STATUSES.has(task.status)) return null;
  const chips: WorkingOnChip[] = [];

  const model = clean(task.model);
  if (model) chips.push({ kind: "model", tone: "info", label: model });

  const peer = remotePeer(task);
  const paneId = clean(task.assigned_pane_id) ?? clean(task.lane?.pane_id);
  if (paneId) {
    const pane = panes[paneId];
    if (pane) {
      chips.push({ kind: "pane", tone: "info", label: clean(pane.title) ?? shortId(paneId) });
    } else if (peer) {
      chips.push({ kind: "pane", tone: "grey", label: shortId(paneId) });
    } else {
      chips.push({ kind: "warn", tone: "red", label: "pane gone" });
    }
  } else {
    chips.push({ kind: "warn", tone: "amber", label: "no pane recorded" });
  }

  if (peer) chips.push({ kind: "peer", tone: "grey", label: peer });

  const branch = clean(task.lane?.branch);
  if (branch) chips.push({ kind: "branch", tone: "info", label: branch });
  else chips.push({ kind: "warn", tone: "amber", label: "branch missing" });

  const age = ageLabel(activityTs(task), now);
  if (age) chips.push({ kind: "age", tone: "info", label: age });

  return { chips };
}
