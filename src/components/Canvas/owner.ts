import type { OrchestratorRef } from "./types";

// Placeholder owner strings the backend emits when it can't resolve a real owner.
const UNOWNED_SENTINELS = new Set(["", "unknown", "unowned", "none", "null", "__unowned__"]);

export function ownerTabId(owner: OrchestratorRef | null | undefined): string | null {
  const tab = owner?.tab_id?.trim() ?? "";
  return tab && !UNOWNED_SENTINELS.has(tab.toLowerCase()) ? tab : null;
}

export function ownerName(owner: OrchestratorRef | null | undefined): string | null {
  if (!owner) return null;
  const title = owner.title?.trim() ?? "";
  const tab = owner.tab_id?.trim() ?? "";
  const v = (UNOWNED_SENTINELS.has(title.toLowerCase()) ? "" : title) ||
    (UNOWNED_SENTINELS.has(tab.toLowerCase()) ? "" : tab);
  return v.length > 0 ? v : null;
}
