import type { CSSProperties, HTMLAttributes } from "react";
import "./segmented-donut.css";

export type PlanStatusKind = "todo" | "review" | "blocked" | "done";

export interface SegmentedDonutSegment {
  count: number;
  color?: string;
  status?: PlanStatusKind;
}

export interface SegmentedDonutProps extends HTMLAttributes<HTMLSpanElement> {
  segments: readonly SegmentedDonutSegment[];
  size: number;
}

export const PLAN_STATUS_COLORS: Record<PlanStatusKind, string> = {
  todo: "var(--ct-accent, #58a6ff)",
  review: "var(--ct-yellow, #d29922)",
  blocked: "var(--ct-red, #f85149)",
  done: "var(--ct-green, #3fb950)",
};

export function segmentedDonutBackground(segments: readonly SegmentedDonutSegment[]): string {
  const visible = segments.filter((segment) => segment.count > 0);
  const total = visible.reduce((sum, segment) => sum + segment.count, 0);
  if (!total) return "color-mix(in srgb, var(--ct-fg, #c9d1d9) 14%, transparent)";

  let offset = 0;
  const stops = visible.map((segment) => {
    const start = (offset / total) * 100;
    offset += segment.count;
    const color = segment.color ?? (segment.status ? PLAN_STATUS_COLORS[segment.status] : PLAN_STATUS_COLORS.todo);
    return `${color} ${start}% ${(offset / total) * 100}%`;
  });
  return `conic-gradient(${stops.join(", ")})`;
}

export function SegmentedDonut({ segments, size, className = "", style, ...props }: SegmentedDonutProps) {
  const donutStyle = {
    ...style,
    "--ds-segmented-donut-size": `${size}px`,
    "--ds-segmented-donut-background": segmentedDonutBackground(segments),
  } as CSSProperties;

  return <span key={JSON.stringify(segments)} className={`ds-root ds-segmented-donut ${className}`} style={donutStyle} {...props}><i /></span>;
}
