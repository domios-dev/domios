import { AgentAvatar } from "./AgentAvatar";
import { TaskCard } from "./TaskCard";
import type { MissionLane } from "./types";
import type { MissionControlLodLevel } from "./missionControlLod";

type Lod = MissionControlLodLevel;

interface StationProps {
  lod?: Lod;
}

interface WorkFloorBayProps extends StationProps {
  lane: MissionLane;
  onTaskClick?: (taskId: string) => void;
  onAgentClick?: (tabId: string) => void;
}

export function WorkFloorBay({ lane, lod, onTaskClick, onAgentClick }: WorkFloorBayProps) {
  const tasks = lane.activeTasks ?? (lane.activeTask ? [lane.activeTask] : []);
  const agents = lane.agents ?? (lane.agent ? [lane.agent] : []);
  const idleBay = agents.length > 0 && tasks.length === 0;
  const bayIdleAgent = agents.some((a) => a.state === "idle" || a.state === "waiting");

  return (
    <div
      className="mc-bay-slot"
      data-testid={`lane-bay-${lane.id}`}
      data-idle={bayIdleAgent || idleBay ? "true" : undefined}
      data-empty={tasks.length === 0 ? "true" : undefined}
    >
      <div className="mc-bay-header">
        <div style={{ minWidth: 0 }}>
          <div className="mc-bay-label" title={lane.name}>{lane.name}</div>
          {lane.branch && <div className="mc-bay-branch mc-detail-only" title={lane.branch}>{lane.branch}</div>}
        </div>
        <div className="mc-bay-avatars" style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
          {agents.map((agent) => (
            <AgentAvatar key={agent.id} agent={agent} onClick={onAgentClick} />
          ))}
        </div>
      </div>
      {tasks.length > 0 ? (
        tasks.map((task) => <TaskCard key={task.id} task={task} lod={lod} onClick={onTaskClick} />)
      ) : (
        <div className="mc-bay-empty mc-detail-only">Bay idle — awaiting dispatch</div>
      )}
    </div>
  );
}

interface DispatchRailProps extends StationProps {
  title: string;
  tasks: Parameters<typeof TaskCard>[0]["task"][];
  onTaskClick?: (taskId: string) => void;
  stressed?: boolean;
}

export function DispatchRail({ title, tasks, lod, onTaskClick, stressed }: DispatchRailProps) {
  return (
    <section
      className="mc-panel"
      style={{ display: "flex", flexDirection: "column", minHeight: 0 }}
      data-testid="dispatch-rail"
      data-stressed={stressed ? "true" : undefined}
    >
      <div className="mc-panel-title">
        {title}
        <span className="mc-overview-only"> ({tasks.length})</span>
      </div>
      <div className="mc-queue" data-canvas-scroll-body>
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} dense lod={lod} onClick={onTaskClick} />
        ))}
      </div>
    </section>
  );
}

interface SideRailProps extends StationProps {
  title: string;
  tasks: Parameters<typeof TaskCard>[0]["task"][];
  tone?: "warn" | "muted";
  onTaskClick?: (taskId: string) => void;
}

export function SideRail({ title, tasks, lod, tone, onTaskClick }: SideRailProps) {
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
      <div className="mc-queue" data-canvas-scroll-body>
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} dense lod={lod} onClick={onTaskClick} />
        ))}
      </div>
    </section>
  );
}

interface DoneShelfProps extends StationProps {
  count: number;
  recent: Parameters<typeof TaskCard>[0]["task"][];
  onTaskClick?: (taskId: string) => void;
}

export function DoneShelf({ count, recent, lod, onTaskClick }: DoneShelfProps) {
  return (
    <section className="mc-panel mc-done-shelf" data-testid="done-shelf">
      <div className="mc-done-header">
        <div className="mc-panel-title" style={{ border: "none", padding: 0 }}>Done</div>
        <div className="mc-done-count">{count}</div>
      </div>
      <div className="mc-detail-only mc-done-recent">
        {recent.slice(0, 2).map((t) => (
          <TaskCard key={t.id} task={t} dense lod={lod} onClick={onTaskClick} />
        ))}
      </div>
      <div className="mc-overview-only" style={{ fontSize: 10, color: "var(--mc-muted)" }}>+{Math.max(0, count - 2)} more</div>
    </section>
  );
}

interface QaBayProps extends StationProps {
  queue: Parameters<typeof TaskCard>[0]["task"][];
  running?: Parameters<typeof TaskCard>[0]["task"];
  onTaskClick?: (taskId: string) => void;
  bottleneck?: boolean;
}

export function QaBay({ queue, running, lod, onTaskClick, bottleneck }: QaBayProps) {
  return (
    <section
      className="mc-panel"
      style={{ display: "flex", flexDirection: "column", minHeight: 0 }}
      data-testid="qa-bay"
      data-bottleneck={bottleneck ? "true" : undefined}
    >
      <div className="mc-panel-title">
        QA bay
        <span className="mc-overview-only"> ({queue.length + (running ? 1 : 0)})</span>
      </div>
      <div style={{ padding: 8, display: "flex", flexDirection: "column", gap: 8, minHeight: 0, flex: 1 }}>
        <div>
          <div className="mc-zone-subtitle mc-detail-only">Running slot</div>
          {running ? (
            <TaskCard task={running} lod={lod} onClick={onTaskClick} />
          ) : (
            <div className="mc-zone-empty">Empty</div>
          )}
        </div>
        <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          <div className="mc-zone-subtitle">
            Queue <span className="mc-overview-only">({queue.length})</span>
          </div>
          <div className="mc-queue" style={{ padding: 0 }}>
            {queue.map((t) => (
              <TaskCard key={t.id} task={t} dense lod={lod} onClick={onTaskClick} />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

interface GateDeskProps extends StationProps {
  tasks: Parameters<typeof TaskCard>[0]["task"][];
  onTaskClick?: (taskId: string) => void;
}

export function GateDesk({ tasks, lod, onTaskClick }: GateDeskProps) {
  return (
    <section className="mc-panel mc-gate-desk" style={{ display: "flex", flexDirection: "column", minHeight: 0 }} data-testid="gate-desk">
      <div className="mc-panel-title">
        Gate desk
        <span className="mc-overview-only"> ({tasks.length})</span>
      </div>
      <div className="mc-queue" data-canvas-scroll-body>
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} lod={lod} onClick={onTaskClick} />
        ))}
      </div>
    </section>
  );
}
