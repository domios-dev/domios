import { useEffect, useRef, useState, type AnimationEvent as ReactAnimationEvent, type HTMLAttributes } from "react";

export type ActivityDotsSize = "micro" | "small" | "medium";
export type ActivityDotsTone = "accent" | "thinking" | "muted" | "on-accent";
export type ActivityDotsRhythm = "work" | "thinking" | "streaming";
export type ActivityDotsVariant = "workspace-badge";
export type ActivityDotsPresentationState = "active" | "cooling" | "still" | "reactivated";

export interface ActivityDotsProps extends Omit<HTMLAttributes<HTMLSpanElement>, "children"> {
  size?: ActivityDotsSize;
  tone?: ActivityDotsTone;
  rhythm?: ActivityDotsRhythm;
  variant?: ActivityDotsVariant;
  active?: boolean;
}

export function ActivityDots({
  size = "medium",
  tone = "accent",
  rhythm = "work",
  variant,
  active = true,
  className,
  onAnimationEnd,
  ...props
}: ActivityDotsProps) {
  const dotCount = variant === "workspace-badge" ? 2 : size === "micro" ? 1 : 3;
  const previousActive = useRef(active);
  const [presentationState, setPresentationState] = useState<ActivityDotsPresentationState>(
    active ? "active" : "still",
  );

  useEffect(() => {
    if (previousActive.current === active) return;
    previousActive.current = active;
    setPresentationState(active ? "reactivated" : "cooling");
  }, [active]);

  const handleAnimationEnd = (event: ReactAnimationEvent<HTMLSpanElement>) => {
    onAnimationEnd?.(event);
    if (!active && event.animationName === "ct-dots-cooldown") {
      setPresentationState("still");
    }
  };

  return (
    <span
      {...props}
      data-activity-state={presentationState}
      data-active={active ? "true" : "false"}
      className={[
        "activity-dots",
        "ct-phase-dots",
        `activity-dots--${size}`,
        `activity-dots--${tone}`,
        `activity-dots--${rhythm}`,
        variant ? `activity-dots--${variant}` : "",
        `activity-dots--${presentationState}`,
        className ?? "",
      ].filter(Boolean).join(" ")}
      onAnimationEnd={handleAnimationEnd}
      aria-hidden="true"
    >
      {Array.from({ length: dotCount }, (_, index) => <i key={index} />)}
    </span>
  );
}
