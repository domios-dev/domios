import type { WorkingOnTone } from "./workingOnRow";

export type LaneWarningId = "tab_gone" | "no_tab_recorded" | "branch_missing";

export interface LaneWarningDef {
  id: LaneWarningId;
  label: string;
  tone: WorkingOnTone;
  severity: number;
}

// Shared with lane_liveness.rs verdict labels — one severity ladder for UI + watchdog.
export const LANE_WARNINGS: Record<LaneWarningId, LaneWarningDef> = {
  tab_gone: { id: "tab_gone", label: "tab gone", tone: "red", severity: 30 },
  no_tab_recorded: { id: "no_tab_recorded", label: "no tab recorded", tone: "amber", severity: 20 },
  branch_missing: { id: "branch_missing", label: "branch missing", tone: "amber", severity: 10 },
};

export const CHIP_FIELD_LABELS = {
  model: "Model",
  tab: "Tab",
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
