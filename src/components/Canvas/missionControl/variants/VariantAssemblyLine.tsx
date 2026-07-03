import { EventStream } from "../EventStream";
import { PlanHud } from "../PlanHud";
import { DispatchRail, DoneShelf, GateDesk, QaBay, SideRail, WorkFloorBay } from "../stations";
import type { MissionControlSnapshot } from "../types";

interface VariantProps {
  snapshot: MissionControlSnapshot;
}

export function VariantAssemblyLine({ snapshot }: VariantProps) {
  return (
    <div
      data-variant="assembly-line"
      data-testid="variant-assembly-line"
      style={{ display: "grid", gridTemplateRows: "auto 1fr", height: "100%", minHeight: 0 }}
    >
      <PlanHud planTitle={snapshot.planTitle} goal={snapshot.goal} hud={snapshot.hud} />
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "160px 1fr 180px 140px 120px",
          gap: 6,
          padding: 8,
          minHeight: 0,
          alignItems: "stretch",
        }}
      >
        <DispatchRail title="Frontier" tasks={snapshot.dispatchQueue} />
        <section
          className="mc-panel"
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 6,
            padding: 8,
            minHeight: 0,
            overflow: "auto",
          }}
          data-testid="work-floor"
        >
          <div className="mc-panel-title" style={{ border: "none", padding: 0 }}>
            Lanes →
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(120px, 1fr))", gap: 6 }}>
            {snapshot.lanes.map((lane) => (
              <WorkFloorBay key={lane.id} lane={lane} />
            ))}
          </div>
        </section>
        <QaBay queue={snapshot.qaQueue} running={snapshot.qaRunning} />
        <GateDesk tasks={snapshot.gateQueue} />
        <div style={{ display: "flex", flexDirection: "column", gap: 6, minHeight: 0 }}>
          <DoneShelf count={snapshot.doneCount} recent={snapshot.doneRecent} />
          <SideRail title="Parked" tasks={snapshot.parked} />
          <SideRail title="Blocked" tasks={snapshot.blocked} tone="warn" />
        </div>
      </div>
      <EventStream events={snapshot.events} />
    </div>
  );
}
