import { EventStream } from "../EventStream";
import { PlanHud } from "../PlanHud";
import { DispatchRail, DoneShelf, GateDesk, QaBay, SideRail, WorkFloorBay } from "../stations";
import type { MissionControlLodLevel } from "../missionControlLod";
import type { MissionControlSnapshot } from "../types";

interface VariantProps {
  snapshot: MissionControlSnapshot;
  lod: MissionControlLodLevel;
  onTaskClick?: (taskId: string) => void;
  onAgentClick?: (paneId: string) => void;
  dispatchGap?: boolean;
  qaBottleneck?: boolean;
  showEvents?: boolean;
}

export function VariantAssemblyLine({
  snapshot,
  lod,
  onTaskClick,
  onAgentClick,
  dispatchGap,
  qaBottleneck,
  showEvents = true,
}: VariantProps) {
  return (
    <div
      data-variant="assembly-line"
      data-testid="variant-assembly-line"
      className="mc-assembly-root"
    >
      <PlanHud planTitle={snapshot.planTitle} goal={snapshot.goal} hud={snapshot.hud} compact={lod !== "close"} />
      <div className="mc-assembly-grid" data-dispatch-gap={dispatchGap ? "true" : undefined}>
        <DispatchRail
          title="Frontier"
          tasks={snapshot.dispatchQueue}
          lod={lod}
          onTaskClick={onTaskClick}
          stressed={dispatchGap}
        />
        <section className="mc-panel mc-work-floor" data-testid="work-floor" data-canvas-scroll-body data-empty={dispatchGap ? "true" : undefined}>
          <div className="mc-panel-title mc-work-floor-title">
            Lanes
            <span className="mc-overview-only"> ({snapshot.lanes.length})</span>
          </div>
          <div className="mc-work-floor-grid">
            {snapshot.lanes.map((lane) => (
              <WorkFloorBay key={lane.id} lane={lane} lod={lod} onTaskClick={onTaskClick} onAgentClick={onAgentClick} />
            ))}
          </div>
        </section>
        <div className="mc-verify-stack">
          <QaBay
            queue={snapshot.qaQueue}
            running={snapshot.qaRunning}
            lod={lod}
            onTaskClick={onTaskClick}
            bottleneck={qaBottleneck}
          />
          <GateDesk tasks={snapshot.gateQueue} lod={lod} onTaskClick={onTaskClick} />
        </div>
        <div className="mc-assembly-tail">
          <DoneShelf count={snapshot.doneCount} recent={snapshot.doneRecent} lod={lod} onTaskClick={onTaskClick} />
          <SideRail title="Paused" tasks={snapshot.paused} lod={lod} onTaskClick={onTaskClick} />
          <SideRail title="Blocked" tasks={snapshot.blocked} lod={lod} tone="warn" onTaskClick={onTaskClick} />
        </div>
      </div>
      {showEvents && <EventStream events={snapshot.events} />}
    </div>
  );
}
