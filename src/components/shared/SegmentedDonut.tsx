import type { CSSProperties, HTMLAttributes } from "react";

export type PlanStatusKind = "todo" | "review" | "blocked" | "done";

export interface SegmentedDonutSegment {
  count: number;
  status?: PlanStatusKind;
  color?: string;
}

export interface SegmentedDonutProps extends HTMLAttributes<HTMLSpanElement> {
  segments: readonly SegmentedDonutSegment[];
  size: number;
}

export const PLAN_STATUS_COLORS: Record<PlanStatusKind, string> = {
  todo: "var(--ct-accent)",
  review: "var(--ct-yellow)",
  blocked: "var(--ct-red)",
  done: "var(--ct-green)",
};

export function segmentedDonutBackground(
  segments: readonly SegmentedDonutSegment[],
): string {
  const visible = segments.filter((segment) => segment.count > 0);
  const total = visible.reduce((sum, segment) => sum + segment.count, 0);
  if (total === 0) return "color-mix(in srgb, var(--ct-fg) 14%, transparent)";

  let offset = 0;
  const stops = visible.map((segment) => {
    const start = (offset / total) * 100;
    offset += segment.count;
    const color = segment.color
      ?? (segment.status ? PLAN_STATUS_COLORS[segment.status] : PLAN_STATUS_COLORS.todo);
    return `${color} ${start}% ${(offset / total) * 100}%`;
  });
  return `conic-gradient(${stops.join(", ")})`;
}

export function SegmentedDonut({
  segments,
  size,
  className = "",
  style,
  ...props
}: SegmentedDonutProps) {
  const donutStyle = {
    ...style,
    "--ct-segmented-donut-size": `${size}px`,
    "--ct-segmented-donut-background": segmentedDonutBackground(segments),
  } as CSSProperties;

  return (
    <span
      className={`ct-segmented-donut ${className}`.trim()}
      style={donutStyle}
      {...props}
    >
      <span data-testid="segmented-donut-hole" className="ct-segmented-donut-hole" />
    </span>
  );
}
