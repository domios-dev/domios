import type { Tab } from "./types";

export function isReadOnlyTab(tab: Pick<Tab, "role_profile"> | null | undefined): boolean {
  return tab?.role_profile === "curator";
}

/** No user input reaches the tab: a Curator round, or a tab the free plan has locked (the daemon refuses it too). */
export function isInputBlockedTab(tab: Pick<Tab, "role_profile" | "locked"> | null | undefined): boolean {
  return isReadOnlyTab(tab) || tab?.locked === true;
}
