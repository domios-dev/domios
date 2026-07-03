import { EventStream } from "../EventStream";
import { PlanHud } from "../PlanHud";
import { DispatchRail, DoneShelf, GateDesk, QaBay, SideRail, WorkFloorBay } from "../stations";
import type { MissionControlSnapshot } from "../types";

interface VariantProps {
  snapshot: MissionControlSnapshot;
  onTaskClick?: (taskId: string) => void;
  dispatchGap?: boolean;
  qaBottleneck?: boolean;
  showEvents?: boolean;
}

export function VariantAssemblyLine({
  snapshot,
  onTaskClick,
  dispatchGap,
  qaBottleneck,
  showEvents = true,
}: VariantProps) {
  return (
    <div
      data-variant="assembly-line"
      data-testid="variant-assembly-line"
      style={{ display: "grid", gridTemplateRows: "auto 1fr", height: "100%", minHeight: 0 }}
    >
      <PlanHud planTitle={snapshot.planTitle} goal={snapshot.goal} hud={snapshot.hud} />
      <div
        className="mc-assembly-grid"
        data-dispatch-gap={dispatchGap ? "true" : undefined}
        style={{
          display: "grid",
          gridTemplateColumns: "160px 1fr 180px 140px 120px",
          gap: 6,
          padding: 8,
          minHeight: 0,
          alignItems: "stretch",
        }}
      >
        <DispatchRail title="Frontier" tasks={snapshot.dispatchQueue} onTaskClick={onTaskClick} stressed={dispatchGap} />
        <section
          className="mc-panel mc-work-floor"
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 6,
            padding: 8,
            minHeight: 0,
            overflow: "auto",
          }}
          data-testid="work-floor"
          data-canvas-scroll-body
          data-empty={dispatchGap ? "true" : undefined}
        >
          <div className="mc-panel-title" style={{ border: "none", padding: 0 }}>
            Lanes →
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(120px, 1fr))", gap: 6 }}>
            {snapshot.lanes.map((lane) => (
              <WorkFloorBay key={lane.id} lane={lane} onTaskClick={onTaskClick} />
            ))}
          </div>
        </section>
        <QaBay
          queue={snapshot.qaQueue}
          running={snapshot.qaRunning}
          onTaskClick={onTaskClick}
          bottleneck={qaBottleneck}
        />
        <GateDesk tasks={snapshot.gateQueue} onTaskClick={onTaskClick} />
        <div style={{ display: "flex", flexDirection: "column", gap: 6, minHeight: 0 }}>
          <DoneShelf count={snapshot.doneCount} recent={snapshot.doneRecent} onTaskClick={onTaskClick} />
          <SideRail title="Parked" tasks={snapshot.parked} onTaskClick={onTaskClick} />
          <SideRail title="Blocked" tasks={snapshot.blocked} tone="warn" onTaskClick={onTaskClick} />
        </div>
      </div>
      {showEvents && <EventStream events={snapshot.events} />}
    </div>
  );
}
