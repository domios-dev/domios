import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { CrewStrip } from "./CrewStrip";
import { FlowCurves } from "./FlowCurves";
import { AgentAvatar } from "./AgentAvatar";
import type { FlowChip, FlowRiverModel, GateView } from "./flowModel";
import { edgesFromRects, type MeasuredEdge, type StationRect } from "./flowLayout";
import type { MissionControlLodLevel } from "./missionControlLod";

export interface PlanRiverProps {
  model: FlowRiverModel;
  compact?: boolean;
  lod?: MissionControlLodLevel;
  reducedMotion?: boolean;
  onTaskClick?: (taskId: string) => void;
  onAgentClick?: (tabId: string) => void;
  onOpenPlan?: (planId: string) => void;
}

type StationKey = "frontier" | "milestone" | "verify" | "done";

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

function GateRow({ gate }: { gate: GateView }) {
  return (
    <div className={`mc-flow-gate${gate.escalated ? " mc-flow-gate-esc" : ""}`} data-testid={`flow-gate-${gate.id}`}>
      <span className="mc-flow-gate-q">{gate.question}</span>
      <span className="mc-flow-gate-wait">waits on {gate.waitsOn}</span>
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
  const bandRef = useRef<HTMLDivElement | null>(null);
  const stationRefs = useRef<Map<StationKey, HTMLDivElement>>(new Map());
  const [edges, setEdges] = useState<MeasuredEdge[]>([]);
  const [bandSize, setBandSize] = useState({ width: 0, height: 0 });

  const { milestone, frontier, verify, done, carrier } = model;

  // Which stations render (never a ghost): frontier when it has queue; milestone
  // when active OR there is inflow to target; verify/done when they hold content.
  const present: StationKey[] = [];
  if (frontier.count > 0) present.push("frontier");
  if (milestone.active || frontier.count > 0) present.push("milestone");
  if (verify.count > 0) present.push("verify");
  if (done.count > 0) present.push("done");

  const measure = useCallback(() => {
    const band = bandRef.current;
    if (!band) return;
    const rects: StationRect[] = [];
    for (const key of present) {
      const el = stationRefs.current.get(key);
      if (!el) continue;
      rects.push({ key, left: el.offsetLeft, top: el.offsetTop, width: el.offsetWidth, height: el.offsetHeight });
    }
    setEdges(edgesFromRects(rects));
    setBandSize({ width: band.offsetWidth, height: band.offsetHeight });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [present.join(",")]);

  useLayoutEffect(() => {
    measure();
    const band = bandRef.current;
    if (!band || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(band);
    for (const el of stationRefs.current.values()) ro.observe(el);
    return () => ro.disconnect();
  }, [measure, model]);

  const setStationRef = (key: StationKey) => (el: HTMLDivElement | null) => {
    if (el) stationRefs.current.set(key, el);
    else stationRefs.current.delete(key);
  };

  const animate = !reducedMotion && !compact && milestone.active && lod !== "far";
  const activeEdge = edges.find((e) => e.active) ?? null;

  const header = (
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
  );

  // F4: an empty plan is a head-only block — no ghost river, dot, or zero HUD.
  if (model.isEmpty) {
    return (
      <section
        className="mc-river-plan mc-river-empty"
        data-testid={`plan-river-${model.planId}`}
        data-empty="true"
        data-compact={compact ? "true" : undefined}
      >
        {header}
        <div className="mc-river-empty-note">No active work.</div>
      </section>
    );
  }

  function stationCard(key: StationKey, now: boolean, dim: boolean, name: ReactNode, body: ReactNode) {
    return (
      <div
        ref={setStationRef(key)}
        className={`mc-flow-station${now ? " mc-flow-station-now" : ""}${dim ? " mc-flow-station-dim" : ""}`}
        data-testid={`flow-station-${key}`}
      >
        <div className="mc-flow-station-name">{name}</div>
        {body}
      </div>
    );
  }

  return (
    <section
      className={`mc-river-plan${compact ? " mc-river-mini" : ""}`}
      data-testid={`plan-river-${model.planId}`}
      data-compact={compact ? "true" : undefined}
      data-lod={lod}
    >
      {header}

      <div ref={bandRef} className="mc-river-band">
        <FlowCurves edges={edges} width={bandSize.width} height={bandSize.height} animate={animate} />

        {present.includes("frontier") &&
          stationCard(
            "frontier",
            false,
            false,
            `Frontier · ${frontier.count}`,
            frontier.chips.map((c) => <ChipView key={c.taskId} chip={c} onTaskClick={onTaskClick} />),
          )}

        {present.includes("milestone") &&
          stationCard(
            "milestone",
            milestone.active,
            !milestone.active,
            milestone.active ? (
              <>
                <b>Milestone · {milestone.label}</b> — {milestone.remaining} of {milestone.total} left
              </>
            ) : (
              "Milestone — none active"
            ),
            milestone.chips.map((c) => <ChipView key={c.taskId} chip={c} onTaskClick={onTaskClick} />),
          )}

        {present.includes("verify") &&
          stationCard(
            "verify",
            false,
            false,
            "Verify · review + gates",
            <>
              {verify.inReview.map((c) => (
                <ChipView key={c.taskId} chip={c} onTaskClick={onTaskClick} />
              ))}
              {verify.gates.map((g) => (
                <GateRow key={g.id} gate={g} />
              ))}
            </>,
          )}

        {present.includes("done") &&
          stationCard(
            "done",
            false,
            false,
            "Done",
            <>
              <div className="mc-flow-count">{done.count}</div>
              <div className="mc-flow-sub">+{done.todayDelta} today</div>
              {done.lastStride && !compact && (
                <div className="mc-flow-stride" title={done.lastStride}>
                  ↳ {done.lastStride}
                </div>
              )}
            </>,
          )}

        {!compact && carrier && activeEdge && activeEdge.span >= 44 && (
          <div
            className={`mc-flow-carrier${animate ? " mc-flow-carrier-move" : ""}`}
            style={{ left: activeEdge.mid.x - activeEdge.span / 2 + 6, top: activeEdge.mid.y - 34 }}
            data-testid="flow-carrier"
            title={carrier.label}
          >
            <AgentAvatar agent={carrier.agent} onClick={onAgentClick} />
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
