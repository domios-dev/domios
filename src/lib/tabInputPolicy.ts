import type { LockedView } from "../components/billing/freeLimitModel";
import type { Tab } from "./types";

export function isReadOnlyTab(tab: Pick<Tab, "role_profile"> | null | undefined): boolean {
  return tab?.role_profile === "curator";
}

/** No user input reaches the tab: a Curator round, or a tab the free plan has locked (the daemon refuses it too). */
export function isInputBlockedTab(tab: Pick<Tab, "role_profile"> | null | undefined, lock: LockedView): boolean {
  return isReadOnlyTab(tab) || lock.locked;
}
