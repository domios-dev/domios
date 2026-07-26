import type { HTMLAttributes } from "react";

export type ActivityDotsSize = "micro" | "small" | "medium";
export type ActivityDotsTone = "accent" | "thinking" | "muted";
export type ActivityDotsRhythm = "work" | "thinking" | "streaming";

export interface ActivityDotsProps extends Omit<HTMLAttributes<HTMLSpanElement>, "children"> {
  size?: ActivityDotsSize;
  tone?: ActivityDotsTone;
  rhythm?: ActivityDotsRhythm;
  idle?: boolean;
}

export function ActivityDots({
  size = "medium",
  tone = "accent",
  rhythm = "work",
  idle = false,
  className,
  ...props
}: ActivityDotsProps) {
  return (
    <span
      {...props}
      className={[
        "activity-dots",
        "ct-phase-dots",
        `activity-dots--${size}`,
        `activity-dots--${tone}`,
        `activity-dots--${rhythm}`,
        idle ? "activity-dots--idle" : "",
        className ?? "",
      ].filter(Boolean).join(" ")}
      aria-hidden="true"
    >
      <i />
      <i />
      <i />
    </span>
  );
}
