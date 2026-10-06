import { useEffect, useMemo, useRef, useState } from "react";
import { isTerminalReadingHistory } from "./terminalHistoryReaders";

export const TAB_PREWARM_LIMIT = 2;
/** Each mounted terminal holds a full scrollback buffer and a WebGL context. */
export const MOUNTED_TAB_CAPACITY = 6;

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
  liveTabIds: readonly string[];
  prewarmTab: (tabId: string) => Promise<void>;
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
  prewarm: (tabId: string) => Promise<void>,
  scheduler: FrameScheduler = browserFrameScheduler,
): () => void {
  let cancelled = false;
  let index = 0;
  let handle = 0;
  const next = async () => {
    if (cancelled) return;
    const id = tabIds[index];
    if (!id) return;
    await prewarm(id);
    if (cancelled) return;
    index += 1;
    if (index < tabIds.length) handle = scheduler.request(next);
  };
  if (tabIds.length > 0) handle = scheduler.request(next);
  return () => {
    cancelled = true;
    if (handle) scheduler.cancel(handle);
  };
}

/** Most recently active first. Tabs past the capacity unmount unless `keepWarm`
 *  holds them, and closed tabs always drop. */
export function retainMountedTabs(
  mounted: readonly string[],
  activeTabId: string | null,
  liveTabIds: readonly string[],
  keepWarm: (tabId: string) => boolean = isTerminalReadingHistory,
  capacity = MOUNTED_TAB_CAPACITY,
): readonly string[] {
  const live = new Set(liveTabIds);
  const ordered = activeTabId ? [activeTabId, ...mounted.filter((id) => id !== activeTabId)] : mounted;
  const kept: string[] = [];
  let budget = capacity;
  for (const id of ordered) {
    if (!live.has(id)) continue;
    if (budget > 0) {
      budget -= 1;
      kept.push(id);
    } else if (keepWarm(id)) {
      kept.push(id);
    }
  }
  return kept.length === mounted.length && kept.every((id, index) => id === mounted[index]) ? mounted : kept;
}

export function useTabPrewarm({
  workspaceId,
  activeTabId,
  tabIds,
  liveTabIds,
  prewarmTab,
}: TabPrewarmInput): ReadonlySet<string> {
  const recentByWorkspace = useRef(new Map<string, string[]>());
  const [mountedTabIds, setMountedTabIds] = useState<readonly string[]>([]);
  const liveTabIdsRef = useRef(liveTabIds);
  liveTabIdsRef.current = liveTabIds;
  const tabKey = tabIds.join("\u0000");
  const liveTabKey = liveTabIds.join("\u0000");

  useEffect(() => {
    if (!workspaceId || !activeTabId) return;

    const recent = recordRecentTab(recentByWorkspace.current.get(workspaceId) ?? [], activeTabId);
    recentByWorkspace.current.set(workspaceId, recent.filter((id) => tabIds.includes(id)));
    if (tabIds.includes(activeTabId)) {
      setMountedTabIds((current) => retainMountedTabs(current, activeTabId, liveTabIdsRef.current));
    }
    const candidates = selectPrewarmTabIds(tabIds, activeTabId, recent);
    return scheduleSequentialPrewarm(candidates, prewarmTab);
  }, [workspaceId, activeTabId, tabKey, prewarmTab]);

  useEffect(() => {
    setMountedTabIds((current) => retainMountedTabs(current, null, liveTabIdsRef.current));
  }, [liveTabKey]);

  return useMemo(() => new Set(mountedTabIds), [mountedTabIds]);
}
