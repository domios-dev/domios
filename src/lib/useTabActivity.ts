import { useCallback } from "react";
import { useMuxSelector } from "../store/multiplexer";
import type { ExtendedState } from "../store/multiplexer/tree";
import { tabActivity } from "./tabActivity";
import type { Tab } from "./types";

export function useTabActivity(tabId: string | null | undefined, fallbackTab?: Tab | null) {
  const tab = useMuxSelector(useCallback((state: ExtendedState) => tabId ? state.tabs[tabId] ?? null : null, [tabId]));
  const status = useMuxSelector(useCallback((state: ExtendedState) => tabId ? state.tabStatuses[tabId] ?? null : null, [tabId]));
  return tabActivity(tab ?? fallbackTab, status);
}
