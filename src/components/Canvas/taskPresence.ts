import type { Pane } from "../../lib/types";
import type { AgentRef, Milestone, Plan, Task } from "./types";

export interface TaskPresenceLatest {
  authorLabel: string;
  kindLabel: string;
}

export interface TaskPresence {
  paneId: string;
  label: string;
  focus: string | null;
  latest: TaskPresenceLatest | null;
}

const EMPTY_LABELS = new Set(["", "unknown"]);

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text && !EMPTY_LABELS.has(text.toLowerCase()) ? text : null;
}

function shortPaneId(paneId: string): string {
  return paneId.length > 12 ? paneId.slice(0, 8) : paneId;
}

function paneLabel(paneId: string, panes: Record<string, Pane>): string {
  return clean(panes[paneId]?.title) ?? shortPaneId(paneId);
}

function agentLabel(agent: AgentRef, panes: Record<string, Pane>): string | null {
  const paneId = clean(agent.pane_id);
  return clean(agent.title) ?? (paneId ? paneLabel(paneId, panes) : null);
}

function latestMilestone(task: Task): Milestone | null {
  return task.milestones.reduce<Milestone | null>((latest, m) => (!latest || m.ts > latest.ts ? m : latest), null);
}

function kindLabel(kind: string): string {
  return kind.replace(/[_-]+/g, " ");
}

function focusApplies(plan: Plan, task: Task, paneId: string, focus: string): boolean {
  const candidates = [
    task.id,
    `task:${task.id}`,
    task.handle,
    task.name,
    task.group,
    task.assigned_pane_id,
    task.lane?.pane_id,
    paneId,
    `pane:${paneId}`,
  ].flatMap((v) => {
    const text = clean(v);
    return text ? [text] : [];
  });
  const candidateSet = new Set(candidates);
  if ((plan.heartbeat?.active_lanes ?? []).some((lane) => {
    const text = clean(lane);
    return text ? candidateSet.has(text) : false;
  })) return true;
  const haystack = focus.toLowerCase();
  return candidates.some((c) => haystack.includes(c.toLowerCase()));
}

export function buildTaskPresence(plan: Plan, task: Task, panes: Record<string, Pane>): TaskPresence | null {
  const paneId = clean(task.assigned_pane_id) ?? clean(task.lane?.pane_id);
  if (!paneId) return null;
  const focus = clean(plan.heartbeat?.current_focus);
  const latest = latestMilestone(task);
  return {
    paneId,
    label: paneLabel(paneId, panes),
    focus: focus && focusApplies(plan, task, paneId, focus) ? focus : null,
    latest: latest
      ? {
        authorLabel: agentLabel(latest.from, panes) ?? paneLabel(paneId, panes),
        kindLabel: kindLabel(latest.kind),
      }
      : null,
  };
}
