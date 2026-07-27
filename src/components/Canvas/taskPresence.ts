import type { Tab } from "../../lib/types";
import { presenceColor, type PresenceAgent } from "./presence/identity";
import { isActiveTaskStatus } from "./tabCurrentTask";
import type { AgentRef, Milestone, Plan, Task } from "./types";

export interface TaskPresenceLatest {
  authorLabel: string;
  kindLabel: string;
}

export interface TaskPresence {
  tabId: string;
  label: string;
  focus: string | null;
  latest: TaskPresenceLatest | null;
  /** WHO is executing: lead (the lane's tab) first, then peers. Color-keyed. */
  agents: PresenceAgent[];
  /** WHERE the work lives — surfaced glanceably, not buried in the W3 lane block. */
  branch: string | null;
}

const EMPTY_LABELS = new Set(["", "unknown"]);

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text && !EMPTY_LABELS.has(text.toLowerCase()) ? text : null;
}

function shortTabId(tabId: string): string {
  return tabId.length > 12 ? tabId.slice(0, 8) : tabId;
}

function tabLabel(tabId: string, tabs: Record<string, Tab>): string {
  return clean(tabs[tabId]?.title) ?? shortTabId(tabId);
}

function agentLabel(agent: AgentRef, tabs: Record<string, Tab>): string | null {
  const tabId = clean(agent.tab_id);
  return clean(agent.title) ?? (tabId ? tabLabel(tabId, tabs) : null);
}

function latestMilestone(task: Task): Milestone | null {
  return task.milestones.reduce<Milestone | null>((latest, m) => (!latest || m.ts > latest.ts ? m : latest), null);
}

function kindLabel(kind: string): string {
  return kind.replace(/[_-]+/g, " ");
}

function focusLaneTokens(task: Task, tabId: string): string[] {
  return [
    task.id,
    `task:${task.id}`,
    task.handle,
    tabId,
    `tab:${tabId}`,
  ].flatMap((v) => {
    const text = clean(v);
    return text ? [text.toLowerCase()] : [];
  });
}

function focusApplies(plan: Plan, task: Task, tabId: string, focus: string): boolean {
  const laneTokens = focusLaneTokens(task, tabId);
  const laneSet = new Set(laneTokens);
  if ((plan.heartbeat?.active_lanes ?? []).some((lane) => {
    const text = clean(lane)?.toLowerCase();
    return text ? laneSet.has(text) : false;
  })) return true;

  const haystack = focus.toLowerCase();
  const identityNeedles = [task.id, task.handle, task.name, `task:${task.id}`]
    .flatMap((v) => {
      const text = clean(v);
      return text ? [text.toLowerCase()] : [];
    });
  return identityNeedles.some((needle) => haystack.includes(needle));
}

function buildAgents(task: Task, tabId: string, tabs: Record<string, Tab>): PresenceAgent[] {
  const role = clean(task.taken_by_role);
  const lead: PresenceAgent = {
    id: tabId,
    label: clean(tabs[tabId]?.title) ?? role ?? shortTabId(tabId),
    role,
    color: presenceColor(tabId),
  };
  const peers = (clean(task.peer)?.split(",") ?? [])
    .map((p) => p.trim())
    .filter(Boolean)
    .map<PresenceAgent>((label) => ({ id: `peer:${label}`, label, role: "peer", color: presenceColor(label), isPeer: true }));
  return [lead, ...peers];
}

export function buildTaskPresence(
  plan: Plan,
  task: Task,
  tabs: Record<string, Tab>,
  exclusiveTabTasks?: ReadonlyMap<string, string>,
): TaskPresence | null {
  if (!isActiveTaskStatus(task.status)) return null;
  const tabId = clean(task.assigned_tab_id) ?? clean(task.lane?.tab_id);
  if (!tabId) return null;
  if (!tabs[tabId]) return null;
  const winnerId = exclusiveTabTasks?.get(tabId);
  if (winnerId && winnerId !== task.id) return null;

  const focus = clean(plan.heartbeat?.current_focus);
  const latest = latestMilestone(task);
  return {
    tabId,
    label: tabLabel(tabId, tabs),
    focus: focus && focusApplies(plan, task, tabId, focus) ? focus : null,
    latest: latest
      ? {
        authorLabel: agentLabel(latest.from, tabs) ?? tabLabel(tabId, tabs),
        kindLabel: kindLabel(latest.kind),
      }
      : null,
    agents: buildAgents(task, tabId, tabs),
    branch: clean(task.lane?.branch),
  };
}
