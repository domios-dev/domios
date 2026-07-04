import { useId } from "react";
import { RIVER_WIDTH, RIVER_HEIGHT, activeEdgeRange, flowCurvePaths } from "./flowLayout";

interface FlowCurvesProps {
  /** Draw the traveling pulse on the active edge (milestone → verify). */
  animate: boolean;
  height: number;
}

export function FlowCurves({ animate, height }: FlowCurvesProps) {
  const gradId = useId();
  const curves = flowCurvePaths();
  const edge = activeEdgeRange();
  return (
    <svg
      className="mc-flow-svg"
      viewBox={`0 0 ${RIVER_WIDTH} ${RIVER_HEIGHT}`}
      preserveAspectRatio="none"
      style={{ height }}
      aria-hidden
      data-testid="flow-curves"
    >
      <defs>
        <linearGradient id={gradId} x1="0" x2="1">
          <stop offset="0" stopColor="var(--mc-flow-a)" />
          <stop offset="1" stopColor="var(--mc-flow-b)" />
        </linearGradient>
      </defs>
      {curves.map((c) => (
        <path key={`${c.from}-${c.to}`} d={c.d} stroke={`url(#${gradId})`} strokeWidth={2.5} fill="none" />
      ))}
      <circle
        className="mc-flow-pulse"
        data-testid="flow-pulse"
        cx={animate ? undefined : edge.to}
        cy={edge.y}
        r={3.5}
        fill="var(--mc-ok)"
      >
        {animate && (
          <animate attributeName="cx" values={`${edge.from};${edge.to}`} dur="3s" repeatCount="indefinite" />
        )}
      </circle>
    </svg>
  );
}
