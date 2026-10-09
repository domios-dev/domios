import { useCallback } from "react";
import { useMuxSelector } from "../store/multiplexer";
import type { ExtendedState } from "../store/multiplexer/tree";
import { tabActivity } from "./tabActivity";
import type { TabActivity } from "./tabActivity";
import type { Tab } from "./types";

function sameActivity(a: TabActivity, b: TabActivity): boolean {
  return (Object.keys(a) as Array<keyof TabActivity>).every((key) => a[key] === b[key]);
}

export function useTabActivity(tabId: string | null | undefined, fallbackTab?: Tab | null) {
  return useMuxSelector(useCallback((state: ExtendedState) => tabActivity(
    (tabId ? state.tabs[tabId] : null) ?? fallbackTab,
    tabId ? state.tabStatuses[tabId] : null,
  ), [tabId, fallbackTab]), sameActivity);
}
