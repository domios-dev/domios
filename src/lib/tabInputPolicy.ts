import type { Tab } from "./types";

export function isReadOnlyTab(tab: Pick<Tab, "role_profile"> | null | undefined): boolean {
  return tab?.role_profile === "curator";
}
