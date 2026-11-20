import type { CronRunInfo } from "../../lib/types";

export function cronRunTranscriptRequest(
  cronId: string,
  run: Pick<CronRunInfo, "run_id" | "owner_instance_id">,
  fallbackOwnerInstanceId?: string | null,
): { targetInstanceId?: string; path: string } {
  return {
    targetInstanceId: run.owner_instance_id ?? fallbackOwnerInstanceId ?? undefined,
    path: `/api/crons/${encodeURIComponent(cronId)}/history/${encodeURIComponent(run.run_id)}`,
  };
}
