import type { RestoreStageView } from "./restoreStageView";
import { updatePresentation } from "./updatePresentation";

export type UpdateStage = "downloading" | "applying" | "restarting";

const STEPS = [
  { id: "download", label: "Downloading" },
  { id: "install", label: "Installing" },
  { id: "restart", label: "Restarting" },
] as const;

const CURRENT: Record<UpdateStage, number> = { downloading: 0, applying: 1, restarting: 2 };

export function formatUpdateBytes(value: number): string {
  if (value <= 0) return "0 MB";
  const mb = value / 1024 / 1024;
  return mb < 10 ? `${mb.toFixed(1)} MB` : `${Math.round(mb)} MB`;
}

/** The update reuses the restore stage surface so both full-screen states look the same. */
export function updateStageView(stage: UpdateStage, version: string | null, downloadedBytes = 0, totalBytes: number | null = null): RestoreStageView {
  const current = CURRENT[stage];
  const hint = stage === "downloading" && totalBytes && totalBytes > 0
    ? `${formatUpdateBytes(downloadedBytes)} of ${formatUpdateBytes(totalBytes)}`
    : stage === "restarting" ? "Domios reopens by itself with your sessions." : null;
  return {
    phase: "restoring",
    label: stage === "downloading" ? "Downloading and preparing update" : updatePresentation(stage, true, version).title,
    detail: null,
    hint,
    current,
    steps: STEPS.map((step, index) => ({ ...step, state: index < current ? "complete" : index === current ? "current" : "pending" })),
  };
}
