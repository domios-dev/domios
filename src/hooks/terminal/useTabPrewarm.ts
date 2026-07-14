import { useEffect, useRef, useState } from "react";

export const TAB_PREWARM_LIMIT = 2;

export function recordRecentTab(
  recent: readonly string[],
  activeTabId: string,
  capacity = TAB_PREWARM_LIMIT + 1,
): string[] {
  return [activeTabId, ...recent.filter((id) => id !== activeTabId)].slice(0, capacity);
}

export function selectPrewarmTabIds(
  tabIds: readonly string[],
  activeTabId: string,
  recent: readonly string[],
  limit = TAB_PREWARM_LIMIT,
): string[] {
  if (limit <= 0) return [];
  const eligible = new Set(tabIds);
  const selected: string[] = [];
  const add = (id: string | undefined) => {
    if (!id || id === activeTabId || !eligible.has(id) || selected.includes(id)) return;
    selected.push(id);
  };

  for (const id of recent) {
    add(id);
    if (selected.length === limit) return selected;
  }

  const activeIndex = tabIds.indexOf(activeTabId);
  for (let distance = 1; activeIndex >= 0 && selected.length < limit; distance += 1) {
    add(tabIds[activeIndex - distance]);
    if (selected.length < limit) add(tabIds[activeIndex + distance]);
    if (activeIndex - distance < 0 && activeIndex + distance >= tabIds.length) break;
  }
  return selected;
}

interface TabPrewarmInput {
  workspaceId: string | null;
  activeTabId: string | null;
  tabIds: readonly string[];
}

interface FrameScheduler {
  request(callback: () => void): number;
  cancel(handle: number): void;
}

const browserFrameScheduler: FrameScheduler = {
  request: (callback) => requestAnimationFrame(callback),
  cancel: (handle) => cancelAnimationFrame(handle),
};

export function scheduleSequentialPrewarm(
  tabIds: readonly string[],
  mount: (tabId: string) => void,
  scheduler: FrameScheduler = browserFrameScheduler,
): () => void {
  let cancelled = false;
  let index = 0;
  let handle = 0;
  const next = () => {
    if (cancelled) return;
    const id = tabIds[index];
    if (!id) return;
    mount(id);
    index += 1;
    if (index < tabIds.length) handle = scheduler.request(next);
  };
  if (tabIds.length > 0) handle = scheduler.request(next);
  return () => {
    cancelled = true;
    if (handle) scheduler.cancel(handle);
  };
}

export function useTabPrewarm({ workspaceId, activeTabId, tabIds }: TabPrewarmInput): ReadonlySet<string> {
  const recentByWorkspace = useRef(new Map<string, string[]>());
  const [warmedTabIds, setWarmedTabIds] = useState<Set<string>>(() => new Set());
  const tabKey = tabIds.join("\u0000");

  useEffect(() => {
    setWarmedTabIds(new Set());
    if (!workspaceId || !activeTabId) return;

    const recent = recordRecentTab(recentByWorkspace.current.get(workspaceId) ?? [], activeTabId);
    recentByWorkspace.current.set(workspaceId, recent.filter((id) => tabIds.includes(id)));
    const candidates = selectPrewarmTabIds(tabIds, activeTabId, recent);
    return scheduleSequentialPrewarm(candidates, (id) => {
      setWarmedTabIds((current) => new Set(current).add(id));
    });
  }, [workspaceId, activeTabId, tabKey]);

  return warmedTabIds;
}
