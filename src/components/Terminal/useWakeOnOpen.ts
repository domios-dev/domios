import { useEffect } from "react";
import { invoke } from "../../lib/bridge";
import { shouldWakeOnOpen } from "../../lib/wakeOnOpen";
import type { Tab } from "../../lib/types";

const requestedTabs = new Set<string>();

export function useWakeOnOpen(tab: Tab | undefined, isVisible: boolean) {
  const id = tab?.id;
  const frozen = tab?.frozen;
  useEffect(() => {
    if (!id) return;
    if (!frozen) {
      requestedTabs.delete(id);
      return;
    }
    if (!shouldWakeOnOpen({ frozen }, isVisible, requestedTabs.has(id))) return;
    requestedTabs.add(id);
    void invoke("wake_tab", { tabId: id }).catch(() => requestedTabs.delete(id));
  }, [id, frozen, isVisible]);
}
