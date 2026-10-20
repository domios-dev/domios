import { useMemo } from "react";
import { useMultiplexer } from "../../store/multiplexer";
import { useMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ProviderAvailabilityChange } from "../../lib/providerAvailability";

export interface TabRef {
  workspaceId: string;
  tabId: string;
}

/** What a picker needs to send the user to its installer tab and back. */
export interface PickerNavigation {
  show(tab: TabRef): Promise<void> | void;
  activeTabId(): string | null;
}

/** Return only from the picker's own installer, only on success (a failure stays readable), only while it is in view. */
export function shouldReturnToPicker(
  change: ProviderAvailabilityChange,
  installerTabId: string | null,
  activeTabId: string | null,
): boolean {
  return installerTabId !== null
    && change.tab_id === installerTabId
    && change.availability === "available"
    && activeTabId === installerTabId;
}

export function usePickerNavigation(): PickerNavigation {
  const { selectTabByTab } = useMultiplexer();
  const store = useMuxStateStore();
  return useMemo(() => ({
    show: ({ workspaceId, tabId }: TabRef) => selectTabByTab(workspaceId, tabId),
    activeTabId: () => {
      const state = store.getState();
      return state.active_workspace_id ? state.activeTabIds[state.active_workspace_id] ?? null : null;
    },
  }), [selectTabByTab, store]);
}
