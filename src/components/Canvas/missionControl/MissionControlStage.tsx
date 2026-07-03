import { memo } from "react";
import type { MissionControlSnapshot } from "./types";
import { VariantAssemblyLine } from "./variants/VariantAssemblyLine";

export interface MissionControlStageProps {
  snapshot: MissionControlSnapshot;
  lod: "far" | "mid" | "close";
  dispatchGap: boolean;
  qaBottleneck: boolean;
  showEvents: boolean;
  onTaskClick?: (taskId: string) => void;
}

export const MissionControlStage = memo(function MissionControlStage({
  snapshot,
  lod,
  dispatchGap,
  qaBottleneck,
  showEvents,
  onTaskClick,
}: MissionControlStageProps) {
  return (
    <div
      className="mc-stage mc-live-stage"
      data-lod={lod}
      data-dispatch-gap={dispatchGap ? "true" : undefined}
      data-qa-bottleneck={qaBottleneck ? "true" : undefined}
    >
      <VariantAssemblyLine
        snapshot={snapshot}
        onTaskClick={onTaskClick}
        dispatchGap={dispatchGap}
        qaBottleneck={qaBottleneck}
        showEvents={showEvents}
      />
    </div>
  );
});
