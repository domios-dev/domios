import type { PlanTaskDetail } from "../types";
import type { UsePlanTaskDetailResult } from "../usePlanTaskDetail";

export type TaskHistoryController = Pick<UsePlanTaskDetailResult, "loadHistory" | "historyLoading" | "historyError">;

export function TaskHistoryControls({ detail, controller }: { detail: PlanTaskDetail; controller?: TaskHistoryController }) {
  if (!controller || !detail.history) return null;
  return <div className="flex flex-wrap gap-2 px-4 py-2">
    {(["milestones", "attempts"] as const).map((kind) => detail.history?.[kind] !== null && detail.history?.[kind] !== undefined ?
      <div key={kind}>
        <button type="button" disabled={controller.historyLoading(kind)} onClick={() => void controller.loadHistory(kind)}
          className="rounded border px-3 py-1 text-xs">{controller.historyLoading(kind) ? "Loading…" : `Load earlier ${kind === "milestones" ? "activity" : "attempts"}`}</button>
        {controller.historyError(kind) && <p role="alert">{controller.historyError(kind)}</p>}
      </div> : null)}
  </div>;
}
