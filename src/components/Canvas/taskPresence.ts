import type { Pane } from "../../lib/types";
import { presenceColor, type PresenceAgent } from "./presence/identity";
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
  /** WHO is executing: lead (the lane's pane) first, then peers. Color-keyed. */
  agents: PresenceAgent[];
  /** WHERE the work lives — surfaced glanceably, not buried in the W3 lane block. */
  branch: string | null;
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

// Lead identity prefers the pane's own title; falls back to the orchestrator's
// role label, then the short pane id — so a synthetic/headless lane still reads as
// a handle ("backend-dev") rather than a bare id.
function buildAgents(task: Task, paneId: string, panes: Record<string, Pane>): PresenceAgent[] {
  const role = clean(task.taken_by_role);
  const lead: PresenceAgent = {
    id: paneId,
    label: clean(panes[paneId]?.title) ?? role ?? shortPaneId(paneId),
    role,
    color: presenceColor(paneId),
  };
  // peer may name >1 collaborator (comma-separated) → a real multiplayer stack.
  const peers = (clean(task.peer)?.split(",") ?? [])
    .map((p) => p.trim())
    .filter(Boolean)
    .map<PresenceAgent>((label) => ({ id: `peer:${label}`, label, role: "peer", color: presenceColor(label), isPeer: true }));
  return [lead, ...peers];
}

export function buildTaskPresence(plan: Plan, task: Task, panes: Record<string, Pane>): TaskPresence | null {
  const paneId = clean(task.assigned_pane_id) ?? clean(task.lane?.pane_id);
  if (!paneId) return null;
  // Presence is LIVE / in-RAM, never persisted history: show an agent only while its pane is an
  // actually-running session in THIS daemon. A task's stored assigned_pane_id/lane.pane_id is just
  // a record — without a live pane there is no agent, so no stale "idle" swarm accretes.
  if (!panes[paneId]) return null;
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
    agents: buildAgents(task, paneId, panes),
    branch: clean(task.lane?.branch),
  };
}
