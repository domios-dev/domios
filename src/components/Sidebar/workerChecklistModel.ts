export const TAB_TODO_CHANGED_EVENT = "tab-todo-changed";
export const MAX_WORKER_CHECKLIST_ITEMS = 16;

export type WorkerChecklistState = "open" | "in_progress" | "done" | "blocked" | "skipped";

export interface WorkerChecklistItem {
  id: string;
  title: string;
  state: WorkerChecklistState;
  reason?: string;
}

export interface WorkerChecklist {
  tabId: string;
  generation: number;
  revision: number;
  items: WorkerChecklistItem[];
}

export interface TabTodoChanged {
  tab_id: string;
  generation: number;
  revision: number;
}

const STATE_LABELS: Record<WorkerChecklistState, string> = {
  open: "Open",
  in_progress: "In progress",
  done: "Done",
  blocked: "Blocked",
  skipped: "Skipped",
};

export function checklistStateLabel(state: WorkerChecklistState): string {
  return STATE_LABELS[state];
}

function isState(value: unknown): value is WorkerChecklistState {
  return typeof value === "string" && value in STATE_LABELS;
}

/** Narrows the service checklist to the bounded, ordered fields the row detail shows. */
export function boundedChecklist(raw: unknown): WorkerChecklist | null {
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
    .slice(0, MAX_WORKER_CHECKLIST_ITEMS)
    .map((item) => ({
      id: item.id as string,
      title: item.title as string,
      state: item.state as WorkerChecklistState,
      ...(typeof item.reason === "string" && item.reason.trim() ? { reason: item.reason.trim() } : {}),
    }));
  return { tabId: record.tab_id, generation: record.generation, revision: record.revision, items };
}

export function isTabTodoChanged(payload: unknown): payload is TabTodoChanged {
  if (!payload || typeof payload !== "object") return false;
  const event = payload as Record<string, unknown>;
  return typeof event.tab_id === "string" && typeof event.generation === "number" && typeof event.revision === "number";
}

/** A change event warrants a reread only for this tab and only when it is newer than what is shown. */
export function eventNeedsReread(tabId: string, shown: WorkerChecklist | null, event: TabTodoChanged): boolean {
  if (event.tab_id !== tabId) return false;
  if (!shown) return true;
  if (event.generation !== shown.generation) return event.generation > shown.generation;
  return event.revision > shown.revision;
}
