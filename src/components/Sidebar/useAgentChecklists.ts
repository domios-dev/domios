import { useEffect, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import { readWhenMuxCapacityAvailable } from "../../lib/streamMuxRequestModel";
import { boundedChecklist, eventNeedsReread, isTabTodoChanged, TAB_TODO_CHANGED_EVENT, type AgentChecklist } from "./agentChecklistModel";

export type ChecklistView = { status: "loading" } | { status: "error"; error: unknown; retry: () => void } | { status: "ready"; checklist: AgentChecklist | null };
const LOADING: ChecklistView = { status: "loading" };

export function useAgentChecklists(tabIds: readonly string[]): ReadonlyMap<string, ChecklistView> {
  const key = JSON.stringify([...new Set(tabIds)].sort());
  const [views, setViews] = useState<ReadonlyMap<string, ChecklistView>>(new Map());
  useEffect(() => {
    const ids = JSON.parse(key) as string[];
    const controller = new AbortController();
    let unlisten: UnlistenFn | null = null;
    const shown = new Map<string, AgentChecklist | null>();
    const issued = new Map<string, number>();
    const pending = new Set<string>();
    let reading = false;
    setViews(new Map(ids.map((id) => [id, LOADING])));
    const drain = async () => {
      if (reading) return;
      reading = true;
      try {
        for (const tabId of pending) {
          if (controller.signal.aborted) return;
          pending.delete(tabId);
          const request = issued.get(tabId);
          try {
            const raw = await readWhenMuxCapacityAvailable(() => invoke<unknown>("tab_todo_list", { tabId }), controller.signal);
            if (controller.signal.aborted || issued.get(tabId) !== request) continue;
            const checklist = boundedChecklist(raw);
            shown.set(tabId, checklist);
            setViews((current) => new Map(current).set(tabId, { status: "ready", checklist }));
          } catch (error) {
            if (controller.signal.aborted || issued.get(tabId) !== request) continue;
            setViews((current) => new Map(current).set(tabId, { status: "error", error, retry: () => reread(tabId, true) }));
          }
        }
      } finally { reading = false; }
    };
    const reread = (tabId: string, loading = false) => {
      if (controller.signal.aborted) return;
      issued.set(tabId, (issued.get(tabId) ?? 0) + 1);
      pending.add(tabId);
      if (loading) setViews((current) => new Map(current).set(tabId, LOADING));
      void drain();
    };
    if (ids.length) listen<unknown>(TAB_TODO_CHANGED_EVENT, ({ payload }) => {
      if (!controller.signal.aborted && isTabTodoChanged(payload) && ids.includes(payload.tab_id)
        && eventNeedsReread(payload.tab_id, shown.get(payload.tab_id) ?? null, payload)) reread(payload.tab_id);
    }).then((stop) => {
      if (controller.signal.aborted) stop();
      else { unlisten = stop; ids.forEach((id) => reread(id)); }
    }, () => { if (!controller.signal.aborted) ids.forEach((id) => reread(id)); });
    return () => { controller.abort(); pending.clear(); unlisten?.(); };
  }, [key]);
  return views;
}

export function useAgentChecklist(tabId: string): ChecklistView {
  return useAgentChecklists([tabId]).get(tabId) ?? LOADING;
}
