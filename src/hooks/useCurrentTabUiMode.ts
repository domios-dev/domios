import { useCallback, useSyncExternalStore } from "react";
import { getCurrentTabUiMode, listenCurrentTabUiMode } from "../lib/clientTabUiMode";

export function useCurrentTabUiMode(tabId: string | undefined) {
  const subscribe = useCallback((notify: () => void) => tabId ? listenCurrentTabUiMode(tabId, notify) : () => {}, [tabId]);
  return useSyncExternalStore(subscribe, () => tabId ? getCurrentTabUiMode(tabId) : null, () => null);
}
