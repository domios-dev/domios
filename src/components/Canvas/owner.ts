import type { OrchestratorRef } from "./types";

// Placeholder owner strings the backend emits when it can't resolve a real owner.
const UNOWNED_SENTINELS = new Set(["", "unknown", "unowned", "none", "null", "__unowned__"]);

// An OrchestratorRef can be present-but-empty (all fields null/"") or a literal
// placeholder ("unknown"). Treat all of those as unowned: returns the display
// name, or null when there is nothing real to show.
export function ownerName(owner: OrchestratorRef | null | undefined): string | null {
  if (!owner) return null;
  const title = owner.title?.trim() ?? "";
  const pane = owner.pane_id?.trim() ?? "";
  const v = (UNOWNED_SENTINELS.has(title.toLowerCase()) ? "" : title) ||
    (UNOWNED_SENTINELS.has(pane.toLowerCase()) ? "" : pane);
  return v.length > 0 ? v : null;
}
