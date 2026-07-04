import type { ReactNode } from "react";
import { CrewStrip } from "./CrewStrip";
import { FlowCurves } from "./FlowCurves";
import { AgentAvatar } from "./AgentAvatar";
import type { FlowChip, FlowRiverModel } from "./flowModel";
import {
  CARRIER_LEFT,
  CARRIER_TOP,
  RIVER_HEIGHT,
  RIVER_HEIGHT_MINI,
  RIVER_WIDTH,
  STATION_GEOM,
  type FlowStationKey,
} from "./flowLayout";
import type { MissionControlLodLevel } from "./missionControlLod";

export interface PlanRiverProps {
  model: FlowRiverModel;
  compact?: boolean;
  lod?: MissionControlLodLevel;
  reducedMotion?: boolean;
  onTaskClick?: (taskId: string) => void;
  onAgentClick?: (paneId: string) => void;
  onOpenPlan?: (planId: string) => void;
}

function stationStyle(key: FlowStationKey) {
  const g = STATION_GEOM[key];
  return { left: g.left, width: g.width };
}

function ChipView({ chip, onTaskClick }: { chip: FlowChip; onTaskClick?: (id: string) => void }) {
  return (
    <div
      className={`mc-flow-chip${chip.hot ? " mc-flow-chip-hot" : ""}`}
      data-testid={`flow-chip-${chip.taskId}`}
      data-mc-interactive={onTaskClick ? "true" : undefined}
      role={onTaskClick ? "button" : undefined}
      tabIndex={onTaskClick ? 0 : undefined}
      onClick={onTaskClick ? () => onTaskClick(chip.taskId) : undefined}
      onKeyDown={
        onTaskClick
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onTaskClick(chip.taskId);
              }
            }
          : undefined
      }
    >
      <span className="mc-flow-chip-text">{chip.title}</span>
      {chip.carrier && <span className="mc-flow-chip-carrier">⟵ {chip.carrier.name}</span>}
    </div>
  );
}

function StationCard({
  geomKey,
  now,
  name,
  children,
  dim,
}: {
  geomKey: FlowStationKey;
  now?: boolean;
  name: ReactNode;
  children?: ReactNode;
  dim?: boolean;
}) {
  return (
    <div
      className={`mc-flow-station${now ? " mc-flow-station-now" : ""}${dim ? " mc-flow-station-dim" : ""}`}
      style={stationStyle(geomKey)}
      data-testid={`flow-station-${geomKey}`}
    >
      <div className="mc-flow-station-name">{name}</div>
      {children}
    </div>
  );
}

export function PlanRiver({
  model,
  compact = false,
  lod = "close",
  reducedMotion = false,
  onTaskClick,
  onAgentClick,
  onOpenPlan,
}: PlanRiverProps) {
  const height = compact ? RIVER_HEIGHT_MINI : RIVER_HEIGHT;
  const { milestone, frontier, verify, done, carrier } = model;
  const animate = !reducedMotion && !compact && milestone.active && lod !== "far";

  return (
    <section
      className={`mc-river-plan${compact ? " mc-river-mini" : ""}`}
      data-testid={`plan-river-${model.planId}`}
      data-compact={compact ? "true" : undefined}
      data-lod={lod}
    >
      <div className="mc-river-head">
        <button
          type="button"
          className="mc-river-title"
          data-testid={`river-title-${model.planId}`}
          onClick={onOpenPlan ? () => onOpenPlan(model.planId) : undefined}
        >
          {model.planTitle}
        </button>
        {model.statusLabel ? (
          <span className="mc-river-status">{model.statusLabel}</span>
        ) : (
          <span className="mc-river-pct">{model.progressPct}%</span>
        )}
        <CrewStrip crew={model.crew} onAgentClick={onAgentClick} />
      </div>

      <div className="mc-river-band" style={{ height, width: RIVER_WIDTH }}>
        {!compact && <FlowCurves animate={animate} height={height} />}

        <StationCard geomKey="frontier" name={`Frontier · ${frontier.count}`}>
          {frontier.chips.map((c) => (
            <ChipView key={c.taskId} chip={c} onTaskClick={onTaskClick} />
          ))}
        </StationCard>

        <StationCard
          geomKey="milestone"
          now={milestone.active}
          dim={!milestone.active}
          name={
            milestone.active ? (
              <>
                <b>Milestone · {milestone.label}</b> — {milestone.remaining} of {milestone.total} left
              </>
            ) : (
              "Milestone — none active"
            )
          }
        >
          {milestone.chips.map((c) => (
            <ChipView key={c.taskId} chip={c} onTaskClick={onTaskClick} />
          ))}
        </StationCard>

        {!compact && (
          <StationCard geomKey="verify" name="Verify · QA + Gate">
            {verify.judging.map((c) => (
              <ChipView key={c.taskId} chip={c} onTaskClick={onTaskClick} />
            ))}
            {verify.queue.map((c) => (
              <ChipView key={c.taskId} chip={c} onTaskClick={onTaskClick} />
            ))}
            {verify.count === 0 && <div className="mc-flow-empty">queue empty — flows on close</div>}
          </StationCard>
        )}

        {!compact && (
          <StationCard geomKey="done" name="Done">
            <div className="mc-flow-count">{done.count}</div>
            <div className="mc-flow-sub">+{done.todayDelta} today</div>
          </StationCard>
        )}

        {!compact && carrier && (
          <div
            className={`mc-flow-carrier${animate ? " mc-flow-carrier-move" : ""}`}
            style={{ left: CARRIER_LEFT, top: CARRIER_TOP }}
            data-testid="flow-carrier"
          >
            <AgentAvatar agent={carrier.agent} onClick={onAgentClick} />
            <span className="mc-flow-pkg">{carrier.label}</span>
          </div>
        )}
      </div>

      {!compact && (
        <div className="mc-river-hud" data-testid={`river-hud-${model.planId}`}>
          <span><b>{model.hud.lanesLive}</b>lanes live</span>
          <span><b>{model.hud.qaQueue}</b>qa queue</span>
          <span><b>{model.hud.loops}</b>loops</span>
          <span><b>{model.hud.cleanup}</b>cleanup</span>
          {model.hud.next && <span className="mc-river-next">next: {model.hud.next}</span>}
        </div>
      )}
    </section>
  );
}
