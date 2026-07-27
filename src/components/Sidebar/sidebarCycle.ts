import type { Tab, Tab } from "../../lib/types";
import { flattenSidebarTabs } from "./workspaceTabs";

export type CycleItem =
  | { kind: "plan" }
  | { kind: "tab"; tabId: string };

/** Ordered list of sidebar items the cycle keybinding steps through.
 *  Plan entry (if present) is pinned first, matching its sidebar position. */
export function buildSidebarCycleItems(
  tabs: Tab[],
  tabs: Record<string, Tab>,
  hasPlanEntry: boolean,
): CycleItem[] {
  const items: CycleItem[] = [];
  if (hasPlanEntry) items.push({ kind: "plan" });
  for (const tab of flattenSidebarTabs(tabs, tabs)) {
    items.push({ kind: "tab", tabId: tab.id });
  }
  return items;
}

/** Returns the next item to activate, or null if items is empty.
 *  Wraps around at both ends. */
export function cycleStep(
  items: CycleItem[],
  currentTabId: string | undefined,
  planActive: boolean,
  direction: 1 | -1,
): CycleItem | null {
  if (items.length === 0) return null;

  let idx = -1;
  if (planActive) {
    idx = items.findIndex((i) => i.kind === "plan");
  } else if (currentTabId) {
    idx = items.findIndex((i) => i.kind === "tab" && i.tabId === currentTabId);
  }

  if (idx === -1) {
    return items[direction === 1 ? 0 : items.length - 1];
  }

  return items[(idx + direction + items.length) % items.length];
}
