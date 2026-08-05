import type { HTMLAttributes } from "react";

export type ActivityDotsSize = "micro" | "small" | "medium";
export type ActivityDotsTone = "accent" | "thinking" | "muted" | "on-accent";
export type ActivityDotsRhythm = "work" | "thinking" | "streaming";
export type ActivityDotsVariant = "workspace-badge";

export interface ActivityDotsProps extends Omit<HTMLAttributes<HTMLSpanElement>, "children"> {
  size?: ActivityDotsSize;
  tone?: ActivityDotsTone;
  rhythm?: ActivityDotsRhythm;
  variant?: ActivityDotsVariant;
  idle?: boolean;
}

export function ActivityDots({
  size = "medium",
  tone = "accent",
  rhythm = "work",
  variant,
  idle = false,
  className,
  ...props
}: ActivityDotsProps) {
  const dotCount = variant === "workspace-badge" ? 2 : size === "micro" ? 1 : 3;

  return (
    <span
      {...props}
      className={[
        "activity-dots",
        "ct-phase-dots",
        `activity-dots--${size}`,
        `activity-dots--${tone}`,
        `activity-dots--${rhythm}`,
        variant ? `activity-dots--${variant}` : "",
        idle ? "activity-dots--idle" : "",
        className ?? "",
      ].filter(Boolean).join(" ")}
      aria-hidden="true"
    >
      {Array.from({ length: dotCount }, (_, index) => <i key={index} />)}
    </span>
  );
}
