import { AgentAvatar } from "./AgentAvatar";
import { TaskCard } from "./TaskCard";
import type { MissionLane } from "./types";

interface WorkFloorBayProps {
  lane: MissionLane;
}

export function WorkFloorBay({ lane }: WorkFloorBayProps) {
  return (
    <div className="mc-bay-slot" data-testid={`lane-bay-${lane.id}`}>
      <div className="mc-bay-header">
        <div style={{ minWidth: 0 }}>
          <div className="mc-bay-label">{lane.name}</div>
          <div className="mc-bay-branch mc-detail-only">{lane.branch}</div>
        </div>
        {lane.agent && <AgentAvatar agent={lane.agent} />}
      </div>
      {lane.activeTask ? (
        <TaskCard task={lane.activeTask} />
      ) : (
        <div style={{ fontSize: 10, color: "var(--mc-muted)", fontStyle: "italic" }} className="mc-detail-only">
          Bay idle — awaiting dispatch
        </div>
      )}
    </div>
  );
}

interface DispatchRailProps {
  title: string;
  tasks: Parameters<typeof TaskCard>[0]["task"][];
}

export function DispatchRail({ title, tasks }: DispatchRailProps) {
  return (
    <section className="mc-panel" style={{ display: "flex", flexDirection: "column", minHeight: 0 }} data-testid="dispatch-rail">
      <div className="mc-panel-title">{title}</div>
      <div className="mc-queue">
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} dense />
        ))}
      </div>
    </section>
  );
}

interface SideRailProps {
  title: string;
  tasks: Parameters<typeof TaskCard>[0]["task"][];
  tone?: "warn" | "muted";
}

export function SideRail({ title, tasks, tone }: SideRailProps) {
  return (
    <section
      className="mc-panel"
      style={{
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        borderColor: tone === "warn" ? "color-mix(in srgb, var(--mc-danger) 35%, var(--mc-border))" : undefined,
      }}
      data-testid={`side-rail-${title.toLowerCase().replace(/\s+/g, "-")}`}
    >
      <div className="mc-panel-title">
        {title}
        <span className="mc-overview-only"> ({tasks.length})</span>
      </div>
      <div className="mc-queue">
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} dense />
        ))}
      </div>
    </section>
  );
}

interface DoneShelfProps {
  count: number;
  recent: Parameters<typeof TaskCard>[0]["task"][];
}

export function DoneShelf({ count, recent }: DoneShelfProps) {
  return (
    <section className="mc-panel mc-done-shelf" data-testid="done-shelf">
      <div>
        <div className="mc-panel-title" style={{ border: "none", padding: 0 }}>Done</div>
        <div className="mc-done-count">{count}</div>
      </div>
      <div className="mc-detail-only" style={{ display: "flex", flexDirection: "column", gap: 4, flex: 1, minWidth: 0 }}>
        {recent.slice(0, 2).map((t) => (
          <TaskCard key={t.id} task={t} dense />
        ))}
      </div>
      <div className="mc-overview-only" style={{ fontSize: 10, color: "var(--mc-muted)" }}>+{Math.max(0, count - 2)} more</div>
    </section>
  );
}

interface QaBayProps {
  queue: Parameters<typeof TaskCard>[0]["task"][];
  running?: Parameters<typeof TaskCard>[0]["task"];
}

export function QaBay({ queue, running }: QaBayProps) {
  return (
    <section className="mc-panel" style={{ display: "flex", flexDirection: "column", minHeight: 0 }} data-testid="qa-bay">
      <div className="mc-panel-title">QA bay</div>
      <div style={{ padding: 8, display: "flex", flexDirection: "column", gap: 8, minHeight: 0, flex: 1 }}>
        <div>
          <div style={{ fontSize: 9, color: "var(--mc-muted)", marginBottom: 4, textTransform: "uppercase", letterSpacing: "0.06em" }}>
            Running slot
          </div>
          {running ? (
            <TaskCard task={running} />
          ) : (
            <div style={{ fontSize: 10, color: "var(--mc-muted)" }}>Empty</div>
          )}
        </div>
        <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          <div style={{ fontSize: 9, color: "var(--mc-muted)", marginBottom: 4, textTransform: "uppercase", letterSpacing: "0.06em" }}>
            Queue <span className="mc-overview-only">({queue.length})</span>
          </div>
          <div className="mc-queue" style={{ padding: 0 }}>
            {queue.map((t) => (
              <TaskCard key={t.id} task={t} dense />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

interface GateDeskProps {
  tasks: Parameters<typeof TaskCard>[0]["task"][];
}

export function GateDesk({ tasks }: GateDeskProps) {
  return (
    <section className="mc-panel mc-gate-desk" style={{ display: "flex", flexDirection: "column", minHeight: 0 }} data-testid="gate-desk">
      <div className="mc-panel-title">Gate desk · in review</div>
      <div className="mc-queue">
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} />
        ))}
      </div>
    </section>
  );
}
