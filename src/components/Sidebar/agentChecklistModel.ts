import type { Tab } from "../../lib/types";

export const TAB_TODO_CHANGED_EVENT = "tab-todo-changed";
export const MAX_AGENT_CHECKLIST_ITEMS = 16;

export type AgentChecklistState = "open" | "in_progress" | "done" | "blocked" | "skipped";

export interface AgentChecklistItem {
  id: string;
  title: string;
  state: AgentChecklistState;
  reason?: string;
  description?: string;
  activeForm?: string;
  locked?: boolean;
  link?: { kind: "url"; url: string; label: string } | { kind: "domios_task"; plan_id: string; task_id: string; label: string };
}

export interface AgentChecklist {
  tabId: string;
  generation: number;
  revision: number;
  items: AgentChecklistItem[];
  proposal?: { state: "proposed" | "approved" | "amended"; note?: string };
}

export interface TabTodoChanged {
  tab_id: string;
  generation: number;
  revision: number;
}

const STATE_LABELS: Record<AgentChecklistState, string> = {
  open: "Open",
  in_progress: "In progress",
  done: "Done",
  blocked: "Blocked",
  skipped: "Skipped",
};

export function checklistStateLabel(state: AgentChecklistState): string {
  return STATE_LABELS[state];
}

/** Any agent row, whatever its role, carries the disclosure once its tab reports a checklist. */
export function showsChecklist(tab: Tab): boolean {
  const agent = tab.role != null || tab.roleless_tracked === true || Boolean(tab.agent_type)
    || tab.backing?.kind === "chat_backend";
  return agent && tab.has_checklist === true;
}

function isState(value: unknown): value is AgentChecklistState {
  return typeof value === "string" && value in STATE_LABELS;
}

/** Narrows the service checklist to the bounded, ordered fields the row detail shows. */
export function boundedChecklist(raw: unknown): AgentChecklist | null {
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Record<string, unknown>;
  if (typeof record.tab_id !== "string" || typeof record.generation !== "number" || typeof record.revision !== "number") {
    return null;
  }
  const rows = Array.isArray(record.items) ? record.items : [];
  const items = rows
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === "object")
    .filter((item) => typeof item.id === "string" && typeof item.title === "string" && isState(item.state))
    .sort((a, b) => Number(a.position ?? 0) - Number(b.position ?? 0))
    .slice(0, MAX_AGENT_CHECKLIST_ITEMS)
    .map((item) => ({
      id: item.id as string,
      title: item.title as string,
      state: item.state as AgentChecklistState,
      ...(item.locked === true ? { locked: true } : {}),
      ...(typeof item.description === "string" ? { description: item.description } : {}),
      ...(typeof item.activeForm === "string" ? { activeForm: item.activeForm } : {}),
      ...(checklistLink(item.link) ? { link: checklistLink(item.link) } : {}),
      ...(typeof item.reason === "string" && item.reason.trim() ? { reason: item.reason.trim() } : {}),
    }));
  const proposal = record.proposal as Record<string, unknown> | null | undefined;
  const review = proposal && (proposal.state === "proposed" || proposal.state === "approved" || proposal.state === "amended")
    ? { state: proposal.state, ...(typeof proposal.note === "string" ? { note: proposal.note } : {}) } as AgentChecklist["proposal"] : undefined;
  return { tabId: record.tab_id, generation: record.generation, revision: record.revision, items, proposal: review };
}

export function isTabTodoChanged(payload: unknown): payload is TabTodoChanged {
  if (!payload || typeof payload !== "object") return false;
  const event = payload as Record<string, unknown>;
  return typeof event.tab_id === "string" && typeof event.generation === "number" && typeof event.revision === "number";
}

/** A change event warrants a reread only for this tab and only when it is newer than what is shown. */
export function eventNeedsReread(tabId: string, shown: AgentChecklist | null, event: TabTodoChanged): boolean {
  if (event.tab_id !== tabId) return false;
  if (!shown) return true;
  if (event.generation !== shown.generation) return event.generation > shown.generation;
  return event.revision > shown.revision;
}

function checklistLink(raw: unknown): AgentChecklistItem["link"] {
  if (!raw || typeof raw !== "object") return undefined;
  const link = raw as Record<string, unknown>;
  if (typeof link.label !== "string") return undefined;
  if (link.kind === "domios_task" && typeof link.plan_id === "string" && typeof link.task_id === "string") {
    return { kind: link.kind, plan_id: link.plan_id, task_id: link.task_id, label: link.label };
  }
  if (link.kind === "url" && typeof link.url === "string" && /^https?:\/\//i.test(link.url)) {
    return { kind: link.kind, url: link.url, label: link.label };
  }
  return undefined;
}
