import { EventStream } from "../EventStream";
import { PlanHud } from "../PlanHud";
import { DispatchRail, DoneShelf, GateDesk, QaBay, SideRail, WorkFloorBay } from "../stations";
import type { MissionControlSnapshot } from "../types";

interface VariantProps {
  snapshot: MissionControlSnapshot;
}

export function VariantControlRoom({ snapshot }: VariantProps) {
  return (
    <div
      data-variant="control-room"
      data-testid="variant-control-room"
      style={{ display: "grid", gridTemplateRows: "auto 1fr auto", height: "100%", minHeight: 0 }}
    >
      <PlanHud planTitle={snapshot.planTitle} goal={snapshot.goal} hud={snapshot.hud} compact />
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 280px 1fr",
          gridTemplateRows: "auto 1fr auto",
          gap: 8,
          padding: 8,
          minHeight: 0,
          alignItems: "stretch",
        }}
      >
        <div style={{ gridColumn: 1, gridRow: 2 }}>
          <DispatchRail title="Dispatch" tasks={snapshot.dispatchQueue} />
        </div>
        <section
          className="mc-panel"
          style={{
            gridColumn: 2,
            gridRow: "1 / 4",
            display: "flex",
            flexDirection: "column",
            gap: 8,
            padding: 10,
            minHeight: 0,
            borderColor: "color-mix(in srgb, var(--mc-accent) 35%, var(--mc-border))",
          }}
          data-testid="work-floor"
        >
          <div className="mc-panel-title" style={{ border: "none", padding: 0, textAlign: "center" }}>
            Ops ring · work floor
          </div>
          <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8, overflow: "auto", minHeight: 0 }}>
            {snapshot.lanes.map((lane) => (
              <WorkFloorBay key={lane.id} lane={lane} />
            ))}
          </div>
          <QaBay queue={snapshot.qaQueue} running={snapshot.qaRunning} />
        </section>
        <div style={{ gridColumn: 3, gridRow: 2, display: "flex", flexDirection: "column", gap: 8, minHeight: 0 }}>
          <GateDesk tasks={snapshot.gateQueue} />
          <DoneShelf count={snapshot.doneCount} recent={snapshot.doneRecent} />
        </div>
        <div style={{ gridColumn: 1, gridRow: 3, display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
          <SideRail title="Paused" tasks={snapshot.paused} />
          <SideRail title="Blocked" tasks={snapshot.blocked} tone="warn" />
        </div>
      </div>
      <EventStream events={snapshot.events} />
    </div>
  );
}
