import { useId } from "react";
import type { MeasuredEdge } from "./flowLayout";

interface FlowCurvesProps {
  edges: MeasuredEdge[];
  width: number;
  height: number;
  /** Animate the pulse on the active (milestone → verify) edge. */
  animate: boolean;
}

export function FlowCurves({ edges, width, height, animate }: FlowCurvesProps) {
  const gradId = useId();
  if (width <= 0 || height <= 0 || edges.length === 0) return null;
  const activeEdge = edges.find((e) => e.active) ?? null;
  return (
    <svg
      className="mc-flow-svg"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      aria-hidden
      data-testid="flow-curves"
    >
      <defs>
        <linearGradient id={gradId} x1="0" x2="1">
          <stop offset="0" stopColor="var(--mc-flow-a)" />
          <stop offset="1" stopColor="var(--mc-flow-b)" />
        </linearGradient>
      </defs>
      {edges.map((e) => (
        <path key={`${e.from}-${e.to}`} d={e.d} stroke={`url(#${gradId})`} strokeWidth={2.5} fill="none" />
      ))}
      {activeEdge && (
        <circle
          className="mc-flow-pulse"
          data-testid="flow-pulse"
          cx={activeEdge.mid.x}
          cy={activeEdge.mid.y}
          r={3.5}
          fill="var(--mc-ok)"
        >
          {animate && (
            <animate
              attributeName="cx"
              values={`${activeEdge.mid.x - 60};${activeEdge.mid.x + 60};${activeEdge.mid.x - 60}`}
              dur="3s"
              repeatCount="indefinite"
            />
          )}
        </circle>
      )}
    </svg>
  );
}
