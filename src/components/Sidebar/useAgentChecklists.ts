import { useEffect, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import { boundedChecklist, eventNeedsReread, isTabTodoChanged, TAB_TODO_CHANGED_EVENT, type AgentChecklist } from "./agentChecklistModel";

export type ChecklistView = { status: "loading" } | { status: "error" } | { status: "ready"; checklist: AgentChecklist | null };
const LOADING: ChecklistView = { status: "loading" };

export function useAgentChecklists(tabIds: readonly string[]): ReadonlyMap<string, ChecklistView> {
  const key = JSON.stringify([...new Set(tabIds)].sort());
  const [views, setViews] = useState<ReadonlyMap<string, ChecklistView>>(new Map());
  useEffect(() => {
    const ids = JSON.parse(key) as string[];
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    const shown = new Map<string, AgentChecklist | null>();
    const issued = new Map<string, number>();
    setViews(new Map(ids.map((id) => [id, LOADING])));
    const reread = (tabId: string) => {
      const request = (issued.get(tabId) ?? 0) + 1;
      issued.set(tabId, request);
      invoke<unknown>("tab_todo_list", { tabId }).then((raw) => {
        if (disposed || issued.get(tabId) !== request) return;
        const checklist = boundedChecklist(raw);
        shown.set(tabId, checklist);
        setViews((current) => new Map(current).set(tabId, { status: "ready", checklist }));
      }, () => {
        if (!disposed && issued.get(tabId) === request) setViews((current) => new Map(current).set(tabId, { status: "error" }));
      });
    };
    if (ids.length) listen<unknown>(TAB_TODO_CHANGED_EVENT, ({ payload }) => {
      if (!disposed && isTabTodoChanged(payload) && ids.includes(payload.tab_id)
        && eventNeedsReread(payload.tab_id, shown.get(payload.tab_id) ?? null, payload)) reread(payload.tab_id);
    }).then((stop) => {
      if (disposed) stop();
      else { unlisten = stop; ids.forEach(reread); }
    }, () => { if (!disposed) ids.forEach(reread); });
    return () => { disposed = true; unlisten?.(); };
  }, [key]);
  return views;
}

export function useAgentChecklist(tabId: string): ChecklistView {
  return useAgentChecklists([tabId]).get(tabId) ?? LOADING;
}
