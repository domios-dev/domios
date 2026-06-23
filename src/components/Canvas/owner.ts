import type { OrchestratorRef } from "./types";

// An OrchestratorRef can be present-but-empty (all fields null or ""). Treat that
// as unowned: returns the display name, or null when there is nothing real to show.
export function ownerName(owner: OrchestratorRef | null | undefined): string | null {
  if (!owner) return null;
  const v = owner.title?.trim() || owner.pane_id?.trim() || "";
  return v.length > 0 ? v : null;
}
