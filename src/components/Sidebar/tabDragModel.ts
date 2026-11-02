import type { Tab } from "../../lib/types";

export function isTabDragDisabled(tab: Pick<Tab, "protected">): boolean {
  return tab.protected === true;
}
