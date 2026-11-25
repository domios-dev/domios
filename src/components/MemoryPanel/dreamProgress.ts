import { DREAM_STAGES } from "./dreamModel";
import type { GraphWorkerInfo } from "../../lib/types";

export function dreamProgressDetail(progress?: string | null): string | null {
  const text = progress?.trim();
  return text && !DREAM_STAGES.some(([id]) => id === text) ? text : null;
}

export function dreamProgressLabel(worker: GraphWorkerInfo): string | null {
  if (!worker.intent.startsWith("dreaming:")) return worker.progress_text;
  const stage = DREAM_STAGES.find(([id]) => id === worker.stage);
  if (!stage) return worker.progress_text;
  const detail = dreamProgressDetail(worker.progress_text);
  return detail ? `${stage[1]} · ${detail}` : stage[1];
}
