import type { WorkingOnTone } from "./workingOnRow";

export type LaneWarningId = "pane_gone" | "no_pane_recorded" | "branch_missing";

export interface LaneWarningDef {
  id: LaneWarningId;
  label: string;
  tone: WorkingOnTone;
  severity: number;
}

// Shared with lane_liveness.rs verdict labels — one severity ladder for UI + watchdog.
export const LANE_WARNINGS: Record<LaneWarningId, LaneWarningDef> = {
  pane_gone: { id: "pane_gone", label: "tab gone", tone: "red", severity: 30 },
  no_pane_recorded: { id: "no_pane_recorded", label: "no tab recorded", tone: "amber", severity: 20 },
  branch_missing: { id: "branch_missing", label: "branch missing", tone: "amber", severity: 10 },
};

export const CHIP_FIELD_LABELS = {
  model: "Model",
  pane: "Tab",
  peer: "Peer",
  branch: "Branch",
  age: "Age",
  warn: "Issue",
} as const;

export function pickHighestLaneWarning(ids: readonly LaneWarningId[]): LaneWarningDef | null {
  if (!ids.length) return null;
  return ids
    .map((id) => LANE_WARNINGS[id])
    .reduce((best, cur) => (cur.severity > best.severity ? cur : best));
}
