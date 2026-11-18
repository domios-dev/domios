import type { LockedView } from "../components/billing/freeLimitModel";
import type { Tab } from "./types";

export function isReadOnlyTab(tab: Pick<Tab, "role_profile"> | null | undefined): boolean {
  return tab?.role_profile === "curator";
}

export function isConciergeTab(tab: Pick<Tab, "neighbor_authority"> | null | undefined): boolean {
  return tab?.neighbor_authority != null && tab.neighbor_authority.root_tab_id == null;
}

export function isInputBlockedTab(tab: Pick<Tab, "role_profile" | "neighbor_authority"> | null | undefined, lock: LockedView): boolean {
  return isReadOnlyTab(tab) || isConciergeTab(tab) || lock.locked;
}
