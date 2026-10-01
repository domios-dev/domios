import type { ReactElement } from "react";
import type { HelpFigureId } from "../types";
import { Arrow, Box, accent, muted } from "./primitives";
import { SupporterRecallFigure, SupporterTabStateFigure } from "./supporter";

function AgentHierarchyFigure() {
  return (
    <svg viewBox="0 0 380 240" className="w-full max-w-md mx-auto" role="img" aria-label="Agent hierarchy diagram">
      <defs>
        <marker id="help-arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
          <path d="M0,0 L6,3 L0,6 Z" fill={accent} />
        </marker>
      </defs>
      <Box x={130} y={8} w={120} h={44} label="General Agent" sub="always on" emphasis />
      <Arrow x1={190} y1={52} x2={190} y2={72} />
      <Box x={115} y={72} w={150} h={48} label="Orchestrator" sub="single authority" emphasis />
      <Arrow x1={190} y1={120} x2={98} y2={150} />
      <Arrow x1={190} y1={120} x2={190} y2={150} />
      <Arrow x1={190} y1={120} x2={282} y2={150} />
      <Box x={32} y={150} w={92} h={44} label="Planner" sub="specialist" />
      <Box x={144} y={150} w={92} h={44} label="Worker" sub="task A" />
      <Box x={256} y={150} w={92} h={44} label="Worker" sub="task B" />
      <text x={190} y={215} textAnchor="middle" fill={muted} fontSize={9}>
        Planner drafts; Orchestrator decides
      </text>
    </svg>
  );
}

function PlanPipelineFigure() {
  const boxes = [
    { x: 8, label: "Goal", sub: "north star" },
    { x: 98, label: "Spec", sub: "what / why" },
    { x: 188, label: "Plan", sub: "how / waves" },
    { x: 278, label: "Tasks", sub: "acceptance" },
  ];
  return (
    <svg viewBox="0 0 360 80" className="w-full max-w-lg mx-auto" role="img" aria-label="Plan pipeline diagram">
      <defs>
        <marker id="help-arrow-pipe" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
          <path d="M0,0 L6,3 L0,6 Z" fill={accent} />
        </marker>
      </defs>
      {boxes.map((b, i) => (
        <g key={b.label}>
          <Box x={b.x} y={16} w={74} h={48} label={b.label} sub={b.sub} emphasis={i === 0} />
          {i < boxes.length - 1 && (
            <line
              x1={b.x + 74}
              y1={40}
              x2={boxes[i + 1].x}
              y2={40}
              stroke={accent}
              strokeWidth={1.5}
              markerEnd="url(#help-arrow-pipe)"
            />
          )}
        </g>
      ))}
    </svg>
  );
}

function TaskStatusFlowFigure() {
  const nodes = [
    { x: 8, label: "Pending" },
    { x: 88, label: "In progress" },
    { x: 178, label: "In review" },
    { x: 268, label: "Done" },
  ];
  return (
    <svg viewBox="0 0 360 130" className="w-full max-w-lg mx-auto" role="img" aria-label="Task status flow diagram">
      <defs>
        <marker id="help-arrow-flow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
          <path d="M0,0 L6,3 L0,6 Z" fill={accent} />
        </marker>
      </defs>
      {nodes.map((n, i) => (
        <g key={n.label}>
          <Box x={n.x} y={20} w={78} h={40} label={n.label} emphasis={i === nodes.length - 1} />
          {i < nodes.length - 1 && (
            <line
              x1={n.x + 78}
              y1={40}
              x2={nodes[i + 1].x}
              y2={40}
              stroke={accent}
              strokeWidth={1.5}
              markerEnd="url(#help-arrow-flow)"
            />
          )}
        </g>
      ))}
      <Box x={88} y={82} w={78} h={36} label="Blocked" />
      <Box x={178} y={82} w={78} h={36} label="Paused" />
      <line x1={127} y1={60} x2={127} y2={82} stroke={muted} strokeWidth={1} strokeDasharray="4 3" />
      <line x1={217} y1={60} x2={217} y2={82} stroke={muted} strokeWidth={1} strokeDasharray="4 3" />
    </svg>
  );
}

const FIGURES: Record<HelpFigureId, () => ReactElement> = {
  "agent-hierarchy": AgentHierarchyFigure,
  "plan-pipeline": PlanPipelineFigure,
  "task-status-flow": TaskStatusFlowFigure,
  "supporter-recall": SupporterRecallFigure,
  "supporter-tab-state": SupporterTabStateFigure,
};

export function HelpFigure({ figureId }: { figureId: HelpFigureId }) {
  const Figure = FIGURES[figureId];
  return (
    <div
      className="my-4 rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 p-4"
      data-testid="help-figure"
      data-help-figure={figureId}
    >
      <Figure />
    </div>
  );
}
