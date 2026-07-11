import { EventStream } from "../EventStream";
import { PlanHud } from "../PlanHud";
import { DispatchRail, DoneShelf, GateDesk, QaBay, SideRail, WorkFloorBay } from "../stations";
import type { MissionControlSnapshot } from "../types";

interface VariantProps {
  snapshot: MissionControlSnapshot;
}

export function VariantHangar({ snapshot }: VariantProps) {
  return (
    <div
      data-variant="hangar"
      data-testid="variant-hangar"
      style={{ display: "grid", gridTemplateRows: "auto 1fr", height: "100%", minHeight: 0 }}
    >
      <PlanHud planTitle={snapshot.planTitle} goal={snapshot.goal} hud={snapshot.hud} />
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "180px 1fr 200px 160px",
          gridTemplateRows: "1fr auto",
          gap: 8,
          padding: 8,
          minHeight: 0,
        }}
      >
        <DispatchRail title="Dispatch frontier" tasks={snapshot.dispatchQueue} />
        <section
          className="mc-panel"
          style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 8, padding: 8, minHeight: 0, overflow: "auto" }}
          data-testid="work-floor"
        >
          <div className="mc-panel-title" style={{ gridColumn: "1 / -1", border: "none", padding: "0 0 4px" }}>
            Work floor · live lanes
          </div>
          {snapshot.lanes.map((lane) => (
            <WorkFloorBay key={lane.id} lane={lane} />
          ))}
        </section>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, minHeight: 0 }}>
          <QaBay queue={snapshot.qaQueue} running={snapshot.qaRunning} />
          <GateDesk tasks={snapshot.gateQueue} />
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, minHeight: 0 }}>
          <DoneShelf count={snapshot.doneCount} recent={snapshot.doneRecent} />
          <SideRail title="Paused" tasks={snapshot.paused} tone="muted" />
          <SideRail title="Blocked" tasks={snapshot.blocked} tone="warn" />
        </div>
      </div>
      <EventStream events={snapshot.events} />
    </div>
  );
}
